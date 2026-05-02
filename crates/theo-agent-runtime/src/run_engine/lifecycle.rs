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
        self.tracking.failure_tracker.save();
        self.persist_context_metrics().await;
        let events = self.event_bus.events_for(self.run.run_id.as_str());
        if !events.is_empty() {
            self.persist_episode_summary(&events).await;
        }
        crate::memory_lifecycle::MemoryLifecycle::on_session_end(&self.config).await;
        crate::memory_lifecycle::maybe_index_transcript(
            &self.config,
            &self.project_dir,
            self.run.run_id.as_str(),
            events.clone(),
        )
        .await;
        self.record_session_end_progress(result);
        self.maybe_prune_checkpoints();
        self.obs.last_run_report = self.finalize_observability(result, !events.is_empty());
    }

    /// Save run-scoped context metrics to `.theo/metrics/{run_id}.json`.
    async fn persist_context_metrics(&self) {
        let metrics_dir = self.project_dir.join(".theo").join("metrics");
        if let Err(e) = tokio::fs::create_dir_all(&metrics_dir).await {
            crate::fs_errors::emit_fs_error(
                &self.event_bus,
                self.run.run_id.as_str(),
                "record_session_exit/metrics_mkdir",
                &metrics_dir,
                &e,
            );
            return;
        }
        let report = self.obs.context_metrics.to_report();
        let metrics_path = metrics_dir.join(format!("{}.json", self.run.run_id.as_str()));
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

    /// Build EpisodeSummary from run events, persist to
    /// `.theo/memory/episodes/`, and run lesson/hypothesis pipelines.
    /// Memory namespace decision: meeting 20260420-221947 #4 — episodes
    /// belong to memory, not wiki.
    async fn persist_episode_summary(&self, events: &[theo_domain::event::DomainEvent]) {
        let task_objective = self
            .task_manager
            .get(&self.task_id)
            .map(|t| t.objective.clone())
            .unwrap_or_else(|| "unknown".to_string());
        let mut summary = theo_domain::episode::EpisodeSummary::from_events(
            self.run.run_id.as_str(),
            Some(self.task_id.as_str()),
            &task_objective,
            events,
        );
        let mut usage = self.rt.session_token_usage.clone();
        if let Some(c) = theo_domain::budget::known_model_cost(&self.config.llm().model) {
            usage.recompute_cost(&c);
        }
        summary.token_usage = Some(usage);
        let (lessons_persisted, lessons_attempted) =
            crate::lesson_pipeline::extract_and_persist_for_outcome(
                &self.project_dir,
                summary.machine_summary.outcome,
                events,
            );
        let hypotheses_persisted =
            crate::hypothesis_pipeline::persist_unresolved(&self.project_dir, &summary);
        if lessons_persisted > 0 || lessons_attempted > 0 || hypotheses_persisted > 0 {
            tracing::info!(
                lessons_persisted = lessons_persisted,
                lessons_attempted = lessons_attempted,
                hypotheses_persisted = hypotheses_persisted,
                "session shutdown: persisted learning artifacts"
            );
        }
        self.write_episode_to_disk(&summary).await;
    }

    async fn write_episode_to_disk(&self, summary: &theo_domain::episode::EpisodeSummary) {
        let episodes_dir = self
            .project_dir
            .join(".theo")
            .join("memory")
            .join("episodes");
        if let Err(e) = tokio::fs::create_dir_all(&episodes_dir).await {
            crate::fs_errors::emit_fs_error(
                &self.event_bus,
                self.run.run_id.as_str(),
                "record_session_exit/episode_mkdir",
                &episodes_dir,
                &e,
            );
            return;
        }
        let episode_path = episodes_dir.join(format!("{}.json", summary.summary_id));
        let body = serde_json::to_string_pretty(summary).unwrap_or_default();
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

    /// Cross-session progress tracker — only invoked from the parent
    /// (non-subagent) run so subagents don't pollute the parent's history.
    fn record_session_end_progress(&self, result: &AgentResult) {
        if self.config.loop_cfg().is_subagent {
            return;
        }
        let status = if result.success { "completed" } else { "failed" };
        let tasks = vec![crate::session_bootstrap::CompletedTask {
            name: result.summary.chars().take(100).collect(),
            status: status.to_string(),
            files_changed: result.files_edited.clone(),
        }];
        let last_error = if result.success {
            None
        } else {
            Some(result.summary.clone())
        };
        crate::session_bootstrap::record_session_end(
            &self.project_dir,
            self.run.run_id.as_str(),
            tasks,
            vec![],
            last_error,
        );
    }

    /// T3.5 / find_p5_005 — Prune shadow-git checkpoints so
    /// `.theo/checkpoints/` does not grow unbounded across sessions.
    /// Best-effort: failures log to tracing but don't block shutdown.
    fn maybe_prune_checkpoints(&self) {
        if self.config.loop_cfg().is_subagent {
            return;
        }
        let Some(ckpt) = self.subagent.checkpoint.as_deref() else {
            return;
        };
        let ttl = self.config.checkpoint_ttl_seconds as i64;
        if ttl <= 0 {
            return;
        }
        match ckpt.cleanup(ttl) {
            Ok(pruned) if pruned > 0 => {
                tracing::info!(
                    pruned = pruned,
                    ttl_seconds = ttl,
                    "checkpoint cleanup pruned stale entries"
                );
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "checkpoint cleanup failed at session shutdown"
                );
            }
        }
    }

    pub(super) fn finalize_observability(
        &mut self,
        result: &AgentResult,
        had_events: bool,
    ) -> Option<crate::observability::report::RunReport> {
        let pipeline = self.obs.pipeline.take()?;
        let file_path = pipeline.finalize();
        self.obs.episodes_created = if had_events { 1 } else { 0 };
        let (detected, run_report) = crate::observability::finalize_run_observability(
            &file_path,
            self.run.run_id.as_str(),
            result.success,
            result.files_edited.len() as u64,
            &self.rt.session_token_usage,
            self.config.loop_cfg().max_iterations,
            self.llm.budget_enforcer.usage(),
            &self.obs.context_metrics.to_report(),
            self.tracking.done_attempts,
            self.obs.episodes_injected,
            self.obs.episodes_created,
            self.tracking.failure_tracker.new_fingerprint_count(),
            self.tracking.failure_tracker.recurrent_fingerprint_count(),
            &self.obs.initial_context_files,
            &self.obs.pre_compaction_hot_files,
        );
        detected.publish_events(&self.event_bus, self.run.run_id.as_str());
        run_report
    }
}
