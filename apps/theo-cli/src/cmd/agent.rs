//! Single-cmd slice extracted from `cmd.rs` (T5.3.b of god-files-2026-07-23-plan.md, ADR D6).

#![allow(unused_imports, dead_code)]

use std::path::{Path, PathBuf};
use std::time::Instant;

use theo_application::use_cases::pipeline::{Pipeline, PipelineConfig};

use crate::*;
use super::helpers::*;

pub fn cmd_agent(
    prompt: Vec<String>,
    repo: PathBuf,
    provider_id: Option<String>,
    model: Option<String>,
    max_iter: Option<usize>,
    injections: theo_application::use_cases::run_agent_session::SubagentInjections,
) {
    let project_dir = resolve_dir(repo);

    let inline_prompt = if !prompt.is_empty() {
        Some(prompt.join(" "))
    } else {
        None
    };

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
        // Phase 41 (otlp-exporter-plan): RAII guard. Same as cmd_headless.
        #[cfg(feature = "otel")]
        let _otlp_guard = theo_application::facade::observability::OtlpGuard::install();

        let (config, provider_name) =
            resolve_agent_config(provider_id.as_deref(), model.as_deref(), max_iter).await;

        if let Err(e) = tui::run(config, project_dir, provider_name, inline_prompt, injections).await {
            eprintln!("TUI error: {e}");
            std::process::exit(1);
        }
    });
}
