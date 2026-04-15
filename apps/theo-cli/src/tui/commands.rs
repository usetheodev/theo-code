//! Slash command processing for TUI mode.
//!
//! Full command set: /help /status /clear /export /mode /quit /login /logout
//! /memory /skills /timeline /theme /tab /history /model /sidebar

use super::app::{Msg, TuiState, TranscriptEntry, ToastLevel};

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
        // --- Core ---
        "/help" | "/h" | "/?" => {
            Some(vec![Msg::ToggleHelp])
        }
        "/quit" | "/exit" | "/q" => {
            Some(vec![Msg::Quit])
        }
        "/clear" | "/cls" => {
            Some(vec![Msg::ClearTranscript])
        }

        // --- Status & info ---
        "/status" | "/s" => {
            let status_text = format!(
                "Provider: {} | Model: {} | Mode: {} | Phase: {} | Iter: {}/{} | Tokens: {}in/{}out",
                state.status.provider,
                state.status.model,
                state.status.mode,
                state.status.phase,
                state.status.iteration,
                state.status.max_iterations,
                state.status.tokens_in,
                state.status.tokens_out,
            );
            Some(vec![Msg::ShowToast(status_text, ToastLevel::Info)])
        }

        // --- Auth ---
        "/login" => {
            Some(vec![Msg::LoginStart(arg.to_string())])
        }
        "/logout" => {
            Some(vec![Msg::LogoutRequest])
        }

        // --- Mode & model ---
        "/mode" => {
            if arg.is_empty() {
                Some(vec![Msg::CycleMode])
            } else {
                Some(vec![Msg::SetMode(arg.to_string())])
            }
        }
        "/model" => {
            Some(vec![Msg::ToggleModelPicker])
        }

        // --- Session ---
        "/export" => {
            Some(vec![Msg::ExportSession])
        }
        "/tab" | "/new" => {
            Some(vec![Msg::NewTab])
        }
        "/close" => {
            Some(vec![Msg::CloseTab])
        }

        // --- Memory ---
        "/memory" | "/mem" => {
            Some(vec![Msg::MemoryCommand(arg.to_string())])
        }

        // --- Skills ---
        "/skills" => {
            Some(vec![Msg::SkillsCommand])
        }

        // --- Visual ---
        "/timeline" | "/chain" => {
            Some(vec![Msg::ToggleTimeline])
        }
        "/theme" => {
            if arg.is_empty() {
                Some(vec![Msg::ToggleThemePicker])
            } else {
                Some(vec![Msg::SetTheme(arg.to_string())])
            }
        }
        "/sidebar" | "/panel" => {
            Some(vec![Msg::ToggleSidebar])
        }

        // --- Search ---
        "/history" => {
            let sessions_dir = std::path::PathBuf::from(
                std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string())
            ).join(".config/theo/sessions");
            let mut found = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&sessions_dir) {
                for entry in entries.flatten() {
                    if entry.path().extension().is_some_and(|e| e == "json") {
                        if let Ok(data) = std::fs::read_to_string(entry.path()) {
                            if let Ok(msgs) = serde_json::from_str::<Vec<serde_json::Value>>(&data) {
                                for msg in &msgs {
                                    if let Some(content) = msg.get("content").and_then(|v| v.as_str()) {
                                        if !arg.is_empty() && content.to_lowercase().contains(&arg.to_lowercase()) {
                                            found.push(format!("  {}", &content[..content.len().min(80)]));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if found.is_empty() {
                Some(vec![Msg::ShowToast(
                    if arg.is_empty() { "Usage: /history <query>".into() } else { "No matches found".into() },
                    ToastLevel::Info,
                )])
            } else {
                let summary = format!("{} matches across sessions", found.len());
                Some(vec![Msg::ShowToast(summary, ToastLevel::Info)])
            }
        }
        "/search" | "/find" => {
            Some(vec![Msg::SearchStart])
        }

        // --- Unknown ---
        _ => {
            Some(vec![Msg::ShowToast(
                format!("Unknown command: {cmd}. Try /help"),
                ToastLevel::Warning,
            )])
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
