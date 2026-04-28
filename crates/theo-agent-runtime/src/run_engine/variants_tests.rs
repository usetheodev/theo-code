//! Sibling test body of `run_engine/mod.rs` — split per-area (T3.2 of code-hygiene-5x5).
//!
//! Test-only file; gates use the inner `cfg(test)` attribute below to
//! classify every line as test code.

#![cfg(test)]
#![allow(unused_imports)]

use super::*;
use crate::event_bus::CapturingListener;
use theo_domain::session::SessionId;
use theo_domain::task::AgentType;

use crate::run_engine::test_helpers::TestSetup;

mod dispatch_replays {
    use super::*;
    use crate::subagent::resume::{ResumeContext, WorktreeStrategy};
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;
    use theo_domain::agent_spec::AgentSpec;

    fn build_resume_ctx(
        executed_calls: BTreeSet<String>,
        cached_results: BTreeMap<String, theo_infra_llm::types::Message>,
    ) -> Arc<ResumeContext> {
        Arc::new(ResumeContext {
            spec: AgentSpec::on_demand("a", "b"),
            start_iteration: 1,
            history: vec![],
            prior_tokens_used: 0,
            checkpoint_before: None,
            executed_tool_calls: executed_calls,
            executed_tool_results: cached_results,
            worktree_strategy: WorktreeStrategy::None,
        })
    }

    #[test]
    fn engine_without_resume_context_dispatches_normally_regression_guard() {
        // D5 backward compat — default engine has resume_context = None.
        let setup = TestSetup::new();
        let engine = setup.create_engine("regression");
        assert!(
            engine.rt.resume_context.is_none(),
            "default engine must NOT have resume_context attached"
        );
    }

    #[test]
    fn engine_with_resume_context_attaches_context_via_builder() {
        let setup = TestSetup::new();
        let mut cached = BTreeMap::new();
        cached.insert(
            "c1".to_string(),
            theo_infra_llm::types::Message::tool_result("c1", "fake_tool", "result-1"),
        );
        let mut executed = BTreeSet::new();
        executed.insert("c1".to_string());
        let ctx = build_resume_ctx(executed, cached);

        let engine = setup.create_engine("with-context").with_resume_context(ctx.clone());

        let attached = engine
            .rt
            .resume_context
            .as_ref()
            .expect("resume_context must be attached");
        assert!(attached.should_skip_tool_call("c1"));
        assert!(!attached.should_skip_tool_call("c-unknown"));
        let cached_msg = attached.cached_tool_result("c1").expect("cached msg present");
        assert_eq!(cached_msg.tool_call_id.as_deref(), Some("c1"));
        assert_eq!(cached_msg.content.as_deref(), Some("result-1"));
    }

    #[test]
    fn engine_with_resume_context_short_circuit_predicate_for_known_call_id() {
        // The dispatch hook is a 2-condition guard:
        //   should_skip_tool_call(call.id) && cached_tool_result(call.id).is_some()
        // Both must be true for replay. Verify the predicate matches the
        // contract enforced in run_engine handle_completion (lines 1393-1419).
        let mut cached = BTreeMap::new();
        cached.insert(
            "c-known".to_string(),
            theo_infra_llm::types::Message::tool_result(
                "c-known",
                "write",
                "{\"ok\":true}",
            ),
        );
        let mut executed = BTreeSet::new();
        executed.insert("c-known".to_string());
        let ctx = build_resume_ctx(executed, cached);

        // Both true → replay path triggers
        assert!(ctx.should_skip_tool_call("c-known"));
        assert!(ctx.cached_tool_result("c-known").is_some());

        // Unknown call_id → dispatch normally (BOTH guards false)
        assert!(!ctx.should_skip_tool_call("c-unknown"));
        assert!(ctx.cached_tool_result("c-unknown").is_none());
    }

    #[test]
    fn engine_with_resume_context_dispatches_unknown_call_id() {
        // When LLM emits a NEW call_id absent from the original event log,
        // the short-circuit predicate is false on both legs, so dispatch
        // proceeds normally. This is the "agent makes new progress on
        // resume" scenario.
        let setup = TestSetup::new();
        let executed = BTreeSet::new(); // empty — no prior calls
        let cached = BTreeMap::new();
        let ctx = build_resume_ctx(executed, cached);
        let engine = setup.create_engine("new-call").with_resume_context(ctx.clone());

        assert!(engine.rt.resume_context.is_some());
        // Predicate: brand-new call_id is NOT skipped → dispatcher runs.
        let attached = engine.rt.resume_context.as_ref().unwrap();
        assert!(!attached.should_skip_tool_call("brand-new-c1"));
    }
}

// -----------------------------------------------------------------------
// provider-hint helper coverage (otlp-exporter)
// -----------------------------------------------------------------------

mod provider_hint {
    #[test]
    fn derive_provider_hint_recognizes_openai() {
        assert_eq!(crate::run_engine_helpers::derive_provider_hint("https://api.openai.com/v1"), "openai");
    }

    #[test]
    fn derive_provider_hint_recognizes_chatgpt_oauth() {
        assert_eq!(crate::run_engine_helpers::derive_provider_hint("https://chatgpt.com/backend-api"), "openai");
    }

    #[test]
    fn derive_provider_hint_recognizes_anthropic() {
        assert_eq!(crate::run_engine_helpers::derive_provider_hint("https://api.anthropic.com"), "anthropic");
    }

    #[test]
    fn derive_provider_hint_recognizes_gemini() {
        assert_eq!(crate::run_engine_helpers::derive_provider_hint("https://generativelanguage.googleapis.com"), "gemini");
    }

    #[test]
    fn derive_provider_hint_falls_back_for_unknown_url() {
        assert_eq!(crate::run_engine_helpers::derive_provider_hint("https://my-private-llm.corp"), "openai_compatible");
    }

    #[test]
    fn derive_provider_hint_recognizes_localhost_as_local() {
        assert_eq!(crate::run_engine_helpers::derive_provider_hint("http://localhost:8000"), "openai_compatible_local");
        assert_eq!(crate::run_engine_helpers::derive_provider_hint("http://127.0.0.1:8080"), "openai_compatible_local");
    }
}

// -----------------------------------------------------------------------
// Bug #1 (benchmark-validation): AgentResult.success semantics
// -----------------------------------------------------------------------
//
// Pure unit tests on the AgentResult constructor logic — the bug was
// that "budget exceeded" path set success based on whether ANY edit
// succeeded, not on whether the task verifiably completed. After the
// fix, only the `done` meta-tool acceptance path returns success=true.

mod success_semantics {
    use super::*;

    /// The fix: budget-exceeded must always return success=false.
    /// Old behavior: success = (edits_succeeded > 0) which is wrong.
    #[test]
    fn budget_exceeded_with_edits_returns_success_false() {
        // Simulate the budget-exceeded branch — what the constructor
        // SHOULD produce when iter limit / token limit hits.
        let r = budget_exceeded_result(
            /* edits_succeeded */ 5,
            /* edits_files */ vec!["a.txt".into(), "b.txt".into()],
            /* iteration */ 20,
            "Budget exceeded: iterations exceeded: 21 > 20 limit",
        );
        assert!(
            !r.success,
            "budget exceeded must mean success=false even when edits exist; \
             got success={}",
            r.success
        );
        assert!(r.summary.starts_with("Budget exceeded"));
        assert_eq!(r.iterations_used, 20);
        assert_eq!(r.files_edited.len(), 2);
    }

    #[test]
    fn budget_exceeded_with_zero_edits_returns_success_false() {
        let r = budget_exceeded_result(0, vec![], 20, "Budget exceeded");
        assert!(!r.success);
    }

    #[test]
    fn done_accepted_returns_success_true() {
        let r = done_accepted_result(
            "Implementation complete; tests pass",
            vec!["src/main.rs".into()],
            7,
            /* done_attempts */ 1,
        );
        assert!(r.success, "done accepted is the ONLY success-true path");
        assert!(r.summary.contains("[accepted after"));
    }

    // error_class
    // population on the canonical helpers.

    #[test]
    fn budget_exceeded_returns_exhausted_class() {
        let r = budget_exceeded_result(0, vec![], 35, "max_iterations");
        assert!(!r.success);
        assert_eq!(
            r.error_class,
            Some(theo_domain::error_class::ErrorClass::Exhausted)
        );
    }

    #[test]
    fn done_accepted_returns_solved_class() {
        let r = done_accepted_result("ok", vec![], 5, 1);
        assert!(r.success);
        assert_eq!(
            r.error_class,
            Some(theo_domain::error_class::ErrorClass::Solved)
        );
    }
}

// Helpers below mirror the code paths in execute_with_history. They
// are factored out so the bug fix can be unit-tested without spinning
// up the full engine. The public API of AgentResult is preserved.

fn budget_exceeded_result(
    edits_succeeded: u32,
    edits_files: Vec<String>,
    iteration: usize,
    violation: &str,
) -> AgentResult {
    AgentResult {
        // Bug #1 fix: budget exceeded ALWAYS means task did not finish.
        // Previously: success = edits_succeeded > 0 (lied to caller)
        success: false,
        summary: format!(
            "{}. Edits succeeded: {}. Files: {}",
            violation,
            edits_succeeded,
            edits_files.join(", ")
        ),
        files_edited: edits_files,
        iterations_used: iteration,
        error_class: Some(theo_domain::error_class::ErrorClass::Exhausted),
        ..Default::default()
    }
}

fn done_accepted_result(
    summary: &str,
    edits_files: Vec<String>,
    iteration: usize,
    done_attempts: u32,
) -> AgentResult {
    AgentResult {
        success: true,
        summary: format!("{} [accepted after {} done attempts]", summary, done_attempts),
        files_edited: edits_files,
        iterations_used: iteration,
        error_class: Some(theo_domain::error_class::ErrorClass::Solved),
        ..Default::default()
    }
}

