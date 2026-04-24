//! Main-loop side helpers — `AgentRunEngine` methods that support
//! the `for iteration { ... }` body without owning it yet.
//!
//! Fase 4 (REMEDIATION_PLAN T4.2). Start of the main-loop extraction.
//! This module currently hosts:
//!   - `choose_model` — routing decision + panic-safe fallback
//!   - `handle_context_overflow` — emergency compaction + event
//!
//! Future iterations will move the full loop body here, decomposed
//! into `prepare_iteration`, `call_llm`, `dispatch_tools`, and
//! `finalize_iteration` methods.

use theo_domain::agent_run::RunState;
use theo_domain::error_class::ErrorClass;
use theo_domain::event::{DomainEvent, EventType};
use theo_domain::task::TaskState;
use theo_infra_llm::LlmError;
use theo_infra_llm::types::{ChatRequest, ChatResponse, Message};

use super::AgentRunEngine;
use super::dispatch::DispatchOutcome;
use crate::agent_loop::AgentResult;
use crate::run_engine_helpers::{derive_provider_hint, llm_error_to_class};

/// Routing decision: `(chosen_model, chosen_effort, routing_reason)`.
pub(super) type RoutingDecision = (String, Option<String>, &'static str);

impl AgentRunEngine {
    /// Consult the configured router (if any) to pick the model and
    /// reasoning effort for this turn. Panic-safe: if the router's
    /// `route()` panics, falls back to the session defaults and
    /// tags the decision as `"router_panic_fallback_default"`.
    ///
    /// The router is expected to be called exactly once per turn at
    /// the single site inside the main loop — this method preserves
    /// that invariant.
    pub(super) fn choose_model(
        &self,
        messages: &[Message],
        iteration: usize,
        estimated_context_tokens: u64,
        requires_tool_use: bool,
    ) -> RoutingDecision {
        let latest_user_msg = messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, theo_infra_llm::types::Role::User))
            .and_then(|m| m.content.as_deref());
        let mut routing_ctx = theo_domain::routing::RoutingContext::new(
            theo_domain::routing::RoutingPhase::Normal,
        );
        routing_ctx.latest_user_message = latest_user_msg;
        routing_ctx.conversation_tokens = estimated_context_tokens;
        routing_ctx.iteration = iteration;
        routing_ctx.requires_tool_use = requires_tool_use;

        match &self.config.router {
            Some(handle) => {
                let choice = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    handle.as_router().route(&routing_ctx)
                }));
                match choice {
                    Ok(c) => (c.model_id, c.reasoning_effort, c.routing_reason),
                    Err(_) => (
                        self.config.model.clone(),
                        self.config.reasoning_effort.clone(),
                        "router_panic_fallback_default",
                    ),
                }
            }
            None => (
                self.config.model.clone(),
                self.config.reasoning_effort.clone(),
                "no_router",
            ),
        }
    }

    /// Drive a full LLM call for the current turn: publish
    /// `LlmCallStart`, run the streaming retry loop, and on success
    /// accumulate token usage + publish `LlmCallEnd`. Streaming deltas
    /// (`Reasoning`/`Content`) flow to the event bus as they arrive.
    ///
    /// Returns the raw `ChatResponse` on success. Errors (including
    /// `LlmError::ContextOverflow`) are returned as-is — the caller
    /// inside the main loop decides whether to recover (overflow) or
    /// build an abort result.
    pub(super) async fn call_llm_with_retry(
        &mut self,
        request: &ChatRequest,
        chosen_model: &str,
        iteration: usize,
        routing_reason: &str,
        estimated_context_tokens: usize,
    ) -> Result<ChatResponse, LlmError> {
        // Publish LlmCallStart (triggers "Thinking..." in CLI).
        // OTel payload lets OtelExportingListener build a
        // `gen_ai.*`-attributed span.
        let provider_hint = derive_provider_hint(&self.config.base_url);
        let llm_start_span =
            crate::observability::otel::llm_call_span(provider_hint, chosen_model);
        self.event_bus.publish(DomainEvent::new(
            EventType::LlmCallStart,
            self.run.run_id.as_str(),
            serde_json::json!({
                "iteration": iteration,
                "routing_reason": routing_reason,
                "model": chosen_model,
                "otel": llm_start_span.to_json(),
            }),
        ));

        let llm_start = std::time::Instant::now();

        // Retry loop — delegated to `RetryExecutor::with_retry`. The
        // streaming callback still forwards deltas to the bus per attempt;
        // it's rebuilt inside each invocation so the bus/run-id captures
        // survive the FnMut requirement of `with_retry`.
        let retry_policy = if self.config.aggressive_retry {
            theo_domain::retry_policy::RetryPolicy::benchmark()
        } else {
            theo_domain::retry_policy::RetryPolicy::default_llm()
        };
        let run_id_str = self.run.run_id.as_str().to_string();
        let event_bus = self.event_bus.clone();
        let client = &self.client;

        let resp = crate::retry::RetryExecutor::with_retry(
            &retry_policy,
            &run_id_str,
            &event_bus,
            || {
                let eb = event_bus.clone();
                let rid = run_id_str.clone();
                async move {
                    client
                        .chat_streaming(request, |delta| match delta {
                            theo_infra_llm::stream::StreamDelta::Reasoning(text) => {
                                eb.publish(DomainEvent::new(
                                    EventType::ReasoningDelta,
                                    &rid,
                                    serde_json::json!({ "text": text }),
                                ));
                            }
                            theo_infra_llm::stream::StreamDelta::Content(text) => {
                                eb.publish(DomainEvent::new(
                                    EventType::ContentDelta,
                                    &rid,
                                    serde_json::json!({ "text": text }),
                                ));
                            }
                            _ => {}
                        })
                        .await
                }
            },
            LlmError::is_retryable,
        )
        .await?;

        // Success path: accumulate usage + publish LlmCallEnd.
        let llm_duration = llm_start.elapsed().as_millis() as u64;
        let input_tok = resp
            .usage
            .as_ref()
            .map(|u| u.prompt_tokens as u64)
            .unwrap_or(0);
        let output_tok = resp
            .usage
            .as_ref()
            .map(|u| u.completion_tokens as u64)
            .unwrap_or(0);
        let total_tok = input_tok + output_tok;
        self.budget_enforcer.record_tokens(total_tok);
        self.metrics
            .record_llm_call_detailed(llm_duration, input_tok, output_tok);
        self.session_token_usage
            .accumulate(&theo_domain::budget::TokenUsage {
                input_tokens: input_tok,
                output_tokens: output_tok,
                ..Default::default()
            });

        let mut llm_end_span = crate::observability::otel::llm_call_span(
            derive_provider_hint(&self.config.base_url),
            chosen_model,
        );
        llm_end_span.set(
            crate::observability::otel::ATTR_USAGE_INPUT_TOKENS,
            input_tok,
        );
        llm_end_span.set(
            crate::observability::otel::ATTR_USAGE_OUTPUT_TOKENS,
            output_tok,
        );
        llm_end_span.set(
            crate::observability::otel::ATTR_USAGE_TOTAL_TOKENS,
            total_tok,
        );
        llm_end_span.set(
            crate::observability::otel::ATTR_THEO_DURATION_MS,
            llm_duration,
        );
        self.event_bus.publish(DomainEvent::new(
            EventType::LlmCallEnd,
            self.run.run_id.as_str(),
            serde_json::json!({
                "iteration": iteration,
                "duration_ms": llm_duration,
                "input_tokens": input_tok,
                "output_tokens": output_tok,
                "total_tokens": total_tok,
                "context_tokens": estimated_context_tokens,
                "otel": llm_end_span.to_json(),
            }),
        ));

        Ok(resp)
    }

    /// Persist the current run snapshot (if a snapshot store is
    /// attached). Collects tool calls + results + events + messages
    /// into `RunSnapshot` and forwards to the store. Fail-soft — any
    /// store error is swallowed (shutdown path is best-effort per
    /// Invariant 7).
    pub(super) async fn persist_snapshot_if_configured(&self, messages: &[Message]) {
        let Some(ref store) = self.snapshot_store else {
            return;
        };
        let Some(task) = self.task_manager.get(&self.task_id) else {
            return;
        };
        let tool_calls = self.tool_call_manager.calls_for_task(&self.task_id);
        let tool_results: Vec<theo_domain::tool_call::ToolResultRecord> = tool_calls
            .iter()
            .filter_map(|tc| self.tool_call_manager.get_result(&tc.call_id))
            .collect();
        let messages_json: Vec<serde_json::Value> = messages
            .iter()
            .filter_map(|m| serde_json::to_value(m).ok())
            .collect();
        let mut snapshot = crate::snapshot::RunSnapshot::new(
            self.run.clone(),
            task,
            tool_calls,
            tool_results,
            self.event_bus.events(),
            self.budget_enforcer.usage(),
            messages_json,
            vec![], // DLQ entries
        );
        snapshot.working_set = Some(self.working_set.clone());
        snapshot.checksum = snapshot.compute_checksum();
        let _ = store.save(&self.run.run_id, &snapshot).await;
    }

    /// Execute a non-meta tool call end-to-end: parse args →
    /// prepare_arguments → enqueue → dispatch → budget/metrics record →
    /// failure-pattern tracker. Returns `Some((success, output))` when
    /// the tool ran (even on tool-level failure); `None` when the
    /// arguments failed to parse — in that case a tool_result explaining
    /// the parse error has already been pushed and the caller should
    /// `continue` the main loop.
    pub(super) async fn execute_regular_tool_call(
        &mut self,
        call: &theo_infra_llm::types::ToolCall,
        iteration: usize,
        abort_rx: &tokio::sync::watch::Receiver<bool>,
        messages: &mut Vec<Message>,
    ) -> Option<(bool, String)> {
        use theo_domain::session::{MessageId, SessionId};
        use theo_domain::tool::ToolContext;
        use theo_domain::tool_call::ToolCallState;

        let name = &call.function.name;

        // 1. Parse args. On failure, report to the LLM and signal
        // caller to continue.
        let tool_args = match call.parse_arguments() {
            Ok(args) => args,
            Err(e) => {
                messages.push(Message::tool_result(
                    &call.id,
                    name,
                    format!("Failed to parse arguments: {e}. Please retry with valid JSON."),
                ));
                return None;
            }
        };

        // 2. Apply the tool's `prepare_arguments` hook
        // (normalizes/migrates args before schema validation).
        let tool_args = if let Some(tool) = self.registry.get(name) {
            tool.prepare_arguments(tool_args)
        } else {
            tool_args
        };

        // 3. Enqueue in ToolCallManager (Invariants 2, 3, 5).
        let tool_call_id =
            self.tool_call_manager
                .enqueue(self.task_id.clone(), name.clone(), tool_args);

        // 4. Build the ToolContext for dispatch.
        let ctx = ToolContext {
            session_id: SessionId::new("agent"),
            message_id: MessageId::new(format!("iter_{iteration}")),
            call_id: call.id.clone(),
            agent: "main".to_string(),
            abort: abort_rx.clone(),
            project_dir: self.project_dir.clone(),
            graph_context: self.graph_context.clone(),
            stdout_tx: None,
        };

        // 5. Dispatch + await completion.
        let tool_result = self
            .tool_call_manager
            .dispatch_and_execute(&tool_call_id, &self.registry, &ctx)
            .await;

        let (success, output) = match &tool_result {
            Ok(r) => (r.status == ToolCallState::Succeeded, r.output.clone()),
            Err(e) => (false, format!("Tool call error: {}", e)),
        };

        // 6. Budget + metrics accounting.
        self.budget_enforcer.record_tool_call();
        self.metrics.record_tool_call(name, 0, success);

        // 7. Failure-pattern tracker: on repeated failures, surface a
        // user-directed suggestion as a steering message.
        if !success {
            let pattern = format!("{}_failure", name);
            if let Some(suggestion) = self.failure_tracker.record_and_check(&pattern) {
                messages.push(Message::user(&suggestion));
            }
        }

        Some((success, output))
    }

    /// Emit `RunStateChanged` marking a pre-mutation checkpoint snapshot.
    /// No-op when no checkpoint manager is attached (via
    /// `maybe_checkpoint_for_tool` returning None).
    pub(super) fn emit_checkpoint_event_for_tool(
        &self,
        name: &str,
        iteration: usize,
    ) {
        let Some(sha) = self.maybe_checkpoint_for_tool(name, iteration as u32) else {
            return;
        };
        self.event_bus.publish(DomainEvent::new(
            EventType::RunStateChanged,
            self.run.run_id.as_str(),
            serde_json::json!({
                "from": "Executing",
                "to": format!(
                    "Checkpoint:{}:turn-{}",
                    &sha[..sha.len().min(12)],
                    iteration
                ),
            }),
        ));
    }

    /// Fire the computational-verification sensor for a successful
    /// write-class tool call (edit / write / apply_patch / bash).
    /// Extracts the target `filePath` and invokes
    /// `sensor_runner.fire(name, path, project_dir)`.
    pub(super) fn fire_sensor_for_write_tool(
        &self,
        sensor_runner: Option<&crate::sensor::SensorRunner>,
        call: &theo_infra_llm::types::ToolCall,
        name: &str,
        success: bool,
    ) {
        if !success || !crate::sensor::is_write_tool(name) {
            return;
        }
        let Some(sensor_runner) = sensor_runner else {
            return;
        };
        let file_path = call
            .parse_arguments()
            .ok()
            .and_then(|args| {
                args.get("filePath")
                    .or(args.get("file_path"))
                    .and_then(|p| p.as_str())
                    .map(String::from)
            })
            .unwrap_or_default();
        if !file_path.is_empty() {
            sensor_runner.fire(name, &file_path, &self.project_dir);
        }
    }

    /// Drain the steering message queue (if configured) and inject the
    /// messages as user turns before the next LLM call.
    pub(super) async fn drain_steering_queue(&self, messages: &mut Vec<Message>) {
        let Some(ref steering_fn) = self.message_queues.steering else {
            return;
        };
        let steering_msgs = steering_fn().await;
        for msg in steering_msgs {
            messages.push(msg);
        }
    }

    /// Run the `tool.before` hook (if a hook runner is attached) for
    /// the given call. Returns `true` when the hook *blocks* dispatch
    /// (caller pushes the BLOCKED message and `continue`s). Returns
    /// `false` otherwise — dispatch proceeds.
    pub(super) async fn run_pre_tool_hook(
        &self,
        runner: Option<&crate::hooks::HookRunner>,
        call: &theo_infra_llm::types::ToolCall,
        messages: &mut Vec<Message>,
    ) -> bool {
        let Some(runner) = runner else {
            return false;
        };
        let hook_args = call.parse_arguments().unwrap_or_default();
        let event = crate::hooks::tool_hook_event(
            "tool.before",
            &call.function.name,
            &hook_args,
            &self.project_dir,
        );
        let hook_result = runner.run_pre_hook("tool.before", &event).await;
        if hook_result.allowed {
            return false;
        }
        messages.push(Message::tool_result(
            &call.id,
            &call.function.name,
            format!("BLOCKED by hook: {}", hook_result.output.trim()),
        ));
        true
    }

    /// Run the `tool.after` hook (if a hook runner is attached). Purely
    /// informational — return value is not used by the caller.
    pub(super) async fn run_post_tool_hook(
        &self,
        runner: Option<&crate::hooks::HookRunner>,
        call: &theo_infra_llm::types::ToolCall,
    ) {
        let Some(runner) = runner else {
            return;
        };
        let hook_args = call.parse_arguments().unwrap_or_default();
        let event = crate::hooks::tool_hook_event(
            "tool.after",
            &call.function.name,
            &hook_args,
            &self.project_dir,
        );
        runner.run_post_hook("tool.after", &event).await;
    }

    /// Feed the doom-loop tracker with the current call and return
    /// `Some(result)` when a hard abort is warranted (2× threshold
    /// consecutive identical calls). Non-warning cases return `None`.
    /// When the tracker issues a soft warning, a user-directed nudge
    /// is pushed into `messages` and `None` is returned.
    pub(super) fn update_doom_tracker(
        &mut self,
        doom_tracker: Option<&mut crate::doom_loop::DoomLoopTracker>,
        call: &theo_infra_llm::types::ToolCall,
        name: &str,
        messages: &mut Vec<Message>,
    ) -> Option<AgentResult> {
        let tracker = doom_tracker?;
        let args = call.parse_arguments().unwrap_or_default();
        if !tracker.record(name, &args) {
            return None;
        }
        let threshold = self.config.doom_loop_threshold.unwrap_or(3);
        let warning = format!(
            "⚠️ DOOM LOOP DETECTED: You have called '{}' with identical arguments {} times in a row. \
             You are stuck in a loop. Try a DIFFERENT approach or tool.",
            name, threshold
        );
        self.event_bus.publish(DomainEvent::new(
            EventType::Error,
            self.run.run_id.as_str(),
            serde_json::json!({
                "type": "doom_loop",
                "tool_name": name,
                "threshold": self.config.doom_loop_threshold,
            }),
        ));
        messages.push(Message::user(&warning));

        // Hard abort after 2x threshold (warning wasn't enough).
        if !tracker.should_abort() {
            return None;
        }
        self.transition_run(RunState::Aborted);
        let _ = self
            .task_manager
            .transition(&self.task_id, TaskState::Failed);
        self.metrics.record_run_complete(false);
        let summary = format!(
            "Doom loop abort: '{}' called identically {} times. Agent is stuck.",
            name,
            threshold * 2
        );
        Some(AgentResult::from_engine_state(
            self,
            false,
            summary,
            false,
            ErrorClass::Aborted,
        ))
    }

    /// Post-dispatch working set + context metrics update. Classifies
    /// the tool call by name (read/edit/write/apply_patch vs grep/glob/
    /// codebase_context) and feeds the usefulness pipeline + action log.
    pub(super) fn update_working_set_post_tool(
        &mut self,
        call: &theo_infra_llm::types::ToolCall,
        name: &str,
        iteration: usize,
    ) {
        match name {
            "read" | "edit" | "write" | "apply_patch" => {
                if let Ok(args) = call.parse_arguments()
                    && let Some(path) = args
                        .get("filePath")
                        .or(args.get("file_path"))
                        .and_then(|p| p.as_str())
                {
                    self.working_set.touch_file(path);
                    self.context_metrics.record_artifact_fetch(path, iteration);
                    // Feed usefulness pipeline — which files the agent
                    // actually references vs just scans.
                    self.context_metrics.record_tool_reference(path);
                }
            }
            "grep" | "glob" | "codebase_context" => {
                if let Ok(args) = call.parse_arguments() {
                    let query = args
                        .get("pattern")
                        .or(args.get("query"))
                        .and_then(|p| p.as_str())
                        .unwrap_or("");
                    self.context_metrics
                        .record_action(&format!("{}: {}", name, query), iteration);
                    if let Some(path) = args.get("path").and_then(|p| p.as_str()) {
                        self.context_metrics.record_tool_reference(path);
                    }
                }
            }
            _ => {}
        }
        self.working_set
            .record_event(format!("tool:{}:iter{}", name, iteration), 20);
    }

    /// Post-dispatch context-loop state update. Records reads, search
    /// actions, and edit attempts — the last branch extracts the
    /// edited file path from `filePath` or from a `+++ b/<file>` line
    /// inside `patchText` (apply_patch case).
    pub(super) fn update_context_loop_post_tool(
        &mut self,
        call: &theo_infra_llm::types::ToolCall,
        name: &str,
        success: bool,
        output: &str,
    ) {
        match name {
            "read" => {
                if let Ok(args) = call.parse_arguments()
                    && let Some(path) = args.get("filePath").and_then(|p| p.as_str())
                {
                    self.context_loop_state.record_read(path);
                }
            }
            "grep" | "glob" => self.context_loop_state.record_search(),
            "edit" | "write" | "apply_patch" => {
                let file = call
                    .parse_arguments()
                    .ok()
                    .and_then(|args| {
                        args.get("filePath")
                            .or(args.get("file_path"))
                            .and_then(|p| p.as_str())
                            .map(String::from)
                            .or_else(|| {
                                args.get("patchText").and_then(|p| p.as_str()).and_then(
                                    |patch| {
                                        patch
                                            .lines()
                                            .find(|l| l.starts_with("+++ "))
                                            .and_then(|l| {
                                                l.strip_prefix("+++ b/")
                                                    .or(l.strip_prefix("+++ "))
                                            })
                                            .filter(|f| *f != "/dev/null")
                                            .map(String::from)
                                    },
                                )
                            })
                    })
                    .unwrap_or_default();
                self.context_loop_state.record_edit_attempt(
                    &file,
                    success,
                    if success { None } else { Some(output.to_string()) },
                );
            }
            _ => {}
        }
    }

    /// Resume replay short-circuit. If the engine is in resume mode
    /// AND the tool call's `call_id` already produced a result in the
    /// original run, push the cached `Message::tool_result`, emit a
    /// `ToolCallCompleted` event tagged with `replayed: true`, and
    /// return `true` (caller `continue`s). Returns `false` when no
    /// replay happened — caller proceeds with normal dispatch.
    pub(super) fn try_replay_tool_call(
        &self,
        call: &theo_infra_llm::types::ToolCall,
        messages: &mut Vec<Message>,
    ) -> bool {
        let Some(ref ctx) = self.resume_context else {
            return false;
        };
        if !ctx.should_skip_tool_call(&call.id) {
            return false;
        }
        let Some(cached) = ctx.cached_tool_result(&call.id) else {
            return false;
        };
        messages.push(cached.clone());
        let name = &call.function.name;
        let mut replay_span = crate::observability::otel::tool_call_span(name);
        replay_span.set(
            crate::observability::otel::ATTR_THEO_TOOL_CALL_ID,
            call.id.clone(),
        );
        replay_span.set(crate::observability::otel::ATTR_THEO_TOOL_STATUS, "Succeeded");
        replay_span.set(crate::observability::otel::ATTR_THEO_TOOL_REPLAYED, true);
        self.event_bus.publish(DomainEvent::new(
            EventType::ToolCallCompleted,
            self.run.run_id.as_str(),
            serde_json::json!({
                "tool_name": name,
                "call_id": &call.id,
                "replayed": true,
                "status": "Succeeded",
                "otel": replay_span.to_json(),
            }),
        ));
        true
    }

    /// Plan-mode guard for non-meta tools: blocks `think` (reasoning
    /// must appear in assistant text) and write-class tools (except
    /// writes under `.theo/plans/`). Returns `true` when the tool
    /// was blocked (caller `continue`s); `false` otherwise.
    pub(super) fn enforce_plan_mode_guard(
        &self,
        call: &theo_infra_llm::types::ToolCall,
        messages: &mut Vec<Message>,
    ) -> bool {
        if self.config.mode != crate::config::AgentMode::Plan {
            return false;
        }
        let name = &call.function.name;
        if name == "think" {
            messages.push(Message::tool_result(
                &call.id,
                name,
                "BLOCKED by Plan mode: The `think` tool is forbidden in plan mode. \
                 Write your reasoning and plan as visible markdown text in your assistant message instead. \
                 The user is reading your messages directly.",
            ));
            return true;
        }
        let is_write_tool = matches!(name.as_str(), "edit" | "write" | "apply_patch");
        if !is_write_tool {
            return false;
        }
        let is_roadmap_write = name == "write"
            && call
                .parse_arguments()
                .ok()
                .and_then(|a| a.get("filePath").and_then(|p| p.as_str()).map(String::from))
                .map(|p| p.contains(".theo/plans/"))
                .unwrap_or(false);
        if is_roadmap_write {
            return false;
        }
        messages.push(Message::tool_result(
            &call.id,
            name,
            "BLOCKED by Plan mode guard: You can only write to .theo/plans/. \
             Write the roadmap first. Source code edits are not allowed until user approves.",
        ));
        true
    }

    /// Check budget at the start of an iteration. Returns `Some(result)`
    /// when the budget is exceeded (caller must `return` it); `None` to
    /// proceed. Publishes the `BudgetExceeded` event via the enforcer
    /// and flips task/run states to Failed/Aborted.
    pub(super) fn check_budget_or_exhausted(&mut self) -> Option<AgentResult> {
        self.budget_enforcer.record_iteration();
        let Err(violation) = self.budget_enforcer.check() else {
            return None;
        };
        self.transition_run(RunState::Aborted);
        let _ = self
            .task_manager
            .transition(&self.task_id, TaskState::Failed);

        let summary = format!(
            "Budget exceeded: {}. Edits succeeded: {}. Files: {}",
            violation,
            self.context_loop_state.edits_succeeded,
            self.context_loop_state.edits_files.join(", ")
        );
        self.metrics.record_run_complete(false);
        Some(AgentResult::from_engine_state(
            self,
            false,
            summary,
            false,
            ErrorClass::Exhausted,
        ))
    }

    /// Drain pending sensor results and push them as system messages so
    /// the next LLM call sees computational-verification feedback (e.g.
    /// clippy / cargo test output). Emits `SensorExecuted` per result.
    pub(super) fn drain_sensor_messages(
        &self,
        sensor_runner: Option<&crate::sensor::SensorRunner>,
        messages: &mut Vec<Message>,
    ) {
        let Some(sensor_runner) = sensor_runner else {
            return;
        };
        for result in sensor_runner.drain_pending() {
            let severity = if result.exit_code == 0 { "OK" } else { "ISSUE" };
            let preview = theo_domain::prompt_sanitizer::char_boundary_truncate(
                &result.output,
                crate::constants::SENSOR_OUTPUT_PREVIEW_BYTES,
            );
            messages.push(Message::system(format!(
                "[SENSOR {severity}] {} (via {}): {preview}",
                result.file_path, result.tool_name
            )));
            self.event_bus.publish(DomainEvent::new(
                EventType::SensorExecuted,
                self.run.run_id.as_str(),
                serde_json::json!({
                    "file": result.file_path,
                    "exit_code": result.exit_code,
                    "output_preview": &preview[..preview.len().min(200)],
                    "duration_ms": result.duration_ms,
                    "tool_name": result.tool_name,
                }),
            ));
        }
    }

    /// Iteration prelude: context-loop injection + compaction. Returns
    /// the post-compaction estimated token count so the caller can pass
    /// it through to the router.
    ///
    /// Ordering (do not change without re-verifying the characterization
    /// snapshots): context-loop nudge → phase transitions → pre-compress
    /// memory hook → staged compaction → metrics record.
    pub(super) async fn inject_context_loop_and_compact(
        &mut self,
        iteration: usize,
        messages: &mut Vec<Message>,
    ) -> usize {
        // Context loop injection — fires every N iterations.
        if iteration > 1
            && iteration.is_multiple_of(self.config.context_loop_interval)
        {
            let task_objective = self
                .task_manager
                .get(&self.task_id)
                .map(|t| t.objective.clone())
                .unwrap_or_default();
            let ctx_msg = self.context_loop_state.build_context_loop(
                iteration,
                self.config.max_iterations,
                &task_objective,
            );
            messages.push(Message::user(ctx_msg));
        }

        // Phase transitions (legacy, preserved for context loop diagnostics).
        self.context_loop_state
            .maybe_transition(iteration, self.config.max_iterations);

        // Compaction: compress history with semantic progress context.
        let compaction_ctx = crate::compaction::CompactionContext {
            task_objective: messages
                .iter()
                .find(|m| m.role == theo_infra_llm::types::Role::User)
                .and_then(|m| m.content.clone())
                .unwrap_or_default()
                .chars()
                .take(100)
                .collect(),
            current_phase: format!("{:?}", self.run.state),
            target_files: self.context_loop_state.edits_files.clone(),
            recent_errors: self
                .context_loop_state
                .edit_failures
                .iter()
                .rev()
                .take(2)
                .cloned()
                .collect(),
        };

        // Pre-compression memory hook (persistence survives truncation).
        crate::memory_lifecycle::run_engine_hooks::pre_compress_push(
            &self.config,
            messages,
        )
        .await;

        crate::compaction_stages::compact_staged_with_policy(
            messages,
            self.config.context_window_tokens,
            Some(&compaction_ctx),
            &self.config.compaction_policy,
        );

        // Record context size for metrics (estimated tokens ≈ chars/4).
        let estimated_context_tokens: usize = messages
            .iter()
            .filter_map(|m| m.content.as_ref())
            .map(|c| c.len().div_ceil(4))
            .sum();
        self.context_metrics
            .record_context_size(iteration, estimated_context_tokens);
        estimated_context_tokens
    }

    /// Handle the "no tool calls" branch of the main loop: the LLM
    /// returned text only and wants to converge. Three sub-flows:
    ///
    ///   1. Follow-up queue drain — if the user queued a message mid-run,
    ///      inject it and continue.
    ///   2. Plan-mode nudge — in Plan mode the agent must end with tool
    ///      calls. If it emitted text without persisting a plan file,
    ///      inject a one-shot corrective reminder and continue.
    ///   3. Converge — sync final turn to memory, fire reviewers nudge,
    ///      transition task to Completed, and return the result.
    ///
    /// Returns:
    ///   - `DispatchOutcome::Continue` for (1) / (2) — caller continues
    ///     the main loop.
    ///   - `DispatchOutcome::Converged(result)` for (3) — caller breaks
    ///     with this result.
    pub(super) async fn handle_text_only_response(
        &mut self,
        content: String,
        messages: &mut Vec<Message>,
    ) -> DispatchOutcome {
        // 1. Follow-up queue drain.
        if let Some(ref follow_up_fn) = self.message_queues.follow_up {
            let follow_ups = follow_up_fn().await;
            if !follow_ups.is_empty() {
                messages.push(Message::assistant(&content));
                for fu_msg in follow_ups {
                    messages.push(fu_msg);
                }
                return DispatchOutcome::Continue;
            }
        }

        // 2. Plan-mode nudge (one-shot).
        if self.config.mode == crate::config::AgentMode::Plan
            && !self.plan_mode_nudged
            && !content.is_empty()
        {
            let plans_dir = self.project_dir.join(".theo/plans");
            let plan_written = plans_dir
                .read_dir()
                .ok()
                .map(|mut it| it.next().is_some())
                .unwrap_or(false);
            if !plan_written {
                self.plan_mode_nudged = true;
                messages.push(Message::assistant(&content));
                messages.push(Message::user(
                    "REMINDER: You wrote a plan as text but did not persist it. \
                     You MUST now call the `write` tool to save the plan to \
                     `.theo/plans/01-<slug>.md` (use a kebab-case slug derived from \
                     the task), then call `done` with a one-line summary. Do this in \
                     your next response. Do not write more prose — just call the tools.",
                ));
                return DispatchOutcome::Continue;
            }
        }

        // 3. Converge — persist, nudge reviewers, transition, return.
        // Persist the user→assistant exchange INLINE (not fire-and-forget) —
        // durability > latency.
        crate::memory_lifecycle::run_engine_hooks::sync_final_turn(
            &self.config,
            messages,
            &content,
        )
        .await;

        // Reviewers nudge. Relaxed is sufficient: the flag is a pure
        // per-task counter with no happens-before dependency on any
        // other load; written and read on the same task inside the
        // serial main loop (T5.4).
        let tool_calls_this_task = self.metrics.snapshot().total_tool_calls as usize;
        let skill_created = self
            .skill_created_this_task
            .load(std::sync::atomic::Ordering::Relaxed);
        crate::memory_lifecycle::maybe_spawn_reviewers(
            &self.config,
            &self.memory_nudge_counter,
            &self.skill_nudge_counter,
            messages,
            tool_calls_this_task,
            skill_created,
        );
        self.skill_created_this_task
            .store(false, std::sync::atomic::Ordering::Relaxed);

        self.transition_run(RunState::Converged);
        let _ = self
            .task_manager
            .transition(&self.task_id, TaskState::Completed);
        self.metrics.record_run_complete(true);
        DispatchOutcome::Converged(AgentResult::from_engine_state(
            self,
            true,
            content,
            true,
            ErrorClass::Solved,
        ))
    }

    /// Build the abort `AgentResult` for a non-retryable LLM failure.
    /// Handles state-machine transitions, metrics bump, and ErrorClass
    /// mapping. The caller `return`s this from the main loop.
    pub(super) fn build_llm_abort_result(&mut self, err: &LlmError) -> AgentResult {
        self.transition_run(RunState::Aborted);
        let _ = self
            .task_manager
            .transition(&self.task_id, TaskState::Failed);
        self.metrics.record_run_complete(false);
        let class = llm_error_to_class(err);
        AgentResult::from_engine_state(
            self,
            false,
            format!("LLM error: {err}"),
            false,
            class,
        )
    }

    /// Reactive context-overflow recovery. Invoked when the LLM call
    /// returned `LlmError::ContextOverflow`. Takes a snapshot of hot
    /// files (FM-6 sensor), emits a `ContextOverflowRecovery` event,
    /// and compacts `messages` to `EMERGENCY_COMPACT_RATIO` of the
    /// context window. The caller is expected to `continue` to the
    /// next main-loop iteration.
    pub(super) fn handle_context_overflow(
        &mut self,
        err: &LlmError,
        messages: &mut Vec<Message>,
    ) {
        // Snapshot hot files BEFORE compaction destroys them (FM-6).
        for f in &self.working_set.hot_files {
            self.pre_compaction_hot_files.insert(f.clone());
        }

        self.event_bus.publish(DomainEvent::new(
            EventType::ContextOverflowRecovery,
            self.run.run_id.as_str(),
            serde_json::json!({
                "error": err.to_string(),
                "action": "emergency_compaction",
                "target_ratio": crate::constants::EMERGENCY_COMPACT_RATIO,
            }),
        ));

        // Emergency compaction: keep only a bounded fraction of context.
        let model_ctx = self.config.context_window_tokens;
        let target =
            (model_ctx as f64 * crate::constants::EMERGENCY_COMPACT_RATIO) as usize;
        let before_len = messages.len();
        crate::compaction::compact_messages_to_target(
            messages,
            target,
            "", // No task objective at this level
        );
        eprintln!(
            "[theo] Context overflow recovery: compacted {} → {} messages (target {})",
            before_len,
            messages.len(),
            target
        );
    }
}
