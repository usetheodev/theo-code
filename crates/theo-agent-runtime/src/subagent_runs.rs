//! Sub-agent run persistence — Track C.
//!
//! Persiste cada execucao de sub-agent (started → completed | failed |
//! cancelled | abandoned) com todos os eventos para suportar `theo run resume`.
//!
//! Design: file-based JSON-per-run + JSONL events (alinhado com
//! `FileSnapshotStore` em `persistence.rs`). Evita sqlx + migrations —
//! mantém escopo controlado. Schema sql-equivalente esta documentado em
//! agents-plan.md Fase 10 para futura migracao opcional.
//!
//! Reference: Archon `workflow_runs` + `workflow_events` (CLAUDE.md
//! "Database Schema"). CLI: `bun run cli workflow resume <run-id>`.
//!
//! Principio Archon "No Autonomous Lifecycle Mutation": NAO marcamos
//! runs como failed automaticamente baseado em timeouts. O usuario
//! decide via `theo run abandon`.

use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use theo_domain::agent_spec::AgentSpec;

#[derive(Debug, Error)]
pub enum RunStoreError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("run not found: {0}")]
    NotFound(String),
}

/// Status of a sub-agent run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
    Abandoned,
}

impl RunStatus {
    /// True if the status is terminal (no longer mutable except by user via abandon).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            RunStatus::Completed | RunStatus::Failed | RunStatus::Cancelled | RunStatus::Abandoned
        )
    }
}

/// Single sub-agent run record (rows of `subagent_runs` in the planned schema).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentRun {
    pub run_id: String,
    pub parent_run_id: Option<String>,
    pub agent_name: String,
    pub agent_source: String,
    pub objective: String,
    pub status: RunStatus,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub iterations_used: usize,
    pub tokens_used: u64,
    pub summary: Option<String>,
    /// JSON output if `output_format` was used.
    pub structured_output: Option<serde_json::Value>,
    pub cwd: String,
    pub checkpoint_before: Option<String>,
    /// Frozen `AgentSpec` snapshot used for resume (config_snapshot).
    pub config_snapshot: AgentSpec,
}

impl SubagentRun {
    /// Create a new running sub-agent record.
    pub fn new_running(
        run_id: impl Into<String>,
        parent_run_id: Option<String>,
        spec: &AgentSpec,
        objective: impl Into<String>,
        cwd: impl Into<String>,
        checkpoint_before: Option<String>,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            parent_run_id,
            agent_name: spec.name.clone(),
            agent_source: spec.source.as_str().to_string(),
            objective: objective.into(),
            status: RunStatus::Running,
            started_at: now_unix(),
            finished_at: None,
            iterations_used: 0,
            tokens_used: 0,
            summary: None,
            structured_output: None,
            cwd: cwd.into(),
            checkpoint_before,
            config_snapshot: spec.clone(),
        }
    }
}

/// Single event in the run's history (rows of `subagent_events`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentEvent {
    pub timestamp: i64,
    pub event_type: String,
    pub payload: serde_json::Value,
}

/// File-based store. Layout:
/// ```text
/// {base}/runs/{run_id}.json         # SubagentRun
/// {base}/runs/{run_id}.events.jsonl # append-only SubagentEvent stream
/// ```
pub struct FileSubagentRunStore {
    base_dir: PathBuf,
}

impl FileSubagentRunStore {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    pub fn runs_dir(&self) -> PathBuf {
        self.base_dir.join("runs")
    }

    fn run_path(&self, run_id: &str) -> PathBuf {
        self.runs_dir().join(format!("{}.json", run_id))
    }

    fn events_path(&self, run_id: &str) -> PathBuf {
        self.runs_dir().join(format!("{}.events.jsonl", run_id))
    }

    fn ensure_dir(&self) -> io::Result<()> {
        fs::create_dir_all(self.runs_dir())
    }

    /// Save a run record (overwrite if exists).
    pub fn save(&self, run: &SubagentRun) -> Result<(), RunStoreError> {
        self.ensure_dir()?;
        let json = serde_json::to_string_pretty(run)?;
        fs::write(self.run_path(&run.run_id), json)?;
        Ok(())
    }

    /// Load a run by id.
    pub fn load(&self, run_id: &str) -> Result<SubagentRun, RunStoreError> {
        let path = self.run_path(run_id);
        if !path.exists() {
            return Err(RunStoreError::NotFound(run_id.to_string()));
        }
        let content = fs::read_to_string(&path)?;
        let run = serde_json::from_str(&content)?;
        Ok(run)
    }

    /// List all run ids in storage (newest first by mtime).
    pub fn list(&self) -> Result<Vec<String>, RunStoreError> {
        if !self.runs_dir().exists() {
            return Ok(Vec::new());
        }
        let mut entries: Vec<(PathBuf, std::time::SystemTime)> = fs::read_dir(self.runs_dir())?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s == "json")
                    .unwrap_or(false)
                    && !e.path().to_string_lossy().contains(".events.")
            })
            .filter_map(|e| {
                let path = e.path();
                let mtime = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()?;
                Some((path, mtime))
            })
            .collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1));
        Ok(entries
            .into_iter()
            .filter_map(|(p, _)| {
                p.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string())
            })
            .collect())
    }

    /// Append an event to the run's event log (atomic per-line write).
    pub fn append_event(&self, run_id: &str, event: &SubagentEvent) -> Result<(), RunStoreError> {
        self.ensure_dir()?;
        let line = serde_json::to_string(event)?;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.events_path(run_id))?;
        writeln!(file, "{}", line)?;
        Ok(())
    }

    /// Read the full event history of a run.
    pub fn list_events(&self, run_id: &str) -> Result<Vec<SubagentEvent>, RunStoreError> {
        let path = self.events_path(run_id);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(path)?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let event: SubagentEvent = serde_json::from_str(&line)?;
            events.push(event);
        }
        Ok(events)
    }

    /// Mark a run as abandoned (user-driven). Idempotent on terminal status.
    pub fn abandon(&self, run_id: &str) -> Result<SubagentRun, RunStoreError> {
        let mut run = self.load(run_id)?;
        if !run.status.is_terminal() {
            run.status = RunStatus::Abandoned;
            run.finished_at = Some(now_unix());
            self.save(&run)?;
        }
        Ok(run)
    }

    /// Cleanup terminal runs older than `max_age_seconds`.
    /// Returns the count of removed runs.
    ///
    /// Archon principle: NEVER touches non-terminal status (running) regardless
    /// of age — surfaces ambiguity instead of mutating autonomously.
    pub fn cleanup(&self, max_age_seconds: i64) -> Result<usize, RunStoreError> {
        let now = now_unix();
        let cutoff = now - max_age_seconds;
        let ids = self.list()?;
        let mut removed = 0;
        for id in ids {
            let run = match self.load(&id) {
                Ok(r) => r,
                Err(_) => continue,
            };
            // Skip non-terminal: cannot distinguish "running elsewhere" from "orphan"
            if !run.status.is_terminal() {
                continue;
            }
            let finished = run.finished_at.unwrap_or(run.started_at);
            if finished < cutoff {
                let _ = fs::remove_file(self.run_path(&id));
                let _ = fs::remove_file(self.events_path(&id));
                removed += 1;
            }
        }
        Ok(removed)
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use theo_domain::agent_spec::AgentSpecSource;

    fn fixture_run() -> SubagentRun {
        let spec = AgentSpec::on_demand("explorer", "scan src/");
        SubagentRun::new_running("run-1", None, &spec, "scan src/", "/tmp/proj", None)
    }

    #[test]
    fn run_status_is_terminal_correct() {
        assert!(!RunStatus::Running.is_terminal());
        assert!(RunStatus::Completed.is_terminal());
        assert!(RunStatus::Failed.is_terminal());
        assert!(RunStatus::Cancelled.is_terminal());
        assert!(RunStatus::Abandoned.is_terminal());
    }

    #[test]
    fn run_new_running_uses_spec_metadata() {
        let run = fixture_run();
        assert_eq!(run.agent_name, "explorer");
        assert_eq!(run.agent_source, "on_demand");
        assert_eq!(run.status, RunStatus::Running);
        assert_eq!(run.config_snapshot.source, AgentSpecSource::OnDemand);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let store = FileSubagentRunStore::new(dir.path());
        let run = fixture_run();
        store.save(&run).unwrap();
        let back = store.load(&run.run_id).unwrap();
        assert_eq!(back.run_id, run.run_id);
        assert_eq!(back.agent_name, run.agent_name);
        assert_eq!(back.config_snapshot.name, run.config_snapshot.name);
    }

    #[test]
    fn load_unknown_returns_not_found() {
        let dir = TempDir::new().unwrap();
        let store = FileSubagentRunStore::new(dir.path());
        let err = store.load("missing").unwrap_err();
        assert!(matches!(err, RunStoreError::NotFound(_)));
    }

    #[test]
    fn list_returns_empty_for_new_dir() {
        let dir = TempDir::new().unwrap();
        let store = FileSubagentRunStore::new(dir.path());
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn list_returns_all_run_ids() {
        let dir = TempDir::new().unwrap();
        let store = FileSubagentRunStore::new(dir.path());
        let mut a = fixture_run();
        a.run_id = "a".into();
        let mut b = fixture_run();
        b.run_id = "b".into();
        store.save(&a).unwrap();
        store.save(&b).unwrap();
        let mut ids = store.list().unwrap();
        ids.sort();
        assert_eq!(ids, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn append_event_appends_to_jsonl() {
        let dir = TempDir::new().unwrap();
        let store = FileSubagentRunStore::new(dir.path());
        store.save(&fixture_run()).unwrap();
        let event = SubagentEvent {
            timestamp: 100,
            event_type: "iteration_started".into(),
            payload: serde_json::json!({"iteration": 1}),
        };
        store.append_event("run-1", &event).unwrap();
        store.append_event("run-1", &event).unwrap();
        let events = store.list_events("run-1").unwrap();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn list_events_empty_for_no_events() {
        let dir = TempDir::new().unwrap();
        let store = FileSubagentRunStore::new(dir.path());
        store.save(&fixture_run()).unwrap();
        let events = store.list_events("run-1").unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn abandon_marks_running_as_abandoned() {
        let dir = TempDir::new().unwrap();
        let store = FileSubagentRunStore::new(dir.path());
        store.save(&fixture_run()).unwrap();
        let abandoned = store.abandon("run-1").unwrap();
        assert_eq!(abandoned.status, RunStatus::Abandoned);
        assert!(abandoned.finished_at.is_some());
    }

    #[test]
    fn abandon_idempotent_on_terminal_status() {
        let dir = TempDir::new().unwrap();
        let store = FileSubagentRunStore::new(dir.path());
        let mut r = fixture_run();
        r.status = RunStatus::Completed;
        r.finished_at = Some(now_unix() - 100);
        store.save(&r).unwrap();
        let after = store.abandon("run-1").unwrap();
        assert_eq!(after.status, RunStatus::Completed); // unchanged
    }

    #[test]
    fn cleanup_preserves_running_regardless_of_age() {
        // Archon principle: never auto-mutate non-terminal state
        let dir = TempDir::new().unwrap();
        let store = FileSubagentRunStore::new(dir.path());
        let mut old_running = fixture_run();
        old_running.run_id = "old".into();
        old_running.started_at = 100; // very old
        store.save(&old_running).unwrap();
        let removed = store.cleanup(60).unwrap(); // anything older than 60s
        assert_eq!(removed, 0, "running runs must NEVER be auto-cleaned");
        assert!(store.load("old").is_ok());
    }

    #[test]
    fn cleanup_removes_old_terminal_runs() {
        let dir = TempDir::new().unwrap();
        let store = FileSubagentRunStore::new(dir.path());
        let mut old = fixture_run();
        old.run_id = "old-completed".into();
        old.status = RunStatus::Completed;
        old.started_at = 100;
        old.finished_at = Some(200);
        store.save(&old).unwrap();
        let removed = store.cleanup(60).unwrap();
        assert_eq!(removed, 1);
        assert!(store.load("old-completed").is_err());
    }

    #[test]
    fn cleanup_keeps_recent_terminal_runs() {
        let dir = TempDir::new().unwrap();
        let store = FileSubagentRunStore::new(dir.path());
        let mut recent = fixture_run();
        recent.run_id = "recent".into();
        recent.status = RunStatus::Completed;
        recent.finished_at = Some(now_unix());
        store.save(&recent).unwrap();
        let removed = store.cleanup(60).unwrap();
        assert_eq!(removed, 0);
        assert!(store.load("recent").is_ok());
    }

    #[test]
    fn config_snapshot_preserved_for_resume() {
        // Resume requires the original AgentSpec — verify it's preserved
        let dir = TempDir::new().unwrap();
        let store = FileSubagentRunStore::new(dir.path());
        let spec = AgentSpec::on_demand("custom-name", "weird obj");
        let run = SubagentRun::new_running("r", None, &spec, "weird obj", "/tmp", None);
        store.save(&run).unwrap();
        let loaded = store.load("r").unwrap();
        assert_eq!(loaded.config_snapshot.name, "custom-name");
        assert_eq!(loaded.config_snapshot.max_iterations, 10); // on-demand cap
        assert_eq!(loaded.config_snapshot.timeout_secs, 120);
    }
}
