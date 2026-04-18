use serde::Serialize;

/// Event payload sent to frontend surfaces (desktop, CLI, etc.).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum FrontendEvent {
    #[serde(rename = "token")]
    Token { text: String },
    #[serde(rename = "tool_start")]
    ToolStart {
        name: String,
        args: serde_json::Value,
    },
    #[serde(rename = "tool_end")]
    ToolEnd {
        name: String,
        success: bool,
        output: String,
    },
    #[serde(rename = "phase_change")]
    PhaseChange { from: String, to: String },
    #[serde(rename = "done")]
    Done { success: bool, summary: String },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "llm_call_start")]
    LlmCallStart { iteration: usize },
    #[serde(rename = "llm_call_end")]
    LlmCallEnd { iteration: usize },
}
