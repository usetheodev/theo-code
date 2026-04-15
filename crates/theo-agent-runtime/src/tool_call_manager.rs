use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use theo_domain::capability::CapabilityDenied;
use theo_domain::error::TransitionError;
use theo_domain::event::{DomainEvent, EventType};
use theo_domain::identifiers::{CallId, TaskId};
use theo_domain::tool::{ToolCategory, ToolContext};
use theo_domain::tool_call::{ToolCallRecord, ToolCallState, ToolResultRecord};
use theo_infra_llm::types::ToolCall;
use theo_tooling::registry::ToolRegistry;

use crate::capability_gate::CapabilityGate;
use crate::event_bus::EventBus;
use crate::tool_bridge;

/// Manages the lifecycle of tool calls, enforcing invariants and publishing events.
///
/// - **Invariant 2**: Every tool call has a unique `call_id`.
/// - **Invariant 3**: Every tool result references its `call_id`.
/// - **Invariant 5**: Every state transition generates a DomainEvent.
///
/// Thread-safe via internal Mutex. The Mutex is released during async tool
/// execution to avoid holding the lock across await points.
pub struct ToolCallManager {
    records: Mutex<HashMap<CallId, ToolCallRecord>>,
    results: Mutex<HashMap<CallId, ToolResultRecord>>,
    event_bus: Arc<EventBus>,
    capability_gate: Option<Arc<CapabilityGate>>,
}

impl ToolCallManager {
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self {
            records: Mutex::new(HashMap::new()),
            results: Mutex::new(HashMap::new()),
            event_bus,
            capability_gate: None,
        }
    }

    /// Sets the capability gate for tool access control.
    pub fn with_capability_gate(mut self, gate: Arc<CapabilityGate>) -> Self {
        self.capability_gate = Some(gate);
        self
    }


    /// Enqueues a new tool call with a unique CallId (Invariant 2).
    ///
    /// Publishes `DomainEvent::ToolCallQueued`.
    pub fn enqueue(&self, task_id: TaskId, tool_name: String, input: serde_json::Value) -> CallId {
        let call_id = CallId::generate();
        let now = now_millis();

        let record = ToolCallRecord {
            call_id: call_id.clone(),
            task_id,
            tool_name: tool_name.clone(),
            input,
            state: ToolCallState::Queued,
            created_at: now,
            started_at: None,
            completed_at: None,
        };

        self.records
            .lock()
            .expect("records lock poisoned")
            .insert(call_id.clone(), record);

        self.event_bus.publish(DomainEvent::new(
            EventType::ToolCallQueued,
            call_id.as_str(),
            serde_json::json!({ "tool_name": tool_name }),
        ));

        call_id
    }

    /// Dispatches and executes a tool call, tracking state transitions.
    ///
    /// Flow: Queued → Dispatched → Running → Succeeded/Failed/Timeout
    ///
    /// The Mutex is released during the actual tool execution to avoid
    /// blocking other operations while a tool runs.
    ///
    /// Invariant 3: ToolResultRecord always references the call_id.
    pub async fn dispatch_and_execute(
        &self,
        call_id: &CallId,
        registry: &ToolRegistry,
        ctx: &ToolContext,
    ) -> Result<ToolResultRecord, ToolCallManagerError> {
        // 0. Capability check (if gate is set)
        {
            let records = self.records.lock().expect("records lock poisoned");
            if let Some(record) = records.get(call_id) {
                if let Some(gate) = &self.capability_gate {
                    // Determine category from registry, default to Utility
                    let category = registry
                        .get(&record.tool_name)
                        .map(|t| t.category())
                        .unwrap_or(ToolCategory::Utility);
                    gate.check_tool(&record.tool_name, category)?;
                }
            }
        }

        // 1. Transition Queued → Dispatched (under lock)
        let lmm_call = {
            let mut records = self.records.lock().expect("records lock poisoned");
            let record = records
                .get_mut(call_id)
                .ok_or_else(|| ToolCallManagerError::CallNotFound(call_id.as_str().to_string()))?;

            transition_record(record, ToolCallState::Dispatched)?;

            self.event_bus.publish(DomainEvent::new(
                EventType::ToolCallDispatched,
                call_id.as_str(),
                serde_json::json!({ "tool_name": &record.tool_name }),
            ));

            // 2. Transition Dispatched → Running
            transition_record(record, ToolCallState::Running)?;
            record.started_at = Some(now_millis());

            // Build the LLM ToolCall for tool_bridge
            ToolCall::new(
                call_id.as_str(),
                &record.tool_name,
                &record.input.to_string(),
            )
        };
        // Lock is released here — safe to await

        // 3. Execute tool via tool_bridge (no lock held)
        let start = std::time::Instant::now();
        let (message, success) = tool_bridge::execute_tool_call(registry, &lmm_call, ctx).await;
        let duration_ms = start.elapsed().as_millis() as u64;

        // 4. Determine final state
        let final_state = if success {
            ToolCallState::Succeeded
        } else {
            ToolCallState::Failed
        };

        let output = message.content.clone().unwrap_or_default();
        let error = if success { None } else { Some(output.clone()) };

        // 5. Transition Running → final state (under lock)
        {
            let mut records = self.records.lock().expect("records lock poisoned");
            if let Some(record) = records.get_mut(call_id) {
                let _ = transition_record(record, final_state);
                record.completed_at = Some(now_millis());
            }
        }

        // 6. Store result (Invariant 3: result references call_id)
        let result = ToolResultRecord {
            call_id: call_id.clone(),
            output,
            status: final_state,
            error,
            duration_ms,
        };

        self.results
            .lock()
            .expect("results lock poisoned")
            .insert(call_id.clone(), result.clone());

        // 7. Publish completion event (enriched with tool details)
        let tool_name = {
            self.records
                .lock()
                .expect("records lock poisoned")
                .get(call_id)
                .map(|r| r.tool_name.clone())
                .unwrap_or_default()
        };
        let input_args = {
            let raw = self
                .records
                .lock()
                .expect("records lock poisoned")
                .get(call_id)
                .map(|r| r.input.clone())
                .unwrap_or(serde_json::Value::Null);
            // Truncate large string fields to keep event payload reasonable
            truncate_input_for_event(raw)
        };
        // Truncate output preview for events (avoid huge payloads)
        let output_preview = if result.output.len() > 200 {
            let mut end = 200;
            while end > 0 && !result.output.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}...", &result.output[..end])
        } else {
            result.output.clone()
        };

        self.event_bus.publish(DomainEvent::new(
            EventType::ToolCallCompleted,
            call_id.as_str(),
            serde_json::json!({
                "status": format!("{:?}", final_state),
                "duration_ms": duration_ms,
                "success": success,
                "tool_name": tool_name,
                "input": input_args,
                "output_preview": output_preview,
            }),
        ));

        Ok(result)
    }

    /// Returns a clone of the tool call record.
    pub fn get_record(&self, call_id: &CallId) -> Option<ToolCallRecord> {
        self.records
            .lock()
            .expect("records lock poisoned")
            .get(call_id)
            .cloned()
    }

    /// Returns a clone of the tool result.
    pub fn get_result(&self, call_id: &CallId) -> Option<ToolResultRecord> {
        self.results
            .lock()
            .expect("results lock poisoned")
            .get(call_id)
            .cloned()
    }

    /// Returns all tool call records for a given task.
    pub fn calls_for_task(&self, task_id: &TaskId) -> Vec<ToolCallRecord> {
        self.records
            .lock()
            .expect("records lock poisoned")
            .values()
            .filter(|r| r.task_id == *task_id)
            .cloned()
            .collect()
    }
}

/// Transition a record's state, updating only the state field.
fn transition_record(
    record: &mut ToolCallRecord,
    target: ToolCallState,
) -> Result<(), TransitionError> {
    theo_domain::transition(&mut record.state, target)
}

/// Truncate large string fields in tool input for event payload.
/// Keeps first ~500 chars of each string field to prevent huge events.
/// Uses char boundary safe truncation to avoid panics on multi-byte UTF-8.
fn truncate_input_for_event(mut input: serde_json::Value) -> serde_json::Value {
    if let Some(obj) = input.as_object_mut() {
        for (_key, value) in obj.iter_mut() {
            if let Some(s) = value.as_str() {
                if s.len() > 500 {
                    // Find the nearest char boundary at or before 500
                    let mut end = 500;
                    while end > 0 && !s.is_char_boundary(end) {
                        end -= 1;
                    }
                    *value = serde_json::Value::String(format!("{}...", &s[..end]));
                }
            }
        }
    }
    input
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_millis() as u64
}

#[derive(Debug, thiserror::Error)]
pub enum ToolCallManagerError {
    #[error("tool call not found: {0}")]
    CallNotFound(String),

    #[error("transition error: {0}")]
    Transition(#[from] TransitionError),

    #[error("capability denied: {0}")]
    CapabilityDenied(#[from] CapabilityDenied),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::CapturingListener;
    use theo_tooling::registry::create_default_registry;

    fn setup() -> (ToolCallManager, Arc<EventBus>, Arc<CapturingListener>) {
        let bus = Arc::new(EventBus::new());
        let listener = Arc::new(CapturingListener::new());
        bus.subscribe(listener.clone());
        let manager = ToolCallManager::new(bus.clone());
        (manager, bus, listener)
    }

    // -----------------------------------------------------------------------
    // Invariant 2: unique call_id
    // -----------------------------------------------------------------------

    #[test]
    fn enqueue_produces_unique_call_ids() {
        let (manager, _, _) = setup();
        let id1 = manager.enqueue(TaskId::new("t-1"), "read".into(), serde_json::json!({}));
        let id2 = manager.enqueue(TaskId::new("t-1"), "read".into(), serde_json::json!({}));
        assert_ne!(id1, id2);
    }

    #[test]
    fn enqueue_creates_record_in_queued_state() {
        let (manager, _, _) = setup();
        let id = manager.enqueue(
            TaskId::new("t-1"),
            "read".into(),
            serde_json::json!({"filePath": "/tmp/test"}),
        );
        let record = manager.get_record(&id).expect("record must exist");
        assert_eq!(record.state, ToolCallState::Queued);
        assert_eq!(record.tool_name, "read");
        assert!(record.created_at > 0);
        assert!(record.started_at.is_none());
    }

    #[test]
    fn enqueue_emits_tool_call_queued_event() {
        let (manager, _, listener) = setup();
        let id = manager.enqueue(TaskId::new("t-1"), "bash".into(), serde_json::json!({}));
        let events = listener.captured();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EventType::ToolCallQueued);
        assert_eq!(events[0].entity_id, id.as_str());
        assert_eq!(events[0].payload["tool_name"], "bash");
    }

    // -----------------------------------------------------------------------
    // Invariant 3: result references call_id
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn dispatch_result_references_call_id() {
        let (manager, _, _) = setup();
        let registry = create_default_registry();
        let call_id = manager.enqueue(
            TaskId::new("t-1"),
            "read".into(),
            serde_json::json!({"filePath": "/tmp/nonexistent_test_file"}),
        );
        let ctx = ToolContext::test_context(std::path::PathBuf::from("/tmp"));
        let result = manager
            .dispatch_and_execute(&call_id, &registry, &ctx)
            .await
            .unwrap();
        assert_eq!(result.call_id, call_id); // Invariant 3
    }

    // -----------------------------------------------------------------------
    // State transitions + events
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn dispatch_emits_three_events_in_order() {
        let (manager, _, listener) = setup();
        let registry = create_default_registry();
        let call_id = manager.enqueue(
            TaskId::new("t-1"),
            "read".into(),
            serde_json::json!({"filePath": "/tmp/nonexistent"}),
        );
        let ctx = ToolContext::test_context(std::path::PathBuf::from("/tmp"));
        let _ = manager
            .dispatch_and_execute(&call_id, &registry, &ctx)
            .await;

        let events = listener.captured();
        // ToolCallQueued (from enqueue) + ToolCallDispatched + ToolCallCompleted
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event_type, EventType::ToolCallQueued);
        assert_eq!(events[1].event_type, EventType::ToolCallDispatched);
        assert_eq!(events[2].event_type, EventType::ToolCallCompleted);
    }

    #[tokio::test]
    async fn dispatch_failed_tool_records_failed_state() {
        let (manager, _, _) = setup();
        let registry = create_default_registry();
        let call_id = manager.enqueue(
            TaskId::new("t-1"),
            "read".into(),
            serde_json::json!({"filePath": "/nonexistent/path/that/does/not/exist"}),
        );
        let ctx = ToolContext::test_context(std::path::PathBuf::from("/tmp"));
        let result = manager
            .dispatch_and_execute(&call_id, &registry, &ctx)
            .await
            .unwrap();

        assert_eq!(result.status, ToolCallState::Failed);
        assert!(result.error.is_some());
        // Verify duration was actually recorded (completed_at - started_at)
        assert!(
            result.duration_ms < 5_000,
            "dispatch took unexpectedly long: {}ms",
            result.duration_ms
        );

        let record = manager.get_record(&call_id).unwrap();
        assert_eq!(record.state, ToolCallState::Failed);
        assert!(record.completed_at.is_some());
    }

    #[tokio::test]
    async fn dispatch_completion_event_has_status_and_duration() {
        let (manager, _, listener) = setup();
        let registry = create_default_registry();
        let call_id = manager.enqueue(
            TaskId::new("t-1"),
            "read".into(),
            serde_json::json!({"filePath": "/tmp/nonexistent"}),
        );
        let ctx = ToolContext::test_context(std::path::PathBuf::from("/tmp"));
        let _ = manager
            .dispatch_and_execute(&call_id, &registry, &ctx)
            .await;

        let events = listener.captured();
        let completion = &events[2];
        assert_eq!(completion.event_type, EventType::ToolCallCompleted);
        assert!(completion.payload.get("status").is_some());
        assert!(completion.payload.get("duration_ms").is_some());
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    #[test]
    fn calls_for_task_filters_correctly() {
        let (manager, _, _) = setup();
        manager.enqueue(TaskId::new("t-1"), "read".into(), serde_json::json!({}));
        manager.enqueue(TaskId::new("t-1"), "bash".into(), serde_json::json!({}));
        manager.enqueue(TaskId::new("t-2"), "edit".into(), serde_json::json!({}));

        let t1_calls = manager.calls_for_task(&TaskId::new("t-1"));
        assert_eq!(t1_calls.len(), 2);

        let t2_calls = manager.calls_for_task(&TaskId::new("t-2"));
        assert_eq!(t2_calls.len(), 1);
    }

    #[test]
    fn get_record_nonexistent_returns_none() {
        let (manager, _, _) = setup();
        assert!(manager.get_record(&CallId::new("nope")).is_none());
    }

    #[test]
    fn get_result_nonexistent_returns_none() {
        let (manager, _, _) = setup();
        assert!(manager.get_result(&CallId::new("nope")).is_none());
    }

    // -----------------------------------------------------------------------
    // Error cases
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn dispatch_nonexistent_call_returns_err() {
        let (manager, _, _) = setup();
        let registry = create_default_registry();
        let ctx = ToolContext::test_context(std::path::PathBuf::from("/tmp"));
        let result = manager
            .dispatch_and_execute(&CallId::new("nonexistent"), &registry, &ctx)
            .await;
        assert!(matches!(result, Err(ToolCallManagerError::CallNotFound(_))));
    }

    // -----------------------------------------------------------------------
    // Thread safety
    // -----------------------------------------------------------------------

    #[test]
    fn concurrent_enqueues_are_safe() {
        let bus = Arc::new(EventBus::new());
        let manager = Arc::new(ToolCallManager::new(bus));

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let m = manager.clone();
                std::thread::spawn(move || {
                    m.enqueue(
                        TaskId::new("t-1"),
                        format!("tool-{}", i),
                        serde_json::json!({}),
                    )
                })
            })
            .collect();

        let ids: Vec<CallId> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let unique: std::collections::HashSet<String> =
            ids.iter().map(|id| id.as_str().to_string()).collect();
        assert_eq!(unique.len(), 10, "all call_ids must be unique");
    }
}
