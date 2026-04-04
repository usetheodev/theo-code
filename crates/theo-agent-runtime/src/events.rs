//! Deprecated event system — kept for backward compatibility.
//! Use `theo_domain::event::DomainEvent` + `event_bus::EventListener` instead.
#![allow(deprecated)]

use crate::state::Phase;

/// Events emitted by the agent loop for observability.
///
/// Deprecated: Use `theo_domain::event::DomainEvent` + `event_bus::EventListener` instead.
/// This type will be removed when agent_loop.rs is migrated to use EventBus.
#[deprecated(since = "0.2.0", note = "Use theo_domain::event::DomainEvent + event_bus::EventListener")]
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// A token of streamed text from the LLM.
    Token(String),
    /// LLM call started.
    LlmCallStart { iteration: usize },
    /// LLM call completed.
    LlmCallEnd { iteration: usize },
    /// Tool execution started.
    ToolStart { name: String, args: serde_json::Value },
    /// Tool execution completed.
    ToolEnd { name: String, success: bool, output: String },
    /// Phase changed.
    PhaseChange { from: Phase, to: Phase },
    /// Context loop diagnostic injected.
    ContextLoop { iteration: usize, message: String },
    /// Agent finished.
    Done { success: bool, summary: String },
    /// Error occurred.
    Error(String),
}

/// Trait for receiving agent events.
/// Implementations can log, display, or forward events to a UI.
///
/// Deprecated: Use `event_bus::EventListener` instead.
#[deprecated(since = "0.2.0", note = "Use event_bus::EventListener")]
pub trait EventSink: Send + Sync {
    fn emit(&self, event: AgentEvent);
}

/// Simple event sink that prints to stdout.
pub struct PrintEventSink;

impl EventSink for PrintEventSink {
    fn emit(&self, event: AgentEvent) {
        match &event {
            AgentEvent::Token(text) => print!("{text}"),
            AgentEvent::LlmCallStart { iteration } => {
                eprintln!("\n── LLM call (iteration {iteration}) ──");
            }
            AgentEvent::LlmCallEnd { iteration } => {
                eprintln!("── LLM call {iteration} done ──");
            }
            AgentEvent::ToolStart { name, args: _ } => {
                eprintln!("\n🔧 {name}");
            }
            AgentEvent::ToolEnd { name, success, output } => {
                let status = if *success { "✓" } else { "✗" };
                let preview = if output.len() > 200 {
                    format!("{}...", &output[..200])
                } else {
                    output.clone()
                };
                eprintln!("  {status} {name}: {preview}");
            }
            AgentEvent::PhaseChange { from, to } => {
                eprintln!("📋 Phase: {from} → {to}");
            }
            AgentEvent::ContextLoop { iteration, message } => {
                eprintln!("\n{message}");
                let _ = iteration;
            }
            AgentEvent::Done { success, summary } => {
                let status = if *success { "SUCCESS" } else { "FAILED" };
                eprintln!("\n══ {status} ══\n{summary}");
            }
            AgentEvent::Error(msg) => {
                eprintln!("\n❌ Error: {msg}");
            }
        }
    }
}

/// No-op event sink for testing.
pub struct NullEventSink;

impl EventSink for NullEventSink {
    fn emit(&self, _event: AgentEvent) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_null_sink_accepts_events() {
        let sink = NullEventSink;
        sink.emit(AgentEvent::Token("hello".to_string()));
        sink.emit(AgentEvent::Done {
            success: true,
            summary: "done".to_string(),
        });
    }
}
