//! T6.1 — Replan advisor use case.
//!
//! Takes a `Plan` + a failed task + a failure summary and asks an
//! auxiliary LLM to propose ONE `PlanPatch` that will unstick the
//! plan. The patch is pre-validated by applying it to a clone of
//! the plan; only patches that produce a valid plan are returned.
//!
//! This decouples the *decision* (which task to mutate, how) from
//! the *application* (the existing `plan_replan` tool / the
//! `Plan::apply_patch` method). The agent loop's auto-replan
//! trigger feeds failed tasks here; the returned patch is fed to
//! `Plan::apply_patch` (or surfaced to the user for confirmation).
//!
//! Pure logic: takes an `&dyn LlmProvider` so tests inject a fake
//! that returns canned responses. No tokio, no IO of its own.

use std::sync::Arc;

use serde_json::{Value, json};

use async_trait::async_trait;

use theo_domain::identifiers::PlanTaskId;
use theo_domain::plan::{Plan, PlanTask};
use theo_domain::plan_patch::{PlanPatch, ReplanAdvisor};
use theo_infra_llm::provider::LlmProvider;
use theo_infra_llm::types::{ChatRequest, Message};

/// Errors the advisor surfaces.
#[derive(Debug, thiserror::Error)]
pub enum ReplanAdvisorError {
    #[error("task `{0}` not found in plan")]
    TaskNotFound(u32),
    #[error("LLM call failed: {0}")]
    Llm(String),
    #[error("LLM returned no usable response")]
    EmptyResponse,
    #[error("could not extract JSON patch from LLM response: {0}")]
    BadJson(String),
    #[error("patch failed validation against the current plan: {0}")]
    PatchInvalid(String),
}

/// Build the prompt the auxiliary LLM sees. Public so tests can
/// snapshot the wording — small wording drift is fine but the
/// REQUIRED JSON shape on the output is load-bearing.
pub fn build_replan_prompt(
    plan: &Plan,
    failed_task: &PlanTask,
    failure_summary: &str,
) -> String {
    let mut prompt = String::new();
    prompt.push_str(
        "You are a plan-recovery assistant. ONE task in a multi-task plan \
         is stuck. Propose ONE PlanPatch (and only one) that will let the \
         plan move forward.\n\n",
    );
    prompt.push_str(&format!("Plan goal: {}\n\n", plan.goal));
    prompt.push_str(&format!(
        "Stuck task (id={}, failures={}):\n  title: {}\n  description: {}\n  \
         definition_of_done: {}\n  recent_outcome: {}\n\n",
        failed_task.id.0,
        failed_task.failure_count,
        failed_task.title,
        if failed_task.description.is_empty() {
            "(none)".to_string()
        } else {
            failed_task.description.clone()
        },
        if failed_task.dod.is_empty() {
            "(none)".to_string()
        } else {
            failed_task.dod.clone()
        },
        failed_task.outcome.as_deref().unwrap_or("(none)"),
    ));
    prompt.push_str(&format!(
        "Failure summary:\n{}\n\n",
        if failure_summary.trim().is_empty() {
            "(no summary provided — task has failed multiple times without details)"
        } else {
            failure_summary
        },
    ));
    prompt.push_str(
        "RESPOND WITH JSON ONLY (no prose, no markdown fences). The JSON \
         MUST be exactly one of these PlanPatch shapes:\n\
         \n\
         {\"kind\":\"skip_task\", \"id\":<u32>, \"rationale\":\"<why>\"}\n\
         {\"kind\":\"edit_task\", \"id\":<u32>, \"edits\":{...partial PlanTask...}}\n\
         {\"kind\":\"reorder_deps\", \"id\":<u32>, \"new_deps\":[<u32>...]}\n\
         {\"kind\":\"remove_task\", \"id\":<u32>}\n\
         {\"kind\":\"add_task\", \"phase\":<u32>, \"task\":{...PlanTask...}, \"position\":\"end\"|\"begin\"|{\"after_task\":<u32>}}\n\
         \n\
         Prefer `skip_task` when the failure is unrecoverable. Use \
         `edit_task` when you can clarify the dod or files. Avoid `add_task` \
         unless the recovery genuinely requires NEW work.",
    );
    prompt
}

/// Try to extract a JSON object from `text`. The LLM may wrap the
/// output in ```json fences or prose — strip generously and look for
/// the first balanced `{...}`.
pub fn extract_json_object(text: &str) -> Result<Value, ReplanAdvisorError> {
    let trimmed = text.trim();
    // Common case: model returns clean JSON.
    if let Ok(v) = serde_json::from_str::<Value>(trimmed)
        && v.is_object()
    {
        return Ok(v);
    }
    // Strip fenced code block.
    let stripped = strip_code_fence(trimmed);
    if let Ok(v) = serde_json::from_str::<Value>(stripped)
        && v.is_object()
    {
        return Ok(v);
    }
    // Last resort: scan for the first balanced {...} substring.
    if let Some(slice) = first_balanced_object(text)
        && let Ok(v) = serde_json::from_str::<Value>(slice)
        && v.is_object()
    {
        return Ok(v);
    }
    Err(ReplanAdvisorError::BadJson(format!(
        "no JSON object found in response (first 200 chars: {:.200})",
        text
    )))
}

fn strip_code_fence(s: &str) -> &str {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("```json") {
        return rest.trim_start().trim_end_matches("```").trim();
    }
    if let Some(rest) = s.strip_prefix("```") {
        return rest.trim_start().trim_end_matches("```").trim();
    }
    s
}

fn first_balanced_object(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{')?;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if escape {
            escape = false;
            continue;
        }
        match b {
            b'\\' if in_string => escape = true,
            b'"' => in_string = !in_string,
            b'{' if !in_string => depth += 1,
            b'}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Validate the patch against `plan` by applying it to a clone.
/// Returns the validated patch on success.
pub fn validate_patch_against_plan(
    plan: &Plan,
    patch: PlanPatch,
) -> Result<PlanPatch, ReplanAdvisorError> {
    let mut probe = plan.clone();
    probe
        .apply_patch(&patch)
        .map_err(|e| ReplanAdvisorError::PatchInvalid(format!("{e}")))?;
    Ok(patch)
}

/// Ask the auxiliary LLM to propose a recovery patch for the failed
/// task and return the validated `PlanPatch`.
pub async fn propose_recovery_patch(
    llm: Arc<dyn LlmProvider>,
    plan: &Plan,
    failed_task_id: PlanTaskId,
    failure_summary: &str,
) -> Result<PlanPatch, ReplanAdvisorError> {
    let task = plan
        .all_tasks()
        .into_iter()
        .find(|t| t.id == failed_task_id)
        .ok_or(ReplanAdvisorError::TaskNotFound(failed_task_id.0))?
        .clone();

    let prompt = build_replan_prompt(plan, &task, failure_summary);
    let mut request = ChatRequest::new(llm.model().to_string(), vec![Message::user(&prompt)]);
    request.max_tokens = Some(512);
    request.temperature = Some(0.2);
    let response = llm
        .chat(&request)
        .await
        .map_err(|e| ReplanAdvisorError::Llm(format!("{e}")))?;
    let text = response.content().unwrap_or("").to_string();
    if text.trim().is_empty() {
        return Err(ReplanAdvisorError::EmptyResponse);
    }
    let json_value = extract_json_object(&text)?;
    let patch: PlanPatch = serde_json::from_value(json_value).map_err(|e| {
        ReplanAdvisorError::BadJson(format!("not a valid PlanPatch: {e}"))
    })?;
    let _ = json!(()); // silence unused-import path
    validate_patch_against_plan(plan, patch)
}

/// T6.1 — `ReplanAdvisor` impl that wraps an `LlmProvider`. Apps
/// (theo-cli) construct one of these and hand it to
/// `PilotLoop::with_replan_advisor` so threshold breaches in the
/// pilot loop trigger a real LLM-driven recovery patch.
///
/// `propose` returns `None` (instead of an error) on every failure
/// path (LLM unavailable, response unparseable, patch invalid)
/// because the trait contract is "give me a patch you're confident
/// in, OR fall back". The pilot logs a manual-replan hint when
/// `None` is returned so the operator still sees the breach.
pub struct LlmReplanAdvisor {
    llm: std::sync::Arc<dyn LlmProvider>,
}

impl LlmReplanAdvisor {
    pub fn new(llm: std::sync::Arc<dyn LlmProvider>) -> Self {
        Self { llm }
    }
}

#[async_trait]
impl ReplanAdvisor for LlmReplanAdvisor {
    async fn propose(
        &self,
        plan: &Plan,
        failed_task_id: PlanTaskId,
        failure_summary: &str,
    ) -> Option<PlanPatch> {
        match propose_recovery_patch(
            self.llm.clone(),
            plan,
            failed_task_id,
            failure_summary,
        )
        .await
        {
            Ok(patch) => Some(patch),
            Err(e) => {
                eprintln!(
                    "[theo:replan_advisor] LLM-driven proposal failed (falling \
                     back to manual replan): {e}"
                );
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use async_trait::async_trait;
    use theo_domain::identifiers::{PhaseId, PlanTaskId};
    use theo_domain::plan::{PhaseStatus, Plan as DomainPlan, PlanTask, PlanTaskStatus};
    use theo_infra_llm::error::LlmError;
    use theo_infra_llm::stream::SseStream;
    use theo_infra_llm::types::{ChatResponse, Choice, ChoiceMessage, Role};

    /// Minimal fake provider — returns a canned response for `chat`.
    /// `chat_stream` is unimplemented (advisor doesn't use streaming).
    struct FakeLlm {
        model: String,
        canned: Result<String, String>,
    }

    impl FakeLlm {
        fn ok(text: &str) -> Arc<dyn LlmProvider> {
            Arc::new(Self {
                model: "fake-model".into(),
                canned: Ok(text.into()),
            })
        }

        fn err(msg: &str) -> Arc<dyn LlmProvider> {
            Arc::new(Self {
                model: "fake-model".into(),
                canned: Err(msg.into()),
            })
        }
    }

    #[async_trait]
    impl LlmProvider for FakeLlm {
        async fn chat(&self, _req: &ChatRequest) -> Result<ChatResponse, LlmError> {
            match &self.canned {
                Ok(text) => Ok(ChatResponse {
                    id: None,
                    choices: vec![Choice {
                        index: 0,
                        message: ChoiceMessage {
                            role: Role::Assistant,
                            content: Some(text.clone()),
                            tool_calls: None,
                        },
                        finish_reason: Some("stop".into()),
                    }],
                    usage: None,
                }),
                Err(msg) => Err(LlmError::Api {
                    status: 500,
                    message: msg.clone(),
                }),
            }
        }
        async fn chat_stream(
            &self,
            _req: &ChatRequest,
        ) -> Result<SseStream, LlmError> {
            unimplemented!("advisor doesn't stream")
        }
        fn model(&self) -> &str {
            &self.model
        }
        fn provider_id(&self) -> &str {
            "fake"
        }
    }

    fn small_plan_with_failed_task() -> DomainPlan {
        let task = PlanTask {
            id: PlanTaskId(1),
            title: "Refactor auth module".into(),
            status: PlanTaskStatus::Pending,
            files: vec!["src/auth.rs".into()],
            description: "Split into smaller modules".into(),
            dod: "All tests pass".into(),
            depends_on: vec![],
            rationale: String::new(),
            outcome: Some("Tests still failing after 3 attempts".into()),
            assignee: None,
            failure_count: 3,
        };
        DomainPlan {
            version: 1,
            title: "Test plan".into(),
            goal: "Refactor and ship".into(),
            current_phase: PhaseId(1),
            phases: vec![theo_domain::plan::Phase {
                id: PhaseId(1),
                title: "Phase 1".into(),
                status: PhaseStatus::InProgress,
                tasks: vec![task],
            }],
            decisions: vec![],
            created_at: 0,
            updated_at: 0,
            version_counter: 0,
        }
    }

    // ── prompt builder ────────────────────────────────────────────

    #[test]
    fn t61adv_prompt_includes_plan_goal_and_task_details() {
        let plan = small_plan_with_failed_task();
        let task = plan.all_tasks().into_iter().next().unwrap().clone();
        let prompt = build_replan_prompt(&plan, &task, "compilation failed");
        assert!(prompt.contains("Refactor and ship")); // plan.goal
        assert!(prompt.contains("Refactor auth module")); // task.title
        assert!(prompt.contains("All tests pass")); // task.dod
        assert!(prompt.contains("compilation failed")); // failure summary
        assert!(prompt.contains("failures=3")); // failure_count
    }

    #[test]
    fn t61adv_prompt_lists_all_patch_kinds() {
        let plan = small_plan_with_failed_task();
        let task = plan.all_tasks().into_iter().next().unwrap().clone();
        let prompt = build_replan_prompt(&plan, &task, "x");
        for kind in ["skip_task", "edit_task", "reorder_deps", "remove_task", "add_task"]
        {
            assert!(
                prompt.contains(kind),
                "prompt must mention `{kind}` patch kind"
            );
        }
    }

    #[test]
    fn t61adv_prompt_handles_empty_summary_with_explanatory_text() {
        let plan = small_plan_with_failed_task();
        let task = plan.all_tasks().into_iter().next().unwrap().clone();
        let prompt = build_replan_prompt(&plan, &task, "   ");
        // Empty summary triggers a placeholder so the LLM sees SOMETHING.
        assert!(prompt.contains("no summary provided"));
    }

    // ── JSON extraction ───────────────────────────────────────────

    #[test]
    fn t61adv_extract_json_handles_clean_object() {
        let v = extract_json_object(r#"{"kind":"skip_task","id":1,"rationale":"x"}"#)
            .unwrap();
        assert_eq!(v["kind"], "skip_task");
    }

    #[test]
    fn t61adv_extract_json_strips_json_fence() {
        let text = "```json\n{\"kind\":\"skip_task\",\"id\":1,\"rationale\":\"x\"}\n```";
        let v = extract_json_object(text).unwrap();
        assert_eq!(v["id"], 1);
    }

    #[test]
    fn t61adv_extract_json_strips_bare_fence() {
        let text = "```\n{\"a\":1}\n```";
        let v = extract_json_object(text).unwrap();
        assert_eq!(v["a"], 1);
    }

    #[test]
    fn t61adv_extract_json_finds_object_in_prose() {
        let text =
            "Here is my proposal:\n\n{\"kind\":\"skip_task\",\"id\":2,\"rationale\":\"out of scope\"}\n\nHope this helps!";
        let v = extract_json_object(text).unwrap();
        assert_eq!(v["id"], 2);
    }

    #[test]
    fn t61adv_extract_json_handles_nested_braces_inside_string() {
        // The brace-counter must not get confused by `{` characters
        // inside string literals.
        let text = r#"{"kind":"edit_task","id":1,"edits":{"description":"contains {brace} chars"}}"#;
        let v = extract_json_object(text).unwrap();
        assert_eq!(v["kind"], "edit_task");
    }

    #[test]
    fn t61adv_extract_json_returns_bad_json_on_no_object() {
        let err = extract_json_object("just prose, no JSON anywhere").unwrap_err();
        assert!(matches!(err, ReplanAdvisorError::BadJson(_)));
    }

    // ── patch validation ──────────────────────────────────────────

    #[test]
    fn t61adv_validate_patch_skip_task_succeeds_for_existing_task() {
        let plan = small_plan_with_failed_task();
        let patch = PlanPatch::SkipTask {
            id: PlanTaskId(1),
            rationale: "third-party API removed".into(),
        };
        let validated = validate_patch_against_plan(&plan, patch.clone()).unwrap();
        assert_eq!(validated, patch);
    }

    #[test]
    fn t61adv_validate_patch_for_unknown_task_returns_invalid() {
        let plan = small_plan_with_failed_task();
        let patch = PlanPatch::SkipTask {
            id: PlanTaskId(999),
            rationale: "x".into(),
        };
        let err = validate_patch_against_plan(&plan, patch).unwrap_err();
        assert!(matches!(err, ReplanAdvisorError::PatchInvalid(_)));
    }

    // ── end-to-end with fake LLM ──────────────────────────────────

    #[tokio::test]
    async fn t61adv_propose_succeeds_with_well_formed_skip_task() {
        let plan = small_plan_with_failed_task();
        let llm = FakeLlm::ok(
            r#"{"kind":"skip_task","id":1,"rationale":"unrecoverable"}"#,
        );
        let patch = propose_recovery_patch(llm, &plan, PlanTaskId(1), "summary")
            .await
            .unwrap();
        match patch {
            PlanPatch::SkipTask { id, rationale } => {
                assert_eq!(id, PlanTaskId(1));
                assert_eq!(rationale, "unrecoverable");
            }
            other => panic!("expected SkipTask, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t61adv_propose_handles_fenced_response() {
        let plan = small_plan_with_failed_task();
        let llm = FakeLlm::ok(
            "Sure — here is a patch:\n```json\n{\"kind\":\"skip_task\",\"id\":1,\"rationale\":\"x\"}\n```",
        );
        let patch = propose_recovery_patch(llm, &plan, PlanTaskId(1), "summary")
            .await
            .unwrap();
        assert!(matches!(patch, PlanPatch::SkipTask { .. }));
    }

    #[tokio::test]
    async fn t61adv_propose_returns_task_not_found_for_unknown_task_id() {
        let plan = small_plan_with_failed_task();
        let llm = FakeLlm::ok(r#"{"kind":"skip_task","id":1,"rationale":"x"}"#);
        let err = propose_recovery_patch(llm, &plan, PlanTaskId(999), "x")
            .await
            .unwrap_err();
        match err {
            ReplanAdvisorError::TaskNotFound(id) => assert_eq!(id, 999),
            other => panic!("expected TaskNotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t61adv_propose_surfaces_llm_errors_typed() {
        let plan = small_plan_with_failed_task();
        let llm = FakeLlm::err("rate limited");
        let err = propose_recovery_patch(llm, &plan, PlanTaskId(1), "x")
            .await
            .unwrap_err();
        match err {
            ReplanAdvisorError::Llm(msg) => assert!(msg.contains("rate limited")),
            other => panic!("expected Llm error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t61adv_propose_returns_empty_response_when_content_blank() {
        let plan = small_plan_with_failed_task();
        let llm = FakeLlm::ok("   \n\n  ");
        let err = propose_recovery_patch(llm, &plan, PlanTaskId(1), "x")
            .await
            .unwrap_err();
        assert!(matches!(err, ReplanAdvisorError::EmptyResponse));
    }

    #[tokio::test]
    async fn t61adv_propose_returns_bad_json_on_unparseable_response() {
        let plan = small_plan_with_failed_task();
        let llm = FakeLlm::ok("definitely not json");
        let err = propose_recovery_patch(llm, &plan, PlanTaskId(1), "x")
            .await
            .unwrap_err();
        assert!(matches!(err, ReplanAdvisorError::BadJson(_)));
    }

    #[tokio::test]
    async fn t61adv_propose_rejects_patch_that_fails_validation() {
        // LLM hallucinates a non-existent task id.
        let plan = small_plan_with_failed_task();
        let llm = FakeLlm::ok(r#"{"kind":"skip_task","id":999,"rationale":"x"}"#);
        let err = propose_recovery_patch(llm, &plan, PlanTaskId(1), "x")
            .await
            .unwrap_err();
        assert!(matches!(err, ReplanAdvisorError::PatchInvalid(_)));
    }

    // ── LlmReplanAdvisor (the trait wrapper used by the pilot) ────

    #[tokio::test]
    async fn t61trait_llm_advisor_returns_some_on_well_formed_patch() {
        let plan = small_plan_with_failed_task();
        let llm = FakeLlm::ok(
            r#"{"kind":"skip_task","id":1,"rationale":"unrecoverable"}"#,
        );
        let advisor = LlmReplanAdvisor::new(llm);
        let result = advisor.propose(&plan, PlanTaskId(1), "context").await;
        match result {
            Some(PlanPatch::SkipTask { id, rationale }) => {
                assert_eq!(id, PlanTaskId(1));
                assert_eq!(rationale, "unrecoverable");
            }
            other => panic!("expected Some(SkipTask), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t61trait_llm_advisor_returns_none_on_llm_error() {
        // Trait contract: propose returns Option, NOT Result. Every
        // failure mode collapses to None so the pilot's fallback
        // (log + manual replan hint) fires reliably.
        let plan = small_plan_with_failed_task();
        let llm = FakeLlm::err("rate limited");
        let advisor = LlmReplanAdvisor::new(llm);
        let result = advisor.propose(&plan, PlanTaskId(1), "x").await;
        assert!(result.is_none(), "LLM error must collapse to None");
    }

    #[tokio::test]
    async fn t61trait_llm_advisor_returns_none_on_unparseable_response() {
        let plan = small_plan_with_failed_task();
        let llm = FakeLlm::ok("definitely not json");
        let advisor = LlmReplanAdvisor::new(llm);
        let result = advisor.propose(&plan, PlanTaskId(1), "x").await;
        assert!(result.is_none(), "unparseable response must collapse to None");
    }

    #[tokio::test]
    async fn t61trait_llm_advisor_returns_none_on_invalid_patch() {
        // LLM hallucinates a task id that doesn't exist; advisor
        // catches via validate_patch_against_plan and returns None.
        let plan = small_plan_with_failed_task();
        let llm = FakeLlm::ok(r#"{"kind":"skip_task","id":999,"rationale":"x"}"#);
        let advisor = LlmReplanAdvisor::new(llm);
        let result = advisor.propose(&plan, PlanTaskId(1), "x").await;
        assert!(result.is_none(), "invalid patch must collapse to None");
    }

    #[tokio::test]
    async fn t61trait_llm_advisor_returns_none_on_unknown_failed_task_id() {
        // Caller asks the advisor to recover a task that doesn't
        // exist. The wrapper silently downgrades to None so the
        // pilot loop doesn't crash.
        let plan = small_plan_with_failed_task();
        let llm = FakeLlm::ok(r#"{"kind":"skip_task","id":1,"rationale":"x"}"#);
        let advisor = LlmReplanAdvisor::new(llm);
        let result = advisor.propose(&plan, PlanTaskId(999), "x").await;
        assert!(result.is_none(), "unknown task_id must collapse to None");
    }
}
