//! Single-cmd slice extracted from `cmd.rs` (T5.3.b of god-files-2026-07-23-plan.md, ADR D6).

#![allow(unused_imports, dead_code)]

use std::path::{Path, PathBuf};
use std::time::Instant;

use theo_application::use_cases::pipeline::{Pipeline, PipelineConfig};

use crate::*;
use super::helpers::*;

pub fn cmd_pilot(
    promise_args: Vec<String>,
    repo: PathBuf,
    provider_id: Option<String>,
    model: Option<String>,
    max_calls: Option<usize>,
    rate: Option<usize>,
    complete: Option<String>,
) {
    let project_dir = resolve_dir(repo);

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
        let (config, _provider_name) =
            resolve_agent_config(provider_id.as_deref(), model.as_deref(), None).await;

        let mut pilot_config = theo_application::facade::agent::PilotConfig::load(&project_dir);
        if let Some(calls) = max_calls {
            pilot_config.max_total_calls = calls;
        }
        if let Some(r) = rate {
            pilot_config.max_loops_per_hour = r;
        }

        let promise = pilot::resolve_promise(&promise_args, &project_dir);
        let Some(promise) = promise else {
            eprintln!("Error: No promise provided.");
            eprintln!("Usage: theo-code pilot \"your promise here\"");
            eprintln!("   or: create .theo/PROMPT.md with the promise");
            std::process::exit(1);
        };

        let result = pilot::run_pilot(config, pilot_config, project_dir, promise, complete).await;
        std::process::exit(if result.success { 0 } else { 1 });
    });
}

