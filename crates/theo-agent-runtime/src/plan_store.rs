//! Persistence for `theo_domain::plan::Plan` — schema-validated JSON I/O.
//!
//! Mirrors the conventions of `roadmap.rs` (atomic write via temp + rename)
//! but operates on canonical JSON instead of markdown string-matching.
//!
//! Layout convention:
//!
//! ```text
//! .theo/plans/
//!   plan.json       ← canonical Plan (this module)
//!   findings.json   ← runtime-only PlanFindings (plan_findings.rs)
//!   progress.json   ← runtime-only PlanProgress (plan_progress.rs)
//!   <legacy>.md     ← deprecated markdown plans (still parsable via roadmap.rs)
//! ```
//!
//! See `docs/plans/sota-planning-system.md` Fase 2.

use std::path::{Path, PathBuf};

use theo_domain::plan::{PLAN_FORMAT_VERSION, Plan, PlanError};

/// Loads a Plan from a JSON file. Validates schema + version + invariants.
pub fn load_plan(path: &Path) -> Result<Plan, PlanError> {
    let content = std::fs::read_to_string(path)?;
    let plan: Plan = serde_json::from_str(&content)
        .map_err(|e| PlanError::InvalidFormat(e.to_string()))?;
    if plan.version > PLAN_FORMAT_VERSION {
        return Err(PlanError::UnsupportedVersion {
            found: plan.version,
            max_supported: PLAN_FORMAT_VERSION,
        });
    }
    plan.validate()?;
    Ok(plan)
}

/// Saves a Plan as pretty-printed JSON. Atomic: write to `<path>.tmp`, then
/// rename. The plan is `validate()`'d first — invalid plans never hit disk.
pub fn save_plan(path: &Path, plan: &Plan) -> Result<(), PlanError> {
    plan.validate()?;
    write_plan_atomic(path, plan)
}

/// T7.1 — Compare-and-swap save: writes only if the on-disk plan's
/// `version_counter` equals `expected`. Otherwise returns
/// `PlanError::VersionMismatch` so a parallel worker that lost the race
/// can reload and retry.
///
/// The race is closed for **single-process, single-fs** scenarios: read,
/// claim, swap. Multi-host coordination is out of scope (Theo runs
/// per-developer, single fs).
///
/// Special case: when the on-disk file does NOT exist, the save proceeds
/// only when `expected == 0` (the "fresh plan" semantic). Any other
/// expected value yields `VersionMismatch { expected, actual: 0 }`.
pub fn save_plan_if_version(
    path: &Path,
    plan: &Plan,
    expected: u64,
) -> Result<(), PlanError> {
    plan.validate()?;
    let actual = on_disk_version(path)?;
    if actual != expected {
        return Err(PlanError::VersionMismatch { expected, actual });
    }
    write_plan_atomic(path, plan)
}

/// Read the `version_counter` of the on-disk plan, or `0` when the file
/// does not exist. Other IO errors propagate.
fn on_disk_version(path: &Path) -> Result<u64, PlanError> {
    if !path.exists() {
        return Ok(0);
    }
    let content = std::fs::read_to_string(path)?;
    let plan: Plan = serde_json::from_str(&content)
        .map_err(|e| PlanError::InvalidFormat(e.to_string()))?;
    Ok(plan.version_counter)
}

/// Pure write helper — atomic temp+rename, parent dir created on demand.
/// Caller is responsible for `validate()` and CAS checks.
fn write_plan_atomic(path: &Path, plan: &Plan) -> Result<(), PlanError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(plan)
        .map_err(|e| PlanError::InvalidFormat(e.to_string()))?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, json.as_bytes())?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

/// Returns the most recent plan file under `<project>/.theo/plans/`. Prefers
/// `.json` (canonical) over `.md` (legacy `roadmap.rs`).
///
/// Returns `None` when neither format is present.
pub fn find_latest_plan(project_dir: &Path) -> Option<PathBuf> {
    let plans_dir = project_dir.join(".theo").join("plans");
    if let Some(json_plan) = find_latest_by_ext(&plans_dir, "json") {
        return Some(json_plan);
    }
    find_latest_by_ext(&plans_dir, "md")
}

fn find_latest_by_ext(dir: &Path, ext: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some(ext))
        // Skip the temp file produced by save_plan().
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| !n.ends_with(".json.tmp"))
                .unwrap_or(true)
        })
        .collect();
    files.sort();
    files.last().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use theo_domain::identifiers::{PhaseId, PlanTaskId};
    use theo_domain::plan::{Phase, PhaseStatus, PlanTask, PlanTaskStatus};

    fn sample_plan() -> Plan {
        Plan {
            version: PLAN_FORMAT_VERSION,
            title: "Persisted plan".into(),
            goal: "Round-trip plans through disk".into(),
            current_phase: PhaseId(1),
            phases: vec![Phase {
                id: PhaseId(1),
                title: "Phase 1".into(),
                status: PhaseStatus::InProgress,
                tasks: vec![PlanTask {
                    id: PlanTaskId(1),
                    title: "First task".into(),
                    status: PlanTaskStatus::Pending,
                    files: vec!["src/lib.rs".into()],
                    description: "Do the thing".into(),
                    dod: "Tests pass".into(),
                    depends_on: vec![],
                    rationale: String::new(),
                    outcome: None,
                    assignee: None,
                }],
            }],
            decisions: vec![],
            created_at: 1_000,
            updated_at: 1_000,
            version_counter: 0,
        }
    }

    // ----- RED 13 -----
    #[test]
    fn test_load_plan_from_json_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plan.json");
        let plan = sample_plan();
        let json = serde_json::to_string_pretty(&plan).unwrap();
        std::fs::write(&path, json).unwrap();

        let loaded = load_plan(&path).unwrap();
        assert_eq!(loaded, plan);
    }

    // ----- RED 14 -----
    #[test]
    fn test_save_plan_atomic_write() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plan.json");
        let plan = sample_plan();

        save_plan(&path, &plan).unwrap();

        // file exists with correct content
        assert!(path.exists());
        let loaded = load_plan(&path).unwrap();
        assert_eq!(loaded, plan);

        // no leftover .json.tmp
        let temp = path.with_extension("json.tmp");
        assert!(!temp.exists(), "temp file should be removed after rename");
    }

    // ----- RED 15 -----
    #[test]
    fn test_load_plan_rejects_invalid_json() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plan.json");
        std::fs::write(&path, "{ this is not valid").unwrap();

        let err = load_plan(&path).unwrap_err();
        assert!(matches!(err, PlanError::InvalidFormat(_)));
    }

    // ----- RED 16 -----
    #[test]
    fn test_load_plan_rejects_future_version() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plan.json");
        let mut plan = sample_plan();
        plan.version = 999;
        std::fs::write(&path, serde_json::to_string(&plan).unwrap()).unwrap();

        let err = load_plan(&path).unwrap_err();
        match err {
            PlanError::UnsupportedVersion { found, max_supported } => {
                assert_eq!(found, 999);
                assert_eq!(max_supported, PLAN_FORMAT_VERSION);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    // ----- RED 17 -----
    #[test]
    fn test_find_latest_plan_prefers_json() {
        let dir = tempdir().unwrap();
        let plans = dir.path().join(".theo").join("plans");
        std::fs::create_dir_all(&plans).unwrap();
        std::fs::write(plans.join("aaa.md"), "legacy").unwrap();
        std::fs::write(plans.join("zzz.json"), "{}").unwrap();

        let found = find_latest_plan(dir.path()).unwrap();
        assert_eq!(found.extension().unwrap(), "json");
    }

    #[test]
    fn test_find_latest_plan_falls_back_to_md() {
        let dir = tempdir().unwrap();
        let plans = dir.path().join(".theo").join("plans");
        std::fs::create_dir_all(&plans).unwrap();
        std::fs::write(plans.join("legacy.md"), "legacy").unwrap();

        let found = find_latest_plan(dir.path()).unwrap();
        assert_eq!(found.extension().unwrap(), "md");
    }

    #[test]
    fn test_find_latest_plan_none_when_empty() {
        let dir = tempdir().unwrap();
        assert!(find_latest_plan(dir.path()).is_none());
    }

    #[test]
    fn test_find_latest_plan_skips_tmp_files() {
        let dir = tempdir().unwrap();
        let plans = dir.path().join(".theo").join("plans");
        std::fs::create_dir_all(&plans).unwrap();
        std::fs::write(plans.join("plan.json.tmp"), "{}").unwrap();
        assert!(find_latest_plan(dir.path()).is_none());
    }

    #[test]
    fn test_save_plan_rejects_invalid_plan() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plan.json");
        let mut plan = sample_plan();
        plan.title = String::new(); // invalidate

        let err = save_plan(&path, &plan).unwrap_err();
        assert!(matches!(err, PlanError::Validation(_)));
        assert!(!path.exists(), "invalid plan must never reach disk");
    }

    #[test]
    fn test_save_plan_creates_parent_dir() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("plan.json");
        let plan = sample_plan();
        save_plan(&path, &plan).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_load_plan_io_error_when_missing() {
        let err = load_plan(Path::new("/nonexistent/path/plan.json")).unwrap_err();
        assert!(matches!(err, PlanError::Io(_)));
    }

    #[test]
    fn test_load_plan_rejects_validation_failure() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plan.json");
        // Plan with duplicate task IDs — passes JSON parse, fails validate.
        let json = r#"{
            "version": 1,
            "title": "Bad",
            "goal": "x",
            "current_phase": 1,
            "phases": [{
                "id": 1,
                "title": "P1",
                "status": "in_progress",
                "tasks": [
                    {"id": 1, "title": "A", "status": "pending"},
                    {"id": 1, "title": "B", "status": "pending"}
                ]
            }],
            "created_at": 0,
            "updated_at": 0
        }"#;
        std::fs::write(&path, json).unwrap();

        let err = load_plan(&path).unwrap_err();
        assert!(matches!(err, PlanError::Validation(_)));
    }

    // ----- T7.1: save_plan_if_version (CAS) -----

    #[test]
    fn t71_save_if_version_succeeds_when_expected_matches() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plan.json");
        let mut plan = sample_plan();
        plan.version_counter = 5;

        // Seed: write directly with current version_counter.
        save_plan(&path, &plan).unwrap();
        assert_eq!(on_disk_version(&path).unwrap(), 5);

        // Bump and CAS-save with `expected = 5`.
        let mut next = plan.clone();
        next.version_counter = 6;
        save_plan_if_version(&path, &next, 5).unwrap();
        assert_eq!(on_disk_version(&path).unwrap(), 6);
    }

    #[test]
    fn t71_save_if_version_returns_mismatch_when_disk_is_newer() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plan.json");
        let mut plan = sample_plan();
        plan.version_counter = 7;
        save_plan(&path, &plan).unwrap();

        // Worker A read at version 5 but on-disk is now 7 → mismatch.
        let mut stale_attempt = plan.clone();
        stale_attempt.version_counter = 6;
        let err = save_plan_if_version(&path, &stale_attempt, 5).unwrap_err();
        match err {
            PlanError::VersionMismatch { expected, actual } => {
                assert_eq!(expected, 5);
                assert_eq!(actual, 7);
            }
            other => panic!("expected VersionMismatch, got {other:?}"),
        }

        // Disk still at 7 — failed CAS does not corrupt.
        assert_eq!(on_disk_version(&path).unwrap(), 7);
    }

    #[test]
    fn t71_save_if_version_zero_expected_succeeds_for_fresh_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("fresh.json");
        let plan = sample_plan(); // version_counter = 0
        save_plan_if_version(&path, &plan, 0).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn t71_save_if_version_nonzero_expected_fails_for_fresh_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("fresh.json");
        let plan = sample_plan();
        let err = save_plan_if_version(&path, &plan, 1).unwrap_err();
        match err {
            PlanError::VersionMismatch { expected, actual } => {
                assert_eq!(expected, 1);
                assert_eq!(actual, 0);
            }
            other => panic!("expected VersionMismatch, got {other:?}"),
        }
        assert!(!path.exists(), "no file should be written on CAS failure");
    }

    #[test]
    fn t71_save_if_version_rejects_invalid_plan_before_cas_check() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plan.json");
        let mut bad = sample_plan();
        bad.title = String::new(); // invalid
        let err = save_plan_if_version(&path, &bad, 0).unwrap_err();
        // Validation runs first — caller learns about the bug, not the
        // version mismatch.
        assert!(matches!(err, PlanError::Validation(_)));
    }

    #[test]
    fn t71_save_if_version_atomic_temp_cleanup_on_disk_mismatch() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plan.json");
        let mut plan = sample_plan();
        plan.version_counter = 3;
        save_plan(&path, &plan).unwrap();

        // CAS attempt with wrong expected — must NOT leave a `.json.tmp`.
        let _ = save_plan_if_version(&path, &plan, 99);
        let temp = path.with_extension("json.tmp");
        assert!(!temp.exists(), "temp file leaked after failed CAS");
    }
}
