//! TUI state and update logic — pure Elm/Redux pattern.
//!
//! `TuiState` holds all UI state. `Msg` represents all possible state transitions.
//! `update()` is a pure function: (state, msg) → mutated state, no IO.

#![allow(dead_code, unused_imports)] // Scaffolded helpers — kept for upcoming TUI features.
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
    pub prompt_history: Vec<String>,
    pub copy_mode: bool, // when true, mouse capture is disabled for native selection
    pub show_sidebar: bool,
    pub sidebar_tab: super::super::widgets::sidebar::SidebarTab,
    pub show_model_picker: bool,
    pub available_models: Vec<String>,
    pub model_picker_selected: usize,
    pub theme: super::super::theme::Theme,
    pub show_theme_picker: bool,
    pub theme_picker_selected: usize,
    pub autocomplete: super::super::autocomplete::AutocompleteState,
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
    /// T14.1 — Latest debounced partial-progress lines (one per
    /// active tool, sorted alphabetically). Populated by the
    /// drainer task; rendered above / inside the status line.
    /// Empty Vec when no tool is emitting progress.
    pub partial_progress: Vec<String>,
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

