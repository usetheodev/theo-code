use tauri::{AppHandle, Emitter};
// T1.3: facade re-export.
use theo_application::facade::agent::EventListener;
pub use theo_api_contracts::events::FrontendEvent;
use theo_domain::event::{DomainEvent, EventType};

/// Event listener that emits runtime domain events to the Tauri frontend.
pub struct TauriEventListener {
    app: AppHandle,
}

impl TauriEventListener {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl EventListener for TauriEventListener {
    fn on_event(&self, event: &DomainEvent) {
        let frontend_event = match event.event_type {
            EventType::ContentDelta => {
                event
                    .payload
                    .get("text")
                    .and_then(|v| v.as_str())
                    .map(|text| FrontendEvent::Token {
                        text: text.to_string(),
                    })
            }
            EventType::ToolCallQueued => Some(FrontendEvent::ToolStart {
                name: event
                    .payload
                    .get("tool_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
                    .to_string(),
                args: event
                    .payload
                    .get("input")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            }),
            EventType::ToolCallCompleted => {
                let output = event
                    .payload
                    .get("output_preview")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let output = if output.len() > 5000 {
                    format!("{}...\n[truncated]", &output[..5000])
                } else {
                    output
                };
                Some(FrontendEvent::ToolEnd {
                    name: event
                        .payload
                        .get("tool_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?")
                        .to_string(),
                    success: event
                        .payload
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    output,
                })
            }
            EventType::RunStateChanged => Some(FrontendEvent::PhaseChange {
                from: event
                    .payload
                    .get("from")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
                    .to_string(),
                to: event
                    .payload
                    .get("to")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
                    .to_string(),
            }),
            EventType::LlmCallStart => Some(FrontendEvent::LlmCallStart {
                iteration: event
                    .payload
                    .get("iteration")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize,
            }),
            EventType::LlmCallEnd => Some(FrontendEvent::LlmCallEnd {
                iteration: event
                    .payload
                    .get("iteration")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize,
            }),
            EventType::Error | EventType::BudgetExceeded => Some(FrontendEvent::Error {
                message: event
                    .payload
                    .get("error")
                    .or(event.payload.get("reason"))
                    .or(event.payload.get("violation"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error")
                    .to_string(),
            }),
            _ => None,
        };

        if let Some(frontend_event) = frontend_event {
            let _ = self.app.emit("agent-event", &frontend_event);
        }
    }
}
