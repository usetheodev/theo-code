//! TUI state and update logic — pure Elm/Redux pattern.
//!
//! `TuiState` holds all UI state. `Msg` represents all possible state transitions.
//! `update()` is a pure function: (state, msg) → mutated state, no IO.

use std::collections::HashMap;
use std::time::Instant;

use theo_domain::event::{DomainEvent, EventType};

// ---------------------------------------------------------------------------
// Transcript entries
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum TranscriptEntry {
    User(String),
    Assistant(String),
    ToolCard(ToolCardState),
    SystemMessage(String),
}

#[derive(Debug, Clone)]
pub struct ToolCardState {
    pub call_id: String,
    pub tool_name: String,
    pub status: ToolCardStatus,
    pub started_at: Instant,
    pub duration_ms: Option<u64>,
    pub stdout_lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolCardStatus {
    Running,
    Succeeded,
    Failed,
}

// ---------------------------------------------------------------------------
// StatusLine state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct StatusLineState {
    pub mode: String,
    pub model: String,
    pub provider: String,
    pub phase: String,
    pub iteration: usize,
    pub max_iterations: usize,
    pub tools_running: usize,
    pub tokens_in: u64,
    pub tokens_out: u64,
}

// ---------------------------------------------------------------------------
// TUI State
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct TuiState {
    pub should_quit: bool,
    pub transcript: Vec<TranscriptEntry>,
    pub active_tool_cards: HashMap<String, usize>, // call_id → transcript index
    pub input_text: String,
    pub input_cursor: usize,
    pub status: StatusLineState,
    pub cursor_visible: bool,
    pub size: (u16, u16),
    pub agent_running: bool,
    pub events_lost: u64,
    pub scroll_offset: usize,
    pub scroll_locked_to_bottom: bool,
    pub streaming_assistant: bool,
    pub show_help: bool,
    pub search_mode: bool,
    pub search_query: String,
    pub search_results: Vec<usize>, // indices into transcript
    pub search_current: usize,
    pub session_picker: Option<SessionPickerState>,
    pub toasts: Vec<Toast>,
    pub prompt_history: Vec<String>,
    pub show_sidebar: bool,
    pub sidebar_tab: super::widgets::sidebar::SidebarTab,
    pub show_model_picker: bool,
    pub available_models: Vec<String>,
    pub model_picker_selected: usize,
    pub theme: super::theme::Theme,
    pub show_theme_picker: bool,
    pub theme_picker_selected: usize,
    pub autocomplete: super::autocomplete::AutocompleteState,
    pub project_dir: std::path::PathBuf,
    // Session tabs (F6)
    pub tabs: Vec<TabState>,
    pub active_tab: usize,
    // Budget visual (F5-T04)
    pub budget_tokens_used: u64,
    pub budget_tokens_limit: u64,
    // Todo list (F5-T02)
    pub todos: Vec<TodoItem>,
    // Timeline (F4-T04)
    pub tool_chain: Vec<ToolChainEntry>,
    pub show_timeline: bool,
    // Approval modal (F4-T02)
    pub pending_approval: Option<PendingApproval>,
}

#[derive(Debug, Clone)]
pub struct TabState {
    pub name: String,
    pub transcript_snapshot: Vec<TranscriptEntry>,
}

#[derive(Debug, Clone)]
pub struct TodoItem {
    pub id: String,
    pub content: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct PendingApproval {
    pub decision_id: String,
    pub tool_name: String,
    pub risk_level: String,
    pub args_preview: String,
}

#[derive(Debug, Clone)]
pub struct ToolChainEntry {
    pub tool_name: String,
    pub reason: String, // why this tool was called
    pub status: ToolCardStatus,
    pub duration_ms: Option<u64>,
}

#[derive(Debug)]
pub struct SessionPickerState {
    pub sessions: Vec<SessionMeta>,
    pub selected: usize,
}

#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub path: std::path::PathBuf,
    pub modified: String, // formatted date
    pub message_count: usize,
    pub preview: String, // first user message, truncated
}

impl TuiState {
    pub fn new(
        provider: String,
        model: String,
        max_iterations: usize,
        width: u16,
        height: u16,
    ) -> Self {
        Self {
            should_quit: false,
            transcript: Vec::new(),
            active_tool_cards: HashMap::new(),
            input_text: String::new(),
            input_cursor: 0,
            status: StatusLineState {
                mode: "AGENT".to_string(),
                model,
                provider,
                phase: "READY".to_string(),
                iteration: 0,
                max_iterations,
                tools_running: 0,
                tokens_in: 0,
                tokens_out: 0,
            },
            cursor_visible: true,
            size: (width, height),
            agent_running: false,
            events_lost: 0,
            scroll_offset: 0,
            scroll_locked_to_bottom: true,
            streaming_assistant: false,
            show_help: false,
            search_mode: false,
            search_query: String::new(),
            search_results: Vec::new(),
            search_current: 0,
            session_picker: None,
            toasts: Vec::new(),
            prompt_history: Vec::new(),
            show_sidebar: width > 120,
            sidebar_tab: super::widgets::sidebar::SidebarTab::Status,
            show_model_picker: false,
            available_models: vec![
                "gpt-4o".to_string(),
                "gpt-4o-mini".to_string(),
                "gpt-5.3-codex".to_string(),
                "claude-sonnet-4-5-20250514".to_string(),
                "claude-opus-4-5-20250514".to_string(),
                "o3-mini".to_string(),
            ],
            model_picker_selected: 0,
            theme: super::theme::Theme::dark(),
            show_theme_picker: false,
            theme_picker_selected: 0,
            autocomplete: super::autocomplete::AutocompleteState::new(),
            project_dir: std::path::PathBuf::new(),
            tabs: vec![TabState { name: "Session 1".to_string(), transcript_snapshot: Vec::new() }],
            active_tab: 0,
            budget_tokens_used: 0,
            budget_tokens_limit: 200_000,
            todos: Vec::new(),
            tool_chain: Vec::new(),
            show_timeline: false,
            pending_approval: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub message: String,
    pub level: ToastLevel,
    pub created: Instant,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToastLevel {
    Info,
    Warning,
    Error,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum Msg {
    Quit,
    Resize(u16, u16),
    DomainEvent(DomainEvent),
    DomainEventBatch(Vec<DomainEvent>),
    EventsLost(u64),
    InputChar(char),
    InputBackspace,
    InputDelete,
    InputLeft,
    InputRight,
    InputHome,
    InputEnd,
    Submit(String),
    CursorBlink,
    ScrollUp(usize),
    ScrollDown(usize),
    ScrollToBottom,
    ToggleHelp,
    CycleMode,
    SearchStart,
    SearchChar(char),
    SearchBackspace,
    SearchNext,
    SearchPrev,
    SearchClose,
    AgentComplete(String, bool), // (summary, success)
    RestoreLastPrompt,
    ShowToast(String, ToastLevel),
    DismissExpiredToasts,
    CopyToClipboard(String),
    InterruptAgent,
    ClearTranscript,
    ExportSession,
    ToggleSidebar,
    NextSidebarTab,
    ToggleModelPicker,
    ModelPickerUp,
    ModelPickerDown,
    ModelPickerSelect,
    ToggleThemePicker,
    ThemePickerUp,
    ThemePickerDown,
    ThemePickerSelect,
    AutocompleteUpdate,
    AutocompleteUp,
    AutocompleteDown,
    AutocompleteAccept,
    AutocompleteClose,
    // Session tabs (F6)
    NewTab,
    CloseTab,
    SwitchTab(usize),
    // Timeline (F4)
    ToggleTimeline,
    // Notifications
    NotifyCompletion(String),
    // Approval
    ApproveDecision,
    RejectDecision,
    // Auth
    LoginStart(String), // provider name (empty = auto-detect)
    LoginComplete(String), // success message
    LoginFailed(String), // error message
    LogoutRequest,
    // Memory
    MemoryCommand(String), // "list", "search <q>", "delete <k>"
    // Skills
    SkillsCommand,
    // Mode set
    SetMode(String), // "agent", "plan", "ask"
    // Theme set
    SetTheme(String), // theme name
}

// ---------------------------------------------------------------------------
// Update — pure function, no IO
// ---------------------------------------------------------------------------

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
        Msg::InputChar(c) => {
            state.input_text.insert(state.input_cursor, c);
            state.input_cursor += c.len_utf8();
        }
        Msg::InputBackspace => {
            if state.input_cursor > 0 {
                // Find previous char boundary
                let prev = state.input_text[..state.input_cursor]
                    .char_indices()
                    .last()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                state.input_text.drain(prev..state.input_cursor);
                state.input_cursor = prev;
            }
        }
        Msg::InputDelete => {
            if state.input_cursor < state.input_text.len() {
                let next = state.input_text[state.input_cursor..]
                    .char_indices()
                    .nth(1)
                    .map(|(i, _)| state.input_cursor + i)
                    .unwrap_or(state.input_text.len());
                state.input_text.drain(state.input_cursor..next);
            }
        }
        Msg::InputLeft => {
            if state.input_cursor > 0 {
                state.input_cursor = state.input_text[..state.input_cursor]
                    .char_indices()
                    .last()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
            }
        }
        Msg::InputRight => {
            if state.input_cursor < state.input_text.len() {
                state.input_cursor = state.input_text[state.input_cursor..]
                    .char_indices()
                    .nth(1)
                    .map(|(i, _)| state.input_cursor + i)
                    .unwrap_or(state.input_text.len());
            }
        }
        Msg::InputHome => {
            state.input_cursor = 0;
        }
        Msg::InputEnd => {
            state.input_cursor = state.input_text.len();
        }
        Msg::Submit(text) => {
            if !text.is_empty() {
                state.prompt_history.push(text.clone());
                state.transcript.push(TranscriptEntry::User(text));
                state.input_text.clear();
                state.input_cursor = 0;
                state.agent_running = true;
                state.streaming_assistant = false;
                if state.scroll_locked_to_bottom {
                    state.scroll_offset = 0;
                }
            }
        }
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
            if state.input_text.is_empty() {
                if let Some(last) = state.prompt_history.last() {
                    state.input_text = last.clone();
                    state.input_cursor = state.input_text.len();
                }
            }
        }
        Msg::ShowToast(message, level) => {
            state.toasts.push(Toast {
                message,
                level,
                created: Instant::now(),
            });
        }
        Msg::DismissExpiredToasts => {
            state.toasts.retain(|t| t.created.elapsed().as_secs() < 5);
        }
        Msg::CopyToClipboard(text) => {
            // OSC52 clipboard escape sequence (works in most modern terminals + tmux + SSH)
            eprint!("\x1b]52;c;{}\x07", base64_encode(&text));
            state.toasts.push(Toast {
                message: "Copied to clipboard".to_string(),
                level: ToastLevel::Info,
                created: Instant::now(),
            });
        }
        Msg::InterruptAgent => {
            if state.agent_running {
                state.agent_running = false;
                state.streaming_assistant = false;
                state.transcript.push(TranscriptEntry::SystemMessage(
                    "⏸ Agent interrupted. Enter a new prompt to continue.".to_string(),
                ));
                state.toasts.push(Toast {
                    message: "Agent interrupted".to_string(),
                    level: ToastLevel::Warning,
                    created: Instant::now(),
                });
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
            state.toasts.push(Toast {
                message: "Exporting session...".to_string(),
                level: ToastLevel::Info,
                created: Instant::now(),
            });
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
        Msg::ModelPickerSelect => {
            if let Some(model) = state.available_models.get(state.model_picker_selected) {
                state.status.model = model.clone();
                state.show_model_picker = false;
                state.toasts.push(Toast {
                    message: format!("Model: {model}"),
                    level: ToastLevel::Info,
                    created: Instant::now(),
                });
            }
        }
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
        Msg::ThemePickerSelect => {
            let themes = super::theme::Theme::all();
            if let Some(theme) = themes.get(state.theme_picker_selected) {
                state.theme = theme.clone();
                state.show_theme_picker = false;
                state.toasts.push(Toast {
                    message: format!("Theme: {}", theme.name),
                    level: ToastLevel::Info,
                    created: Instant::now(),
                });
            }
        }
        Msg::AutocompleteUpdate => {
            update_autocomplete(state);
        }
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
        Msg::AutocompleteAccept => {
            if let Some(text) = state.autocomplete.selected_text().map(|s| s.to_string()) {
                state.input_text = if state.autocomplete.trigger == super::autocomplete::AutocompleteTrigger::Slash {
                    text
                } else {
                    // For @file, insert at cursor position
                    let before = &state.input_text[..state.input_text.rfind('@').unwrap_or(0)];
                    format!("{}{} ", before, text)
                };
                state.input_cursor = state.input_text.len();
                state.autocomplete.active = false;
            }
        }
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
        Msg::LoginStart(_provider) => {
            state.transcript.push(TranscriptEntry::SystemMessage(
                "Starting login flow... Check your browser.".to_string(),
            ));
        }
        Msg::LoginComplete(msg) => {
            state.transcript.push(TranscriptEntry::SystemMessage(
                format!("✓ {msg}"),
            ));
            state.toasts.push(Toast {
                message: msg,
                level: ToastLevel::Info,
                created: Instant::now(),
            });
        }
        Msg::LoginFailed(err) => {
            state.transcript.push(TranscriptEntry::SystemMessage(
                format!("✗ Login failed: {err}"),
            ));
            state.toasts.push(Toast {
                message: format!("Login failed: {err}"),
                level: ToastLevel::Error,
                created: Instant::now(),
            });
        }
        Msg::LogoutRequest => {
            // Clear stored tokens
            let auth_path = std::path::PathBuf::from(
                std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string())
            ).join(".config/theo/auth.json");
            let _ = std::fs::remove_file(&auth_path);
            state.toasts.push(Toast {
                message: "Logged out. Tokens cleared.".to_string(),
                level: ToastLevel::Info,
                created: Instant::now(),
            });
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
                    state.toasts.push(Toast {
                        message: format!("Unknown mode: {mode_str}. Use: agent, plan, ask"),
                        level: ToastLevel::Warning,
                        created: Instant::now(),
                    });
                    return;
                }
            };
            state.status.mode = mode.to_string();
            state.toasts.push(Toast {
                message: format!("Mode: {mode}"),
                level: ToastLevel::Info,
                created: Instant::now(),
            });
        }
        Msg::SetTheme(name) => {
            let themes = super::theme::Theme::all();
            if let Some(theme) = themes.iter().find(|t| t.name == name) {
                state.theme = theme.clone();
                state.toasts.push(Toast {
                    message: format!("Theme: {name}"),
                    level: ToastLevel::Info,
                    created: Instant::now(),
                });
            } else {
                let available: Vec<&str> = themes.iter().map(|t| t.name.as_str()).collect();
                state.toasts.push(Toast {
                    message: format!("Unknown theme. Available: {}", available.join(", ")),
                    level: ToastLevel::Warning,
                    created: Instant::now(),
                });
            }
        }
    }
}

fn update_autocomplete(state: &mut TuiState) {
    use super::autocomplete::{self, AutocompleteTrigger};

    let input = &state.input_text;

    if input.starts_with('/') {
        // Slash command autocomplete
        let query = &input[1..];
        let all = autocomplete::slash_commands();
        let filtered = autocomplete::filter_candidates(&all, query);
        state.autocomplete.active = !filtered.is_empty();
        state.autocomplete.trigger = AutocompleteTrigger::Slash;
        state.autocomplete.query = query.to_string();
        state.autocomplete.candidates = filtered;
        state.autocomplete.selected = 0;
    } else if let Some(at_pos) = input.rfind('@') {
        // @file autocomplete
        let query = &input[at_pos + 1..];
        let candidates = autocomplete::file_candidates(&state.project_dir, query);
        state.autocomplete.active = !candidates.is_empty();
        state.autocomplete.trigger = AutocompleteTrigger::AtFile;
        state.autocomplete.query = query.to_string();
        state.autocomplete.candidates = candidates;
        state.autocomplete.selected = 0;
    } else {
        state.autocomplete.active = false;
    }
}

/// Simple base64 encoding for OSC52 clipboard
fn base64_encode(input: &str) -> String {
    use std::io::Write;
    let mut buf = Vec::new();
    {
        let mut encoder = Base64Encoder::new(&mut buf);
        encoder.write_all(input.as_bytes()).ok();
    }
    String::from_utf8(buf).unwrap_or_default()
}

struct Base64Encoder<'a> {
    out: &'a mut Vec<u8>,
    buf: [u8; 3],
    len: usize,
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

impl<'a> Base64Encoder<'a> {
    fn new(out: &'a mut Vec<u8>) -> Self {
        Self { out, buf: [0; 3], len: 0 }
    }

    fn flush_block(&mut self) {
        let b = &self.buf;
        self.out.push(B64[(b[0] >> 2) as usize]);
        self.out.push(B64[((b[0] & 0x03) << 4 | b[1] >> 4) as usize]);
        if self.len > 1 {
            self.out.push(B64[((b[1] & 0x0f) << 2 | b[2] >> 6) as usize]);
        } else {
            self.out.push(b'=');
        }
        if self.len > 2 {
            self.out.push(B64[(b[2] & 0x3f) as usize]);
        } else {
            self.out.push(b'=');
        }
        self.buf = [0; 3];
        self.len = 0;
    }
}

impl std::io::Write for Base64Encoder<'_> {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        for &byte in data {
            self.buf[self.len] = byte;
            self.len += 1;
            if self.len == 3 {
                self.flush_block();
            }
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if self.len > 0 {
            self.flush_block();
        }
        Ok(())
    }
}

impl Drop for Base64Encoder<'_> {
    fn drop(&mut self) {
        let _ = std::io::Write::flush(self);
    }
}

fn run_search(state: &mut TuiState) {
    state.search_results.clear();
    state.search_current = 0;
    if state.search_query.is_empty() {
        return;
    }
    let query_lower = state.search_query.to_lowercase();
    for (i, entry) in state.transcript.iter().enumerate() {
        let text = match entry {
            TranscriptEntry::User(t) | TranscriptEntry::Assistant(t) | TranscriptEntry::SystemMessage(t) => t,
            TranscriptEntry::ToolCard(card) => &card.tool_name,
        };
        if text.to_lowercase().contains(&query_lower) {
            state.search_results.push(i);
        }
    }
}

fn handle_domain_event(state: &mut TuiState, event: DomainEvent) {
    match event.event_type {
        EventType::ContentDelta => {
            if let Some(text) = event.payload.get("text").and_then(|v| v.as_str()) {
                if state.streaming_assistant {
                    // Append to last assistant entry
                    if let Some(TranscriptEntry::Assistant(s)) = state.transcript.last_mut() {
                        s.push_str(text);
                    }
                } else {
                    // Start new assistant entry
                    state.transcript.push(TranscriptEntry::Assistant(text.to_string()));
                    state.streaming_assistant = true;
                }
                state.cursor_visible = true;
            }
        }
        EventType::ReasoningDelta => {
            // Reasoning is dimmed — we append to assistant message with marker
            if let Some(text) = event.payload.get("text").and_then(|v| v.as_str()) {
                if !state.streaming_assistant {
                    state.transcript.push(TranscriptEntry::Assistant(String::new()));
                    state.streaming_assistant = true;
                }
                // Reasoning is handled at render time via prefix
                if let Some(TranscriptEntry::Assistant(s)) = state.transcript.last_mut() {
                    s.push_str(text);
                }
            }
        }
        EventType::ToolCallQueued => {
            state.streaming_assistant = false;
            let call_id = event.entity_id.clone();
            let tool_name = event.payload.get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string();

            let card = ToolCardState {
                call_id: call_id.clone(),
                tool_name,
                status: ToolCardStatus::Running,
                started_at: Instant::now(),
                duration_ms: None,
                stdout_lines: Vec::new(),
            };

            let idx = state.transcript.len();
            state.transcript.push(TranscriptEntry::ToolCard(card));
            state.active_tool_cards.insert(call_id, idx);
            state.status.tools_running += 1;
        }
        EventType::ToolCallStdoutDelta => {
            if let Some(line) = event.payload.get("line").and_then(|v| v.as_str()) {
                if let Some(&idx) = state.active_tool_cards.get(&event.entity_id) {
                    if let Some(TranscriptEntry::ToolCard(card)) = state.transcript.get_mut(idx) {
                        card.stdout_lines.push(line.to_string());
                        // Keep only last 5 lines visible
                        if card.stdout_lines.len() > 5 {
                            card.stdout_lines.drain(..card.stdout_lines.len() - 5);
                        }
                    }
                }
            }
        }
        EventType::ToolCallCompleted => {
            let success = event.payload.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
            let duration_ms = event.payload.get("duration_ms").and_then(|v| v.as_u64());

            if let Some(&idx) = state.active_tool_cards.get(&event.entity_id) {
                if let Some(TranscriptEntry::ToolCard(card)) = state.transcript.get_mut(idx) {
                    card.status = if success { ToolCardStatus::Succeeded } else { ToolCardStatus::Failed };
                    card.duration_ms = duration_ms;
                }
            }
            // Track in tool chain for timeline
            let tool_name = event.payload.get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string();
            state.tool_chain.push(ToolChainEntry {
                tool_name,
                reason: String::new(), // TODO: extract from LLM reasoning
                status: if success { ToolCardStatus::Succeeded } else { ToolCardStatus::Failed },
                duration_ms,
            });

            state.active_tool_cards.remove(&event.entity_id);
            state.status.tools_running = state.status.tools_running.saturating_sub(1);
        }
        EventType::RunStateChanged => {
            if let Some(to) = event.payload.get("to").and_then(|v| v.as_str()) {
                state.status.phase = to.to_string();
            }
            if let Some(max) = event.payload.get("max_iterations").and_then(|v| v.as_u64()) {
                state.status.max_iterations = max as usize;
            }
        }
        EventType::LlmCallStart => {
            if let Some(iter) = event.payload.get("iteration").and_then(|v| v.as_u64()) {
                state.status.iteration = iter as usize;
            }
        }
        EventType::LlmCallEnd => {
            if let Some(t_in) = event.payload.get("tokens_in").and_then(|v| v.as_u64()) {
                state.status.tokens_in += t_in;
            }
            if let Some(t_out) = event.payload.get("tokens_out").and_then(|v| v.as_u64()) {
                state.status.tokens_out += t_out;
            }
            state.budget_tokens_used = state.status.tokens_in + state.status.tokens_out;
            state.agent_running = false;
            state.streaming_assistant = false;
        }
        EventType::BudgetExceeded => {
            let msg = event.payload.get("violation")
                .and_then(|v| v.as_str())
                .unwrap_or("budget exceeded");
            state.transcript.push(TranscriptEntry::SystemMessage(
                format!("⚠ {msg}"),
            ));
            state.toasts.push(Toast {
                message: format!("⚠ {msg}"),
                level: ToastLevel::Warning,
                created: Instant::now(),
            });
        }
        EventType::Error => {
            if event.payload.get("type").and_then(|v| v.as_str()) == Some("retry") {
                return; // Don't show retry errors as toasts
            }
            let msg = event.payload.get("error")
                .or(event.payload.get("reason"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            state.transcript.push(TranscriptEntry::SystemMessage(
                format!("❌ {msg}"),
            ));
            state.toasts.push(Toast {
                message: msg.to_string(),
                level: ToastLevel::Error,
                created: Instant::now(),
            });
        }
        EventType::GovernanceDecisionPending => {
            let decision_id = event.payload.get("decision_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let tool_name = event.payload.get("tool_name").and_then(|v| v.as_str()).unwrap_or("?").to_string();
            let risk_level = event.payload.get("risk_level").and_then(|v| v.as_str()).unwrap_or("Medium").to_string();
            let args_preview = event.payload.get("args_preview").and_then(|v| v.as_str()).unwrap_or("").to_string();
            state.pending_approval = Some(PendingApproval {
                decision_id,
                tool_name,
                risk_level,
                args_preview,
            });
        }
        EventType::GovernanceDecisionResolved => {
            state.pending_approval = None;
        }
        EventType::TodoUpdated => {
            let action = event.payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let content = event.payload.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let id = event.payload.get("id").and_then(|v| v.as_str())
                .or_else(|| event.payload.get("id").and_then(|v| v.as_u64()).map(|_| ""))
                .unwrap_or(&event.entity_id);
            let status = event.payload.get("status").and_then(|v| v.as_str()).unwrap_or("pending");

            match action {
                "task_create" => {
                    state.todos.push(TodoItem {
                        id: id.to_string(),
                        content: content.to_string(),
                        status: "pending".to_string(),
                    });
                }
                "task_update" => {
                    if let Some(todo) = state.todos.iter_mut().find(|t| t.id == id) {
                        todo.status = status.to_string();
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use theo_domain::event::DomainEvent;

    fn make_event(event_type: EventType, entity: &str, payload: serde_json::Value) -> DomainEvent {
        DomainEvent::new(event_type, entity, payload)
    }

    fn new_state() -> TuiState {
        TuiState::new("test".into(), "gpt-4o".into(), 40, 80, 24)
    }

    #[test]
    fn update_quit_sets_should_quit() {
        let mut state = new_state();
        update(&mut state, Msg::Quit);
        assert!(state.should_quit);
    }

    #[test]
    fn update_resize_updates_dimensions() {
        let mut state = new_state();
        update(&mut state, Msg::Resize(200, 50));
        assert_eq!(state.size, (200, 50));
    }

    #[test]
    fn update_content_delta_appends_assistant_message() {
        let mut state = new_state();
        let event = make_event(EventType::ContentDelta, "r-1", serde_json::json!({"text": "hello"}));
        update(&mut state, Msg::DomainEvent(event));

        assert_eq!(state.transcript.len(), 1);
        match &state.transcript[0] {
            TranscriptEntry::Assistant(text) => assert_eq!(text, "hello"),
            _ => panic!("expected Assistant"),
        }
    }

    #[test]
    fn update_content_delta_streaming_appends() {
        let mut state = new_state();
        let e1 = make_event(EventType::ContentDelta, "r-1", serde_json::json!({"text": "hel"}));
        let e2 = make_event(EventType::ContentDelta, "r-1", serde_json::json!({"text": "lo"}));
        update(&mut state, Msg::DomainEvent(e1));
        update(&mut state, Msg::DomainEvent(e2));

        assert_eq!(state.transcript.len(), 1);
        match &state.transcript[0] {
            TranscriptEntry::Assistant(text) => assert_eq!(text, "hello"),
            _ => panic!("expected Assistant"),
        }
    }

    #[test]
    fn update_tool_queued_creates_running_card() {
        let mut state = new_state();
        let event = make_event(EventType::ToolCallQueued, "c-1", serde_json::json!({"tool_name": "bash"}));
        update(&mut state, Msg::DomainEvent(event));

        assert_eq!(state.transcript.len(), 1);
        match &state.transcript[0] {
            TranscriptEntry::ToolCard(card) => {
                assert_eq!(card.tool_name, "bash");
                assert_eq!(card.status, ToolCardStatus::Running);
            }
            _ => panic!("expected ToolCard"),
        }
        assert_eq!(state.status.tools_running, 1);
    }

    #[test]
    fn update_tool_stdout_delta_appends_line() {
        let mut state = new_state();
        // Create card first
        let queued = make_event(EventType::ToolCallQueued, "c-1", serde_json::json!({"tool_name": "bash"}));
        update(&mut state, Msg::DomainEvent(queued));
        // Send stdout
        let delta = make_event(EventType::ToolCallStdoutDelta, "c-1", serde_json::json!({"line": "Compiling..."}));
        update(&mut state, Msg::DomainEvent(delta));

        match &state.transcript[0] {
            TranscriptEntry::ToolCard(card) => {
                assert_eq!(card.stdout_lines, vec!["Compiling..."]);
            }
            _ => panic!("expected ToolCard"),
        }
    }

    #[test]
    fn update_tool_completed_sets_status() {
        let mut state = new_state();
        let queued = make_event(EventType::ToolCallQueued, "c-1", serde_json::json!({"tool_name": "bash"}));
        update(&mut state, Msg::DomainEvent(queued));

        let completed = make_event(EventType::ToolCallCompleted, "c-1", serde_json::json!({
            "success": true, "duration_ms": 3200
        }));
        update(&mut state, Msg::DomainEvent(completed));

        match &state.transcript[0] {
            TranscriptEntry::ToolCard(card) => {
                assert_eq!(card.status, ToolCardStatus::Succeeded);
                assert_eq!(card.duration_ms, Some(3200));
            }
            _ => panic!("expected ToolCard"),
        }
        assert_eq!(state.status.tools_running, 0);
    }

    #[test]
    fn update_cursor_blink_toggles() {
        let mut state = new_state();
        let initial = state.cursor_visible;
        update(&mut state, Msg::CursorBlink);
        assert_ne!(state.cursor_visible, initial);
        update(&mut state, Msg::CursorBlink);
        assert_eq!(state.cursor_visible, initial);
    }

    #[test]
    fn update_events_lost_increments() {
        let mut state = new_state();
        update(&mut state, Msg::EventsLost(5));
        assert_eq!(state.events_lost, 5);
        assert_eq!(state.transcript.len(), 1);
    }

    #[test]
    fn update_submit_adds_user_message() {
        let mut state = new_state();
        state.input_text = "fix the bug".to_string();
        state.input_cursor = 11;
        update(&mut state, Msg::Submit("fix the bug".to_string()));
        assert!(state.input_text.is_empty());
        assert_eq!(state.input_cursor, 0);
        match &state.transcript[0] {
            TranscriptEntry::User(text) => assert_eq!(text, "fix the bug"),
            _ => panic!("expected User"),
        }
        assert!(state.agent_running);
    }

    #[test]
    fn update_llm_call_end_accumulates_tokens() {
        let mut state = new_state();
        let e1 = make_event(EventType::LlmCallEnd, "r-1", serde_json::json!({
            "iteration": 1, "tokens_in": 100, "tokens_out": 50, "duration_ms": 500
        }));
        update(&mut state, Msg::DomainEvent(e1));
        assert_eq!(state.status.tokens_in, 100);
        assert_eq!(state.status.tokens_out, 50);

        let e2 = make_event(EventType::LlmCallEnd, "r-1", serde_json::json!({
            "iteration": 2, "tokens_in": 200, "tokens_out": 100, "duration_ms": 300
        }));
        update(&mut state, Msg::DomainEvent(e2));
        assert_eq!(state.status.tokens_in, 300);
        assert_eq!(state.status.tokens_out, 150);
    }
}
