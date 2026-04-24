//! Autodream — post-session memory consolidation (of
//! `PLAN_AUTO_EVOLUTION_SOTA`).
//!
//! Pattern ported from `referencias/opendev/crates/opendev-agents/src/
//! memory_consolidation.rs` (441 LOC, Rust). Key differences from the
//! original plan text (which said "trigger on session_end"):
//!
//! - **Trigger at session START, not end.** OpenDev evidence shows this
//!   avoids slowing shutdown; session N's data is consolidated when
//!   session N+1 boots. We adopt the same model.
//! - **24h cooldown** between runs (`ConsolidationMeta.last_run`).
//! - **Lock file** (`.consolidation.lock`) prevents concurrent runs
//!   across processes.
//! - **Backup before mutation** — copy originals to `<memory>/.bak/`.
//! - **Never touches `user` or `reference` memories** — they're atomic
//!   and personal.
//! - **Iteration cap** — process at most `MAX_FILES_PER_RUN` session
//!   files in one pass.
//!
//! Safety: all consolidated content passes `security::scan_memory_body`
//! before persistence (delegated to the `AutodreamExecutor` impl so
//! this module stays free of the `theo-infra-memory` dep).
//!
//! Errors are logged via `eprintln!` and never propagate to the main
//! loop — autodream is best-effort.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Minimum number of episodic memory files to trigger consolidation.
/// Matches OpenDev `MIN_SESSION_FILES` (`memory_consolidation.rs:22`).
pub const MIN_EPISODIC_FILES: usize = 5;

/// Maximum episodic files processed per consolidation run. Matches
/// OpenDev `MAX_FILES_PER_RUN` (`memory_consolidation.rs:24`).
pub const MAX_FILES_PER_RUN: usize = 20;

/// Cooldown between consolidation runs. OpenDev default is 24h; we
/// honor the same to prevent thrashing.
pub const COOLDOWN_HOURS: i64 = 24;

/// Lock file name (relative to memory root).
pub const LOCK_FILE_NAME: &str = ".consolidation.lock";

/// Metadata file name (relative to memory root).
pub const META_FILE_NAME: &str = ".consolidation-meta.json";

/// Backup directory (relative to memory root).
pub const BACKUP_DIR_NAME: &str = ".bak";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Metadata persisted between consolidation runs.
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct ConsolidationMeta {
    /// Unix timestamp (seconds) of the last completed run.
    pub last_run_unix_secs: Option<u64>,
    /// Number of files processed in the last run.
    pub files_processed: usize,
}

/// Summary of a consolidation run. Returned to callers for telemetry
/// but never drives control flow.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConsolidationReport {
    pub files_consolidated: usize,
    pub files_pruned: usize,
    pub files_backed_up: usize,
    pub duration_ms: u64,
}

/// Reason an existing memory was flagged stale. Kept as an explicit
/// enum so future additions don't silently collapse into "other".
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StalenessReason {
    /// Evidence in the current session contradicts the memory.
    ContradictedByNewEvidence {
        memory_id: String,
        evidence_event: String,
    },
    /// A more authoritative lesson supersedes this memory.
    SupersededByLesson {
        memory_id: String,
        lesson_id: String,
    },
    /// TTL expired — purge regardless of content.
    ExpiredTtl { memory_id: String },
    /// Not referenced in N most recent turns — candidate for archival.
    UnreferencedForN {
        memory_id: String,
        turns_unused: u64,
    },
}

#[derive(Debug, Error)]
pub enum AutodreamError {
    #[error("autodream executor backend failure: {0}")]
    Backend(String),
    #[error("autodream exceeded timeout")]
    Timeout,
    #[error("autodream lock held by another process")]
    LockHeld,
    #[error("autodream filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("autodream meta parse error: {0}")]
    Meta(String),
}

/// Executor contract. Concrete implementations (e.g.
/// `LlmAutodreamExecutor` in `theo-application`) own the LLM
/// integration; this crate only wires lifecycle + locking.
#[async_trait]
pub trait AutodreamExecutor: Send + Sync {
    /// Consolidate eligible memories. Returns a report on success. The
    /// caller has already verified `should_consolidate`, acquired the
    /// lock, and is responsible for releasing it.
    async fn consolidate(
        &self,
        memory_dir: &Path,
        session_id: &str,
    ) -> Result<ConsolidationReport, AutodreamError>;

    /// Short identifier for tracing/testing.
    fn name(&self) -> &'static str;
}

/// No-op executor. Kept so callers can stub autodream without pulling
/// in an LLM client.
#[derive(Debug, Clone, Default)]
pub struct NullAutodreamExecutor;

#[async_trait]
impl AutodreamExecutor for NullAutodreamExecutor {
    async fn consolidate(
        &self,
        _memory_dir: &Path,
        _session_id: &str,
    ) -> Result<ConsolidationReport, AutodreamError> {
        Ok(ConsolidationReport::default())
    }

    fn name(&self) -> &'static str {
        "null"
    }
}

#[derive(Clone)]
pub struct AutodreamHandle(pub std::sync::Arc<dyn AutodreamExecutor>);

impl AutodreamHandle {
    pub fn new(exec: std::sync::Arc<dyn AutodreamExecutor>) -> Self {
        Self(exec)
    }

    pub fn as_executor(&self) -> &dyn AutodreamExecutor {
        self.0.as_ref()
    }
}

impl std::fmt::Debug for AutodreamHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("AutodreamHandle")
            .field(&self.0.name())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Pure helpers (no I/O, testable)
// ---------------------------------------------------------------------------

/// Parse a stored UNIX timestamp (seconds since epoch) and tell us
/// whether the cooldown window has elapsed. Returns `true` when
/// consolidation is allowed.
///
/// We persist simple `u64` seconds instead of RFC-3339 because
/// `theo-agent-runtime` has no `chrono` dependency and dragging one in
/// just for autodream would bloat every crate that depends on us.
pub fn cooldown_elapsed(last_run_unix_secs: Option<u64>, cooldown_hours: i64) -> bool {
    let Some(last) = last_run_unix_secs else {
        return true;
    };
    let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) else {
        return true;
    };
    let now_secs = now.as_secs();
    let cooldown_secs = (cooldown_hours.max(0) as u64).saturating_mul(3600);
    now_secs.saturating_sub(last) >= cooldown_secs
}

/// Aggregate the three boolean conditions OpenDev checks in
/// `should_consolidate`. Pure, easily testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsolidationGate {
    /// All preconditions met; consolidation should run.
    Go,
    /// Another process holds the lock.
    LockHeld,
    /// Too soon since the last run.
    CooldownActive,
    /// Not enough session files to justify a run.
    InsufficientFiles { found: usize, required: usize },
    /// Memory directory missing.
    NoMemoryDir,
}

/// Decide whether consolidation should run, given resolved inputs.
///
/// Pure function — accepts the booleans the caller has already
/// computed. Wire-up helpers turn filesystem state into these
/// booleans.
pub fn evaluate_gate(
    memory_dir_exists: bool,
    lock_file_exists: bool,
    cooldown_ok: bool,
    session_file_count: usize,
) -> ConsolidationGate {
    if !memory_dir_exists {
        return ConsolidationGate::NoMemoryDir;
    }
    if lock_file_exists {
        return ConsolidationGate::LockHeld;
    }
    if !cooldown_ok {
        return ConsolidationGate::CooldownActive;
    }
    if session_file_count < MIN_EPISODIC_FILES {
        return ConsolidationGate::InsufficientFiles {
            found: session_file_count,
            required: MIN_EPISODIC_FILES,
        };
    }
    ConsolidationGate::Go
}

// ---------------------------------------------------------------------------
// Filesystem helpers (isolated + well-named so phase-2 tests can exercise
// them directly)
// ---------------------------------------------------------------------------

/// Path to the lock file.
pub fn lock_path(memory_dir: &Path) -> PathBuf {
    memory_dir.join(LOCK_FILE_NAME)
}

/// Path to the meta file.
pub fn meta_path(memory_dir: &Path) -> PathBuf {
    memory_dir.join(META_FILE_NAME)
}

/// Path to the backup directory (created on demand).
pub fn backup_dir(memory_dir: &Path) -> PathBuf {
    memory_dir.join(BACKUP_DIR_NAME)
}

/// Load metadata; returns defaults when file missing or malformed.
pub fn load_meta(memory_dir: &Path) -> ConsolidationMeta {
    let path = meta_path(memory_dir);
    match std::fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => ConsolidationMeta::default(),
    }
}

/// Persist metadata. Silent on I/O failures (best-effort).
pub fn save_meta(memory_dir: &Path, meta: &ConsolidationMeta) -> Result<(), AutodreamError> {
    let path = meta_path(memory_dir);
    let raw = serde_json::to_string_pretty(meta)
        .map_err(|e| AutodreamError::Meta(e.to_string()))?;
    std::fs::write(&path, raw)?;
    Ok(())
}

/// Acquire the consolidation lock using `create_new` semantics. Returns
/// `Err(LockHeld)` when the lock already exists so the caller can
/// skip cleanly.
pub fn acquire_lock(memory_dir: &Path) -> Result<LockGuard, AutodreamError> {
    let path = lock_path(memory_dir);
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(_f) => Ok(LockGuard { path }),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            // Stale lock recovery — if the file is older than 2×
            // cooldown_hours, treat as orphan from a crashed run.
            if lock_is_stale(&path, COOLDOWN_HOURS * 2) {
                if let Err(e) = std::fs::remove_file(&path) {
                    crate::fs_errors::warn_fs_error(
                        "autodream/stale_lock_rm",
                        &path,
                        &e,
                    );
                }
                return acquire_lock(memory_dir);
            }
            Err(AutodreamError::LockHeld)
        }
        Err(e) => Err(e.into()),
    }
}

fn lock_is_stale(lock_path: &Path, max_age_hours: i64) -> bool {
    let Ok(meta) = std::fs::metadata(lock_path) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    let Ok(elapsed) = modified.elapsed() else {
        return false;
    };
    elapsed.as_secs() > (max_age_hours.max(0) as u64).saturating_mul(3600)
}

/// RAII-ish guard that releases the lock on drop.
#[must_use = "holding a LockGuard without using it is a bug; drop it only after consolidation completes"]
pub struct LockGuard {
    path: PathBuf,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        // Stderr log on drop — never panic inside Drop.
        if let Err(e) = std::fs::remove_file(&self.path) {
            crate::fs_errors::warn_fs_error("autodream/lock_release", &self.path, &e);
        }
    }
}

/// Count files in `memory_dir` whose frontmatter declares
/// `type: session`. We use a quick-and-dirty string match rather than a
/// full YAML parse — matches OpenDev's heuristic.
pub fn count_session_files(memory_dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(memory_dir) else {
        return 0;
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("md"))
        .filter(|e| {
            std::fs::read_to_string(e.path())
                .map(|body| body.contains("type: session") || body.contains("type: episodic"))
                .unwrap_or(false)
        })
        .count()
}

/// Back up a file to `<memory>/.bak/<filename>`. Used before any
/// mutation per OpenDev protocol.
pub fn backup_file(memory_dir: &Path, source: &Path) -> Result<(), AutodreamError> {
    let dir = backup_dir(memory_dir);
    std::fs::create_dir_all(&dir)?;
    let Some(name) = source.file_name() else {
        return Err(AutodreamError::Meta("backup source lacks filename".into()));
    };
    let dest = dir.join(name);
    std::fs::copy(source, dest)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Top-level orchestration (session-start entry point)
// ---------------------------------------------------------------------------

/// Full session-start dream sequence: lock → evaluate → execute →
/// update meta → release lock. Fire-and-forget friendly: any error
/// becomes a log line and the main loop is unaffected.
///
/// Returns `Ok(None)` when consolidation was skipped (lock held,
/// cooldown, insufficient files). Returns `Ok(Some(report))` on a
/// completed run.
pub async fn run_autodream(
    memory_dir: &Path,
    session_id: &str,
    executor: &dyn AutodreamExecutor,
) -> Result<Option<ConsolidationReport>, AutodreamError> {
    let memory_dir_exists = memory_dir.is_dir();
    let lock_file_exists = lock_path(memory_dir).exists();
    let meta = load_meta(memory_dir);
    let cooldown_ok = cooldown_elapsed(meta.last_run_unix_secs, COOLDOWN_HOURS);
    let session_count = if memory_dir_exists {
        count_session_files(memory_dir)
    } else {
        0
    };

    match evaluate_gate(memory_dir_exists, lock_file_exists, cooldown_ok, session_count) {
        ConsolidationGate::Go => {}
        ConsolidationGate::NoMemoryDir => return Ok(None),
        ConsolidationGate::LockHeld => return Err(AutodreamError::LockHeld),
        ConsolidationGate::CooldownActive => return Ok(None),
        ConsolidationGate::InsufficientFiles { .. } => return Ok(None),
    }

    let _lock = acquire_lock(memory_dir)?;
    let start = std::time::Instant::now();
    let mut report = executor.consolidate(memory_dir, session_id).await?;
    report.duration_ms = start.elapsed().as_millis() as u64;

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let new_meta = ConsolidationMeta {
        last_run_unix_secs: Some(now_secs),
        files_processed: report.files_consolidated,
    };
    save_meta(memory_dir, &new_meta)?;
    Ok(Some(report))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs()
    }

    // ── AC-2.6 & AC-2.8 ────────────────────────────────────────────
    #[test]
    fn test_cooldown_elapsed_none_means_never_run() {
        assert!(cooldown_elapsed(None, 24));
    }

    #[test]
    fn test_cooldown_elapsed_recent_blocks() {
        assert!(!cooldown_elapsed(Some(now_secs()), 24));
    }

    #[test]
    fn test_cooldown_elapsed_old_allows() {
        // 25 hours ago.
        let past = now_secs().saturating_sub(25 * 3600);
        assert!(cooldown_elapsed(Some(past), 24));
    }

    #[test]
    fn test_cooldown_elapsed_future_timestamp_does_not_panic() {
        // Clock skew: stored timestamp is in the future; saturating_sub
        // handles it, so we treat it as "recent" (=blocks).
        let future = now_secs().saturating_add(24 * 3600);
        assert!(!cooldown_elapsed(Some(future), 24));
    }

    // ── AC-2.7 gate logic ──────────────────────────────────────────
    #[test]
    fn test_gate_lock_held_stops_run() {
        let g = evaluate_gate(true, true, true, 10);
        assert_eq!(g, ConsolidationGate::LockHeld);
    }

    #[test]
    fn test_gate_cooldown_stops_run() {
        let g = evaluate_gate(true, false, false, 10);
        assert_eq!(g, ConsolidationGate::CooldownActive);
    }

    #[test]
    fn test_gate_insufficient_files_stops_run() {
        let g = evaluate_gate(true, false, true, MIN_EPISODIC_FILES - 1);
        assert_eq!(
            g,
            ConsolidationGate::InsufficientFiles {
                found: MIN_EPISODIC_FILES - 1,
                required: MIN_EPISODIC_FILES
            }
        );
    }

    #[test]
    fn test_gate_missing_dir_stops_run() {
        let g = evaluate_gate(false, false, true, 99);
        assert_eq!(g, ConsolidationGate::NoMemoryDir);
    }

    #[test]
    fn test_gate_go_when_all_conditions_met() {
        let g = evaluate_gate(true, false, true, MIN_EPISODIC_FILES);
        assert_eq!(g, ConsolidationGate::Go);
    }

    // ── Lock acquire/release ───────────────────────────────────────
    #[test]
    fn test_acquire_lock_then_second_acquire_fails() {
        let tmp = tempfile::tempdir().expect("tmp");
        let guard = acquire_lock(tmp.path()).expect("first acquire");
        assert!(lock_path(tmp.path()).exists());

        let second = acquire_lock(tmp.path());
        assert!(matches!(second, Err(AutodreamError::LockHeld)));

        drop(guard);
        assert!(!lock_path(tmp.path()).exists(), "lock must release on drop");
    }

    // ── Meta round-trip ────────────────────────────────────────────
    #[test]
    fn test_meta_save_and_load_round_trip() {
        let tmp = tempfile::tempdir().expect("tmp");
        let meta = ConsolidationMeta {
            last_run_unix_secs: Some(1_745_000_000),
            files_processed: 7,
        };
        save_meta(tmp.path(), &meta).expect("save");
        let loaded = load_meta(tmp.path());
        assert_eq!(loaded, meta);
    }

    #[test]
    fn test_load_meta_returns_default_when_missing() {
        let tmp = tempfile::tempdir().expect("tmp");
        let loaded = load_meta(tmp.path());
        assert_eq!(loaded, ConsolidationMeta::default());
    }

    // ── Session file counting (AC-2.8 input) ───────────────────────
    #[test]
    fn test_count_session_files_matches_frontmatter_type() {
        let tmp = tempfile::tempdir().expect("tmp");
        std::fs::write(
            tmp.path().join("a.md"),
            "---\ntype: session\n---\nbody a",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("b.md"),
            "---\ntype: episodic\n---\nbody b",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("c.md"),
            "---\ntype: user\n---\nbody c",
        )
        .unwrap();
        std::fs::write(tmp.path().join("readme.txt"), "not md").unwrap();

        assert_eq!(count_session_files(tmp.path()), 2);
    }

    // ── AC-2.9 backup ──────────────────────────────────────────────
    #[test]
    fn test_backup_file_copies_into_bak_dir() {
        let tmp = tempfile::tempdir().expect("tmp");
        let src = tmp.path().join("original.md");
        std::fs::write(&src, "hello").unwrap();
        backup_file(tmp.path(), &src).expect("backup");
        let dest = backup_dir(tmp.path()).join("original.md");
        assert!(dest.exists());
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "hello");
    }

    // ── AC-2.1 + AC-2.3 end-to-end with null executor ──────────────
    #[tokio::test]
    async fn test_run_autodream_skips_when_no_dir() {
        let tmp = tempfile::tempdir().expect("tmp");
        let missing = tmp.path().join("not-created");
        let report = run_autodream(&missing, "s1", &NullAutodreamExecutor)
            .await
            .expect("ok");
        assert!(report.is_none());
    }

    #[tokio::test]
    async fn test_run_autodream_skips_when_cooldown_active() {
        let tmp = tempfile::tempdir().expect("tmp");
        let meta = ConsolidationMeta {
            last_run_unix_secs: Some(now_secs()),
            files_processed: 0,
        };
        save_meta(tmp.path(), &meta).unwrap();

        let out = run_autodream(tmp.path(), "s1", &NullAutodreamExecutor)
            .await
            .expect("ok");
        assert!(out.is_none(), "cooldown must suppress run");
    }

    #[tokio::test]
    async fn test_run_autodream_skips_when_insufficient_files() {
        let tmp = tempfile::tempdir().expect("tmp");
        // Only 2 session files — below MIN_EPISODIC_FILES (5).
        for i in 0..2 {
            std::fs::write(
                tmp.path().join(format!("s{i}.md")),
                "---\ntype: session\n---\nbody",
            )
            .unwrap();
        }
        let out = run_autodream(tmp.path(), "s1", &NullAutodreamExecutor)
            .await
            .expect("ok");
        assert!(out.is_none());
    }

    #[tokio::test]
    async fn test_run_autodream_executes_and_persists_meta() {
        let tmp = tempfile::tempdir().expect("tmp");
        for i in 0..MIN_EPISODIC_FILES {
            std::fs::write(
                tmp.path().join(format!("s{i}.md")),
                "---\ntype: session\n---\nbody",
            )
            .unwrap();
        }
        let report = run_autodream(tmp.path(), "s1", &NullAutodreamExecutor)
            .await
            .expect("ok")
            .expect("must run");
        assert_eq!(report.files_consolidated, 0); // Null executor is a no-op.

        let loaded = load_meta(tmp.path());
        assert!(
            loaded.last_run_unix_secs.is_some(),
            "meta must persist last_run"
        );
    }
}
