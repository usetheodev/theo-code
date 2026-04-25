//! Session lifecycle: shutdown path (record_session_exit) and
//! observability finalization.
//!
//! Extracted from `run_engine/mod.rs` (Fase 4 — REMEDIATION_PLAN T4.2)
//! into a separate `impl AgentRunEngine` block. This keeps the ~155 LOC
//! of shutdown ceremony out of the main state-machine file.

use crate::agent_loop::AgentResult;

use super::AgentRunEngine;

impl AgentRunEngine {
    /// `AgentLoop::run_with_history` adapter — shares `execute()`'s
    /// shutdown path.
    pub async fn record_session_exit_public(&mut self, r: &AgentResult) {
        self.record_session_exit(r).await;
    }

    /// Record session exit.
    ///
    /// Async tokio::fs + on_session_end hook. Every persistence call is
    /// best-effort: failures emit `DomainEvent::Error{type:"fs"}` via
    /// `crate::fs_errors::emit_fs_error` so the observability pipeline
    /// can alert, but the shutdown path does not abort.
    pub(super) async fn record_session_exit(&mut self, result: &AgentResult) {
        // Save failure pattern tracker
        self.failure_tracker.save();

        // Save context metrics to .theo/metrics/{run_id}.json.
        let metrics_dir = self.project_dir.join(".theo").join("metrics");
        match tokio::fs::create_dir_all(&metrics_dir).await {
            Ok(()) => {
                let report = self.context_metrics.to_report();
                let metrics_path =
                    metrics_dir.join(format!("{}.json", self.run.run_id.as_str()));
                let body = serde_json::to_string_pretty(&report).unwrap_or_default();
                if let Err(e) = tokio::fs::write(&metrics_path, body).await {
                    crate::fs_errors::emit_fs_error(
                        &self.event_bus,
                        self.run.run_id.as_str(),
                        "record_session_exit/metrics_write",
                        &metrics_path,
                        &e,
                    );
                }
            }
            Err(e) => {
                crate::fs_errors::emit_fs_error(
                    &self.event_bus,
                    self.run.run_id.as_str(),
                    "record_session_exit/metrics_mkdir",
                    &metrics_dir,
                    &e,
                );
            }
        }

        // Generate EpisodeSummary from run events and persist to
        // .theo/memory/episodes/ (decision: meeting 20260420-221947 #4 —
        // episodes belong to memory namespace, not wiki; wiki is
        // reserved for compiled content).
        //
        // T6.2 — scope to events for THIS run via `events_for(run_id)`
        // instead of cloning the entire process-wide log. The episode
        // summary should only reflect this run anyway, so the filter
        // is also semantically tighter.
        let events = self.event_bus.events_for(self.run.run_id.as_str());
        if !events.is_empty() {
            let task_objective = self
                .task_manager
                .get(&self.task_id)
                .map(|t| t.objective.clone())
                .unwrap_or_else(|| "unknown".to_string());
            let mut summary = theo_domain::episode::EpisodeSummary::from_events(
                self.run.run_id.as_str(),
                Some(self.task_id.as_str()),
                &task_objective,
                &events,
            );
            // Usage + cost accounting + lesson/hypothesis pipelines.
            let mut usage = self.session_token_usage.clone();
            if let Some(c) = theo_domain::budget::known_model_cost(self.config.llm().model) {
                usage.recompute_cost(&c);
            }
            summary.token_usage = Some(usage);
            let _ = crate::lesson_pipeline::extract_and_persist_for_outcome(
                &self.project_dir,
                summary.machine_summary.outcome,
                &events,
            );
            let _ = crate::hypothesis_pipeline::persist_unresolved(
                &self.project_dir,
                &summary,
            );
            let episodes_dir = self
                .project_dir
                .join(".theo")
                .join("memory")
                .join("episodes");
            match tokio::fs::create_dir_all(&episodes_dir).await {
                Ok(()) => {
                    let episode_path =
                        episodes_dir.join(format!("{}.json", summary.summary_id));
                    let body =
                        serde_json::to_string_pretty(&summary).unwrap_or_default();
                    if let Err(e) = tokio::fs::write(&episode_path, body).await {
                        crate::fs_errors::emit_fs_error(
                            &self.event_bus,
                            self.run.run_id.as_str(),
                            "record_session_exit/episode_write",
                            &episode_path,
                            &e,
                        );
                    }
                }
                Err(e) => {
                    crate::fs_errors::emit_fs_error(
                        &self.event_bus,
                        self.run.run_id.as_str(),
                        "record_session_exit/episode_mkdir",
                        &episodes_dir,
                        &e,
                    );
                }
            }
        }

        // Memory-provider session-end hook (every exit path).
        crate::memory_lifecycle::MemoryLifecycle::on_session_end(&self.config).await;

        // Index session transcript via the pluggable TranscriptIndexer
        // trait (concrete impl lives in theo-application). Awaited
        // inline so shutdown completes only after Tantivy has committed
        // to disk.
        crate::memory_lifecycle::maybe_index_transcript(
            &self.config,
            &self.project_dir,
            self.run.run_id.as_str(),
            events.clone(),
        )
        .await;

        // Record session end for cross-session progress tracking
        if !self.config.loop_cfg().is_subagent {
            let tasks = if result.success {
                vec![crate::session_bootstrap::CompletedTask {
                    name: result.summary.chars().take(100).collect(),
                    status: "completed".to_string(),
                    files_changed: result.files_edited.clone(),
                }]
            } else {
                vec![crate::session_bootstrap::CompletedTask {
                    name: result.summary.chars().take(100).collect(),
                    status: "failed".to_string(),
                    files_changed: result.files_edited.clone(),
                }]
            };
            let last_error = if result.success {
                None
            } else {
                Some(result.summary.clone())
            };
            crate::session_bootstrap::record_session_end(
                &self.project_dir,
                self.run.run_id.as_str(),
                tasks,
                vec![], // next_steps are determined by the LLM, not the engine
                last_error,
            );
        }

        // Observability: drain writer, compute RunReport, append summary line.
        self.finalize_observability(result, !events.is_empty());
    }

    pub(super) fn finalize_observability(
        &mut self,
        result: &AgentResult,
        had_events: bool,
    ) {
        let Some(pipeline) = self.observability.take() else {
            return;
        };
        let file_path = pipeline.finalize();
        self.episodes_created = if had_events { 1 } else { 0 };
        let detected = crate::observability::finalize_run_observability(
            &file_path,
            self.run.run_id.as_str(),
            result.success,
            result.files_edited.len() as u64,
            &self.session_token_usage,
            self.config.loop_cfg().max_iterations,
            self.budget_enforcer.usage(),
            &self.context_metrics.to_report(),
            self.done_attempts,
            self.episodes_injected,
            self.episodes_created,
            self.failure_tracker.new_fingerprint_count(),
            self.failure_tracker.recurrent_fingerprint_count(),
            &self.initial_context_files,
            &self.pre_compaction_hot_files,
        );
        detected.publish_events(&self.event_bus, self.run.run_id.as_str());
    }
}
