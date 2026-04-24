use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;
use theo_domain::event::DomainEvent;
use theo_domain::graph_context::EventSink;

/// Synchronous event listener trait.
///
/// Implementations receive domain events for logging, metrics, persistence, etc.
/// The trait is intentionally sync — async work should be done internally
/// via channels (e.g., mpsc::Sender), not by making the trait async.
pub trait EventListener: Send + Sync {
    fn on_event(&self, event: &DomainEvent);
}

/// Default maximum number of events kept in the in-memory log.
const DEFAULT_MAX_EVENTS: usize = 10_000;

/// Event bus that dispatches domain events to registered listeners
/// and maintains an in-memory event log.
///
/// The log is bounded by `max_events` to prevent unbounded memory growth.
/// When the limit is reached, oldest events are dropped (FIFO).
///
/// Listeners that panic during `on_event` are caught via `catch_unwind`
/// and logged — the bus continues dispatching to remaining listeners.
pub struct EventBus {
    listeners: Mutex<Vec<Arc<dyn EventListener>>>,
    // VecDeque gives O(1) pop_front when evicting oldest events under
    // `max_events` pressure. Vec::remove(0) was O(n) (T6.1).
    log: Mutex<VecDeque<DomainEvent>>,
    max_events: usize,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            listeners: Mutex::new(Vec::new()),
            log: Mutex::new(VecDeque::new()),
            max_events: DEFAULT_MAX_EVENTS,
        }
    }

    pub fn with_max_events(max_events: usize) -> Self {
        Self {
            listeners: Mutex::new(Vec::new()),
            log: Mutex::new(VecDeque::new()),
            max_events,
        }
    }

    pub fn subscribe(&self, listener: Arc<dyn EventListener>) {
        self.listeners
            .lock()
            .push(listener);
    }

    /// Publishes an event: appends to the log and notifies all listeners.
    ///
    /// - If no listeners are registered, the event is still logged.
    /// - If a listener panics, the panic is caught and the bus continues
    ///   with the remaining listeners.
    /// - If the log exceeds `max_events`, the oldest event is dropped.
    pub fn publish(&self, event: DomainEvent) {
        // Append to log (bounded)
        {
            let mut log = self.log.lock();
            if log.len() >= self.max_events {
                log.pop_front();
            }
            log.push_back(event.clone());
        }

        // Notify listeners (with panic protection)
        let listeners = self
            .listeners
            .lock()
            .clone();
        for listener in &listeners {
            let listener = Arc::clone(listener);
            let event_ref = &event;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                listener.on_event(event_ref);
            }));
            if let Err(_panic) = result {
                // Do not log the entity_id: it may be a session_id or
                // call_id that leaks PII into stderr-captured logs. Event
                // type alone is enough for operators to correlate with
                // the structured log stream.
                eprintln!(
                    "[EventBus] listener panicked on event {:?} (entity redacted)",
                    event.event_type
                );
            }
        }
    }

    /// Returns a snapshot of all events in the log, in insertion order.
    ///
    /// **Note (T6.2):** clones the entire log. Prefer [`events_range`]
    /// or [`events_since`] for large logs (e.g., `record_session_exit`
    /// on a long-running run).
    pub fn events(&self) -> Vec<DomainEvent> {
        self.log.lock().iter().cloned().collect()
    }

    /// Returns a paginated window of the log starting at `offset`,
    /// up to `limit` entries. Offset past the end returns an empty vec.
    pub fn events_range(&self, offset: usize, limit: usize) -> Vec<DomainEvent> {
        self.log
            .lock()
            .iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect()
    }

    /// Returns every event inserted AFTER `event_id`. The starting event
    /// is itself excluded. If `event_id` is unknown, the full log is
    /// returned (so initial subscribers still see history).
    ///
    /// Designed for progressive consumers (dashboard, OTel exporter) that
    /// poll the bus and want only the delta since their last poll.
    pub fn events_since(
        &self,
        event_id: &theo_domain::identifiers::EventId,
    ) -> Vec<DomainEvent> {
        let log = self.log.lock();
        // Find the index of `event_id` (if any) and take everything strictly after.
        match log.iter().position(|e| e.event_id == *event_id) {
            Some(idx) => log.iter().skip(idx + 1).cloned().collect(),
            None => log.iter().cloned().collect(),
        }
    }

    /// Returns events filtered by entity_id, in insertion order.
    pub fn events_for(&self, entity_id: &str) -> Vec<DomainEvent> {
        self.log
            .lock()
            .iter()
            .filter(|e| e.entity_id == entity_id)
            .cloned()
            .collect()
    }

    /// Returns the number of events in the log.
    pub fn len(&self) -> usize {
        self.log.lock().len()
    }

    /// Returns true if the log is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Subscribe with an async broadcast channel.
    ///
    /// Returns a `tokio::sync::broadcast::Receiver<DomainEvent>` that receives
    /// clones of all published events. Useful for async consumers like the TUI
    /// that cannot block in `on_event`.
    ///
    /// `capacity` controls the broadcast buffer size. If the receiver falls behind
    /// by more than `capacity` events, it will receive `RecvError::Lagged(n)`.
    /// Recommended: 1024 (15× estimated burst of ~400 events/s with 16ms batching).
    pub fn subscribe_broadcast(&self, capacity: usize) -> tokio::sync::broadcast::Receiver<DomainEvent> {
        let (tx, rx) = tokio::sync::broadcast::channel(capacity);
        self.subscribe(Arc::new(BroadcastListener { tx }));
        rx
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Adapter that lets components outside `theo-agent-runtime` (e.g.
/// `theo-application::GraphContextService`) publish through the bus
/// without depending on the concrete `EventBus` type.
///
/// Implements `theo_domain::graph_context::EventSink`, so the service
/// can accept it as `Arc<dyn EventSink>` and the runtime supplies it
/// via `.with_event_sink(Arc::new(EventBusSink::new(bus.clone())))`.
pub struct EventBusSink {
    bus: Arc<EventBus>,
}

impl EventBusSink {
    pub fn new(bus: Arc<EventBus>) -> Self {
        Self { bus }
    }
}

impl EventSink for EventBusSink {
    fn emit(&self, event: DomainEvent) {
        self.bus.publish(event);
    }
}

/// Bridge listener: forwards events from sync EventBus to async broadcast channel.
struct BroadcastListener {
    tx: tokio::sync::broadcast::Sender<DomainEvent>,
}

impl EventListener for BroadcastListener {
    fn on_event(&self, event: &DomainEvent) {
        // Ignore SendError: no receivers means nobody is listening (ok to drop)
        let _ = self.tx.send(event.clone());
    }
}

/// Event listener that prints events to stderr.
pub struct PrintEventListener;

impl EventListener for PrintEventListener {
    fn on_event(&self, event: &DomainEvent) {
        eprintln!(
            "[{}] {} entity={} payload={}",
            event.event_type, event.event_id, event.entity_id, event.payload,
        );
    }
}

/// No-op event listener for testing.
pub struct NullEventListener;

impl EventListener for NullEventListener {
    fn on_event(&self, _event: &DomainEvent) {}
}

/// Capturing event listener for testing — stores all received events.
#[cfg(test)]
pub struct CapturingListener {
    events: Mutex<Vec<DomainEvent>>,
}

#[cfg(test)]
impl Default for CapturingListener {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl CapturingListener {
    pub fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }

    pub fn captured(&self) -> Vec<DomainEvent> {
        self.events.lock().clone()
    }
}

#[cfg(test)]
impl EventListener for CapturingListener {
    fn on_event(&self, event: &DomainEvent) {
        self.events.lock().push(event.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theo_domain::event::{ALL_EVENT_TYPES, EventType};

    fn make_event(event_type: EventType, entity: &str) -> DomainEvent {
        DomainEvent::new(event_type, entity, serde_json::Value::Null)
    }

    // -----------------------------------------------------------------------
    // EventBus core
    // -----------------------------------------------------------------------

    #[test]
    fn publish_appends_to_log() {
        let bus = EventBus::new();
        bus.publish(make_event(EventType::TaskCreated, "t-1"));
        assert_eq!(bus.len(), 1);
    }

    #[test]
    fn publish_notifies_all_listeners() {
        let bus = EventBus::new();
        let l1 = Arc::new(CapturingListener::new());
        let l2 = Arc::new(CapturingListener::new());
        bus.subscribe(l1.clone());
        bus.subscribe(l2.clone());

        bus.publish(make_event(EventType::TaskCreated, "t-1"));

        assert_eq!(l1.captured().len(), 1);
        assert_eq!(l2.captured().len(), 1);
    }

    #[test]
    fn publish_with_zero_listeners_still_logs() {
        let bus = EventBus::new();
        bus.publish(make_event(EventType::Error, "err-1"));
        assert_eq!(bus.len(), 1);
        assert_eq!(bus.events()[0].event_type, EventType::Error);
    }

    #[test]
    fn events_returns_insertion_order() {
        let bus = EventBus::new();
        bus.publish(make_event(EventType::TaskCreated, "t-1"));
        bus.publish(make_event(EventType::TaskStateChanged, "t-1"));
        bus.publish(make_event(EventType::RunInitialized, "r-1"));

        let events = bus.events();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event_type, EventType::TaskCreated);
        assert_eq!(events[1].event_type, EventType::TaskStateChanged);
        assert_eq!(events[2].event_type, EventType::RunInitialized);
    }

    #[test]
    fn events_for_filters_by_entity() {
        let bus = EventBus::new();
        bus.publish(make_event(EventType::TaskCreated, "t-1"));
        bus.publish(make_event(EventType::TaskCreated, "t-2"));
        bus.publish(make_event(EventType::TaskStateChanged, "t-1"));

        let t1_events = bus.events_for("t-1");
        assert_eq!(t1_events.len(), 2);
        assert!(t1_events.iter().all(|e| e.entity_id == "t-1"));

        let t2_events = bus.events_for("t-2");
        assert_eq!(t2_events.len(), 1);
    }

    #[test]
    fn events_for_nonexistent_entity_returns_empty() {
        let bus = EventBus::new();
        bus.publish(make_event(EventType::TaskCreated, "t-1"));
        assert!(bus.events_for("no-such-entity").is_empty());
    }

    #[test]
    fn max_events_bound_drops_oldest() {
        let bus = EventBus::with_max_events(3);
        bus.publish(make_event(EventType::TaskCreated, "t-1"));
        bus.publish(make_event(EventType::TaskStateChanged, "t-2"));
        bus.publish(make_event(EventType::RunInitialized, "r-1"));
        bus.publish(make_event(EventType::LlmCallStart, "r-1"));

        assert_eq!(bus.len(), 3);
        let events = bus.events();
        // Oldest (TaskCreated) was dropped
        assert_eq!(events[0].event_type, EventType::TaskStateChanged);
        assert_eq!(events[1].event_type, EventType::RunInitialized);
        assert_eq!(events[2].event_type, EventType::LlmCallStart);
    }

    #[test]
    fn is_empty_and_len() {
        let bus = EventBus::new();
        assert!(bus.is_empty());
        assert_eq!(bus.len(), 0);
        bus.publish(make_event(EventType::Error, "x"));
        assert!(!bus.is_empty());
        assert_eq!(bus.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Listener panic handling
    // -----------------------------------------------------------------------

    struct PanickingListener;
    impl EventListener for PanickingListener {
        fn on_event(&self, _event: &DomainEvent) {
            panic!("listener exploded!");
        }
    }

    // T6.2 — events_range pagination
    #[test]
    fn events_range_returns_window_within_log() {
        let bus = EventBus::new();
        for i in 0..5 {
            bus.publish(make_event(EventType::TaskCreated, &format!("t-{i}")));
        }
        let page = bus.events_range(1, 2);
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].entity_id, "t-1");
        assert_eq!(page[1].entity_id, "t-2");
    }

    #[test]
    fn events_range_empty_when_offset_past_end() {
        let bus = EventBus::new();
        bus.publish(make_event(EventType::TaskCreated, "t-0"));
        assert!(bus.events_range(10, 5).is_empty());
    }

    // T6.2 — events_since delta
    #[test]
    fn events_since_returns_only_entries_after_given_id() {
        use theo_domain::identifiers::EventId;
        let bus = EventBus::new();
        for i in 0..4 {
            bus.publish(DomainEvent {
                event_id: EventId::new(format!("e-{i}")),
                event_type: EventType::TaskCreated,
                entity_id: format!("t-{i}"),
                timestamp: i as u64,
                payload: serde_json::Value::Null,
                supersedes_event_id: None,
            });
        }
        let since = bus.events_since(&EventId::new("e-1"));
        assert_eq!(since.len(), 2, "must return only e-2 and e-3");
        assert_eq!(since[0].entity_id, "t-2");
        assert_eq!(since[1].entity_id, "t-3");
    }

    #[test]
    fn events_since_returns_full_log_when_id_unknown() {
        use theo_domain::identifiers::EventId;
        let bus = EventBus::new();
        bus.publish(make_event(EventType::TaskCreated, "t-0"));
        bus.publish(make_event(EventType::TaskCreated, "t-1"));
        let since = bus.events_since(&EventId::new("nonexistent"));
        assert_eq!(since.len(), 2);
    }

    #[test]
    fn panicking_listener_does_not_crash_bus() {
        let bus = EventBus::new();
        let capturing = Arc::new(CapturingListener::new());
        bus.subscribe(Arc::new(PanickingListener));
        bus.subscribe(capturing.clone());

        // Should not panic — bus catches the panic
        bus.publish(make_event(EventType::TaskCreated, "t-1"));

        // Second listener still received the event
        assert_eq!(capturing.captured().len(), 1);
        // Event still logged
        assert_eq!(bus.len(), 1);
    }

    // T2.1 AC: parking_lot::Mutex never poisons, so a listener panic does
    // not prevent subsequent publishes from acquiring the log/listeners
    // locks. Under std::sync::Mutex this regressed silently when the panic
    // poisoned the mutex.
    #[test]
    fn listener_panic_does_not_poison_bus_for_subsequent_publish() {
        let bus = EventBus::new();
        let capturing = Arc::new(CapturingListener::new());
        bus.subscribe(Arc::new(PanickingListener));
        bus.subscribe(capturing.clone());

        // First publish — panics internally, caught.
        bus.publish(make_event(EventType::TaskCreated, "t-1"));
        // Second publish — must succeed, not panic on "lock poisoned".
        bus.publish(make_event(EventType::TaskStateChanged, "t-1"));
        // Third publish on a different entity — still fine.
        bus.publish(make_event(EventType::RunInitialized, "r-1"));

        assert_eq!(bus.len(), 3, "all three events must be logged");
        assert_eq!(
            capturing.captured().len(),
            3,
            "non-panicking listener must keep receiving events"
        );
    }

    // -----------------------------------------------------------------------
    // NullEventListener
    // -----------------------------------------------------------------------

    #[test]
    fn null_listener_accepts_all_event_types() {
        let listener = NullEventListener;
        for et in &ALL_EVENT_TYPES {
            let event = make_event(*et, "test");
            listener.on_event(&event); // must not panic
        }
    }

    // -----------------------------------------------------------------------
    // PrintEventListener
    // -----------------------------------------------------------------------

    #[test]
    fn print_listener_accepts_all_event_types() {
        let listener = PrintEventListener;
        for et in &ALL_EVENT_TYPES {
            let event = make_event(*et, "test");
            listener.on_event(&event); // must not panic
        }
    }

    // -----------------------------------------------------------------------
    // Duplicate event_id
    // -----------------------------------------------------------------------

    #[test]
    fn duplicate_event_id_accepted_in_log() {
        use theo_domain::identifiers::EventId;
        let bus = EventBus::new();
        let event1 = DomainEvent {
            event_id: EventId::new("same-id"),
            event_type: EventType::TaskCreated,
            entity_id: "t-1".into(),
            timestamp: 1000,
            payload: serde_json::Value::Null,
            supersedes_event_id: None,
        };
        let event2 = DomainEvent {
            event_id: EventId::new("same-id"),
            event_type: EventType::TaskStateChanged,
            entity_id: "t-1".into(),
            timestamp: 2000,
            payload: serde_json::Value::Null,
            supersedes_event_id: None,
        };
        bus.publish(event1);
        bus.publish(event2);
        // Append-only: both events accepted
        assert_eq!(bus.len(), 2);
    }

    // -----------------------------------------------------------------------
    // Thread safety (compilation test)
    // -----------------------------------------------------------------------

    #[test]
    fn event_bus_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<EventBus>();
    }

    #[test]
    fn event_bus_works_across_threads() {
        let bus = Arc::new(EventBus::new());
        let listener = Arc::new(CapturingListener::new());
        bus.subscribe(listener.clone());

        let bus_clone = bus.clone();
        let handle = std::thread::spawn(move || {
            bus_clone.publish(make_event(EventType::TaskCreated, "t-from-thread"));
        });
        handle.join().unwrap();

        assert_eq!(bus.len(), 1);
        assert_eq!(listener.captured().len(), 1);
        assert_eq!(listener.captured()[0].entity_id, "t-from-thread");
    }

    // -----------------------------------------------------------------------
    // Broadcast bridge
    // -----------------------------------------------------------------------

    #[test]
    fn broadcast_receives_events() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe_broadcast(1024);

        bus.publish(make_event(EventType::TaskCreated, "t-1"));
        bus.publish(make_event(EventType::ToolCallQueued, "c-1"));
        bus.publish(make_event(EventType::ContentDelta, "r-1"));

        let e1 = rx.try_recv().expect("should receive first event");
        assert_eq!(e1.event_type, EventType::TaskCreated);
        let e2 = rx.try_recv().expect("should receive second event");
        assert_eq!(e2.event_type, EventType::ToolCallQueued);
        let e3 = rx.try_recv().expect("should receive third event");
        assert_eq!(e3.event_type, EventType::ContentDelta);
        assert!(rx.try_recv().is_err(), "no more events");
    }

    #[test]
    fn broadcast_lagged_returns_error() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe_broadcast(2);

        // Publish 5 events without consuming — buffer is 2
        for i in 0..5 {
            bus.publish(make_event(EventType::TaskCreated, &format!("t-{i}")));
        }

        // First recv should report lagged
        match rx.try_recv() {
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
                assert!(n >= 1, "should have lagged at least 1 event, got {n}");
            }
            other => panic!("expected Lagged, got {other:?}"),
        }
    }

    #[test]
    fn broadcast_drop_receiver_no_crash() {
        let bus = EventBus::new();
        let rx = bus.subscribe_broadcast(16);
        drop(rx); // Drop immediately

        // Publishing after drop must not panic
        bus.publish(make_event(EventType::Error, "e-1"));
        bus.publish(make_event(EventType::Error, "e-2"));

        // Log still works
        assert_eq!(bus.len(), 2);
    }

    #[test]
    fn broadcast_coexists_with_sync_listeners() {
        let bus = EventBus::new();

        // Sync listener registered BEFORE broadcast
        let sync_listener = Arc::new(CapturingListener::new());
        bus.subscribe(sync_listener.clone());

        // Broadcast registered AFTER
        let mut rx = bus.subscribe_broadcast(1024);

        bus.publish(make_event(EventType::RunInitialized, "r-1"));

        // Both received the event
        assert_eq!(sync_listener.captured().len(), 1);
        let broadcast_event = rx.try_recv().expect("broadcast should receive");
        assert_eq!(broadcast_event.event_type, EventType::RunInitialized);
    }

    // ────────────────────────────────────────────────────────────────
    // Phase 4 — EventBusSink adapter bridges theo-application's
    // EventSink trait to this concrete bus (PLAN_CONTEXT_WIRING)
    // ────────────────────────────────────────────────────────────────

    #[test]
    fn event_bus_sink_forwards_emitted_event_to_underlying_bus() {
        use theo_domain::graph_context::EventSink;

        let bus = Arc::new(EventBus::new());
        let listener = Arc::new(CapturingListener::new());
        bus.subscribe(listener.clone());

        let sink = EventBusSink::new(bus.clone());
        sink.emit(DomainEvent::new(
            EventType::RetrievalExecuted,
            "graph-context",
            serde_json::json!({
                "primary_files": 5,
                "harm_removals": 1,
                "compression_savings_tokens": 256,
                "inline_slices_count": 0,
            }),
        ));

        let captured = listener.captured();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].event_type, EventType::RetrievalExecuted);
        assert_eq!(
            captured[0].payload.get("primary_files").and_then(|v| v.as_u64()),
            Some(5)
        );
    }

    #[test]
    fn event_bus_sink_is_dyn_compatible_with_event_sink_trait() {
        // Smoke: EventBusSink is usable as Arc<dyn EventSink>, the exact
        // shape `GraphContextService::with_event_sink` expects.
        let bus = Arc::new(EventBus::new());
        let sink: Arc<dyn theo_domain::graph_context::EventSink> =
            Arc::new(EventBusSink::new(bus.clone()));
        sink.emit(DomainEvent::new(
            EventType::RetrievalExecuted,
            "x",
            serde_json::json!({}),
        ));
        // Publish went through: event appears in the bus log.
        let logged = bus.events();
        assert_eq!(logged.len(), 1);
    }
}
