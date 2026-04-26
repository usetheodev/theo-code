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
}
