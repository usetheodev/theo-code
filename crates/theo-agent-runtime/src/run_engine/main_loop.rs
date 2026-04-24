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

        // Retry loop. Streaming deltas forwarded to the bus via a
        // cloned sender closure per attempt.
        let retry_policy = if self.config.aggressive_retry {
            theo_domain::retry_policy::RetryPolicy::benchmark()
        } else {
            theo_domain::retry_policy::RetryPolicy::default_llm()
        };
        let max_retries = retry_policy.max_retries;
        let mut llm_result: Option<Result<ChatResponse, LlmError>> = None;

        for attempt in 0..=max_retries {
            let eb = self.event_bus.clone();
            let rid = self.run.run_id.as_str().to_string();

            let response = self
                .client
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
                .await;

            match response {
                Ok(resp) => {
                    llm_result = Some(Ok(resp));
                    break;
                }
                Err(ref e) if e.is_retryable() && attempt < max_retries => {
                    let delay = retry_policy.delay_for_attempt(attempt);
                    self.event_bus.publish(DomainEvent::new(
                        EventType::Error,
                        self.run.run_id.as_str(),
                        serde_json::json!({
                            "type": "retry",
                            "attempt": attempt + 1,
                            "max_retries": max_retries,
                            "error": e.to_string(),
                            "delay_ms": delay.as_millis() as u64,
                        }),
                    ));
                    tokio::time::sleep(delay).await;
                    continue;
                }
                Err(e) => {
                    llm_result = Some(Err(e));
                    break;
                }
            }
        }

        // Defensive: the retry loop always assigns llm_result at least
        // once, but relying on that invariant via .unwrap() would panic
        // on any future refactor that breaks it.
        let llm_result = llm_result.unwrap_or_else(|| {
            Err(LlmError::Parse(
                "LLM retry loop produced no result (invariant broken)".to_string(),
            ))
        });

        let resp = llm_result?;

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
