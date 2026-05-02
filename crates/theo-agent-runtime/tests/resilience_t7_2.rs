//! REMEDIATION_PLAN T7.2 — Resilience / failure-mode integration tests.
//!
//! Covers the resilience invariants in REVIEW §5 that have landed as
//! code in earlier iterations (T2.1 panic isolation, T6.3 record purge,
//! T6.5 mutex-contention lock discipline). Each test exercises the
//! public API through its documented contract, not internal state.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use theo_domain::event::{DomainEvent, EventType};
use theo_domain::identifiers::TaskId;
use theo_domain::tool::ToolContext;
use theo_tooling::registry::create_default_registry;

use theo_agent_runtime::event_bus::{EventBus, EventListener};
use theo_agent_runtime::tool_call_manager::ToolCallManager;

// ────────────────────────────────────────────────────────────────────
// T2.1 — Listener panics MUST NOT poison the EventBus.
//
// The bus runs each `on_event` inside `catch_unwind`. A misbehaving
// listener (third-party integration, unwrapped option, overflowed
// arithmetic) must not prevent well-behaved listeners from receiving
// the same event, and must not prevent future events from being
// delivered.
// ────────────────────────────────────────────────────────────────────

/// Listener that always panics. Captures its invocation count so the test
/// can verify it was actually called (and not silently skipped).
#[derive(Default)]
struct PanickingListener {
    invocations: AtomicUsize,
}

impl EventListener for PanickingListener {
    fn on_event(&self, _event: &DomainEvent) {
        self.invocations.fetch_add(1, Ordering::SeqCst);
        panic!("intentional test panic from listener");
    }
}

/// Listener that records every event it sees. Used as the "good
/// citizen" the panic test verifies still works after a misbehaver.
#[derive(Default)]
struct CountingListener {
    events: Mutex<Vec<EventType>>,
}

impl EventListener for CountingListener {
    fn on_event(&self, event: &DomainEvent) {
        let mut events = self.events.lock().expect("listener mutex unpoisoned");
        events.push(event.event_type);
    }
}

#[test]
fn listener_panic_does_not_poison_event_bus() {
    let bus = EventBus::new();

    // Two listeners: panic first, counter second.
    // Subscribe order matters — if the bus stopped on first panic, the
    // counter would see 0 events. We want non-zero.
    let panicker = Arc::new(PanickingListener::default());
    let counter = Arc::new(CountingListener::default());

    bus.subscribe(panicker.clone());
    bus.subscribe(counter.clone());

    // Publish a sequence of 3 distinct events.
    for et in [
        EventType::RunInitialized,
        EventType::TaskCreated,
        EventType::ToolCallQueued,
    ] {
        bus.publish(DomainEvent::new(et, "test-entity", serde_json::json!({})));
    }

    // Post-condition 1: the panic listener WAS invoked (bus did not
    // silently skip it after the first fault).
    let panic_count = panicker.invocations.load(Ordering::SeqCst);
    assert_eq!(
        panic_count, 3,
        "panic listener should be invoked per event despite panicking"
    );

    // Post-condition 2: the counter listener received every event
    // AFTER the panicker despite the panic.
    let received = counter.events.lock().expect("unpoisoned");
    assert_eq!(
        received.len(),
        3,
        "counter should have received all 3 events; got {:?}",
        *received
    );
    assert_eq!(
        *received,
        vec![
            EventType::RunInitialized,
            EventType::TaskCreated,
            EventType::ToolCallQueued,
        ]
    );

    // Post-condition 3: bus's internal log is still consistent.
    let events = bus.events();
    assert_eq!(events.len(), 3);
}

// ────────────────────────────────────────────────────────────────────
// T6.3 — Terminal tool-call records are purgeable.
//
// Long REPL sessions can accrue 10k+ tool calls. The manager's
// `records` and `results` HashMaps grow unboundedly unless pruned.
// `purge_completed` removes terminal records (Succeeded/Failed/Timeout)
// older than a cutoff; non-terminal (Queued/Dispatched/Running) MUST
// survive.
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn tool_call_records_purged_after_n_terminal() {
    let bus = Arc::new(EventBus::new());
    let manager = ToolCallManager::new(bus);
    let registry = create_default_registry();
    let ctx = ToolContext::test_context(std::path::PathBuf::from("/tmp"));

    // Queue and complete 5 tool calls. Each targets a non-existent
    // file so the tool returns Failed — both failure and success are
    // terminal states for purge purposes.
    let mut terminal_ids = Vec::with_capacity(5);
    for i in 0..5 {
        let id = manager.enqueue(
            TaskId::new(format!("t-{i}")),
            "read".into(),
            serde_json::json!({"filePath": format!("/nonexistent/{i}")}),
        );
        let _ = manager.dispatch_and_execute(&id, &registry, &ctx).await;
        terminal_ids.push(id);
    }

    // Also queue 2 calls we do NOT dispatch — they stay in Queued
    // (non-terminal). These MUST NOT be purged.
    let _surviving1 = manager.enqueue(
        TaskId::new("t-queued-1"),
        "read".into(),
        serde_json::json!({"filePath": "/q1"}),
    );
    let _surviving2 = manager.enqueue(
        TaskId::new("t-queued-2"),
        "read".into(),
        serde_json::json!({"filePath": "/q2"}),
    );

    assert_eq!(manager.record_count(), 7, "5 terminal + 2 queued");

    // Purge every terminal record regardless of age.
    let far_future = theo_domain::clock::now_millis() + 10_000_000;
    let purged = manager.purge_completed(far_future, 0);
    assert_eq!(purged, 5, "exactly 5 terminal records should be removed");
    assert_eq!(
        manager.record_count(),
        2,
        "only the 2 queued records should remain"
    );

    // Subsequent purge is a no-op (idempotent).
    let purged2 = manager.purge_completed(far_future, 0);
    assert_eq!(purged2, 0);
    assert_eq!(manager.record_count(), 2);
}

// ────────────────────────────────────────────────────────────────────
// T6.5 — `dispatch_and_execute` lock discipline under 100-way parallel
// contention.
//
// Prior impl took 6 locks per dispatch; the T6.5 refactor collapsed to
// 3. The scenario is: spawn N parallel dispatches against a shared
// manager, each hitting a distinct tool call. Under the old impl the
// contention could either deadlock (if locks were held across an await
// boundary) or starve listeners. The new impl MUST complete them all
// without timing out.
// ────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dispatch_under_mutex_contention_100_parallel() {
    let bus = Arc::new(EventBus::new());
    let manager = Arc::new(ToolCallManager::new(bus));
    let registry = Arc::new(create_default_registry());

    // Pre-enqueue 100 calls so the parallel phase only dispatches.
    // `read` of a non-existent file fails fast — keeps the test quick.
    let mut ids = Vec::with_capacity(100);
    for i in 0..100 {
        let id = manager.enqueue(
            TaskId::new(format!("t-{i}")),
            "read".into(),
            serde_json::json!({"filePath": format!("/nonexistent/{i}")}),
        );
        ids.push(id);
    }
    assert_eq!(manager.record_count(), 100);

    // Spawn 100 parallel dispatches.
    let deadline = std::time::Duration::from_secs(10);
    let handles: Vec<_> = ids
        .into_iter()
        .map(|id| {
            let m = manager.clone();
            let reg = registry.clone();
            tokio::spawn(async move {
                let ctx = ToolContext::test_context(std::path::PathBuf::from("/tmp"));
                m.dispatch_and_execute(&id, &reg, &ctx).await
            })
        })
        .collect();

    // All must finish within the deadline — if any deadlock existed,
    // `tokio::time::timeout` would trip.
    let results = tokio::time::timeout(deadline, async {
        let mut out = Vec::with_capacity(100);
        for h in handles {
            out.push(h.await.expect("task panicked"));
        }
        out
    })
    .await
    .expect("100-way dispatch must finish within deadline");

    assert_eq!(results.len(), 100);
    // Each dispatch should return Ok — the tool failing is fine; what
    // we assert is that the manager didn't return CallNotFound or a
    // lock error.
    for r in &results {
        assert!(
            r.is_ok(),
            "dispatch must not yield manager-level error: {r:?}"
        );
    }

    // After all are done every record should be terminal.
    let far_future = theo_domain::clock::now_millis() + 10_000_000;
    let purged = manager.purge_completed(far_future, 0);
    assert_eq!(
        purged, 100,
        "every dispatched record should be terminal and purgeable"
    );
    assert_eq!(manager.record_count(), 0);
}

// ────────────────────────────────────────────────────────────────────
// Bonus T2.1 — publish-after-panic invariant.
//
// Separate test to verify that a listener that panics does NOT prevent
// FUTURE events from being delivered even when the panicking listener
// is the ONLY listener.
// ────────────────────────────────────────────────────────────────────

#[test]
fn solo_panicking_listener_does_not_stop_future_publishes() {
    let bus = EventBus::new();
    let panicker = Arc::new(PanickingListener::default());
    bus.subscribe(panicker.clone());

    // 5 events should invoke the panicker 5 times — bus keeps going.
    for _ in 0..5 {
        bus.publish(DomainEvent::new(
            EventType::RunInitialized,
            "solo",
            serde_json::json!({}),
        ));
    }

    assert_eq!(
        panicker.invocations.load(Ordering::SeqCst),
        5,
        "bus MUST continue delivering to a panicking listener"
    );
    assert_eq!(bus.events().len(), 5);
}

// ────────────────────────────────────────────────────────────────────
// T6.3 stress — long-session leak guard.
//
// The plan AC literal is `long_session_10k_tool_calls_does_not_leak_records`:
// after a session that dispatches 10 000 tool calls, a single
// `purge_completed` call MUST reclaim every terminal record so the
// manager's `records` HashMap returns to zero. The 5-call test above
// pins the per-call semantics; this one pins the scaling property
// — that purge is O(n) and complete, not bounded by some hidden
// internal cap.
// ────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn long_session_10k_tool_calls_does_not_leak_records() {
    const N: usize = 10_000;

    let bus = Arc::new(EventBus::new());
    let manager = ToolCallManager::new(bus);
    let registry = create_default_registry();
    let ctx = ToolContext::test_context(std::path::PathBuf::from("/tmp"));

    // Drive N enqueue+dispatch cycles. Each `read` of a non-existent
    // path fails fast (~microseconds), so 10 000 cycles complete
    // comfortably within the test's deadline budget. Failure and
    // success are both terminal states — what matters here is that
    // the record reaches a purgeable state.
    for i in 0..N {
        let id = manager.enqueue(
            TaskId::new(format!("t-{i}")),
            "read".into(),
            serde_json::json!({ "filePath": format!("/nonexistent/{i}") }),
        );
        let _ = manager.dispatch_and_execute(&id, &registry, &ctx).await;
    }
    assert_eq!(
        manager.record_count(),
        N,
        "all N enqueued records must persist before purge"
    );

    // Single sweep — far_future cutoff so age never gates eviction;
    // older_than_ms = 0 so every terminal record qualifies.
    let far_future = theo_domain::clock::now_millis() + 10_000_000;
    let purged = manager.purge_completed(far_future, 0);
    assert_eq!(
        purged, N,
        "purge_completed must reclaim every terminal record from a 10k-call session"
    );
    assert_eq!(
        manager.record_count(),
        0,
        "no records may remain after purging a long session — leak guard"
    );
}
