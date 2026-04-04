#![allow(deprecated)]

use tauri::{AppHandle, Emitter};
use theo_agent_runtime::events::{AgentEvent, EventSink};
pub use theo_api_contracts::events::FrontendEvent;

/// EventSink that emits to the Tauri frontend.
pub struct TauriEventSink {
    app: AppHandle,
}

impl TauriEventSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl EventSink for TauriEventSink {
    fn emit(&self, event: AgentEvent) {
        let fe = match event {
            AgentEvent::Token(text) => FrontendEvent::Token { text },
            AgentEvent::ToolStart { name, args } => FrontendEvent::ToolStart { name, args },
            AgentEvent::ToolEnd { name, success, output } => {
                let output = if output.len() > 5000 {
                    format!("{}...\n[truncated]", &output[..5000])
                } else {
                    output
                };
                FrontendEvent::ToolEnd { name, success, output }
            }
            AgentEvent::PhaseChange { from, to } => FrontendEvent::PhaseChange {
                from: from.to_string(),
                to: to.to_string(),
            },
            AgentEvent::Done { success, summary } => FrontendEvent::Done { success, summary },
            AgentEvent::Error(message) => FrontendEvent::Error { message },
            AgentEvent::LlmCallStart { iteration } => FrontendEvent::LlmCallStart { iteration },
            AgentEvent::LlmCallEnd { iteration } => FrontendEvent::LlmCallEnd { iteration },
            AgentEvent::ContextLoop { message, .. } => FrontendEvent::Error { message },
        };

        let _ = self.app.emit("agent-event", &fe);
    }
}
