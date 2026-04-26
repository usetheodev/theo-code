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

impl AgentRunEngine {
    /// Execute the full agent run cycle with fresh messages (no session history).
    ///
    /// Flow: Initialized → Planning → Executing → Evaluating → Converged/Replanning/Aborted
    pub async fn execute(&mut self) -> crate::agent_loop::AgentResult {
        let mut result = self.execute_with_history(Vec::new()).await;
        self.record_session_exit(&result).await;
        result.run_report = self.last_run_report.take();
        result
    }

    /// Execute with session history from previous REPL prompts.
    /// `history` contains messages from prior runs in this session.
    /// The current task objective is appended as the last user message.
    pub async fn execute_with_history(
        &mut self,
        history: Vec<Message>,
    ) -> crate::agent_loop::AgentResult {
        // Fase 4 (T4.2): the 200-LOC setup phase — state-machine
        // transitions, auto-init, autodream spawn, system prompt,
        // memory prefetch, episode replay, git boot context, GRAPHCTX
        // planning hints, skills summary, history merge, task
        // objective — lives in `bootstrap.rs`.
        let mut messages = self.assemble_initial_messages(history).await;

        // Initialize state manager for file-backed persistence (crash recovery).
        // Best-effort: if creation fails, continue without persistence.
        let mut state_manager = if !self.config.loop_cfg().is_subagent {
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
        } else {
            None
        };

        // Initialize hook runner for pre/post tool hooks
        let hook_runner = if !self.config.loop_cfg().is_subagent {
            Some(crate::hooks::HookRunner::new(
                &self.project_dir,
                crate::hooks::HookConfig::default(),
            ))
        } else {
            None // Sub-agents don't run hooks
        };

        // Initialize sensor runner for computational verification after write tools.
        // Sensors fire asynchronously and results are drained before each LLM call.
        let sensor_runner = if !self.config.loop_cfg().is_subagent {
            let runner = crate::sensor::SensorRunner::new(
                &self.project_dir,
                crate::hooks::HookConfig::default(),
            );
            if runner.has_sensors() {
                Some(runner)
            } else {
                None
            }
        } else {
            None
        };

        // Doom loop detector — tracks recent tool calls to detect repetition
        let mut doom_tracker = self.config.loop_cfg().doom_loop_threshold.map(DoomLoopTracker::new);

        // Layer 1: Schema stripping — sub-agents get filtered tool definitions
        // that exclude delegation meta-tools (subagent, subagent_parallel, skill).
        let tool_defs = if self.config.loop_cfg().is_subagent {
            tool_bridge::registry_to_definitions_for_subagent(&self.registry)
        } else {
            tool_bridge::registry_to_definitions(&self.registry)
        };
        // T1.1 / find_p7_001 / INV-008 — bridge user cancellation to the
        // tools' watch::Receiver.
        //
        // Tools accept `watch::Receiver<bool>` for abort signalling, but
        // the runtime's source-of-truth is `CancellationTree`. Without
        // this bridge the sender of the abort channel was being prefixed
        // with `_` which made Rust drop it immediately, leaving any tool
        // long-running after a `cancel_agent()` call (the previous
        // behaviour leaked dozens of seconds of latency on `git clone`,
        // `web-fetch`, etc.).
        //
        // The sender is kept alive for the entire scope via
        // `_abort_tx_keepalive` (the `_` prefix is intentional and only
        // applies to the keep-alive binding, *not* to the original
        // sender).
        let (abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        if let Some(ct) = self.subagent_cancellation.as_ref() {
            let token = ct.child(self.run.run_id.as_str());
            let tx = abort_tx.clone();
            tokio::spawn(async move {
                token.cancelled().await;
                let _ = tx.send(true);
            });
        }
        // Keep `abort_tx` alive for the entire `execute_with_history` scope
        // so the bridge above (and any future bridges) can still send.
        let _abort_tx_keepalive = abort_tx;

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

            // LLM call
            self.transition_run(RunState::Planning);

            // Routing decision — extracted to main_loop::choose_model.
            // The router is called exactly once per turn at this single
            // call site (invariant enforced by structural_hygiene.rs).
            let (chosen_model, chosen_effort, routing_reason) = self.choose_model(
                &messages,
                iteration,
                estimated_context_tokens as u64,
                !tool_defs.is_empty(),
            );

            let mut request = ChatRequest::new(&chosen_model, messages.clone())
                .with_tools(tool_defs.clone())
                .with_max_tokens(self.config.llm().max_tokens)
                .with_temperature(self.config.llm().temperature);

            if let Some(ref effort) = chosen_effort {
                request = request.with_reasoning_effort(effort);
            }

            if let Some(forced) = forced_tool_choice(&tool_defs) {
                request = request.with_tool_choice(forced);
            }

            // Full LLM call (start event + retry loop + end event)
            // extracted to main_loop::call_llm_with_retry.
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
                Err(e) => {
                    return self.build_llm_abort_result(&e);
                }
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

            // Persist assistant message to state manager (crash recovery).
            //
            // T1.3 / find_p4_002 / INV-002 — failures here used to be
            // silently discarded via `let _ = ...`, leaving the JSONL
            // crash-recovery file inconsistent without any operational
            // signal. Now they fan out to tracing + EventBus so the
            // failure is observable; the run still continues because
            // persistence is best-effort.
            if let Some(ref mut sm) = state_manager {
                let content = response.content().unwrap_or("");
                if let Err(e) = sm.append_message("assistant", content) {
                    self.publish_state_append_failure("assistant", &e);
                }
            }

            let mut should_return = None;

            for call in tool_calls {
                let name = &call.function.name;

                // Resume-replay short-circuit — extracted to
                // main_loop::try_replay_tool_call.
                if self.try_replay_tool_call(call, &mut messages) {
                    continue;
                }

                // Meta-tool routing (T4.3). A single match in
                // `dispatch/router.rs` replaces the previous 4-way
                // if-chain. Returns `Some(outcome)` when the call name
                // matched a registered meta-tool; `None` falls through
                // to regular-tool dispatch below.
                if let Some(outcome) = self
                    .dispatch_meta_tool(dispatch::router::MetaToolContext {
                        call,
                        iteration,
                        abort_rx: &abort_rx,
                        messages: &mut messages,
                    })
                    .await
                {
                    match outcome {
                        dispatch::DispatchOutcome::Converged(result) => {
                            should_return = Some(result);
                            break;
                        }
                        dispatch::DispatchOutcome::Continue => continue,
                    }
                }

                // Plan mode guard — extracted to
                // main_loop::enforce_plan_mode_guard.
                if self.enforce_plan_mode_guard(call, &mut messages) {
                    continue;
                }

                // Pre-hook — extracted to main_loop::run_pre_tool_hook.
                if self
                    .run_pre_tool_hook(hook_runner.as_ref(), call, &mut messages)
                    .await
                {
                    continue;
                }

                // Pre-mutation checkpoint — extracted to
                // main_loop::emit_checkpoint_event_for_tool. Idempotent
                // within an iteration (CAS); no-op when no checkpoint
                // manager is attached.
                self.emit_checkpoint_event_for_tool(name, iteration);

                // ── PHASE 8: MCP DISPATCH ──
                // If the tool name is in the `mcp:<server>:<tool>`
                // namespace, route to McpDispatcher (transient stdio
                // client). Otherwise fall through to the normal dispatch.
                if let Some(mcp_msg) = self.try_dispatch_mcp_tool(call).await {
                    messages.push(mcp_msg);
                    continue;
                }

                // Regular tool dispatch — extracted to
                // main_loop::execute_regular_tool_call.
                let Some((success, output)) = self
                    .execute_regular_tool_call(call, iteration, &abort_rx, &mut messages)
                    .await
                else {
                    continue;
                };

                // Doom loop tracker — extracted to main_loop::update_doom_tracker.
                if let Some(result) =
                    self.update_doom_tracker(doom_tracker.as_mut(), call, name, &mut messages)
                {
                    return result;
                }

                // T2.1 / FIND-P6-001 / D5 — all untrusted tool output is
                // fenced before becoming a `Message::tool_result(...)` so
                // injection tokens (`<|im_start|>`, `[INST]`, `<system>`,
                // …) embedded in file contents, shell output, fetched
                // pages, etc. cannot hijack the next LLM turn. The
                // `tool:{name}` source label flows into the fence tag for
                // auditability.
                let fenced_output = theo_domain::prompt_sanitizer::fence_untrusted(
                    &output,
                    &format!("tool:{name}"),
                    crate::constants::MAX_TOOL_OUTPUT_BYTES,
                );
                let result_msg = Message::tool_result(&call.id, name, &fenced_output);

                // Working set + context metrics — extracted to
                // main_loop::update_working_set_post_tool.
                self.update_working_set_post_tool(call, name, iteration);

                // Context-loop state — extracted to
                // main_loop::update_context_loop_post_tool.
                self.update_context_loop_post_tool(call, name, success, &output);

                // Persist tool result to state manager (crash recovery).
                // T1.3 / find_p4_002 — see assistant-side comment above.
                if let Some(ref mut sm) = state_manager {
                    if let Err(e) = sm.append_message("tool", &output) {
                        self.publish_state_append_failure("tool", &e);
                    }
                }

                // Sensor fire — extracted to
                // main_loop::fire_sensor_for_write_tool.
                self.fire_sensor_for_write_tool(sensor_runner.as_ref(), call, name, success);

                messages.push(result_msg);

                // Post-hook — extracted to main_loop::run_post_tool_hook.
                self.run_post_tool_hook(hook_runner.as_ref(), call).await;
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
            unsafe { std::env::remove_var("THEO_FORCE_TOOL_CHOICE") };
            Self { prior }
        }
    }
    impl Drop for EnvSnapshot {
        fn drop(&mut self) {
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
        unsafe { std::env::set_var("THEO_FORCE_TOOL_CHOICE", "required") };
        assert_eq!(
            forced_tool_choice(&tool_defs_with("x")).as_deref(),
            Some("required")
        );
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
