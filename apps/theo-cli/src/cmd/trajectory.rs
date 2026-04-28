//! Single-cmd slice extracted from `cmd.rs` (T5.3.b of god-files-2026-07-23-plan.md, ADR D6).

#![allow(unused_imports, dead_code)]

use std::path::{Path, PathBuf};
use std::time::Instant;

use theo_application::use_cases::pipeline::{Pipeline, PipelineConfig};

use crate::*;
use super::helpers::*;

pub fn cmd_trajectory_export_rlhf(
    repo: &Path,
    out: &Path,
    filter: &str,
) -> i32 {
    use theo_application::use_cases::trajectory_export::{
        export_rlhf, ExportError, RatingFilter,
    };

    let project_dir = resolve_dir(repo.to_path_buf());

    // Parse the filter string. Mirrors RatingFilter::All|Positive|Negative|Exact(i8).
    let filter = match filter.to_ascii_lowercase().as_str() {
        "all" => RatingFilter::All,
        "positive" | "pos" | "+" => RatingFilter::Positive,
        "negative" | "neg" | "-" => RatingFilter::Negative,
        s => match s.parse::<i8>() {
            Ok(n) => RatingFilter::Exact(n),
            Err(_) => {
                eprintln!(
                    "✗ unknown filter `{filter}`. Valid: all | positive | \
                     negative | <integer> (e.g. 1, -1)."
                );
                return 2;
            }
        },
    };

    match export_rlhf(&project_dir, out, filter) {
        Ok(n) => {
            if n == 0 {
                eprintln!(
                    "⚠ wrote 0 records to `{}` (no rating envelopes \
                     matched). Check `.theo/trajectories/` exists and \
                     contains rated runs.",
                    out.display()
                );
            } else {
                eprintln!(
                    "✓ wrote {n} record(s) to `{}` (filter applied at read \
                     time).",
                    out.display()
                );
            }
            0
        }
        Err(ExportError::Io(e)) => {
            eprintln!("✗ I/O error: {e}");
            1
        }
        Err(ExportError::InvalidJson(msg)) => {
            eprintln!("✗ invalid JSONL in trajectory file: {msg}");
            1
        }
    }
}

