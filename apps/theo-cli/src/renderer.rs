//! CLI event renderer — real-time display of agent activity.
//!
//! Shows tool calls with rich details similar to Claude Code's output.

use theo_agent_runtime::event_bus::EventListener;
use theo_domain::event::{DomainEvent, EventType};

pub struct CliRenderer;

impl CliRenderer {
    pub fn new() -> Self {
        Self
    }
}

impl EventListener for CliRenderer {
    fn on_event(&self, event: &DomainEvent) {
        match event.event_type {
            EventType::RunStateChanged => {
                let to = event.payload.get("to").and_then(|v| v.as_str()).unwrap_or("?");
                match to {
                    "Planning" => eprintln!("  \x1b[34m📋 Planning\x1b[0m"),
                    "Executing" => eprintln!("  \x1b[33m⚡ Executing\x1b[0m"),
                    "Evaluating" => eprintln!("  \x1b[35m🔍 Evaluating\x1b[0m"),
                    "Replanning" => eprintln!("  \x1b[34m🔄 Replanning\x1b[0m"),
                    "Converged" => eprintln!("  \x1b[32m✅ Converged\x1b[0m"),
                    "Aborted" => eprintln!("  \x1b[31m⛔ Aborted\x1b[0m"),
                    _ => {}
                }
            }
            EventType::ToolCallQueued => {
                // Don't print here — we print rich info on ToolCallCompleted
            }
            EventType::ToolCallCompleted => {
                render_tool_completed(event);
            }
            EventType::LlmCallStart => {
                eprint!("  \x1b[90m💭 ");
            }
            EventType::LlmCallEnd => {
                eprintln!("\x1b[0m");
            }
            EventType::ReasoningDelta => {
                // Show reasoning text in real-time (dimmed)
                if let Some(text) = event.payload.get("text").and_then(|v| v.as_str()) {
                    eprint!("\x1b[90m{text}\x1b[0m");
                }
            }
            EventType::ContentDelta => {
                // Show content streaming in real-time
                if let Some(text) = event.payload.get("text").and_then(|v| v.as_str()) {
                    eprint!("{text}");
                }
            }
            EventType::BudgetExceeded => {
                let violation = event.payload.get("violation").and_then(|v| v.as_str()).unwrap_or("budget exceeded");
                eprintln!("  \x1b[33m⚠️  {violation}\x1b[0m");
            }
            EventType::Error => {
                if event.payload.get("type").and_then(|v| v.as_str()) == Some("retry") {
                    return;
                }
                if event.payload.get("type").and_then(|v| v.as_str()) == Some("capability_denied") {
                    let tool = event.payload.get("tool_name").and_then(|v| v.as_str()).unwrap_or("?");
                    eprintln!("  \x1b[31m🚫 {tool} denied\x1b[0m");
                    return;
                }
                let msg = event.payload.get("error")
                    .or(event.payload.get("reason"))
                    .or(event.payload.get("violation"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error");
                eprintln!("  \x1b[31m❌ {msg}\x1b[0m");
            }
            _ => {}
        }
    }
}

fn render_tool_completed(event: &DomainEvent) {
    let success = event.payload.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
    let tool_name = event.payload.get("tool_name").and_then(|v| v.as_str()).unwrap_or("?");
    let input = &event.payload["input"];
    let output = event.payload.get("output_preview").and_then(|v| v.as_str()).unwrap_or("");
    let duration = event.payload.get("duration_ms").and_then(|v| v.as_u64()).unwrap_or(0);

    let status = if success {
        "\x1b[32m✓\x1b[0m"
    } else {
        "\x1b[31m✗\x1b[0m"
    };

    let duration_str = if duration > 1000 {
        format!(" \x1b[90m({:.1}s)\x1b[0m", duration as f64 / 1000.0)
    } else {
        String::new()
    };

    match tool_name {
        "read" => {
            let path = input.get("filePath").and_then(|v| v.as_str()).unwrap_or("?");
            let lines = output.lines().count();
            eprintln!("  \x1b[36m🔧 read\x1b[0m {path} {status} \x1b[90m({lines} lines)\x1b[0m{duration_str}");
        }
        "write" => {
            let path = input.get("filePath").and_then(|v| v.as_str()).unwrap_or("?");
            let lines = input.get("content").and_then(|v| v.as_str()).map(|c| c.lines().count()).unwrap_or(0);
            eprintln!("  \x1b[36m🔧 write\x1b[0m {path} {status} \x1b[90m({lines} lines)\x1b[0m{duration_str}");
            if success {
                // Show first 3 lines preview
                if let Some(content) = input.get("content").and_then(|v| v.as_str()) {
                    for line in content.lines().take(3) {
                        eprintln!("    \x1b[32m+\x1b[0m {}", truncate_line(line, 80));
                    }
                    let total = content.lines().count();
                    if total > 3 {
                        eprintln!("    \x1b[90m... +{} more lines\x1b[0m", total - 3);
                    }
                }
            }
        }
        "edit" => {
            let path = input.get("filePath").and_then(|v| v.as_str()).unwrap_or("?");
            eprintln!("  \x1b[36m🔧 edit\x1b[0m {path} {status}{duration_str}");
            if success {
                if let Some(old) = input.get("oldString").and_then(|v| v.as_str()) {
                    let old_first = old.lines().next().unwrap_or("");
                    eprintln!("    \x1b[31m-\x1b[0m {}", truncate_line(old_first, 80));
                }
                if let Some(new) = input.get("newString").and_then(|v| v.as_str()) {
                    let new_first = new.lines().next().unwrap_or("");
                    eprintln!("    \x1b[32m+\x1b[0m {}", truncate_line(new_first, 80));
                    let new_lines = new.lines().count();
                    if new_lines > 1 {
                        eprintln!("    \x1b[90m  ... +{} more lines\x1b[0m", new_lines - 1);
                    }
                }
            }
        }
        "glob" => {
            let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("*");
            let count = output.lines().filter(|l| !l.is_empty()).count();
            eprintln!("  \x1b[36m🔧 glob\x1b[0m {pattern} {status} \x1b[90m({count} files)\x1b[0m{duration_str}");
        }
        "grep" => {
            let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("?");
            let count = output.lines().filter(|l| !l.is_empty()).count();
            eprintln!("  \x1b[36m🔧 grep\x1b[0m \"{pattern}\" {status} \x1b[90m({count} matches)\x1b[0m{duration_str}");
        }
        "apply_patch" => {
            // Extract filenames from the patch text (--- a/file, +++ b/file)
            let patch = input.get("patchText").and_then(|v| v.as_str()).unwrap_or("");
            let files: Vec<&str> = patch.lines()
                .filter(|l| l.starts_with("+++ ") || l.starts_with("--- "))
                .filter_map(|l| l.strip_prefix("+++ b/").or(l.strip_prefix("+++ ")))
                .filter(|f| *f != "/dev/null")
                .collect();
            let file_list = if files.is_empty() {
                "patch".to_string()
            } else {
                files.join(", ")
            };
            let hunks = patch.lines().filter(|l| l.starts_with("@@")).count();
            eprintln!("  \x1b[36m🔧 edit\x1b[0m {file_list} {status} \x1b[90m({hunks} hunks)\x1b[0m{duration_str}");
            if success && !patch.is_empty() {
                // Show first changed line
                for line in patch.lines().take(30) {
                    if line.starts_with('+') && !line.starts_with("+++") {
                        eprintln!("    \x1b[32m{}\x1b[0m", truncate_line(line, 80));
                        break;
                    }
                }
            }
        }
        "bash" => {
            let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("?");
            let cmd_short = truncate_line(cmd, 60);
            eprintln!("  \x1b[36m🔧 bash\x1b[0m {cmd_short} {status}{duration_str}");
        }
        "think" => {
            let thought = input.get("thought").and_then(|v| v.as_str()).unwrap_or("");
            let preview = truncate_line(thought, 70);
            eprintln!("  \x1b[36m🔧 think\x1b[0m \x1b[90m\"{preview}\"\x1b[0m {status}");
        }
        "reflect" => {
            let confidence = input.get("confidence").and_then(|v| v.as_u64()).unwrap_or(0);
            let color = if confidence >= 70 { "32" } else if confidence >= 40 { "33" } else { "31" };
            eprintln!("  \x1b[36m🔧 reflect\x1b[0m {status} \x1b[{color}m(confidence: {confidence}%)\x1b[0m");
        }
        "memory" => {
            let action = input.get("action").and_then(|v| v.as_str()).unwrap_or("?");
            let key = input.get("key").and_then(|v| v.as_str()).unwrap_or("");
            if key.is_empty() {
                eprintln!("  \x1b[36m🔧 memory\x1b[0m {action} {status}");
            } else {
                eprintln!("  \x1b[36m🔧 memory\x1b[0m {action}: {key} {status}");
            }
        }
        "done" => {
            eprintln!("  \x1b[36m🔧 done\x1b[0m {status}");
        }
        _ => {
            eprintln!("  \x1b[36m🔧 {tool_name}\x1b[0m {status}{duration_str}");
        }
    }
}

fn truncate_line(s: &str, max: usize) -> String {
    let first_line = s.lines().next().unwrap_or(s);
    if first_line.len() > max {
        let mut end = max;
        while end > 0 && !first_line.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &first_line[..end])
    } else {
        first_line.to_string()
    }
}
