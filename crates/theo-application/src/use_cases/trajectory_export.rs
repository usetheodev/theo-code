//! T16.1 — RLHF trajectory export use case.
//!
//! Public façade over `theo_agent_runtime::trajectory_export`. Apps
//! (theo-cli) consume this module to expose the
//! `theo trajectory export-rlhf` subcommand promised by ADR D16
//! ("RLHF dataset é apenas export, não treina dentro do Theo").
//!
//! Why a use case instead of direct re-export: it gives the CLI a
//! stable API even if the runtime module is refactored, and lets
//! the CLI test against this surface (which can be mocked) instead
//! of the deeper runtime IO.
//!
//! See `docs/plans/sota-tier1-tier2-plan.md` §T16.1 + ADR D16.

use std::path::Path;

pub use theo_agent_runtime::trajectory_export::{
    ExportError, RatingFilter, RlhfRecord,
};

/// Export every rating envelope under `<project_dir>/.theo/trajectories/`
/// matching `filter` to the JSONL file at `out`. Returns the number of
/// records written.
///
/// Failure modes (typed via `ExportError`):
///   - IO error reading the trajectory directory or writing `out`
///   - Malformed JSON in any of the source JSONL files
///
/// The empty-trajectories case (no files / no rating envelopes) is
/// NOT a failure — `out` is created with zero lines and `Ok(0)` is
/// returned. CLI consumers can decide whether to treat 0 as a
/// warning.
pub fn export_rlhf(
    project_dir: &Path,
    out: &Path,
    filter: RatingFilter,
) -> Result<usize, ExportError> {
    theo_agent_runtime::trajectory_export::export_rlhf_dataset(project_dir, out, filter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    /// Helper: write a single trajectory JSONL line into the given dir.
    fn write_traj_line(dir: &Path, run_id: &str, line: &str) {
        let path = dir.join(format!("{run_id}.jsonl"));
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(f, "{}", line).unwrap();
    }

    #[test]
    fn t161_export_empty_project_writes_empty_file_and_returns_zero() {
        let dir = tempdir().unwrap();
        let out = dir.path().join("rlhf.jsonl");
        let n = export_rlhf(dir.path(), &out, RatingFilter::All).unwrap();
        assert_eq!(n, 0);
        assert!(out.exists(), "out file should be created even if empty");
        assert_eq!(std::fs::read_to_string(&out).unwrap().len(), 0);
    }

    #[test]
    fn t161_export_with_one_rating_writes_one_record() {
        let dir = tempdir().unwrap();
        let traj = dir.path().join(".theo").join("trajectories");
        std::fs::create_dir_all(&traj).unwrap();
        // A canonical rating envelope (T16.1 wire format).
        write_traj_line(
            &traj,
            "run-a",
            r#"{"v":1,"seq":0,"ts":1700000000,"run_id":"run-a","kind":"rating","payload":{"rating":1,"turn_index":3}}"#,
        );
        let out = dir.path().join("rlhf.jsonl");
        let n = export_rlhf(dir.path(), &out, RatingFilter::All).unwrap();
        assert_eq!(n, 1);
        let body = std::fs::read_to_string(&out).unwrap();
        // Parseable JSONL.
        let parsed: serde_json::Value = serde_json::from_str(body.trim()).unwrap();
        assert_eq!(parsed["rating"], 1);
        assert_eq!(parsed["run_id"], "run-a");
    }

    #[test]
    fn t161_export_positive_filter_excludes_negative_ratings() {
        let dir = tempdir().unwrap();
        let traj = dir.path().join(".theo").join("trajectories");
        std::fs::create_dir_all(&traj).unwrap();
        write_traj_line(
            &traj,
            "run-pos",
            r#"{"v":1,"seq":0,"ts":1700000000,"run_id":"run-pos","kind":"rating","payload":{"rating":1,"turn_index":1}}"#,
        );
        write_traj_line(
            &traj,
            "run-neg",
            r#"{"v":1,"seq":0,"ts":1700000001,"run_id":"run-neg","kind":"rating","payload":{"rating":-1,"turn_index":1}}"#,
        );
        let out = dir.path().join("rlhf.jsonl");
        let n = export_rlhf(dir.path(), &out, RatingFilter::Positive).unwrap();
        assert_eq!(
            n, 1,
            "Positive filter must keep rating=+1 and drop rating=-1"
        );
    }
}
