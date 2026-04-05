//! Pilot CLI runner — autonomous development loop with real-time display.

use std::path::PathBuf;
use std::sync::Arc;

use theo_agent_runtime::event_bus::EventBus;
use theo_agent_runtime::pilot::{PilotConfig, PilotLoop, PilotResult, load_promise};
use theo_agent_runtime::AgentConfig;
use theo_domain::graph_context::GraphContextProvider;

use crate::renderer::CliRenderer;

/// Run the pilot loop with CLI rendering.
pub async fn run_pilot(
    config: AgentConfig,
    pilot_config: PilotConfig,
    project_dir: PathBuf,
    promise: String,
    complete: Option<String>,
) -> PilotResult {
    // Print banner
    eprintln!("\x1b[1m✈  Theo Pilot\x1b[0m — autonomous mode");
    eprintln!("  Promise: \x1b[36m{}\x1b[0m", truncate(&promise, 80));
    if let Some(ref dod) = complete {
        eprintln!("  Done when: \x1b[33m{}\x1b[0m", truncate(dod, 80));
    }
    eprintln!("  Project: {}", project_dir.display());
    eprintln!("  Limits: {} max calls, {}/hour", pilot_config.max_total_calls, pilot_config.max_loops_per_hour);
    eprintln!();

    // Create parent EventBus with renderer
    let event_bus = Arc::new(EventBus::new());
    let renderer = Arc::new(PilotRenderer::new());
    event_bus.subscribe(renderer);
    let cli_renderer = Arc::new(CliRenderer::new());
    event_bus.subscribe(cli_renderer);

    // Check if there's a roadmap to execute from (before moving project_dir)
    let roadmap_path = theo_agent_runtime::roadmap::find_latest_roadmap(&project_dir);

    // Initialize GRAPHCTX — fire-and-forget background build.
    let graph_context: Option<Arc<dyn theo_domain::graph_context::GraphContextProvider>> = {
        let service = Arc::new(
            theo_application::use_cases::graph_context_service::GraphContextService::new(),
        );
        let _ = service.initialize(&project_dir).await; // Returns immediately.
        eprintln!("[theo] GRAPHCTX building in background");
        Some(service)
    };

    // Create pilot loop
    let mut pilot = PilotLoop::new(config, pilot_config, project_dir, promise, complete, event_bus);
    if let Some(gc) = graph_context {
        pilot = pilot.with_graph_context(gc);
    }

    // Setup Ctrl+C handler
    let interrupt = pilot.interrupt_flag();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            interrupt.store(true, std::sync::atomic::Ordering::Release);
            eprintln!("\n\x1b[33m⚠  Ctrl+C — finishing current loop then stopping...\x1b[0m");
        }
    });

    // Execute from roadmap if available, otherwise normal loop
    let result = if let Some(ref rmap) = roadmap_path {
        let tasks = theo_agent_runtime::roadmap::parse_roadmap(rmap).unwrap_or_default();
        let pending = tasks.iter().filter(|t| !t.completed).count();
        if pending > 0 {
            eprintln!("  Roadmap: \x1b[36m{}\x1b[0m ({} pending tasks)", rmap.display(), pending);
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
    let promise_parts: Vec<&str> = args.iter()
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
    let status = if result.success {
        "\x1b[32m✓ Pilot Complete\x1b[0m"
    } else {
        "\x1b[31m✗ Pilot Stopped\x1b[0m"
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

use theo_agent_runtime::event_bus::EventListener;
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
                let iteration = event.payload.get("iteration")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                if iteration == 1 {
                    let loop_num = self.loop_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    eprintln!("\n\x1b[1m── Pilot Loop {} ──\x1b[0m", loop_num);
                }
            }
            EventType::RunStateChanged => {
                // Pilot loop summary: "PilotLoopComplete:N:files:tokens:iters"
                if let Some(to) = event.payload.get("to").and_then(|v| v.as_str()) {
                    if let Some(data) = to.strip_prefix("PilotLoopComplete:") {
                        let parts: Vec<&str> = data.split(':').collect();
                        if parts.len() >= 4 {
                            let loop_n = parts[0];
                            let files = parts[1];
                            let tokens = parts[2].parse::<u64>().unwrap_or(0);
                            let iters = parts[3];
                            eprintln!(
                                "\x1b[90m── Loop {} complete: {} files, {} tokens, {} iterations ──\x1b[0m",
                                loop_n, files, format_tokens(tokens), iters
                            );
                        }
                    }
                }
            }
            _ => {} // CliRenderer handles all other events
        }
    }
}
