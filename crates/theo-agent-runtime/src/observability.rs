use std::io::Write;
use std::sync::Mutex;

use theo_domain::event::DomainEvent;

use crate::event_bus::EventListener;

/// Event listener that writes structured JSON lines to a writer.
///
/// Each DomainEvent is serialized as a single JSON line for log aggregation.
pub struct StructuredLogListener {
    writer: Mutex<Box<dyn Write + Send>>,
}

impl StructuredLogListener {
    pub fn new(writer: Box<dyn Write + Send>) -> Self {
        Self {
            writer: Mutex::new(writer),
        }
    }

    /// Creates a listener that writes to stdout.
    pub fn stdout() -> Self {
        Self::new(Box::new(std::io::stdout()))
    }

    /// Creates a listener that writes to a file.
    pub fn file(path: &std::path::Path) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        Ok(Self::new(Box::new(file)))
    }
}

impl EventListener for StructuredLogListener {
    fn on_event(&self, event: &DomainEvent) {
        if let Ok(json) = serde_json::to_string(event) {
            if let Ok(mut writer) = self.writer.lock() {
                let _ = writeln!(writer, "{}", json);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use theo_domain::event::{ALL_EVENT_TYPES, EventType};

    fn make_event(event_type: EventType) -> DomainEvent {
        DomainEvent::new(event_type, "test-entity", serde_json::Value::Null)
    }

    #[test]
    fn writes_valid_json_line() {
        let buffer = Arc::new(Mutex::new(Vec::<u8>::new()));
        let writer = {
            let b = buffer.clone();
            struct VecWriter(Arc<Mutex<Vec<u8>>>);
            impl Write for VecWriter {
                fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                    self.0.lock().unwrap().extend_from_slice(buf);
                    Ok(buf.len())
                }
                fn flush(&mut self) -> std::io::Result<()> {
                    Ok(())
                }
            }
            VecWriter(b)
        };

        let listener = StructuredLogListener::new(Box::new(writer));
        listener.on_event(&make_event(EventType::TaskCreated));

        let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        let lines: Vec<&str> = output.trim().split('\n').collect();
        assert_eq!(lines.len(), 1);

        // Verify it's valid JSON
        let parsed: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parsed["event_type"], "TaskCreated");
        assert_eq!(parsed["entity_id"], "test-entity");
    }

    #[test]
    fn handles_all_event_types_without_panic() {
        let listener = StructuredLogListener::new(Box::new(std::io::sink()));
        for et in &ALL_EVENT_TYPES {
            listener.on_event(&make_event(*et)); // must not panic
        }
    }

    #[test]
    fn multiple_events_write_multiple_lines() {
        let buffer = Arc::new(Mutex::new(Vec::<u8>::new()));
        let writer = {
            let b = buffer.clone();
            struct VecWriter(Arc<Mutex<Vec<u8>>>);
            impl Write for VecWriter {
                fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                    self.0.lock().unwrap().extend_from_slice(buf);
                    Ok(buf.len())
                }
                fn flush(&mut self) -> std::io::Result<()> {
                    Ok(())
                }
            }
            VecWriter(b)
        };

        let listener = StructuredLogListener::new(Box::new(writer));
        listener.on_event(&make_event(EventType::TaskCreated));
        listener.on_event(&make_event(EventType::RunStateChanged));
        listener.on_event(&make_event(EventType::Error));

        let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        let lines: Vec<&str> = output.trim().split('\n').collect();
        assert_eq!(lines.len(), 3);

        // Each line is valid JSON
        for line in &lines {
            let _: serde_json::Value = serde_json::from_str(line).unwrap();
        }
    }
}
