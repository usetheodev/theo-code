//! Reader for trajectory JSONL files with crash-tolerant parsing.
//!
//! Parses line-by-line. If the last line is truncated (crash mid-write) it is
//! ignored and `IntegrityReport.complete` is set to false. Sequence gaps are
//! detected and reported as missing ranges.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::observability::envelope::{EnvelopeKind, TrajectoryEnvelope, ENVELOPE_SCHEMA_VERSION};

/// A contiguous range of missing sequence numbers `[start, end)`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MissingRange {
    pub start: u64,
    pub end: u64,
}

/// Integrity report for a trajectory file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IntegrityReport {
    pub complete: bool,
    pub total_events_expected: u64,
    pub total_events_received: u64,
    pub missing_sequences: Vec<MissingRange>,
    pub drop_sentinels_found: u64,
    pub writer_recoveries_found: u64,
    pub confidence: f64,
    pub schema_version: u32,
}

impl Default for IntegrityReport {
    fn default() -> Self {
        Self {
            complete: true,
            total_events_expected: 0,
            total_events_received: 0,
            missing_sequences: Vec::new(),
            drop_sentinels_found: 0,
            writer_recoveries_found: 0,
            confidence: 0.0,
            schema_version: ENVELOPE_SCHEMA_VERSION,
        }
    }
}

/// Reader error.
#[derive(Debug, thiserror::Error)]
pub enum ReaderError {
    #[error("I/O error reading trajectory: {0}")]
    Io(#[from] std::io::Error),
}

/// Read a trajectory JSONL file and compute its integrity report.
pub fn read_trajectory<P: AsRef<Path>>(
    path: P,
) -> Result<(Vec<TrajectoryEnvelope>, IntegrityReport), ReaderError> {
    let file = File::open(path.as_ref())?;
    let reader = BufReader::new(file);

    let mut envelopes = Vec::new();
    let mut last_line_complete = true;
    let mut drop_sentinels = 0u64;
    let mut writer_recoveries = 0u64;
    let mut max_seq: Option<u64> = None;
    let mut seen_seqs: Vec<u64> = Vec::new();

    let lines: Vec<_> = reader.lines().collect();
    for (i, line_res) in lines.into_iter().enumerate() {
        let line = match line_res {
            Ok(l) => l,
            Err(_) => {
                if i > 0 {
                    last_line_complete = false;
                }
                break;
            }
        };
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<TrajectoryEnvelope>(&line) {
            Ok(env) => {
                match env.kind {
                    EnvelopeKind::DropSentinel => drop_sentinels += 1,
                    EnvelopeKind::WriterRecovered => writer_recoveries += 1,
                    _ => {}
                }
                seen_seqs.push(env.seq);
                max_seq = Some(max_seq.map_or(env.seq, |m| m.max(env.seq)));
                envelopes.push(env);
            }
            Err(_) => {
                // Last line truncated — typical in crash mid-write.
                last_line_complete = false;
            }
        }
    }

    // Detect missing sequences.
    seen_seqs.sort_unstable();
    let mut missing: Vec<MissingRange> = Vec::new();
    let expected = max_seq.map(|m| m + 1).unwrap_or(0);
    let mut expected_next: u64 = 0;
    for s in &seen_seqs {
        if *s > expected_next {
            missing.push(MissingRange {
                start: expected_next,
                end: *s,
            });
        }
        expected_next = s + 1;
    }

    let received = seen_seqs.len() as u64;
    let confidence = if expected == 0 {
        0.0
    } else {
        let ratio = received as f64 / expected as f64;
        ratio.clamp(0.0, 1.0)
    };
    let complete = last_line_complete && missing.is_empty() && expected > 0;

    Ok((
        envelopes,
        IntegrityReport {
            complete,
            total_events_expected: expected,
            total_events_received: received,
            missing_sequences: missing,
            drop_sentinels_found: drop_sentinels,
            writer_recoveries_found: writer_recoveries,
            confidence,
            schema_version: ENVELOPE_SCHEMA_VERSION,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_jsonl(path: &Path, lines: &[&str]) {
        let mut f = File::create(path).unwrap();
        for l in lines {
            f.write_all(l.as_bytes()).unwrap();
            f.write_all(b"\n").unwrap();
        }
    }

    fn env_line(seq: u64, run_id: &str, kind: EnvelopeKind) -> String {
        let env = match kind {
            EnvelopeKind::DropSentinel => {
                TrajectoryEnvelope::drop_sentinel(run_id, seq, 0, 1)
            }
            EnvelopeKind::WriterRecovered => {
                TrajectoryEnvelope::writer_recovered(run_id, seq, 0, 1, "test")
            }
            EnvelopeKind::Event => TrajectoryEnvelope {
                v: 1,
                seq,
                ts: 0,
                run_id: run_id.into(),
                kind: EnvelopeKind::Event,
                event_type: Some("X".into()),
                event_kind: None,
                entity_id: Some("e".into()),
                payload: serde_json::Value::Null,
                dropped_since_last: 0,
            },
            EnvelopeKind::Summary => {
                TrajectoryEnvelope::summary(run_id, seq, 0, serde_json::json!({}))
            }
            EnvelopeKind::Rating => TrajectoryEnvelope::rating(run_id, seq, 0, 1, 0, None),
        };
        serde_json::to_string(&env).unwrap()
    }

    #[test]
    fn test_reader_parses_valid_jsonl() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("run.jsonl");
        let lines: Vec<String> = (0..10).map(|i| env_line(i, "r", EnvelopeKind::Event)).collect();
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        write_jsonl(&path, &refs);
        let (env, rep) = read_trajectory(&path).unwrap();
        assert_eq!(env.len(), 10);
        assert!(rep.complete);
        assert_eq!(rep.confidence, 1.0);
    }

    #[test]
    fn test_reader_tolerates_truncated_last_line() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("r.jsonl");
        let mut f = File::create(&path).unwrap();
        for i in 0..9u64 {
            let l = env_line(i, "r", EnvelopeKind::Event);
            f.write_all(l.as_bytes()).unwrap();
            f.write_all(b"\n").unwrap();
        }
        // Truncated last line.
        f.write_all(b"{\"v\":1,\"seq\":9,\"ts\":0,\"run_id\":\"r\",\"kind\":\"e").unwrap();
        drop(f);
        let (env, rep) = read_trajectory(&path).unwrap();
        assert_eq!(env.len(), 9);
        assert!(!rep.complete);
    }

    #[test]
    fn test_reader_detects_sequence_gaps() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("r.jsonl");
        let lines = [
            env_line(0, "r", EnvelopeKind::Event),
            env_line(1, "r", EnvelopeKind::Event),
            env_line(2, "r", EnvelopeKind::Event),
            env_line(5, "r", EnvelopeKind::Event),
            env_line(6, "r", EnvelopeKind::Event),
        ];
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        write_jsonl(&path, &refs);
        let (_, rep) = read_trajectory(&path).unwrap();
        assert_eq!(rep.missing_sequences.len(), 1);
        assert_eq!(rep.missing_sequences[0], MissingRange { start: 3, end: 5 });
    }

    #[test]
    fn test_reader_counts_drop_sentinels() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("r.jsonl");
        let lines = [
            env_line(0, "r", EnvelopeKind::Event),
            env_line(1, "r", EnvelopeKind::DropSentinel),
            env_line(2, "r", EnvelopeKind::Event),
            env_line(3, "r", EnvelopeKind::DropSentinel),
        ];
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        write_jsonl(&path, &refs);
        let (_, rep) = read_trajectory(&path).unwrap();
        assert_eq!(rep.drop_sentinels_found, 2);
    }

    #[test]
    fn test_reader_counts_writer_recoveries() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("r.jsonl");
        let lines = [
            env_line(0, "r", EnvelopeKind::Event),
            env_line(1, "r", EnvelopeKind::WriterRecovered),
        ];
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        write_jsonl(&path, &refs);
        let (_, rep) = read_trajectory(&path).unwrap();
        assert_eq!(rep.writer_recoveries_found, 1);
    }

    #[test]
    fn test_reader_computes_confidence() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("r.jsonl");
        let lines = [
            env_line(0, "r", EnvelopeKind::Event),
            env_line(1, "r", EnvelopeKind::Event),
            env_line(2, "r", EnvelopeKind::Event),
            env_line(3, "r", EnvelopeKind::Event),
            env_line(4, "r", EnvelopeKind::Event),
            env_line(5, "r", EnvelopeKind::Event),
            env_line(6, "r", EnvelopeKind::Event),
            env_line(7, "r", EnvelopeKind::Event),
            env_line(9, "r", EnvelopeKind::Event),
        ];
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        write_jsonl(&path, &refs);
        let (_, rep) = read_trajectory(&path).unwrap();
        // 9 received / 10 expected = 0.9
        assert!((rep.confidence - 0.9).abs() < 1e-6);
    }

    #[test]
    fn test_reader_handles_empty_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("r.jsonl");
        File::create(&path).unwrap();
        let (env, rep) = read_trajectory(&path).unwrap();
        assert_eq!(env.len(), 0);
        assert!(!rep.complete);
        assert_eq!(rep.confidence, 0.0);
    }
}
