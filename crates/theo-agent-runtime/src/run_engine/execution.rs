//! The top-level agent run cycle — `execute` / `execute_with_history`.
//! Orchestrates state-machine transitions, LLM calls, tool dispatch, and
//! session persistence.
//!
//! Fase 4 (REMEDIATION_PLAN T4.2). Extracted from `run_engine/mod.rs`.
//! Behavior is byte-identical. Heavy lifting is delegated to helpers in
//! sibling modules (`bootstrap`, `dispatch/*`, `main_loop`, `lifecycle`).

use theo_domain::agent_run::RunState;
use theo_infra_llm::types::{ChatRequest, Message};

use crate::doom_loop::DoomLoopTracker;
use crate::run_engine::{dispatch, AgentRunEngine};
use crate::tool_bridge;

/// Per-tool-call control flow returned by `dispatch_one_tool_call`.
enum ToolCallFlow {
    /// Continue with the next tool call (the previous `continue` arms).
    Next,
    /// Return immediately from the run (doom-loop tripped).
    AbortRun(crate::agent_loop::AgentResult),
    /// Stop iterating tool calls; record the result in `should_return`
    /// and let the outer loop converge after the current iteration.
    ConvergeAfterLoop(crate::agent_loop::AgentResult),
}

impl AgentRunEngine {
    /// Execute the full agent run cycle with fresh messages (no session history).
    ///
    /// Flow: Initialized → Planning → Executing → Evaluating → Converged/Replanning/Aborted
    pub async fn execute(&mut self) -> crate::agent_loop::AgentResult {
        let mut result = self.execute_with_history(Vec::new()).await;
        self.record_session_exit(&result).await;
        result.run_report = self.obs.last_run_report.take();
        result
    }

    /// Execute with session history from previous REPL prompts.
    /// `history` contains messages from prior runs in this session.
    /// The current task objective is appended as the last user message.
    pub async fn execute_with_history(
        &mut self,
        history: Vec<Message>,
    ) -> crate::agent_loop::AgentResult {
        // Fase 4 (T4.2): bootstrap.rs assembles initial messages.
        let mut messages = self.assemble_initial_messages(history).await;
        let mut state_manager = self.init_state_manager();
        let hook_runner = self.init_hook_runner();
        let sensor_runner = self.init_sensor_runner();
        let mut doom_tracker = self
            .config
            .loop_cfg()
            .doom_loop_threshold
            .map(DoomLoopTracker::new);
        let tool_defs = self.build_tool_definitions();
        let (abort_rx, _abort_tx_keepalive) = self.bridge_cancellation_to_abort_channel();

        loop {
            self.run.iteration += 1;
            let iteration = self.run.iteration;

            // Reset per-turn checkpoint flag so the first mutating
            // tool of THIS iteration triggers a snapshot.
            self.reset_turn_checkpoint();

            // Budget check — extracted to main_loop::check_budget_or_exhausted.
            if let Some(result) = self.check_budget_or_exhausted() {
                return result;
            }

            // Sensor drain — extracted to main_loop::drain_sensor_messages.
            self.drain_sensor_messages(sensor_runner.as_ref(), &mut messages);

            // Context-loop + compaction + metrics — extracted to
            // main_loop::inject_context_loop_and_compact.
            let estimated_context_tokens = self
                .inject_context_loop_and_compact(iteration, &mut messages)
                .await;

            // LLM call.
            self.transition_run(RunState::Planning);
            let (request, chosen_model, routing_reason) =
                self.prepare_chat_request(&messages, iteration, &tool_defs, estimated_context_tokens);
            let response = match self
                .call_llm_with_retry(
                    &request,
                    &chosen_model,
                    iteration,
                    routing_reason,
                    estimated_context_tokens,
                )
                .await
            {
                Ok(resp) => resp,
                Err(e) if e.is_context_overflow() => {
                    self.handle_context_overflow(&e, &mut messages);
                    continue;
                }
                Err(e) => return self.build_llm_abort_result(&e),
            };

            let tool_calls = response.tool_calls();

            // No tool calls → text-only response (OpenCode pattern)
            // LLM decided to respond with text, not use tools.
            // This handles conversational messages ("hello"), informational queries,
            // and any response where the agent chose not to invoke tools.
            //
            // Before converging, check follow-up queue: if the user typed something
            // while the agent was working, inject it and continue.
            // Pi-mono ref: `packages/agent/src/agent-loop.ts:220-228`
            if tool_calls.is_empty() {
                let content = response.content().unwrap_or("").to_string();
                // Extracted to main_loop::handle_text_only_response.
                match self.handle_text_only_response(content, &mut messages).await {
                    dispatch::DispatchOutcome::Continue => continue,
                    dispatch::DispatchOutcome::Converged(result) => return result,
                }
            }

            // ── EXECUTING phase ──
            self.transition_run(RunState::Executing);

            messages.push(Message::assistant_with_tool_calls(
                response.content().map(String::from),
                tool_calls.to_vec(),
            ));

            self.persist_assistant_message(&mut state_manager, &response);

            let mut should_return = None;
            for call in tool_calls {
                match self
                    .dispatch_one_tool_call(
                        call,
                        iteration,
                        &abort_rx,
                        &mut state_manager,
                        hook_runner.as_ref(),
                        sensor_runner.as_ref(),
                        doom_tracker.as_mut(),
                        &mut messages,
                    )
                    .await
                {
                    ToolCallFlow::Next => continue,
                    ToolCallFlow::AbortRun(result) => return result,
                    ToolCallFlow::ConvergeAfterLoop(result) => {
                        should_return = Some(result);
                        break;
                    }
                }
            }
            if let Some(result) = should_return {
                return result;
            }

            // Steering queue drain — extracted to main_loop::drain_steering_queue.
            self.drain_steering_queue(&mut messages).await;

            // ── EVALUATING phase ──
            self.transition_run(RunState::Evaluating);

            // Snapshot persistence — extracted to
            // main_loop::persist_snapshot_if_configured.
            self.persist_snapshot_if_configured(&messages).await;

            // After executing tools, evaluate and loop back to Planning
            self.transition_run(RunState::Replanning);
        }
    }

    /// Build the LLM `ChatRequest` for the current turn: route the
    /// model, attach tool defs, apply max_tokens / temperature /
    /// reasoning_effort, and force `tool_choice` when the catalog
    /// requires it. Returns the request, the chosen model name, and
    /// the routing reason for downstream telemetry.
    fn prepare_chat_request(
        &mut self,
        messages: &[Message],
        iteration: usize,
        tool_defs: &[theo_infra_llm::types::ToolDefinition],
        estimated_context_tokens: usize,
    ) -> (ChatRequest, String, &'static str) {
        // Routing decision — invariant enforced by structural_hygiene.rs:
        // the router is called exactly once per turn at this single
        // call site.
        let (chosen_model, chosen_effort, routing_reason) = self.choose_model(
            messages,
            iteration,
            estimated_context_tokens as u64,
            !tool_defs.is_empty(),
        );
        let mut request = ChatRequest::new(&chosen_model, messages.to_vec())
            .with_tools(tool_defs.to_vec())
            .with_max_tokens(self.config.llm().max_tokens)
            .with_temperature(self.config.llm().temperature);
        if let Some(ref effort) = chosen_effort {
            request = request.with_reasoning_effort(effort);
        }
        if let Some(forced) = forced_tool_choice(tool_defs) {
            request = request.with_tool_choice(forced);
        }
        (request, chosen_model, routing_reason)
    }

    /// Persist the assistant message to the crash-recovery state
    /// manager. Best-effort: failures fan out to tracing + EventBus
    /// (T1.3 / find_p4_002 / INV-002) so they are observable, but the
    /// run continues.
    fn persist_assistant_message(
        &mut self,
        state_manager: &mut Option<crate::state_manager::StateManager>,
        response: &theo_infra_llm::types::ChatResponse,
    ) {
        let Some(sm) = state_manager else {
            return;
        };
        let content = response.content().unwrap_or("");
        if let Err(e) = sm.append_message("assistant", content) {
            self.publish_state_append_failure("assistant", &e);
        }
    }

    /// Dispatch a single tool call through the full pipeline:
    /// resume-replay → meta-tool routing → plan-mode guard → pre-hook
    /// → checkpoint → MCP dispatch → regular dispatch → doom-tracker
    /// → fence + persist + working-set + sensor + vision follow-up
    /// → post-hook. Returns:
    /// - `Next` — proceed to the next call (the previous `continue` arms).
    /// - `AbortRun(result)` — return immediately from the run (doom tracker tripped).
    /// - `ConvergeAfterLoop(result)` — break out of the for-loop and
    ///   return after `should_return` is set (meta-tool converged).
    #[allow(clippy::too_many_arguments)]
    async fn dispatch_one_tool_call(
        &mut self,
        call: &theo_infra_llm::types::ToolCall,
        iteration: usize,
        abort_rx: &tokio::sync::watch::Receiver<bool>,
        state_manager: &mut Option<crate::state_manager::StateManager>,
        hook_runner: Option<&crate::hooks::HookRunner>,
        sensor_runner: Option<&crate::sensor::SensorRunner>,
        doom_tracker: Option<&mut DoomLoopTracker>,
        messages: &mut Vec<Message>,
    ) -> ToolCallFlow {
        let name = &call.function.name;
        // Resume-replay short-circuit.
        if self.try_replay_tool_call(call, messages) {
            return ToolCallFlow::Next;
        }
        // Meta-tool routing (T4.3) — single match in dispatch/router.rs.
        if let Some(outcome) = self
            .dispatch_meta_tool(dispatch::router::MetaToolContext {
                call,
                iteration,
                abort_rx,
                messages,
            })
            .await
        {
            return match outcome {
                dispatch::DispatchOutcome::Converged(result) => {
                    ToolCallFlow::ConvergeAfterLoop(result)
                }
                dispatch::DispatchOutcome::Continue => ToolCallFlow::Next,
            };
        }
        // Plan-mode guard.
        if self.enforce_plan_mode_guard(call, messages) {
            return ToolCallFlow::Next;
        }
        // Pre-hook.
        if self.run_pre_tool_hook(hook_runner, call, messages).await {
            return ToolCallFlow::Next;
        }
        // Pre-mutation checkpoint (CAS-idempotent within iteration).
        self.emit_checkpoint_event_for_tool(name, iteration);
        // MCP dispatch (mcp:<server>:<tool> namespace).
        if let Some(mcp_msg) = self.try_dispatch_mcp_tool(call).await {
            messages.push(mcp_msg);
            return ToolCallFlow::Next;
        }
        // Regular tool dispatch. The 3rd element is `ToolOutput.metadata`
        // propagated through `ToolResultRecord.metadata` (T1.2/T0.1
        // vision channel).
        let Some((success, output, metadata)) = self
            .execute_regular_tool_call(call, iteration, abort_rx, messages)
            .await
        else {
            return ToolCallFlow::Next;
        };
        // Doom loop tracker.
        if let Some(result) = self.update_doom_tracker(doom_tracker, call, name, messages) {
            return ToolCallFlow::AbortRun(result);
        }
        self.finalise_tool_call_result(
            call,
            name,
            iteration,
            success,
            &output,
            metadata.as_ref(),
            state_manager,
            sensor_runner,
            messages,
        );
        // Post-hook.
        self.run_post_tool_hook(hook_runner, call).await;
        ToolCallFlow::Next
    }

    /// Fence + persist + working-set + sensor + vision follow-up after
    /// a regular tool call. Extracted from `dispatch_one_tool_call` so
    /// the per-call helper stays under the size budget.
    #[allow(clippy::too_many_arguments)]
    fn finalise_tool_call_result(
        &mut self,
        call: &theo_infra_llm::types::ToolCall,
        name: &str,
        iteration: usize,
        success: bool,
        output: &str,
        metadata: Option<&serde_json::Value>,
        state_manager: &mut Option<crate::state_manager::StateManager>,
        sensor_runner: Option<&crate::sensor::SensorRunner>,
        messages: &mut Vec<Message>,
    ) {
        // T2.1 / FIND-P6-001 / D5 — fence untrusted tool output before
        // it becomes a tool_result so embedded injection tokens can't
        // hijack the next LLM turn. The `tool:{name}` source label
        // flows into the fence tag for auditability.
        let fenced_output = theo_domain::prompt_sanitizer::fence_untrusted(
            output,
            &format!("tool:{name}"),
            crate::constants::MAX_TOOL_OUTPUT_BYTES,
        );
        let result_msg = Message::tool_result(&call.id, name, &fenced_output);
        self.update_working_set_post_tool(call, name, iteration);
        self.update_context_loop_post_tool(call, name, success, output);
        // Persist tool result for crash recovery (T1.3 / find_p4_002).
        if let Some(sm) = state_manager
            && let Err(e) = sm.append_message("tool", output)
        {
            self.publish_state_append_failure("tool", &e);
        }
        self.fire_sensor_for_write_tool(sensor_runner, call, name, success);
        messages.push(result_msg);
        // T1.2 / T0.1 — vision follow-up when the tool surfaced an
        // image_block in metadata (e.g., `read_image`).
        if let Some(meta) = metadata {
            crate::vision_propagation::push_image_followup(messages, meta, name);
        }
    }

    /// File-backed crash-recovery state manager. Best-effort — failure
    /// to create logs and returns None. Disabled inside subagents.
    fn init_state_manager(&self) -> Option<crate::state_manager::StateManager> {
        if self.config.loop_cfg().is_subagent {
            return None;
        }
        match crate::state_manager::StateManager::create(
            &self.project_dir,
            self.run.run_id.as_str(),
        ) {
            Ok(sm) => Some(sm),
            Err(e) => {
                tracing::warn!(error = %e, "state manager init failed (non-fatal)");
                None
            }
        }
    }

    /// Pre/post-tool hooks runner. Disabled inside subagents.
    fn init_hook_runner(&self) -> Option<crate::hooks::HookRunner> {
        if self.config.loop_cfg().is_subagent {
            return None;
        }
        Some(crate::hooks::HookRunner::new(
            &self.project_dir,
            crate::hooks::HookConfig::default(),
        ))
    }

    /// Async sensor runner for computational verification after writes.
    /// Returns None when no sensors are configured. Disabled in subagents.
    fn init_sensor_runner(&self) -> Option<crate::sensor::SensorRunner> {
        if self.config.loop_cfg().is_subagent {
            return None;
        }
        let runner = crate::sensor::SensorRunner::new(
            &self.project_dir,
            crate::hooks::HookConfig::default(),
        );
        if runner.has_sensors() {
            Some(runner)
        } else {
            None
        }
    }

    /// Layer 1 schema stripping: subagents get filtered tool definitions
    /// that exclude delegation meta-tools (subagent, subagent_parallel,
    /// skill) so they can't recursively dispatch.
    fn build_tool_definitions(&self) -> Vec<theo_infra_llm::types::ToolDefinition> {
        if self.config.loop_cfg().is_subagent {
            tool_bridge::registry_to_definitions_for_subagent(&self.llm.registry)
        } else {
            tool_bridge::registry_to_definitions(&self.llm.registry)
        }
    }

    /// T1.1 / find_p7_001 / INV-008 — bridge user cancellation
    /// (`CancellationTree`) to the tools' `watch::Receiver<bool>` so
    /// long-running tools (`git clone`, `web-fetch`) abort within
    /// milliseconds instead of running to completion. The returned
    /// sender (`_keepalive`) MUST be held by the caller for the entire
    /// agent loop scope; dropping it deactivates the bridge.
    fn bridge_cancellation_to_abort_channel(
        &self,
    ) -> (
        tokio::sync::watch::Receiver<bool>,
        tokio::sync::watch::Sender<bool>,
    ) {
        let (abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        if let Some(ct) = self.subagent.cancellation.as_ref() {
            let token = ct.child(self.run.run_id.as_str());
            let tx = abort_tx.clone();
            tokio::spawn(async move {
                token.cancelled().await;
                let _ = tx.send(true);
            });
        }
        (abort_rx, abort_tx)
    }
}

/// `THEO_FORCE_TOOL_CHOICE` env var lets operators force the model to call
/// a tool. Useful for benchmarks / OAuth E2E tests where chatty models like
/// gpt-5.3-codex would otherwise generate text instead of invoking
/// `delegate_task`.
///
///   - `"required"` / `"any"` → model MUST call some tool (any)
///   - `"none"`               → model MUST NOT call a tool
///   - `"function:NAME"`      → shorthand for forcing a specific tool
///   - JSON `{"type":"function","name":"X"}` → passed through verbatim
///   - any other value        → passed through verbatim (string)
///
/// Skipped silently when no tools are exposed for this turn.
fn forced_tool_choice(
    tool_defs: &[theo_infra_llm::types::ToolDefinition],
) -> Option<String> {
    if tool_defs.is_empty() {
        return None;
    }
    let forced = theo_domain::environment::theo_var("THEO_FORCE_TOOL_CHOICE")?;
    let normalized = match forced.as_str() {
        "any" => "required".to_string(),
        other if other.starts_with("function:") => {
            let name = other.trim_start_matches("function:");
            // Use serde_json to safely encode the tool name.
            // Hand-rolled format!(r#"{{"name":"{}"}}"#, name)
            // produces broken JSON if `name` contains a quote.
            serde_json::json!({"type": "function", "name": name}).to_string()
        }
        other => other.to_string(),
    };
    if theo_domain::environment::bool_var("THEO_DEBUG_CODEX", false) {
        tracing::debug!(
            forced = %forced,
            normalized = %normalized,
            "THEO_FORCE_TOOL_CHOICE active"
        );
    }
    Some(normalized)
}

#[cfg(test)]
mod forced_tool_choice_tests {
    //! T1.5 — verifies the migration from hand-rolled JSON to
    //! `serde_json::json!` correctly escapes pathological tool names.
    //! The legacy `format!(r#"{{"type":"function","name":"{}"}}"#, name)`
    //! produced broken JSON if `name` contained a `"`; this guard
    //! prevents future regressions.

    use super::forced_tool_choice;

    /// Process-wide env var lock — same pattern used in
    /// `project_config::tests` and `observability::otel_exporter::tests`.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static M: OnceLock<Mutex<()>> = OnceLock::new();
        M.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    struct EnvSnapshot {
        prior: Option<std::ffi::OsString>,
    }
    impl EnvSnapshot {
        fn capture() -> Self {
            let prior = std::env::var_os("THEO_FORCE_TOOL_CHOICE");
            // SAFETY: pre-fork setup — the call site is synchronous w.r.t. the child process, runs after the parent has prepared all FDs, and never executes in async context.
            unsafe { std::env::remove_var("THEO_FORCE_TOOL_CHOICE") };
            Self { prior }
        }
    }
    impl Drop for EnvSnapshot {
        fn drop(&mut self) {
            // SAFETY: pre-fork setup — the call site is synchronous w.r.t. the child process, runs after the parent has prepared all FDs, and never executes in async context.
            unsafe {
                match &self.prior {
                    Some(v) => std::env::set_var("THEO_FORCE_TOOL_CHOICE", v),
                    None => std::env::remove_var("THEO_FORCE_TOOL_CHOICE"),
                }
            }
        }
    }

    /// Trivial tool-defs vec — `forced_tool_choice` returns `None` on
    /// empty, so we provide a dummy with one entry to exercise every
    /// branch.
    fn tool_defs_with(name: &str) -> Vec<theo_infra_llm::types::ToolDefinition> {
        vec![theo_infra_llm::types::ToolDefinition::new(
            name,
            "test",
            serde_json::json!({}),
        )]
    }

    #[test]
    fn force_tool_choice_with_quote_in_name_serializes_correctly() {
        let _l = env_lock();
        let _s = EnvSnapshot::capture();
        // SAFETY: pre-fork setup — the call site is synchronous w.r.t. the child process, runs after the parent has prepared all FDs, and never executes in async context.
        unsafe {
            std::env::set_var("THEO_FORCE_TOOL_CHOICE", r#"function:weird"name"#);
        }
        let out = forced_tool_choice(&tool_defs_with("anything"))
            .expect("env set ⇒ Some");
        // Round-trip: must parse back into a JSON object with the
        // exact name (escapes preserved). Hand-rolled format! would
        // produce `{"name":"weird"name"}` — invalid JSON.
        let parsed: serde_json::Value =
            serde_json::from_str(&out).expect("forced_tool_choice produced invalid JSON");
        assert_eq!(parsed["type"], "function");
        assert_eq!(parsed["name"], r#"weird"name"#);
    }

    #[test]
    fn force_tool_choice_with_backslash_in_name_serializes_correctly() {
        let _l = env_lock();
        let _s = EnvSnapshot::capture();
        // SAFETY: pre-fork setup — the call site is synchronous w.r.t. the child process, runs after the parent has prepared all FDs, and never executes in async context.
        unsafe {
            std::env::set_var("THEO_FORCE_TOOL_CHOICE", r#"function:back\slash"#);
        }
        let out = forced_tool_choice(&tool_defs_with("anything"))
            .expect("env set ⇒ Some");
        let parsed: serde_json::Value =
            serde_json::from_str(&out).expect("forced_tool_choice produced invalid JSON");
        assert_eq!(parsed["name"], r#"back\slash"#);
    }

    #[test]
    fn force_tool_choice_passes_through_verbatim_strings() {
        let _l = env_lock();
        let _s = EnvSnapshot::capture();
        // SAFETY: pre-fork setup — the call site is synchronous w.r.t. the child process, runs after the parent has prepared all FDs, and never executes in async context.
        unsafe { std::env::set_var("THEO_FORCE_TOOL_CHOICE", "required") };
        assert_eq!(
            forced_tool_choice(&tool_defs_with("x")).as_deref(),
            Some("required")
        );
        // SAFETY: pre-fork setup — the call site is synchronous w.r.t. the child process, runs after the parent has prepared all FDs, and never executes in async context.
        unsafe { std::env::set_var("THEO_FORCE_TOOL_CHOICE", "none") };
        assert_eq!(
            forced_tool_choice(&tool_defs_with("x")).as_deref(),
            Some("none")
        );
    }

    #[test]
    fn force_tool_choice_normalizes_any_to_required() {
        let _l = env_lock();
        let _s = EnvSnapshot::capture();
        // SAFETY: pre-fork setup — the call site is synchronous w.r.t. the child process, runs after the parent has prepared all FDs, and never executes in async context.
        unsafe { std::env::set_var("THEO_FORCE_TOOL_CHOICE", "any") };
        assert_eq!(
            forced_tool_choice(&tool_defs_with("x")).as_deref(),
            Some("required")
        );
    }

    #[test]
    fn force_tool_choice_returns_none_when_no_tools_exposed() {
        let _l = env_lock();
        let _s = EnvSnapshot::capture();
        // SAFETY: pre-fork setup — the call site is synchronous w.r.t. the child process, runs after the parent has prepared all FDs, and never executes in async context.
        unsafe { std::env::set_var("THEO_FORCE_TOOL_CHOICE", "required") };
        // Empty tool_defs — early return before consulting env var.
        assert!(forced_tool_choice(&[]).is_none());
    }

    #[test]
    fn force_tool_choice_returns_none_when_env_unset() {
        let _l = env_lock();
        let _s = EnvSnapshot::capture(); // strips var
        assert!(forced_tool_choice(&tool_defs_with("x")).is_none());
    }
}
