//! Slash command processing for TUI mode.
//!
//! Handles /help, /clear, /status, /export, /mode commands inline.

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
    let _arg = parts.get(1).copied().unwrap_or("");

    match cmd.as_str() {
        "/help" | "/h" | "/?" => {
            Some(vec![Msg::ToggleHelp])
        }
        "/clear" => {
            Some(vec![Msg::ClearTranscript])
        }
        "/status" | "/s" => {
            let status_text = format!(
                "Provider: {} | Model: {} | Mode: {} | Phase: {} | Iteration: {}/{} | Tokens: {}in/{}out",
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
        "/export" => {
            Some(vec![Msg::ExportSession])
        }
        "/mode" => {
            Some(vec![Msg::CycleMode])
        }
        "/quit" | "/exit" | "/q" => {
            Some(vec![Msg::Quit])
        }
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
