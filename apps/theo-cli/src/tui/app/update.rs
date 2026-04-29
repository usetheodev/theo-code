//! Single-purpose slice extracted from `tui/app.rs` (T5.4 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(dead_code, unused_imports)]

use std::collections::HashMap;
use std::time::Instant;

use theo_domain::event::{DomainEvent, EventType};

use super::*;
use super::state_types::*;

pub fn update(state: &mut TuiState, msg: Msg) {
    match msg {
        Msg::Quit => {
            state.should_quit = true;
        }
        Msg::Resize(w, h) => {
            state.size = (w, h);
        }
        Msg::DomainEvent(event) => {
            handle_domain_event(state, event);
        }
        Msg::DomainEventBatch(events) => {
            for event in events {
                handle_domain_event(state, event);
            }
        }
        Msg::EventsLost(n) => {
            state.events_lost += n;
            state.transcript.push(TranscriptEntry::SystemMessage(
                format!("[{n} events lost — display may be incomplete]"),
            ));
        }
        Msg::InputChar(c) => apply_input_char(state, c),
        Msg::InputBackspace => apply_input_backspace(state),
        Msg::InputDelete => apply_input_delete(state),
        Msg::InputLeft => apply_input_left(state),
        Msg::InputRight => apply_input_right(state),
        Msg::InputHome => state.input_cursor = 0,
        Msg::InputEnd => state.input_cursor = state.input_text.len(),
        Msg::Submit(text) => apply_submit(state, text),
        Msg::CursorBlink => {
            state.cursor_visible = !state.cursor_visible;
        }
        Msg::ScrollUp(n) => {
            state.scroll_offset = state.scroll_offset.saturating_add(n);
            state.scroll_locked_to_bottom = false;
        }
        Msg::ScrollDown(n) => {
            state.scroll_offset = state.scroll_offset.saturating_sub(n);
            if state.scroll_offset == 0 {
                state.scroll_locked_to_bottom = true;
            }
        }
        Msg::ScrollToBottom => {
            state.scroll_offset = 0;
            state.scroll_locked_to_bottom = true;
        }
        Msg::ToggleHelp => {
            state.show_help = !state.show_help;
        }
        Msg::CycleMode => {
            state.status.mode = match state.status.mode.as_str() {
                "AGENT" => "PLAN".to_string(),
                "PLAN" => "ASK".to_string(),
                _ => "AGENT".to_string(),
            };
        }
        Msg::SearchStart => {
            state.search_mode = true;
            state.search_query.clear();
            state.search_results.clear();
            state.search_current = 0;
        }
        Msg::SearchChar(c) => {
            state.search_query.push(c);
            run_search(state);
        }
        Msg::SearchBackspace => {
            state.search_query.pop();
            run_search(state);
        }
        Msg::SearchNext => {
            if !state.search_results.is_empty() {
                state.search_current = (state.search_current + 1) % state.search_results.len();
            }
        }
        Msg::SearchPrev => {
            if !state.search_results.is_empty() {
                state.search_current = if state.search_current == 0 {
                    state.search_results.len() - 1
                } else {
                    state.search_current - 1
                };
            }
        }
        Msg::SearchClose => {
            state.search_mode = false;
            state.search_query.clear();
            state.search_results.clear();
        }
        Msg::AgentComplete(summary, success) => {
            state.agent_running = false;
            state.streaming_assistant = false;
            if !summary.is_empty() {
                let icon = if success { "✓" } else { "✗" };
                state.transcript.push(TranscriptEntry::SystemMessage(
                    format!("{icon} {summary}"),
                ));
                // OS notification (F7-T05)
                #[cfg(target_os = "linux")]
                {
                    let title = if success { "Theo ✓" } else { "Theo ✗" };
                    let _ = std::process::Command::new("notify-send")
                        .args([title, &summary])
                        .spawn();
                }
                #[cfg(target_os = "macos")]
                {
                    let script = format!(
                        "display notification \"{}\" with title \"Theo\"",
                        summary.replace('"', "\\\"")
                    );
                    let _ = std::process::Command::new("osascript")
                        .args(["-e", &script])
                        .spawn();
                }
            }
        }
        Msg::RestoreLastPrompt => {
            if state.input_text.is_empty()
                && let Some(last) = state.prompt_history.last() {
                    state.input_text = last.clone();
                    state.input_cursor = state.input_text.len();
                }
        }
        Msg::Notify(message) => {
            state.transcript.push(TranscriptEntry::SystemMessage(message));
        }
        Msg::PartialProgressUpdate(lines) => {
            // T14.1 — replace (latest-wins) the partial-progress
            // status. Empty Vec from the drainer = nothing in flight,
            // so clear the status. Pure assignment; the renderer
            // reads `state.partial_progress` each frame.
            state.partial_progress = lines;
        }
        Msg::CopyToClipboard(text) => {
            // OSC52 clipboard escape sequence (works in most modern terminals + tmux + SSH)
            eprint!("\x1b]52;c;{}\x07", base64_encode(&text));
            state.transcript.push(TranscriptEntry::SystemMessage("Copied to clipboard".to_string()));
        }
        Msg::InterruptAgent => {
            if state.agent_running {
                state.agent_running = false;
                state.streaming_assistant = false;
                state.transcript.push(TranscriptEntry::SystemMessage(
                    "⏸ Agent interrupted. Enter a new prompt to continue.".to_string(),
                ));
                state.transcript.push(TranscriptEntry::SystemMessage("Agent interrupted".to_string()));
            } else {
                // If agent is not running, Ctrl+C quits
                state.should_quit = true;
            }
        }
        Msg::ClearTranscript => {
            state.transcript.clear();
            state.active_tool_cards.clear();
            state.scroll_offset = 0;
            state.scroll_locked_to_bottom = true;
        }
        Msg::ExportSession => {
            state.transcript.push(TranscriptEntry::SystemMessage("Exporting session...".to_string()));
        }
        Msg::ToggleSidebar => {
            state.show_sidebar = !state.show_sidebar;
        }
        Msg::NextSidebarTab => {
            state.sidebar_tab = state.sidebar_tab.next();
        }
        Msg::ToggleModelPicker => {
            state.show_model_picker = !state.show_model_picker;
            state.model_picker_selected = 0;
        }
        Msg::ModelPickerUp => {
            if state.model_picker_selected > 0 {
                state.model_picker_selected -= 1;
            }
        }
        Msg::ModelPickerDown => {
            if state.model_picker_selected < state.available_models.len().saturating_sub(1) {
                state.model_picker_selected += 1;
            }
        }
        Msg::ModelPickerSelect => apply_model_picker_select(state),
        Msg::ToggleThemePicker => {
            state.show_theme_picker = !state.show_theme_picker;
            state.theme_picker_selected = 0;
        }
        Msg::ThemePickerUp => {
            if state.theme_picker_selected > 0 {
                state.theme_picker_selected -= 1;
            }
        }
        Msg::ThemePickerDown => {
            state.theme_picker_selected += 1;
        }
        Msg::ThemePickerSelect => apply_theme_picker_select(state),
        Msg::AutocompleteUpdate => update_autocomplete(state),
        Msg::AutocompleteUp => {
            if state.autocomplete.selected > 0 {
                state.autocomplete.selected -= 1;
            }
        }
        Msg::AutocompleteDown => {
            if state.autocomplete.selected < state.autocomplete.candidates.len().saturating_sub(1) {
                state.autocomplete.selected += 1;
            }
        }
        Msg::AutocompleteAccept => apply_autocomplete_accept(state),
        Msg::AutocompleteClose => {
            state.autocomplete.active = false;
        }
        Msg::NewTab => {
            let n = state.tabs.len() + 1;
            state.tabs.push(TabState {
                name: format!("Session {n}"),
                transcript_snapshot: Vec::new(),
            });
            // Save current transcript to current tab
            if let Some(tab) = state.tabs.get_mut(state.active_tab) {
                tab.transcript_snapshot = state.transcript.clone();
            }
            state.active_tab = state.tabs.len() - 1;
            state.transcript.clear();
            state.active_tool_cards.clear();
            state.tool_chain.clear();
        }
        Msg::CloseTab => {
            if state.tabs.len() > 1 {
                state.tabs.remove(state.active_tab);
                if state.active_tab >= state.tabs.len() {
                    state.active_tab = state.tabs.len() - 1;
                }
                // Restore transcript from new active tab
                if let Some(tab) = state.tabs.get(state.active_tab) {
                    state.transcript = tab.transcript_snapshot.clone();
                }
            }
        }
        Msg::SwitchTab(idx) => {
            if idx < state.tabs.len() && idx != state.active_tab {
                // Save current transcript
                if let Some(tab) = state.tabs.get_mut(state.active_tab) {
                    tab.transcript_snapshot = state.transcript.clone();
                }
                state.active_tab = idx;
                // Restore target tab transcript
                if let Some(tab) = state.tabs.get(state.active_tab) {
                    state.transcript = tab.transcript_snapshot.clone();
                }
            }
        }
        Msg::ToggleTimeline => {
            state.show_timeline = !state.show_timeline;
        }
        Msg::NotifyCompletion(summary) => {
            // OS notification for task completion (Linux: notify-send)
            #[cfg(target_os = "linux")]
            {
                let _ = std::process::Command::new("notify-send")
                    .args(["Theo", &summary])
                    .spawn();
            }
            #[cfg(target_os = "macos")]
            {
                let script = format!(
                    "display notification \"{}\" with title \"Theo\"",
                    summary.replace('"', "\\\"")
                );
                let _ = std::process::Command::new("osascript")
                    .args(["-e", &script])
                    .spawn();
            }
        }
        Msg::ApproveDecision => {
            // Actual resolution happens in mod.rs which has access to the gate
            state.pending_approval = None;
        }
        Msg::RejectDecision => {
            state.pending_approval = None;
        }
        // Auth — actual login/logout happens in mod.rs; these are UI state updates
        Msg::LoginStart(_provider) => state.transcript.push(TranscriptEntry::SystemMessage(
            "🔐 Starting OpenAI device flow...".to_string(),
        )),
        Msg::LoginServer(url) => state.transcript.push(TranscriptEntry::SystemMessage(format!(
            "🔐 Connecting to {}...",
            url
        ))),
        Msg::LoginWithKey(key) => apply_login_with_key(state, key),
        Msg::LoginComplete(msg) => {
            state.transcript.push(TranscriptEntry::SystemMessage(msg));
        }
        Msg::LoginFailed(err) => apply_login_failed(state, err),
        Msg::LogoutRequest => {
            // Clear stored tokens
            let auth_path = std::path::PathBuf::from(
                std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string())
            ).join(".config/theo/auth.json");
            let _ = std::fs::remove_file(&auth_path);
            state.transcript.push(TranscriptEntry::SystemMessage("Logged out. Tokens cleared.".to_string()));
        }
        Msg::MemoryCommand(arg) => {
            // Memory operations are handled in mod.rs (needs async IO)
            // Here we just show the intent
            if arg.is_empty() || arg == "list" {
                state.transcript.push(TranscriptEntry::SystemMessage(
                    "Loading memories...".to_string(),
                ));
            }
        }
        Msg::SkillsCommand => {
            state.transcript.push(TranscriptEntry::SystemMessage(
                "Loading skills...".to_string(),
            ));
        }
        Msg::SetMode(mode_str) => {
            let mode = match mode_str.to_lowercase().as_str() {
                "agent" => "AGENT",
                "plan" => "PLAN",
                "ask" => "ASK",
                _ => {
                    state.transcript.push(TranscriptEntry::SystemMessage(format!("Unknown mode: {mode_str}. Use: agent, plan, ask")));
                    return;
                }
            };
            state.status.mode = mode.to_string();
            state.transcript.push(TranscriptEntry::SystemMessage(format!("Mode: {mode}")));
        }
        Msg::ToggleCopyMode => {
            state.copy_mode = !state.copy_mode;
            let msg = if state.copy_mode {
                "📋 Copy mode ON — use mouse to select text, then right-click or Ctrl+Shift+C to copy. Press Ctrl+Y again to exit."
            } else {
                "📋 Copy mode OFF"
            };
            state.transcript.push(TranscriptEntry::SystemMessage(msg.to_string()));
        }
        Msg::CopyLastResponse => {
            // Find the last assistant message and copy via OSC52
            let last_assistant = state.transcript.iter().rev().find_map(|e| {
                if let TranscriptEntry::Assistant(text) = e { Some(text.clone()) } else { None }
            });
            if let Some(text) = last_assistant {
                eprint!("\x1b]52;c;{}\x07", base64_encode(&text));
                state.transcript.push(TranscriptEntry::SystemMessage(
                    "📋 Last response copied to clipboard".to_string()
                ));
            } else {
                state.transcript.push(TranscriptEntry::SystemMessage(
                    "No assistant response to copy".to_string()
                ));
            }
        }
        Msg::CopyLastCodeBlock => {
            // Find last code block in assistant messages
            let last_code = state.transcript.iter().rev().find_map(|e| {
                if let TranscriptEntry::Assistant(text) = e {
                    // Extract content between ``` markers
                    if let Some(start) = text.find("```") {
                        let after = &text[start + 3..];
                        // Skip language identifier line
                        let code_start = after.find('\n').map(|i| i + 1).unwrap_or(0);
                        if let Some(end) = after[code_start..].find("```") {
                            return Some(after[code_start..code_start + end].trim().to_string());
                        }
                    }
                    None
                } else {
                    None
                }
            });
            if let Some(code) = last_code {
                eprint!("\x1b]52;c;{}\x07", base64_encode(&code));
                state.transcript.push(TranscriptEntry::SystemMessage(
                    "📋 Code block copied to clipboard".to_string()
                ));
            } else {
                state.transcript.push(TranscriptEntry::SystemMessage(
                    "No code block found to copy".to_string()
                ));
            }
        }
        Msg::SetTheme(name) => {
            let themes = super::super::theme::Theme::all();
            if let Some(theme) = themes.iter().find(|t| t.name == name) {
                state.theme = theme.clone();
                state.transcript.push(TranscriptEntry::SystemMessage(format!("Theme: {name}")));
            } else {
                let available: Vec<&str> = themes.iter().map(|t| t.name.as_str()).collect();
                state.transcript.push(TranscriptEntry::SystemMessage(format!("Unknown theme. Available: {}", available.join(", "))));
            }
        }
    }
}


fn apply_input_char(state: &mut TuiState, c: char) {
    state.input_text.insert(state.input_cursor, c);
    state.input_cursor += c.len_utf8();
}

fn apply_input_backspace(state: &mut TuiState) {
    if state.input_cursor == 0 {
        return;
    }
    let prev = state.input_text[..state.input_cursor]
        .char_indices()
        .last()
        .map(|(i, _)| i)
        .unwrap_or(0);
    state.input_text.drain(prev..state.input_cursor);
    state.input_cursor = prev;
}

fn apply_input_delete(state: &mut TuiState) {
    if state.input_cursor >= state.input_text.len() {
        return;
    }
    let next = state.input_text[state.input_cursor..]
        .char_indices()
        .nth(1)
        .map(|(i, _)| state.input_cursor + i)
        .unwrap_or(state.input_text.len());
    state.input_text.drain(state.input_cursor..next);
}

fn apply_input_left(state: &mut TuiState) {
    if state.input_cursor == 0 {
        return;
    }
    state.input_cursor = state.input_text[..state.input_cursor]
        .char_indices()
        .last()
        .map(|(i, _)| i)
        .unwrap_or(0);
}

fn apply_input_right(state: &mut TuiState) {
    if state.input_cursor >= state.input_text.len() {
        return;
    }
    state.input_cursor = state.input_text[state.input_cursor..]
        .char_indices()
        .nth(1)
        .map(|(i, _)| state.input_cursor + i)
        .unwrap_or(state.input_text.len());
}

fn apply_submit(state: &mut TuiState, text: String) {
    if text.is_empty() {
        return;
    }
    state.prompt_history.push(text.clone());
    state.transcript.push(TranscriptEntry::User(text));
    state.input_text.clear();
    state.input_cursor = 0;
    // NOTE: agent_running is set by mod.rs AFTER spawning the agent;
    // setting it here would prevent the agent from launching.
    state.streaming_assistant = false;
    if state.scroll_locked_to_bottom {
        state.scroll_offset = 0;
    }
}

fn apply_model_picker_select(state: &mut TuiState) {
    if let Some(model) = state.available_models.get(state.model_picker_selected) {
        state.status.model = model.clone();
        state.show_model_picker = false;
        state
            .transcript
            .push(TranscriptEntry::SystemMessage(format!("Model: {model}")));
    }
}

fn apply_theme_picker_select(state: &mut TuiState) {
    let themes = super::super::theme::Theme::all();
    if let Some(theme) = themes.get(state.theme_picker_selected) {
        state.theme = theme.clone();
        state.show_theme_picker = false;
        state
            .transcript
            .push(TranscriptEntry::SystemMessage(format!("Theme: {}", theme.name)));
    }
}

fn apply_autocomplete_accept(state: &mut TuiState) {
    let Some(text) = state.autocomplete.selected_text().map(|s| s.to_string()) else {
        return;
    };
    state.input_text = if state.autocomplete.trigger
        == super::super::autocomplete::AutocompleteTrigger::Slash
    {
        text
    } else {
        // For @file, insert at cursor position.
        let before = &state.input_text[..state.input_text.rfind('@').unwrap_or(0)];
        format!("{}{} ", before, text)
    };
    state.input_cursor = state.input_text.len();
    state.autocomplete.active = false;
}

fn apply_login_with_key(state: &mut TuiState, key: String) {
    // SAFETY: ADR-021#rust_2024_test_env_var — single-threaded render-loop
    // context; no other task observes env vars during this update frame.
    unsafe {
        std::env::set_var("OPENAI_API_KEY", &key);
    }
    let masked = if key.len() > 8 {
        format!("{}...{}", &key[..6], &key[key.len() - 4..])
    } else {
        "***".to_string()
    };
    state.status.provider = "OpenAI".to_string();
    state
        .transcript
        .push(TranscriptEntry::SystemMessage(format!(
            "✓ API key set: {masked}"
        )));
    state.transcript.push(TranscriptEntry::SystemMessage(
        "Provider ready. You can now send tasks to the agent.".to_string(),
    ));
}

fn apply_login_failed(state: &mut TuiState, err: String) {
    state
        .transcript
        .push(TranscriptEntry::SystemMessage(format!(
            "✗ Login failed: {err}"
        )));
    let preview = if err.len() > 50 { &err[..50] } else { &err };
    state
        .transcript
        .push(TranscriptEntry::SystemMessage(format!(
            "Login failed: {}",
            preview
        )));
}
