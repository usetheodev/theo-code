//! Single-purpose slice extracted from `tui/app.rs` (T5.4 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(dead_code, unused_imports)]

use std::collections::HashMap;
use std::time::Instant;

use theo_domain::event::{DomainEvent, EventType};

use super::*;
use super::state_types::*;

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
            prompt_history: Vec::new(),
            copy_mode: false,
            show_sidebar: width > 120,
            sidebar_tab: super::super::widgets::sidebar::SidebarTab::Status,
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
            theme: super::super::theme::Theme::dark(),
            show_theme_picker: false,
            theme_picker_selected: 0,
            autocomplete: super::super::autocomplete::AutocompleteState::new(),
            project_dir: std::path::PathBuf::new(),
            tabs: vec![TabState { name: "Session 1".to_string(), transcript_snapshot: Vec::new() }],
            active_tab: 0,
            budget_tokens_used: 0,
            budget_tokens_limit: 200_000,
            todos: Vec::new(),
            tool_chain: Vec::new(),
            show_timeline: false,
            pending_approval: None,
            partial_progress: Vec::new(),
        }
    }
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
    Notify(String),
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
    LoginStart(String), // "device" for device flow
    LoginWithKey(String), // direct API key
    LoginServer(String), // server URL for device flow
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
    ToggleCopyMode,
    CopyLastResponse,
    CopyLastCodeBlock,
    /// T14.1 — Partial-progress status line update. Carries the
    /// rendered lines (one per active tool, alphabetically sorted)
    /// produced by `partial_progress::run_drainer` after each 50 ms
    /// debounce window. Empty Vec → clear the status line.
    PartialProgressUpdate(Vec<String>),
}

// ---------------------------------------------------------------------------
// Update — pure function, no IO
