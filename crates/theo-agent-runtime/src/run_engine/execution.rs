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
        let result = self.execute_with_history(Vec::new()).await;
        self.record_session_exit(&result).await;
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
        let mut state_manager = if !self.config.is_subagent {
            match crate::state_manager::StateManager::create(
                &self.project_dir,
                self.run.run_id.as_str(),
            ) {
                Ok(sm) => Some(sm),
                Err(e) => {
                    eprintln!("[theo] State manager init failed (non-fatal): {e}");
                    None
                }
            }
        } else {
            None
        };

        // Initialize hook runner for pre/post tool hooks
        let hook_runner = if !self.config.is_subagent {
            Some(crate::hooks::HookRunner::new(
                &self.project_dir,
                crate::hooks::HookConfig::default(),
            ))
        } else {
            None // Sub-agents don't run hooks
        };

        // Initialize sensor runner for computational verification after write tools.
        // Sensors fire asynchronously and results are drained before each LLM call.
        let sensor_runner = if !self.config.is_subagent {
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
        let mut doom_tracker = self.config.doom_loop_threshold.map(DoomLoopTracker::new);

        // Layer 1: Schema stripping — sub-agents get filtered tool definitions
        // that exclude delegation meta-tools (subagent, subagent_parallel, skill).
        let tool_defs = if self.config.is_subagent {
            tool_bridge::registry_to_definitions_for_subagent(&self.registry)
        } else {
            tool_bridge::registry_to_definitions(&self.registry)
        };
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);

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
                .with_max_tokens(self.config.max_tokens)
                .with_temperature(self.config.temperature);

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

            // Persist assistant message to state manager (crash recovery)
            if let Some(ref mut sm) = state_manager {
                let content = response.content().unwrap_or("");
                let _ = sm.append_message("assistant", content);
            }

            let mut should_return = None;

            for call in tool_calls {
                let name = &call.function.name;

                // Resume-replay short-circuit — extracted to
                // main_loop::try_replay_tool_call.
                if self.try_replay_tool_call(call, &mut messages) {
                    continue;
                }

                // `done` meta-tool — gates Gate 0/1/2 live in
                // `dispatch/done.rs` (Fase 4 — T4.2).
                if name == "done" {
                    match self.handle_done_call(call, iteration, &mut messages).await {
                        dispatch::DispatchOutcome::Converged(result) => {
                            should_return = Some(result);
                            break;
                        }
                        dispatch::DispatchOutcome::Continue => continue,
                    }
                }

                // `delegate_task` / `delegate_task_single` /
                // `delegate_task_parallel` — extracted to dispatch/delegate.rs.
                if name == "delegate_task"
                    || name == "delegate_task_single"
                    || name == "delegate_task_parallel"
                {
                    self.dispatch_delegate_task(call, &mut messages).await;
                    continue;
                }

                // `skill` — extracted to dispatch/skill.rs.
                if name == "skill" {
                    self.dispatch_skill(call, &mut messages).await;
                    continue;
                }

                // `batch` — extracted to dispatch/batch.rs.
                if name == "batch" {
                    self.dispatch_batch(call, &abort_rx, &mut messages).await;
                    continue;
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

                let result_msg = Message::tool_result(&call.id, name, &output);

                // Working set + context metrics — extracted to
                // main_loop::update_working_set_post_tool.
                self.update_working_set_post_tool(call, name, iteration);

                // Context-loop state — extracted to
                // main_loop::update_context_loop_post_tool.
                self.update_context_loop_post_tool(call, name, success, &output);

                // Persist tool result to state manager (crash recovery)
                if let Some(ref mut sm) = state_manager {
                    let _ = sm.append_message("tool", &output);
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
        eprintln!(
            "[theo] THEO_FORCE_TOOL_CHOICE active: {} → {}",
            forced, normalized
        );
    }
    Some(normalized)
}
