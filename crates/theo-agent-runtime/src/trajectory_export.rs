//! T16.1 — RLHF dataset export from `.theo/trajectories/*.jsonl`.
//!
//! Reads trajectory JSONL lines, filters by `EnvelopeKind::Rating`, and
//! produces a JSONL file in a format consumable by `axolotl`/`trl` DPO
//! training pipelines.
//!
//! Wire format we emit (one record per rated turn):
//!
//! ```json
//! {
//!   "run_id": "...",
//!   "turn_index": 5,
//!   "rating": 1,
//!   "comment": "good explanation",
//!   "timestamp": 1700000000000
//! }
//! ```
//!
//! Joining the rating with the original LLM prompt/response is left to the
//! consumer pipeline (which has access to the full transcript via
//! `.theo/state/<run>/state.jsonl`). This keeps the export small and
//! lossless.
//!
//! See `docs/plans/sota-tier1-tier2-plan.md` §T16.1 + ADR D16.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::observability::envelope::{EnvelopeKind, TrajectoryEnvelope};

/// One record in the exported RLHF dataset.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RlhfRecord {
    pub run_id: String,
    pub turn_index: u64,
    pub rating: i8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    pub timestamp: u64,
}

/// Errors specific to RLHF export.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid JSONL: {0}")]
    InvalidJson(String),
}

/// Filter for which ratings to include.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum RatingFilter {
    /// Include every rating envelope.
    #[default]
    All,
    /// Include only positive (`rating > 0`) — typical for SFT positive set.
    Positive,
    /// Include only negative (`rating < 0`) — typical for DPO rejected set.
    Negative,
    /// Include only entries with `rating == value`.
    Exact(i8),
}

impl RatingFilter {
    fn accepts(&self, rating: i8) -> bool {
        match self {
            RatingFilter::All => true,
            RatingFilter::Positive => rating > 0,
            RatingFilter::Negative => rating < 0,
            RatingFilter::Exact(v) => rating == *v,
        }
    }
}

/// Read all `Rating` envelopes from a single `.jsonl` trajectory file.
///
/// Non-rating lines are silently skipped. Malformed JSON lines bubble up
/// as `ExportError::InvalidJson` so the caller can decide whether to halt
/// or continue.
pub fn read_ratings(path: &Path) -> Result<Vec<RlhfRecord>, ExportError> {
    let f = std::fs::File::open(path)?;
    let reader = BufReader::new(f);
    let mut records = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let env: TrajectoryEnvelope = serde_json::from_str(&line)
            .map_err(|e| ExportError::InvalidJson(e.to_string()))?;
        if env.kind != EnvelopeKind::Rating {
            continue;
        }
        let Some(rating) = env.rating_value() else {
            continue;
        };
        let turn_index = env
            .payload
            .get("turn_index")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let comment = env
            .payload
            .get("comment")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        records.push(RlhfRecord {
            run_id: env.run_id,
            turn_index,
            rating,
            comment,
            timestamp: env.ts,
        });
    }
    Ok(records)
}

/// Read `Rating` envelopes from every `*.jsonl` file under a directory.
/// Subdirectories are NOT walked — `.theo/trajectories/` is flat.
pub fn read_ratings_from_dir(
    dir: &Path,
    filter: RatingFilter,
) -> Result<Vec<RlhfRecord>, ExportError> {
    let mut all = Vec::new();
    let entries = std::fs::read_dir(dir)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let mut records = read_ratings(&path)?;
        records.retain(|r| filter.accepts(r.rating));
        all.append(&mut records);
    }
    // Stable ordering: sort by (timestamp, run_id, turn_index).
    all.sort_by(|a, b| {
        a.timestamp
            .cmp(&b.timestamp)
            .then_with(|| a.run_id.cmp(&b.run_id))
            .then_with(|| a.turn_index.cmp(&b.turn_index))
    });
    Ok(all)
}

/// Write records as JSONL — one record per line, no trailing newline.
pub fn write_records(out: &Path, records: &[RlhfRecord]) -> Result<(), ExportError> {
    if let Some(parent) = out.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::File::create(out)?;
    for (i, r) in records.iter().enumerate() {
        let line = serde_json::to_string(r)
            .map_err(|e| ExportError::InvalidJson(e.to_string()))?;
        if i > 0 {
            file.write_all(b"\n")?;
        }
        file.write_all(line.as_bytes())?;
    }
    Ok(())
}

/// One-shot helper: read all ratings under `<project>/.theo/trajectories/`,
/// filter, write to `out`. Returns the number of records written.
pub fn export_rlhf_dataset(
    project_dir: &Path,
    out: &Path,
    filter: RatingFilter,
) -> Result<usize, ExportError> {
    let traj_dir = project_dir.join(".theo").join("trajectories");
    let records = if traj_dir.exists() {
        read_ratings_from_dir(&traj_dir, filter)?
    } else {
        Vec::new()
    };
    write_records(out, &records)?;
    Ok(records.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_jsonl(path: &Path, lines: &[&str]) {
        std::fs::write(path, lines.join("\n")).unwrap();
    }

    #[test]
    fn t161_rating_filter_all_accepts_everything() {
        let f = RatingFilter::All;
        assert!(f.accepts(-1));
        assert!(f.accepts(0));
        assert!(f.accepts(1));
    }

    #[test]
    fn t161_rating_filter_positive_only() {
        let f = RatingFilter::Positive;
        assert!(!f.accepts(0));
        assert!(!f.accepts(-1));
        assert!(f.accepts(1));
        assert!(f.accepts(3));
    }

    #[test]
    fn t161_rating_filter_negative_only() {
        let f = RatingFilter::Negative;
        assert!(!f.accepts(0));
        assert!(!f.accepts(1));
        assert!(f.accepts(-1));
    }

    #[test]
    fn t161_rating_filter_exact() {
        let f = RatingFilter::Exact(2);
        assert!(f.accepts(2));
        assert!(!f.accepts(1));
        assert!(!f.accepts(3));
    }

    #[test]
    fn t161_read_ratings_filters_non_rating_lines() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("run.jsonl");
        // EventKind is PascalCase in serde — see theo_domain::event::EventKind.
        let event = serde_json::json!({
            "v": 1, "seq": 0, "ts": 100, "run_id": "r",
            "kind": "event", "event_type": "ToolCallCompleted",
            "event_kind": "Tooling", "entity_id": "c1",
            "payload": {}
        });
        let rating = serde_json::json!({
            "v": 1, "seq": 1, "ts": 200, "run_id": "r",
            "kind": "rating",
            "payload": {"rating": 1, "turn_index": 0}
        });
        write_jsonl(
            &path,
            &[&event.to_string(), &rating.to_string()],
        );

        let records = read_ratings(&path).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].rating, 1);
        assert_eq!(records[0].run_id, "r");
    }

    #[test]
    fn t161_read_ratings_invalid_json_returns_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bad.jsonl");
        std::fs::write(&path, "not json").unwrap();
        let err = read_ratings(&path).unwrap_err();
        assert!(matches!(err, ExportError::InvalidJson(_)));
    }

    #[test]
    fn t161_read_ratings_empty_lines_skipped() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("e.jsonl");
        std::fs::write(&path, "\n\n").unwrap();
        let records = read_ratings(&path).unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn t161_read_ratings_from_dir_walks_jsonl_only() {
        let dir = tempdir().unwrap();
        // Create a jsonl with a rating and a non-jsonl with content.
        let r1 = serde_json::json!({
            "v": 1, "seq": 0, "ts": 100, "run_id": "a",
            "kind": "rating", "payload": {"rating": 1, "turn_index": 1}
        });
        write_jsonl(&dir.path().join("a.jsonl"), &[&r1.to_string()]);
        std::fs::write(dir.path().join("ignore.txt"), "should be skipped").unwrap();

        let records = read_ratings_from_dir(dir.path(), RatingFilter::All).unwrap();
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn t161_read_ratings_from_dir_applies_filter() {
        let dir = tempdir().unwrap();
        let pos = serde_json::json!({
            "v": 1, "seq": 0, "ts": 100, "run_id": "a",
            "kind": "rating", "payload": {"rating": 1, "turn_index": 0}
        });
        let neg = serde_json::json!({
            "v": 1, "seq": 1, "ts": 200, "run_id": "a",
            "kind": "rating", "payload": {"rating": -1, "turn_index": 1}
        });
        write_jsonl(
            &dir.path().join("a.jsonl"),
            &[&pos.to_string(), &neg.to_string()],
        );

        let recs = read_ratings_from_dir(dir.path(), RatingFilter::Positive).unwrap();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].rating, 1);
    }

    #[test]
    fn t161_write_records_roundtrips_through_jsonl() {
        let dir = tempdir().unwrap();
        let out = dir.path().join("out.jsonl");
        let records = vec![
            RlhfRecord {
                run_id: "a".into(),
                turn_index: 0,
                rating: 1,
                comment: Some("nice".into()),
                timestamp: 100,
            },
            RlhfRecord {
                run_id: "a".into(),
                turn_index: 1,
                rating: -1,
                comment: None,
                timestamp: 200,
            },
        ];
        write_records(&out, &records).unwrap();

        let content = std::fs::read_to_string(&out).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        let r0: RlhfRecord = serde_json::from_str(lines[0]).unwrap();
        let r1: RlhfRecord = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(r0, records[0]);
        assert_eq!(r1, records[1]);
    }

    #[test]
    fn t161_write_records_creates_parent_dir() {
        let dir = tempdir().unwrap();
        let out = dir.path().join("nested").join("deep").join("out.jsonl");
        write_records(&out, &[]).unwrap();
        assert!(out.exists());
    }

    #[test]
    fn t161_export_rlhf_dataset_e2e() {
        let dir = tempdir().unwrap();
        let traj = dir.path().join(".theo").join("trajectories");
        std::fs::create_dir_all(&traj).unwrap();
        let r = serde_json::json!({
            "v": 1, "seq": 0, "ts": 100, "run_id": "a",
            "kind": "rating", "payload": {"rating": 1, "turn_index": 0}
        });
        write_jsonl(&traj.join("a.jsonl"), &[&r.to_string()]);

        let out = dir.path().join("rlhf.jsonl");
        let n = export_rlhf_dataset(dir.path(), &out, RatingFilter::All).unwrap();
        assert_eq!(n, 1);
        assert!(out.exists());
        let content = std::fs::read_to_string(&out).unwrap();
        assert!(content.contains("\"rating\":1"));
    }

    #[test]
    fn t161_export_rlhf_dataset_no_trajectory_dir_writes_empty_file() {
        let dir = tempdir().unwrap();
        let out = dir.path().join("empty.jsonl");
        let n = export_rlhf_dataset(dir.path(), &out, RatingFilter::All).unwrap();
        assert_eq!(n, 0);
        assert!(out.exists());
        assert!(std::fs::read_to_string(&out).unwrap().is_empty());
    }

    #[test]
    fn t161_export_orders_records_chronologically() {
        let dir = tempdir().unwrap();
        let traj = dir.path().join(".theo").join("trajectories");
        std::fs::create_dir_all(&traj).unwrap();
        let later = serde_json::json!({
            "v": 1, "seq": 0, "ts": 200, "run_id": "b",
            "kind": "rating", "payload": {"rating": 1, "turn_index": 0}
        });
        let earlier = serde_json::json!({
            "v": 1, "seq": 0, "ts": 100, "run_id": "a",
            "kind": "rating", "payload": {"rating": -1, "turn_index": 0}
        });
        write_jsonl(&traj.join("b.jsonl"), &[&later.to_string()]);
        write_jsonl(&traj.join("a.jsonl"), &[&earlier.to_string()]);

        let out = dir.path().join("o.jsonl");
        export_rlhf_dataset(dir.path(), &out, RatingFilter::All).unwrap();
        let content = std::fs::read_to_string(&out).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        // earlier (ts=100) MUST appear before later (ts=200)
        let r0: RlhfRecord = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(r0.run_id, "a");
        assert_eq!(r0.timestamp, 100);
    }
}
