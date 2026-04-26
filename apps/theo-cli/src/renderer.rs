//! CLI event renderer — real-time display of agent activity.
//!
//! This file is a thin adapter: it listens to [`DomainEvent`]s and
//! delegates all formatting to pure functions in
//! [`crate::render::tool_result`]. It contains NO raw ANSI escape
//! sequences; all styling flows through `crate::render::style`.
//!
//! See `docs/roadmap/cli-professionalization.md` (T1.1) and
//! `docs/adr/ADR-001-streaming-markdown.md`.

#![allow(dead_code)] // Scaffolded helpers — kept for upcoming TUI features.
use std::sync::Mutex;

// T1.2: route the event-bus types through the theo-application facade
// so `apps/theo-cli` respects the "apps → theo-application" contract
// without needing a direct theo-agent-runtime dependency.
use theo_application::facade::agent::EventListener;
use theo_domain::event::{DomainEvent, EventType};

use crate::render::streaming::StreamingMarkdownRenderer;
use crate::render::style::StyleCaps;
use crate::render::tool_result as tr;
use crate::tty::TtyCaps;

pub struct CliRenderer {
    caps: StyleCaps,
    /// Buffered incremental markdown renderer for `ContentDelta` events.
    /// Wrapped in `Mutex` because [`EventListener::on_event`] takes `&self`
    /// and we need interior mutability to update the buffer as chunks arrive.
    streaming: Mutex<StreamingMarkdownRenderer>,
}

impl CliRenderer {
    pub fn new() -> Self {
        let caps = TtyCaps::detect().style_caps();
        Self {
            caps,
            streaming: Mutex::new(StreamingMarkdownRenderer::new(caps)),
        }
    }

    /// Construct with explicit caps (used in tests / integration with
    /// a controlled terminal).
    pub fn with_caps(caps: StyleCaps) -> Self {
        Self {
            caps,
            streaming: Mutex::new(StreamingMarkdownRenderer::new(caps)),
        }
    }
    /// Flush any buffered streaming markdown state. Call at turn
    /// boundaries to avoid leaking unclosed tokens across messages.
    fn flush_streaming(&self) {
        if let Ok(mut r) = self.streaming.lock() {
            r.flush();
            let out = r.take_output();
            if !out.is_empty() {
                eprint!("{out}");
            }
        }
    }
}

impl EventListener for CliRenderer {
    fn on_event(&self, event: &DomainEvent) {
        match event.event_type {
            EventType::RunStateChanged => {
                // Flush any in-flight streaming markdown at turn boundaries.
                self.flush_streaming();
                let to = tr::json_str(&event.payload, "to", "?");
                if let Some(banner) = tr::render_subagent_banner(to, self.caps) {
                    eprintln!("{banner}");
                }
            }
            EventType::ToolCallQueued => {
                let tool_name = event.payload.get("tool_name").and_then(|v| v.as_str()).unwrap_or("?");
                eprint!("\n  \x1b[36m⠋\x1b[0m {tool_name} \x1b[90mrunning...\x1b[0m");
            }
            EventType::ToolCallProgress => {
                if let Some(line) = event.payload.get("line").and_then(|v| v.as_str()) {
                    let display = if line.len() > 120 {
                        format!("{}…", &line[..119])
                    } else {
                        line.to_string()
                    };
                    eprintln!("  \x1b[90m│\x1b[0m {display}");
                }
            }
            EventType::ToolCallCompleted => {
                // Flush any pending streaming text before tool output.
                self.flush_streaming();
                render_tool_completed(event, self.caps);
            }
            EventType::LlmCallStart | EventType::LlmCallEnd => {}
            EventType::ReasoningDelta => {
                if let Some(text) = event.payload.get("text").and_then(|v| v.as_str()) {
                    eprint!("{}", tr::render_reasoning_chunk(text, self.caps));
                }
            }
            EventType::ContentDelta => {
                if let Some(text) = event.payload.get("text").and_then(|v| v.as_str())
                    && let Ok(mut r) = self.streaming.lock() {
                        r.push(text);
                        let chunk = r.take_output();
                        if !chunk.is_empty() {
                            eprint!("{chunk}");
                        }
                    }
            }
            EventType::BudgetExceeded => {
                let violation = tr::json_str(&event.payload, "violation", "budget exceeded");
                eprintln!("{}", tr::render_budget_warning(violation, self.caps));
            }
            EventType::TodoUpdated => {}
            EventType::Error => {
                let kind = event.payload.get("type").and_then(|v| v.as_str());
                if kind == Some("retry") {
                    return;
                }
                if kind == Some("capability_denied") {
                    let tool = tr::json_str(&event.payload, "tool_name", "?");
                    eprintln!("{}", tr::render_denied(tool, self.caps));
                    return;
                }
                let msg = event
                    .payload
                    .get("error")
                    .or(event.payload.get("reason"))
                    .or(event.payload.get("violation"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error");
                eprintln!("{}", tr::render_error(msg, self.caps));
            }
            _ => {}
        }
    }
}

fn render_tool_completed(event: &DomainEvent, caps: StyleCaps) {
    let prefix = tr::sub_agent_prefix(&event.entity_id, caps);
    let success = event
        .payload
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let tool_name = tr::json_str(&event.payload, "tool_name", "?");
    let input = &event.payload["input"];
    let output = tr::json_str(&event.payload, "output_preview", "");
    let duration = event
        .payload
        .get("duration_ms")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    match tool_name {
        "read" => {
            let path = tr::json_str(input, "filePath", "?");
            let lines = output.lines().count();
            eprintln!(
                "{}",
                tr::render_read(&prefix, path, lines, success, duration, caps)
            );
        }
        "write" => {
            let path = tr::json_str(input, "filePath", "?");
            let content = input.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let lines = content.lines().count();
            eprintln!(
                "{}",
                tr::render_write_header(&prefix, path, lines, success, duration, caps)
            );
            if success && !content.is_empty() {
                for line in tr::render_write_preview(content, 3, caps) {
                    eprintln!("{line}");
                }
            }
        }
        "edit" => {
            let path = tr::json_str(input, "filePath", "?");
            eprintln!(
                "{}",
                tr::render_edit_header(&prefix, path, success, duration, caps)
            );
            if success {
                if let Some(old) = input.get("oldString").and_then(|v| v.as_str()) {
                    let first = old.lines().next().unwrap_or("");
                    eprintln!("{}", tr::render_diff_line('-', first, caps));
                }
                if let Some(new) = input.get("newString").and_then(|v| v.as_str()) {
                    let first = new.lines().next().unwrap_or("");
                    eprintln!("{}", tr::render_diff_line('+', first, caps));
                    let total = new.lines().count();
                    if total > 1 {
                        eprintln!(
                            "    {}",
                            crate::render::style::dim(
                                format!("  … +{} more lines", total - 1),
                                caps
                            )
                        );
                    }
                }
            }
        }
        "apply_patch" => {
            let patch = tr::json_str(input, "patchText", "");
            let files: Vec<&str> = patch
                .lines()
                .filter(|l| l.starts_with("+++ "))
                .filter_map(|l| l.strip_prefix("+++ b/").or(l.strip_prefix("+++ ")))
                .filter(|f| *f != "/dev/null")
                .collect();
            let file_list = if files.is_empty() {
                "patch".to_string()
            } else {
                files.join(", ")
            };
            let hunks = patch.lines().filter(|l| l.starts_with("@@")).count();
            eprintln!(
                "{}",
                tr::render_patch(&prefix, &file_list, hunks, success, duration, caps)
            );
        }
        "glob" => {
            let pattern = tr::json_str(input, "pattern", "*");
            let count = output.lines().filter(|l| !l.is_empty()).count();
            eprintln!(
                "{}",
                tr::render_glob(&prefix, pattern, count, success, duration, caps)
            );
        }
        "grep" => {
            let pattern = tr::json_str(input, "pattern", "?");
            let count = output.lines().filter(|l| !l.is_empty()).count();
            eprintln!(
                "{}",
                tr::render_grep(&prefix, pattern, count, success, duration, caps)
            );
        }
        "bash" => {
            let cmd = tr::json_str(input, "command", "?");
            eprintln!(
                "{}",
                tr::render_bash_header(&prefix, cmd, success, duration, caps)
            );
            if success && !output.is_empty() {
                for line in tr::render_bash_preview(output, caps) {
                    eprintln!("{line}");
                }
            }
        }
        "think" => {
            let thought = tr::json_str(input, "thought", "");
            eprintln!("{}", tr::render_think(thought, caps));
        }
        "reflect" => {
            let confidence = input
                .get("confidence")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            eprintln!(
                "{}",
                tr::render_reflect(&prefix, confidence, success, duration, caps)
            );
        }
        "memory" => {
            let action = tr::json_str(input, "action", "?");
            let key = tr::json_str(input, "key", "");
            eprintln!(
                "{}",
                tr::render_memory(&prefix, action, key, success, caps)
            );
        }
        "task_create" => {
            let content = tr::json_str(input, "content", "?");
            eprintln!(
                "  {} +task {}",
                crate::render::style::accent("📋", caps),
                content
            );
        }
        "task_update" => {
            let id = input.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let new_status = tr::json_str(input, "status", "?");
            let icon = match new_status {
                "completed" => "✅",
                "in_progress" => "🔄",
                "cancelled" => "❌",
                _ => "⬜",
            };
            eprintln!(
                "  {} task {id} {icon} {new_status}",
                crate::render::style::accent("📋", caps)
            );
        }
        "done" => {
            eprintln!(
                "{}",
                tr::render_generic(&prefix, "Done", success, duration, caps)
            );
        }
        _ => {
            eprintln!(
                "{}",
                tr::render_generic(&prefix, tool_name, success, duration, caps)
            );
        }
    }
}
