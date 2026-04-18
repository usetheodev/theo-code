use std::sync::Arc;

use theo_domain::capability::{CapabilityDenied, CapabilitySet};
use theo_domain::event::{DomainEvent, EventType};
use theo_domain::tool::ToolCategory;

use crate::event_bus::EventBus;

/// Gate that enforces capability restrictions on tool usage and path access.
///
/// Wraps a CapabilitySet and publishes denial events via EventBus.
pub struct CapabilityGate {
    capabilities: CapabilitySet,
    event_bus: Arc<EventBus>,
}

impl CapabilityGate {
    pub fn new(capabilities: CapabilitySet, event_bus: Arc<EventBus>) -> Self {
        Self {
            capabilities,
            event_bus,
        }
    }

    /// Checks if a tool is allowed by the capability set.
    ///
    /// Returns Ok(()) if allowed, Err(CapabilityDenied) if denied.
    /// Publishes an Error event on denial.
    pub fn check_tool(
        &self,
        tool_name: &str,
        tool_category: ToolCategory,
    ) -> Result<(), CapabilityDenied> {
        if self.capabilities.can_use_tool(tool_name, tool_category) {
            Ok(())
        } else {
            let denied = CapabilityDenied {
                tool_name: tool_name.to_string(),
                reason: format!(
                    "tool '{}' (category {:?}) not allowed by capability set",
                    tool_name, tool_category
                ),
            };

            self.event_bus.publish(DomainEvent::new(
                EventType::Error,
                tool_name,
                serde_json::json!({
                    "type": "capability_denied",
                    "tool_name": tool_name,
                    "category": format!("{:?}", tool_category),
                    "reason": &denied.reason,
                }),
            ));

            Err(denied)
        }
    }

    /// Checks if writing to a path is allowed.
    ///
    /// Returns Ok(()) if allowed, Err(CapabilityDenied) if denied.
    pub fn check_path_write(&self, path: &str) -> Result<(), CapabilityDenied> {
        if self.capabilities.can_write_path(path) {
            Ok(())
        } else {
            let denied = CapabilityDenied {
                tool_name: "write".to_string(),
                reason: format!("path '{}' not in allowed paths", path),
            };

            self.event_bus.publish(DomainEvent::new(
                EventType::Error,
                "capability_gate",
                serde_json::json!({
                    "type": "capability_denied",
                    "path": path,
                    "reason": &denied.reason,
                }),
            ));

            Err(denied)
        }
    }

    /// Returns a reference to the underlying capability set.
    pub fn capabilities(&self) -> &CapabilitySet {
        &self.capabilities
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::CapturingListener;

    fn setup(caps: CapabilitySet) -> (CapabilityGate, Arc<CapturingListener>) {
        let bus = Arc::new(EventBus::new());
        let listener = Arc::new(CapturingListener::new());
        bus.subscribe(listener.clone());
        let gate = CapabilityGate::new(caps, bus);
        (gate, listener)
    }

    #[test]
    fn check_tool_passes_for_unrestricted() {
        let (gate, _) = setup(CapabilitySet::unrestricted());
        assert!(gate.check_tool("bash", ToolCategory::Execution).is_ok());
        assert!(gate.check_tool("read", ToolCategory::FileOps).is_ok());
    }

    #[test]
    fn check_tool_denied_returns_error() {
        let (gate, _) = setup(CapabilitySet::read_only());
        let err = gate
            .check_tool("bash", ToolCategory::Execution)
            .unwrap_err();
        assert_eq!(err.tool_name, "bash");
        assert!(err.reason.contains("not allowed"));
    }

    #[test]
    fn check_path_write_passes_for_allowed() {
        let caps = CapabilitySet {
            allowed_paths: vec!["/home/user/".to_string()],
            ..CapabilitySet::unrestricted()
        };
        let (gate, _) = setup(caps);
        assert!(gate.check_path_write("/home/user/src/main.rs").is_ok());
    }

    #[test]
    fn check_path_write_denied_returns_error() {
        let caps = CapabilitySet {
            allowed_paths: vec!["/home/user/".to_string()],
            ..CapabilitySet::unrestricted()
        };
        let (gate, _) = setup(caps);
        let err = gate.check_path_write("/etc/passwd").unwrap_err();
        assert!(err.reason.contains("not in allowed paths"));
    }

    #[test]
    fn denied_tool_publishes_event() {
        let (gate, listener) = setup(CapabilitySet::read_only());
        let _ = gate.check_tool("bash", ToolCategory::Execution);

        let events = listener.captured();
        let denied_events: Vec<_> = events
            .iter()
            .filter(|e| {
                e.event_type == EventType::Error
                    && e.payload.get("type").and_then(|v| v.as_str()) == Some("capability_denied")
            })
            .collect();
        assert_eq!(denied_events.len(), 1);
        assert_eq!(denied_events[0].payload["tool_name"], "bash");
    }

    #[test]
    fn denied_path_publishes_event() {
        let caps = CapabilitySet {
            allowed_paths: vec!["/safe/".to_string()],
            ..CapabilitySet::unrestricted()
        };
        let (gate, listener) = setup(caps);
        let _ = gate.check_path_write("/dangerous/file.txt");

        let events = listener.captured();
        let denied_events: Vec<_> = events
            .iter()
            .filter(|e| e.payload.get("type").and_then(|v| v.as_str()) == Some("capability_denied"))
            .collect();
        assert_eq!(denied_events.len(), 1);
        assert_eq!(denied_events[0].payload["path"], "/dangerous/file.txt");
    }
}
