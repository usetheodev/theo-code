//! Single-cmd slice extracted from `cmd.rs` (T5.3.b of god-files-2026-07-23-plan.md, ADR D6).

#![allow(unused_imports, dead_code)]

use std::path::{Path, PathBuf};
use std::time::Instant;

use theo_application::use_cases::pipeline::{Pipeline, PipelineConfig};

use crate::*;
use super::helpers::*;

pub fn cmd_dashboard(repo: PathBuf, port: u16, static_dir: Option<PathBuf>) {
    let project_dir = resolve_dir(repo);
    let static_dir = static_dir.or_else(dashboard::find_default_static_dir);
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    if let Err(e) = rt.block_on(dashboard::serve(project_dir, port, static_dir)) {
        eprintln!("✗ dashboard failed: {e}");
        std::process::exit(1);
    }
}
