//! Slash command processing for TUI mode.
//!
//! Full command set: /help /status /clear /export /mode /quit /login /logout
//! /memory /skills /timeline /theme /tab /history /model /sidebar

use super::app::{Msg, TuiState, TranscriptEntry};

/// Check if input is a slash command and return the corresponding Msg(s).
/// Returns None if not a command.
pub fn process_command(input: &str, state: &TuiState) -> Option<Vec<Msg>> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return None;
    }
    let parts: Vec<&str> = trimmed.splitn(2, ' ').collect();
    let cmd = parts[0].to_lowercase();
    let arg = parts.get(1).copied().unwrap_or("");

    match cmd.as_str() {
        "/help" | "/h" | "/?" => Some(vec![Msg::ToggleHelp]),
        "/quit" | "/exit" | "/q" => Some(vec![Msg::Quit]),
        "/clear" | "/cls" => Some(vec![Msg::ClearTranscript]),
        "/status" | "/s" => Some(vec![Msg::Notify(format_status(state))]),
        "/login" => Some(handle_login(arg)),
        "/logout" => Some(vec![Msg::LogoutRequest]),
        "/mode" => Some(if arg.is_empty() {
            vec![Msg::CycleMode]
        } else {
            vec![Msg::SetMode(arg.to_string())]
        }),
        "/model" => Some(vec![Msg::ToggleModelPicker]),
        "/export" => Some(vec![Msg::ExportSession]),
        "/tab" | "/new" => Some(vec![Msg::NewTab]),
        "/close" => Some(vec![Msg::CloseTab]),
        "/memory" | "/mem" => Some(vec![Msg::MemoryCommand(arg.to_string())]),
        "/skills" => Some(vec![Msg::SkillsCommand]),
        "/timeline" | "/chain" => Some(vec![Msg::ToggleTimeline]),
        "/theme" => Some(if arg.is_empty() {
            vec![Msg::ToggleThemePicker]
        } else {
            vec![Msg::SetTheme(arg.to_string())]
        }),
        "/sidebar" | "/panel" => Some(vec![Msg::ToggleSidebar]),
        "/history" => Some(vec![Msg::Notify(handle_history(arg))]),
        "/search" | "/find" => Some(vec![Msg::SearchStart]),
        "/copy" => Some(match arg {
            "code" | "block" => vec![Msg::CopyLastCodeBlock],
            _ => vec![Msg::CopyLastResponse],
        }),
        "/select" => Some(vec![Msg::ToggleCopyMode]),
        _ => Some(vec![Msg::Notify(format!(
            "Unknown command: {cmd}. Try /help"
        ))]),
    }
}

fn format_status(state: &TuiState) -> String {
    format!(
        "Provider: {} | Model: {} | Mode: {} | Phase: {} | Iter: {}/{} | Tokens: {}in/{}out",
        state.status.provider,
        state.status.model,
        state.status.mode,
        state.status.phase,
        state.status.iteration,
        state.status.max_iterations,
        state.status.tokens_in,
        state.status.tokens_out,
    )
}

fn handle_login(arg: &str) -> Vec<Msg> {
    if arg.starts_with("sk-") || arg.starts_with("sess-") {
        return vec![Msg::LoginWithKey(arg.to_string())];
    }
    if arg.starts_with("http") {
        return vec![Msg::LoginServer(arg.to_string())];
    }
    if arg == "device" || arg == "oauth" {
        return vec![Msg::LoginStart(arg.to_string())];
    }
    if arg.is_empty() {
        return vec![
            Msg::Notify("─── Login Options ───".into()),
            Msg::Notify("/login https://api.opencode.ai   Device flow (uses your plan)".into()),
            Msg::Notify("/login sk-xxxxx                  API key directly".into()),
            Msg::Notify("/login device                    OpenAI device flow".into()),
            Msg::Notify("Or: OPENAI_API_KEY=sk-xxx theo   Env var".into()),
        ];
    }
    vec![Msg::LoginWithKey(arg.to_string())]
}

fn handle_history(arg: &str) -> String {
    let sessions_dir = std::path::PathBuf::from(
        std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()),
    )
    .join(".config/theo/sessions");
    let mut found: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&sessions_dir) {
        for entry in entries.flatten() {
            collect_history_matches(&entry.path(), arg, &mut found);
        }
    }
    if found.is_empty() {
        if arg.is_empty() {
            "Usage: /history <query>".into()
        } else {
            "No matches found".into()
        }
    } else {
        format!("{} matches across sessions", found.len())
    }
}

fn collect_history_matches(path: &std::path::Path, arg: &str, found: &mut Vec<String>) {
    if path.extension().is_some_and(|e| e != "json") {
        return;
    }
    let Ok(data) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(msgs) = serde_json::from_str::<Vec<serde_json::Value>>(&data) else {
        return;
    };
    for msg in &msgs {
        let Some(content) = msg.get("content").and_then(|v| v.as_str()) else {
            continue;
        };
        if !arg.is_empty() && content.to_lowercase().contains(&arg.to_lowercase()) {
            found.push(format!("  {}", &content[..content.len().min(80)]));
        }
    }
}

/// Export transcript as markdown string.
pub fn export_transcript(state: &TuiState) -> String {
    let mut md = String::new();
    md.push_str("# Theo Session Export\n\n");
    md.push_str(&format!("Model: {} | Mode: {}\n\n", state.status.model, state.status.mode));
    md.push_str("---\n\n");

    for entry in &state.transcript {
        match entry {
            TranscriptEntry::User(text) => {
                md.push_str(&format!("**User:** {text}\n\n"));
            }
            TranscriptEntry::Assistant(text) => {
                md.push_str(&format!("{text}\n\n"));
            }
            TranscriptEntry::ToolCard(card) => {
                let status = match card.status {
                    super::app::ToolCardStatus::Running => "running",
                    super::app::ToolCardStatus::Succeeded => "ok",
                    super::app::ToolCardStatus::Failed => "failed",
                };
                let duration = card.duration_ms.map(|ms| format!(" ({ms}ms)")).unwrap_or_default();
                md.push_str(&format!("**Tool: {}** — {status}{duration}\n", card.tool_name));
                if !card.stdout_lines.is_empty() {
                    md.push_str("```\n");
                    for line in &card.stdout_lines {
                        md.push_str(line);
                        md.push('\n');
                    }
                    md.push_str("```\n");
                }
                md.push('\n');
            }
            TranscriptEntry::SystemMessage(text) => {
                md.push_str(&format!("> {text}\n\n"));
            }
        }
    }

    md
}
