//! Single-cmd slice extracted from `cmd.rs` (T5.3.b of god-files-2026-07-23-plan.md, ADR D6).

#![allow(unused_imports, dead_code)]

use std::path::{Path, PathBuf};
use std::time::Instant;

use theo_application::use_cases::pipeline::{Pipeline, PipelineConfig};

use crate::*;
use super::helpers::*;

pub fn cmd_init(repo: PathBuf) {
    let project_dir = resolve_dir(repo);
    let caps = tty::TtyCaps::detect().style_caps();
    use render::style::{bold, error, success, warn};

    eprintln!("{} — initializing project", bold("theo init", caps));

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
        let (config, _provider) = resolve_agent_config(None, None, None).await;
        match init::run_init_with_agent(&project_dir, config).await {
            Ok(true) => eprintln!(
                "\n{} Review .theo/theo.md and edit if needed.",
                success("✓ Project initialized.", caps)
            ),
            Ok(false) => eprintln!("\n{}", warn("⚠ Already initialized.", caps)),
            Err(e) => {
                eprintln!("\n{} {e}", error("✗ Error:", caps));
                std::process::exit(1);
            }
        }
    });
}

