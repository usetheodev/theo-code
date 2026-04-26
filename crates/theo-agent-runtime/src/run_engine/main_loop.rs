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
use theo_infra_llm::types::Message;

use super::AgentRunEngine;
use crate::agent_loop::AgentResult;

// `choose_model`, `call_llm_with_retry`, `build_llm_abort_result`, and
// `handle_context_overflow` moved to `run_engine/llm_call.rs` (T4.* —
// production-LOC trim toward the 500-line per-file target).

impl AgentRunEngine {

    /// Persist the current run snapshot (if a snapshot store is
    /// attached). Collects tool calls + results + events + messages
    /// into `RunSnapshot` and forwards to the store. Fail-soft — any
    /// store error is swallowed (shutdown path is best-effort per
    /// Invariant 7).
    pub(super) async fn persist_snapshot_if_configured(&self, messages: &[Message]) {
        let Some(ref store) = self.rt.snapshot_store else {
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
            self.llm.budget_enforcer.usage(),
            messages_json,
            vec![], // DLQ entries
        );
        snapshot.working_set = Some(self.obs.working_set.clone());
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
        let tool_args = if let Some(tool) = self.llm.registry.get(name) {
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
            graph_context: self.rt.graph_context.clone(),
            stdout_tx: None,
        };

        // 5. Dispatch + await completion.
        let tool_result = self
            .tool_call_manager
            .dispatch_and_execute(&tool_call_id, &self.llm.registry, &ctx)
            .await;

        let (success, output) = match &tool_result {
            Ok(r) => (r.status == ToolCallState::Succeeded, r.output.clone()),
            Err(e) => (false, format!("Tool call error: {}", e)),
        };

        // 6. Budget + metrics accounting.
        self.llm.budget_enforcer.record_tool_call();
        self.obs.metrics.record_tool_call(name, 0, success);

        // 7. Failure-pattern tracker: on repeated failures, surface a
        // user-directed suggestion as a steering message.
        if !success {
            let pattern = format!("{}_failure", name);
            if let Some(suggestion) = self.tracking.failure_tracker.record_and_check(&pattern) {
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
        let Some(ref steering_fn) = self.rt.message_queues.steering else {
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
        let threshold = self.config.loop_cfg().doom_loop_threshold.unwrap_or(3);
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
                "threshold": self.config.loop_cfg().doom_loop_threshold,
            }),
        ));
        messages.push(Message::user(&warning));

        // Hard abort after 2x threshold (warning wasn't enough).
        if !tracker.should_abort() {
            return None;
        }
        self.transition_run(RunState::Aborted);
        self.try_task_transition(TaskState::Failed);
        self.obs.metrics.record_run_complete(false);
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
        let Some(ref ctx) = self.rt.resume_context else {
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
        if self.config.loop_cfg().mode != crate::config::AgentMode::Plan {
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
        self.llm.budget_enforcer.record_iteration();
        let Err(violation) = self.llm.budget_enforcer.check() else {
            return None;
        };
        self.transition_run(RunState::Aborted);
        self.try_task_transition(TaskState::Failed);

        let summary = format!(
            "Budget exceeded: {}. Edits succeeded: {}. Files: {}",
            violation,
            self.rt.context_loop_state.edits_succeeded,
            self.rt.context_loop_state.edits_files.join(", ")
        );
        self.obs.metrics.record_run_complete(false);
        Some(AgentResult::from_engine_state(
            self,
            false,
            summary,
            false,
            ErrorClass::Exhausted,
        ))
    }

}
