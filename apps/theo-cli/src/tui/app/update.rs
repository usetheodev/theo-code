//! Single-purpose slice extracted from `tui/app.rs` (T5.4 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(dead_code, unused_imports)]

use std::collections::HashMap;
use std::time::Instant;

use theo_domain::event::{DomainEvent, EventType};

use super::*;
use super::state_types::*;

pub fn update(state: &mut TuiState, msg: Msg) {
    match msg {
        Msg::Quit => state.should_quit = true,
        Msg::Resize(w, h) => state.size = (w, h),
        Msg::DomainEvent(event) => handle_domain_event(state, event),
        Msg::DomainEventBatch(events) => {
            for event in events {
                handle_domain_event(state, event);
            }
        }
        Msg::EventsLost(n) => apply_events_lost(state, n),
        Msg::InputChar(c) => apply_input_char(state, c),
        Msg::InputBackspace => apply_input_backspace(state),
        Msg::InputDelete => apply_input_delete(state),
        Msg::InputLeft => apply_input_left(state),
        Msg::InputRight => apply_input_right(state),
        Msg::InputHome => state.input_cursor = 0,
        Msg::InputEnd => state.input_cursor = state.input_text.len(),
        Msg::Submit(text) => apply_submit(state, text),
        Msg::CursorBlink => state.cursor_visible = !state.cursor_visible,
        Msg::ScrollUp(n) => apply_scroll_up(state, n),
        Msg::ScrollDown(n) => apply_scroll_down(state, n),
        Msg::ScrollToBottom => apply_scroll_to_bottom(state),
        Msg::ToggleHelp => state.show_help = !state.show_help,
        Msg::CycleMode => apply_cycle_mode(state),
        Msg::SearchStart => apply_search_start(state),
        Msg::SearchChar(c) => apply_search_char(state, c),
        Msg::SearchBackspace => apply_search_backspace(state),
        Msg::SearchNext => apply_search_next(state),
        Msg::SearchPrev => apply_search_prev(state),
        Msg::SearchClose => apply_search_close(state),
        Msg::AgentComplete(summary, success) => apply_agent_complete(state, summary, success),
        Msg::RestoreLastPrompt => apply_restore_last_prompt(state),
        Msg::Notify(message) => state.transcript.push(TranscriptEntry::SystemMessage(message)),
        Msg::PartialProgressUpdate(lines) => state.partial_progress = lines,
        Msg::CopyToClipboard(text) => apply_copy_to_clipboard(state, text),
        Msg::InterruptAgent => apply_interrupt_agent(state),
        Msg::ClearTranscript => apply_clear_transcript(state),
        Msg::ExportSession => state.transcript.push(TranscriptEntry::SystemMessage(
            "Exporting session...".to_string(),
        )),
        Msg::ToggleSidebar => state.show_sidebar = !state.show_sidebar,
        Msg::NextSidebarTab => state.sidebar_tab = state.sidebar_tab.next(),
        Msg::ToggleModelPicker => apply_toggle_model_picker(state),
        Msg::ModelPickerUp => apply_model_picker_up(state),
        Msg::ModelPickerDown => apply_model_picker_down(state),
        Msg::ModelPickerSelect => apply_model_picker_select(state),
        Msg::ToggleThemePicker => apply_toggle_theme_picker(state),
        Msg::ThemePickerUp => apply_theme_picker_up(state),
        Msg::ThemePickerDown => state.theme_picker_selected += 1,
        Msg::ThemePickerSelect => apply_theme_picker_select(state),
        Msg::AutocompleteUpdate => update_autocomplete(state),
        Msg::AutocompleteUp => apply_autocomplete_up(state),
        Msg::AutocompleteDown => apply_autocomplete_down(state),
        Msg::AutocompleteAccept => apply_autocomplete_accept(state),
        Msg::AutocompleteClose => state.autocomplete.active = false,
        Msg::NewTab => apply_new_tab(state),
        Msg::CloseTab => apply_close_tab(state),
        Msg::SwitchTab(idx) => apply_switch_tab(state, idx),
        Msg::ToggleTimeline => state.show_timeline = !state.show_timeline,
        Msg::NotifyCompletion(summary) => apply_notify_completion(summary),
        Msg::ApproveDecision | Msg::RejectDecision => state.pending_approval = None,
        // Auth — actual login/logout happens in mod.rs; these are UI state updates.
        Msg::LoginStart(_provider) => state.transcript.push(TranscriptEntry::SystemMessage(
            "🔐 Starting OpenAI device flow...".to_string(),
        )),
        Msg::LoginServer(url) => state.transcript.push(TranscriptEntry::SystemMessage(format!(
            "🔐 Connecting to {url}..."
        ))),
        Msg::LoginWithKey(key) => apply_login_with_key(state, key),
        Msg::LoginComplete(msg) => state.transcript.push(TranscriptEntry::SystemMessage(msg)),
        Msg::LoginFailed(err) => apply_login_failed(state, err),
        Msg::LogoutRequest => apply_logout_request(state),
        Msg::MemoryCommand(arg) => apply_memory_command(state, arg),
        Msg::SkillsCommand => state.transcript.push(TranscriptEntry::SystemMessage(
            "Loading skills...".to_string(),
        )),
        Msg::SetMode(mode_str) => apply_set_mode(state, mode_str),
        Msg::ToggleCopyMode => apply_toggle_copy_mode(state),
        Msg::CopyLastResponse => apply_copy_last_response(state),
        Msg::CopyLastCodeBlock => apply_copy_last_code_block(state),
        Msg::SetTheme(name) => apply_set_theme(state, name),
    }
}

fn apply_events_lost(state: &mut TuiState, n: u64) {
    state.events_lost += n;
    state
        .transcript
        .push(TranscriptEntry::SystemMessage(format!(
            "[{n} events lost — display may be incomplete]"
        )));
}

fn apply_scroll_up(state: &mut TuiState, n: usize) {
    state.scroll_offset = state.scroll_offset.saturating_add(n);
    state.scroll_locked_to_bottom = false;
}

fn apply_scroll_down(state: &mut TuiState, n: usize) {
    state.scroll_offset = state.scroll_offset.saturating_sub(n);
    if state.scroll_offset == 0 {
        state.scroll_locked_to_bottom = true;
    }
}

fn apply_scroll_to_bottom(state: &mut TuiState) {
    state.scroll_offset = 0;
    state.scroll_locked_to_bottom = true;
}

fn apply_cycle_mode(state: &mut TuiState) {
    state.status.mode = match state.status.mode.as_str() {
        "AGENT" => "PLAN".to_string(),
        "PLAN" => "ASK".to_string(),
        _ => "AGENT".to_string(),
    };
}

fn apply_search_start(state: &mut TuiState) {
    state.search_mode = true;
    state.search_query.clear();
    state.search_results.clear();
    state.search_current = 0;
}

fn apply_search_char(state: &mut TuiState, c: char) {
    state.search_query.push(c);
    run_search(state);
}

fn apply_search_backspace(state: &mut TuiState) {
    state.search_query.pop();
    run_search(state);
}

fn apply_search_next(state: &mut TuiState) {
    if state.search_results.is_empty() {
        return;
    }
    state.search_current = (state.search_current + 1) % state.search_results.len();
}

fn apply_search_prev(state: &mut TuiState) {
    if state.search_results.is_empty() {
        return;
    }
    state.search_current = if state.search_current == 0 {
        state.search_results.len() - 1
    } else {
        state.search_current - 1
    };
}

fn apply_search_close(state: &mut TuiState) {
    state.search_mode = false;
    state.search_query.clear();
    state.search_results.clear();
}

fn apply_restore_last_prompt(state: &mut TuiState) {
    if !state.input_text.is_empty() {
        return;
    }
    if let Some(last) = state.prompt_history.last() {
        state.input_text = last.clone();
        state.input_cursor = state.input_text.len();
    }
}

fn apply_clear_transcript(state: &mut TuiState) {
    state.transcript.clear();
    state.active_tool_cards.clear();
    state.scroll_offset = 0;
    state.scroll_locked_to_bottom = true;
}

fn apply_toggle_model_picker(state: &mut TuiState) {
    state.show_model_picker = !state.show_model_picker;
    state.model_picker_selected = 0;
}

fn apply_model_picker_up(state: &mut TuiState) {
    if state.model_picker_selected > 0 {
        state.model_picker_selected -= 1;
    }
}

fn apply_model_picker_down(state: &mut TuiState) {
    if state.model_picker_selected < state.available_models.len().saturating_sub(1) {
        state.model_picker_selected += 1;
    }
}

fn apply_toggle_theme_picker(state: &mut TuiState) {
    state.show_theme_picker = !state.show_theme_picker;
    state.theme_picker_selected = 0;
}

fn apply_theme_picker_up(state: &mut TuiState) {
    if state.theme_picker_selected > 0 {
        state.theme_picker_selected -= 1;
    }
}

fn apply_autocomplete_up(state: &mut TuiState) {
    if state.autocomplete.selected > 0 {
        state.autocomplete.selected -= 1;
    }
}

fn apply_autocomplete_down(state: &mut TuiState) {
    if state.autocomplete.selected < state.autocomplete.candidates.len().saturating_sub(1) {
        state.autocomplete.selected += 1;
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

fn apply_agent_complete(state: &mut TuiState, summary: String, success: bool) {
    state.agent_running = false;
    state.streaming_assistant = false;
    if summary.is_empty() {
        return;
    }
    let icon = if success { "✓" } else { "✗" };
    state
        .transcript
        .push(TranscriptEntry::SystemMessage(format!("{icon} {summary}")));
    // OS notification (F7-T05).
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

fn apply_copy_to_clipboard(state: &mut TuiState, text: String) {
    // OSC52 clipboard escape (works in modern terminals + tmux + SSH).
    eprint!("\x1b]52;c;{}\x07", base64_encode(&text));
    state
        .transcript
        .push(TranscriptEntry::SystemMessage("Copied to clipboard".to_string()));
}

fn apply_interrupt_agent(state: &mut TuiState) {
    if state.agent_running {
        state.agent_running = false;
        state.streaming_assistant = false;
        state.transcript.push(TranscriptEntry::SystemMessage(
            "⏸ Agent interrupted. Enter a new prompt to continue.".to_string(),
        ));
        state.transcript.push(TranscriptEntry::SystemMessage(
            "Agent interrupted".to_string(),
        ));
    } else {
        // Ctrl+C with no running agent quits the TUI.
        state.should_quit = true;
    }
}

fn apply_new_tab(state: &mut TuiState) {
    let n = state.tabs.len() + 1;
    state.tabs.push(TabState {
        name: format!("Session {n}"),
        transcript_snapshot: Vec::new(),
    });
    if let Some(tab) = state.tabs.get_mut(state.active_tab) {
        tab.transcript_snapshot = state.transcript.clone();
    }
    state.active_tab = state.tabs.len() - 1;
    state.transcript.clear();
    state.active_tool_cards.clear();
    state.tool_chain.clear();
}

fn apply_close_tab(state: &mut TuiState) {
    if state.tabs.len() <= 1 {
        return;
    }
    state.tabs.remove(state.active_tab);
    if state.active_tab >= state.tabs.len() {
        state.active_tab = state.tabs.len() - 1;
    }
    if let Some(tab) = state.tabs.get(state.active_tab) {
        state.transcript = tab.transcript_snapshot.clone();
    }
}

fn apply_switch_tab(state: &mut TuiState, idx: usize) {
    if idx >= state.tabs.len() || idx == state.active_tab {
        return;
    }
    if let Some(tab) = state.tabs.get_mut(state.active_tab) {
        tab.transcript_snapshot = state.transcript.clone();
    }
    state.active_tab = idx;
    if let Some(tab) = state.tabs.get(state.active_tab) {
        state.transcript = tab.transcript_snapshot.clone();
    }
}

fn apply_notify_completion(summary: String) {
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
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = summary;
    }
}

fn apply_logout_request(state: &mut TuiState) {
    let auth_path = std::path::PathBuf::from(
        std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()),
    )
    .join(".config/theo/auth.json");
    let _ = std::fs::remove_file(&auth_path);
    state.transcript.push(TranscriptEntry::SystemMessage(
        "Logged out. Tokens cleared.".to_string(),
    ));
}

fn apply_memory_command(state: &mut TuiState, arg: String) {
    // Memory operations are handled in mod.rs (needs async IO).
    if arg.is_empty() || arg == "list" {
        state
            .transcript
            .push(TranscriptEntry::SystemMessage("Loading memories...".to_string()));
    }
}

fn apply_set_mode(state: &mut TuiState, mode_str: String) {
    let mode = match mode_str.to_lowercase().as_str() {
        "agent" => "AGENT",
        "plan" => "PLAN",
        "ask" => "ASK",
        _ => {
            state.transcript.push(TranscriptEntry::SystemMessage(format!(
                "Unknown mode: {mode_str}. Use: agent, plan, ask"
            )));
            return;
        }
    };
    state.status.mode = mode.to_string();
    state
        .transcript
        .push(TranscriptEntry::SystemMessage(format!("Mode: {mode}")));
}

fn apply_toggle_copy_mode(state: &mut TuiState) {
    state.copy_mode = !state.copy_mode;
    let msg = if state.copy_mode {
        "📋 Copy mode ON — use mouse to select text, then right-click or Ctrl+Shift+C to copy. Press Ctrl+Y again to exit."
    } else {
        "📋 Copy mode OFF"
    };
    state
        .transcript
        .push(TranscriptEntry::SystemMessage(msg.to_string()));
}

fn apply_copy_last_response(state: &mut TuiState) {
    let last_assistant = state.transcript.iter().rev().find_map(|e| {
        if let TranscriptEntry::Assistant(text) = e {
            Some(text.clone())
        } else {
            None
        }
    });
    if let Some(text) = last_assistant {
        eprint!("\x1b]52;c;{}\x07", base64_encode(&text));
        state.transcript.push(TranscriptEntry::SystemMessage(
            "📋 Last response copied to clipboard".to_string(),
        ));
    } else {
        state.transcript.push(TranscriptEntry::SystemMessage(
            "No assistant response to copy".to_string(),
        ));
    }
}

fn apply_copy_last_code_block(state: &mut TuiState) {
    let last_code = state.transcript.iter().rev().find_map(|e| {
        let TranscriptEntry::Assistant(text) = e else {
            return None;
        };
        let start = text.find("```")?;
        let after = &text[start + 3..];
        let code_start = after.find('\n').map(|i| i + 1).unwrap_or(0);
        let end = after[code_start..].find("```")?;
        Some(after[code_start..code_start + end].trim().to_string())
    });
    if let Some(code) = last_code {
        eprint!("\x1b]52;c;{}\x07", base64_encode(&code));
        state.transcript.push(TranscriptEntry::SystemMessage(
            "📋 Code block copied to clipboard".to_string(),
        ));
    } else {
        state.transcript.push(TranscriptEntry::SystemMessage(
            "No code block found to copy".to_string(),
        ));
    }
}

fn apply_set_theme(state: &mut TuiState, name: String) {
    let themes = super::super::theme::Theme::all();
    if let Some(theme) = themes.iter().find(|t| t.name == name) {
        state.theme = theme.clone();
        state
            .transcript
            .push(TranscriptEntry::SystemMessage(format!("Theme: {name}")));
    } else {
        let available: Vec<&str> = themes.iter().map(|t| t.name.as_str()).collect();
        state.transcript.push(TranscriptEntry::SystemMessage(format!(
            "Unknown theme. Available: {}",
            available.join(", ")
        )));
    }
}
