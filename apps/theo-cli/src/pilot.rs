//! Pilot CLI runner — autonomous development loop with real-time display.

use std::path::PathBuf;
use std::sync::Arc;

// T1.2: route runtime types through the theo-application facade.
use theo_application::facade::agent::{
    AgentConfig, EventBus, PilotConfig, PilotLoop, PilotResult, load_promise,
};
use theo_domain::graph_context::GraphContextProvider;

use crate::render::style::{StyleCaps, accent, bold, dim, error, success, warn};
use crate::renderer::CliRenderer;
use crate::tty::TtyCaps;

fn caps() -> StyleCaps {
    TtyCaps::detect().style_caps()
}

/// Run the pilot loop with CLI rendering.
pub async fn run_pilot(
    config: AgentConfig,
    pilot_config: PilotConfig,
    project_dir: PathBuf,
    promise: String,
    complete: Option<String>,
) -> PilotResult {
    // Print banner
    let c = caps();
    eprintln!("{} — autonomous mode", bold("✈  Theo Pilot", c));
    eprintln!("  Promise: {}", accent(truncate(&promise, 80), c));
    if let Some(ref dod) = complete {
        eprintln!("  Done when: {}", warn(truncate(dod, 80), c));
    }
    eprintln!("  Project: {}", project_dir.display());
    eprintln!(
        "  Limits: {} max calls, {}/hour",
        pilot_config.max_total_calls, pilot_config.max_loops_per_hour
    );
    eprintln!();

    // Create parent EventBus with renderer
    let event_bus = Arc::new(EventBus::new());
    let renderer = Arc::new(PilotRenderer::new());
    event_bus.subscribe(renderer);
    let cli_renderer = Arc::new(CliRenderer::new());
    event_bus.subscribe(cli_renderer);

    // SOTA Planning System: prefer .theo/plans/*.json over .md (legacy roadmap).
    // `find_latest_plan` returns .json when present, otherwise falls back to .md.
    let plan_path = theo_application::facade::agent::find_latest_plan(&project_dir);
    let is_json_plan = plan_path
        .as_ref()
        .and_then(|p| p.extension())
        .and_then(|e| e.to_str())
        == Some("json");
    let roadmap_path: Option<PathBuf> = if is_json_plan {
        // Found a JSON plan; ignore legacy .md roadmaps.
        None
    } else {
        plan_path.clone()
    };

    // Initialize GRAPHCTX — fire-and-forget background build.
    // Disabled entirely when THEO_NO_GRAPHCTX=1.
    let graph_context: Option<Arc<dyn theo_domain::graph_context::GraphContextProvider>> =
        if std::env::var("THEO_NO_GRAPHCTX").is_ok() {
            None // Enabled by default. Set THEO_NO_GRAPHCTX=1 to disable.
        } else {
            let service = Arc::new(
                theo_application::use_cases::graph_context_service::GraphContextService::new(),
            );
            let _ = service.initialize(&project_dir).await;
            eprintln!("[theo] GRAPHCTX building in background");
            Some(service)
        };

    // Create pilot loop
    let mut pilot = PilotLoop::new(
        config,
        pilot_config,
        project_dir,
        promise,
        complete,
        event_bus,
    );
    if let Some(gc) = graph_context {
        pilot = pilot.with_graph_context(gc);
    }

    // Setup Ctrl+C handler
    let interrupt = pilot.interrupt_flag();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            interrupt.store(true, std::sync::atomic::Ordering::Release);
            eprintln!(
                "\n{}",
                warn("⚠  Ctrl+C — finishing current loop then stopping...", caps())
            );
        }
    });

    // Execute from plan/roadmap if available, otherwise normal loop.
    // Priority: JSON plan > legacy markdown roadmap > pure pilot.
    let result = if is_json_plan
        && let Some(ref pjson) = plan_path
    {
        match theo_application::facade::agent::load_plan(pjson) {
            Ok(plan) => {
                let pending = plan
                    .all_tasks()
                    .iter()
                    .filter(|t| !t.status.is_terminal())
                    .count();
                eprintln!(
                    "  Plan: {} ({} pending tasks)",
                    accent(pjson.display().to_string(), caps()),
                    pending
                );
                eprintln!();
                if pending > 0 {
                    pilot.run_from_plan(pjson).await
                } else {
                    pilot.run().await
                }
            }
            Err(e) => {
                eprintln!(
                    "  {} {}",
                    error("Plan parse error:", caps()),
                    e
                );
                pilot.run().await
            }
        }
    } else if let Some(ref rmap) = roadmap_path {
        let tasks =
            theo_application::facade::agent::parse_roadmap(rmap).unwrap_or_default();
        let pending = tasks.iter().filter(|t| !t.completed).count();
        if pending > 0 {
            eprintln!(
                "  Roadmap: {} ({} pending tasks)",
                accent(rmap.display().to_string(), caps()),
                pending
            );
            eprintln!();
            pilot.run_from_roadmap(rmap).await
        } else {
            pilot.run().await
        }
    } else {
        pilot.run().await
    };

    // Print final summary
    eprintln!();
    print_pilot_summary(&result);

    result
}

/// Resolve the promise from CLI args or .theo/PROMPT.md.
pub fn resolve_promise(args: &[String], project_dir: &std::path::Path) -> Option<String> {
    // Collect non-flag args as promise
    let promise_parts: Vec<&str> = args
        .iter()
        .filter(|a| !a.starts_with("--"))
        .map(|s| s.as_str())
        .collect();

    if !promise_parts.is_empty() {
        Some(promise_parts.join(" "))
    } else {
        load_promise(project_dir)
    }
}

fn print_pilot_summary(result: &PilotResult) {
    let c = caps();
    let status = if result.success {
        success("✓ Pilot Complete", c).to_string()
    } else {
        error("✗ Pilot Stopped", c).to_string()
    };

    eprintln!("{} — {}", status, result.reason);
    eprintln!(
        "  {} loops, {} files, {} tokens",
        result.loops_completed,
        result.files_edited.len(),
        format_tokens(result.total_tokens),
    );

    if !result.files_edited.is_empty() {
        eprintln!("  Files: {}", result.files_edited.join(", "));
    }
    eprintln!();
}

fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        format!("{}", tokens)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    } else {
        s.to_string()
    }
}

// ---------------------------------------------------------------------------
// PilotRenderer — loop-level events
// ---------------------------------------------------------------------------

use theo_application::facade::agent::EventListener;
use theo_domain::event::{DomainEvent, EventType};

struct PilotRenderer {
    loop_count: std::sync::atomic::AtomicUsize,
}

impl PilotRenderer {
    fn new() -> Self {
        Self {
            loop_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

impl EventListener for PilotRenderer {
    fn on_event(&self, event: &DomainEvent) {
        match event.event_type {
            EventType::LlmCallStart => {
                let iteration = event
                    .payload
                    .get("iteration")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                if iteration == 1 {
                    let loop_num = self
                        .loop_count
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                        + 1;
                    eprintln!(
                        "\n{}",
                        bold(format!("── Pilot Loop {loop_num} ──"), caps())
                    );
                }
            }
            EventType::RunStateChanged => {
                // Pilot loop summary: "PilotLoopComplete:N:files:tokens:iters"
                if let Some(to) = event.payload.get("to").and_then(|v| v.as_str())
                    && let Some(data) = to.strip_prefix("PilotLoopComplete:") {
                        let parts: Vec<&str> = data.split(':').collect();
                        if parts.len() >= 4 {
                            let loop_n = parts[0];
                            let files = parts[1];
                            let tokens = parts[2].parse::<u64>().unwrap_or(0);
                            let iters = parts[3];
                            eprintln!(
                                "{}",
                                dim(
                                    format!(
                                        "── Loop {} complete: {} files, {} tokens, {} iterations ──",
                                        loop_n,
                                        files,
                                        format_tokens(tokens),
                                        iters
                                    ),
                                    caps()
                                )
                            );
                        }
                    }
            }
            _ => {} // CliRenderer handles all other events
        }
    }
}
