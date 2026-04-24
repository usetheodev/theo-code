//! Characterization tests for the agent runtime event sequences.
//!
//! REMEDIATION_PLAN T0.1. These tests capture the *observable* event
//! sequence of canonical lifecycle flows (task + tool-call + budget +
//! doom-loop + cancellation) so any future refactor — in particular the
//! split of `run_engine.rs` planned in Fase 4 — provably preserves
//! semantics.
//!
//! Each test arranges a harness, exercises the flow, and asserts the
//! sequence of `EventType`s captured by a `CapturingListener`. The
//! assertions are written as inline `insta::assert_yaml_snapshot!` so
//! regressions produce a reviewable diff rather than an opaque boolean.
//!
//! These are *characterization* tests in Feathers' sense — they pin
//! current behaviour, not desired behaviour. When a deliberate change
//! shifts the event sequence, the author updates the snapshot with
//! `cargo insta review`.
//!
//! **Scope notes:**
//! - The tests stay off the LLM hot path. Scenarios that need a real
//!   LLM response stream (context overflow recovery, retry, done gate)
//!   remain as TODOs until an HTTP mock harness lands.
//! - `TaskManager`, `ToolCallManager`, `BudgetEnforcer`, `DoomLoopTracker`,
//!   and `CancellationTree` are exercised directly — they are the
//!   observable primitives any split of `run_engine.rs` must preserve.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use theo_agent_runtime::budget_enforcer::BudgetEnforcer;
use theo_agent_runtime::cancellation::CancellationTree;
use theo_agent_runtime::event_bus::{EventBus, EventListener};
use theo_agent_runtime::task_manager::TaskManager;
use theo_agent_runtime::tool_call_manager::ToolCallManager;
use theo_domain::budget::Budget;
use theo_domain::event::{DomainEvent, EventType};
use theo_domain::session::{MessageId, SessionId};
use theo_domain::task::{AgentType, TaskState};
use theo_domain::tool::ToolContext;
use theo_tooling::registry::create_default_registry;

/// Local capturing listener (the one in `event_bus.rs` is gated by
/// `#[cfg(test)]` and thus invisible to integration test crates).
pub struct CapturingListener {
    events: Mutex<Vec<DomainEvent>>,
}

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

impl EventListener for CapturingListener {
    fn on_event(&self, event: &DomainEvent) {
        self.events.lock().push(event.clone());
    }
}

/// Returns the ordered list of `EventType` names captured by a listener.
fn event_type_sequence(listener: &CapturingListener) -> Vec<String> {
    listener
        .captured()
        .into_iter()
        .map(|e| format!("{:?}", e.event_type))
        .collect()
}

fn setup_bus() -> (Arc<EventBus>, Arc<CapturingListener>) {
    let bus = Arc::new(EventBus::new());
    let listener = Arc::new(CapturingListener::new());
    bus.subscribe(listener.clone());
    (bus, listener)
}

// --------------------------------------------------------------------------
// Scenario 1 — Task happy path: Pending → Ready → Running → Completed
// --------------------------------------------------------------------------
#[test]
fn scenario_task_happy_path_emits_created_then_three_transitions() {
    let (bus, listener) = setup_bus();
    let tm = TaskManager::new(bus.clone());

    let id = tm.create_task(
        SessionId::new("s-1"),
        AgentType::Coder,
        "fix bug".into(),
    );
    tm.transition(&id, TaskState::Ready).unwrap();
    tm.transition(&id, TaskState::Running).unwrap();
    tm.transition(&id, TaskState::Completed).unwrap();

    insta::assert_yaml_snapshot!(event_type_sequence(&listener), @r"
    - TaskCreated
    - TaskStateChanged
    - TaskStateChanged
    - TaskStateChanged
    ");
}

// --------------------------------------------------------------------------
// Scenario 2 — Task failure path: Pending → Ready → Running → Failed
// --------------------------------------------------------------------------
#[test]
fn scenario_task_failure_path_emits_created_then_three_transitions() {
    let (bus, listener) = setup_bus();
    let tm = TaskManager::new(bus.clone());

    let id = tm.create_task(SessionId::new("s"), AgentType::Coder, "t".into());
    tm.transition(&id, TaskState::Ready).unwrap();
    tm.transition(&id, TaskState::Running).unwrap();
    tm.transition(&id, TaskState::Failed).unwrap();

    insta::assert_yaml_snapshot!(event_type_sequence(&listener), @r"
    - TaskCreated
    - TaskStateChanged
    - TaskStateChanged
    - TaskStateChanged
    ");
}

// --------------------------------------------------------------------------
// Scenario 3 — Invalid transition emits no StateChanged event.
// --------------------------------------------------------------------------
#[test]
fn scenario_invalid_transition_suppresses_event() {
    let (bus, listener) = setup_bus();
    let tm = TaskManager::new(bus.clone());

    let id = tm.create_task(SessionId::new("s"), AgentType::Coder, "t".into());
    // Pending → Completed is invalid — no event should fire.
    let err = tm.transition(&id, TaskState::Completed);
    assert!(err.is_err());

    insta::assert_yaml_snapshot!(event_type_sequence(&listener), @r"
    - TaskCreated
    ");
}

// --------------------------------------------------------------------------
// Scenario 4 — Tool call dispatch emits Queued → Dispatched → Completed.
// --------------------------------------------------------------------------
#[tokio::test]
async fn scenario_tool_call_dispatch_emits_three_events_in_order() {
    let (bus, listener) = setup_bus();
    let tcm = ToolCallManager::new(bus.clone());
    let registry = create_default_registry();

    // `read` on a missing path → Failed terminal. We only care about the
    // sequence of event types, not success.
    let call_id = tcm.enqueue(
        theo_domain::identifiers::TaskId::new("t-1"),
        "read".into(),
        serde_json::json!({"filePath": "/nonexistent/path"}),
    );
    let ctx = ToolContext {
        session_id: SessionId::new("s"),
        message_id: MessageId::new("m"),
        call_id: call_id.as_str().to_string().into(),
        agent: "test".to_string(),
        abort: tokio::sync::watch::channel(false).1,
        project_dir: PathBuf::from("/tmp"),
        graph_context: None,
        stdout_tx: None,
    };
    let _ = tcm.dispatch_and_execute(&call_id, &registry, &ctx).await;

    insta::assert_yaml_snapshot!(event_type_sequence(&listener), @r"
    - ToolCallQueued
    - ToolCallDispatched
    - ToolCallCompleted
    ");
}

// --------------------------------------------------------------------------
// Scenario 5 — Budget violation emits a BudgetExceeded event exactly once
// per check() call. The event payload carries the violation details.
// --------------------------------------------------------------------------
#[test]
fn scenario_budget_iterations_exceeded_emits_single_budget_event() {
    let (bus, listener) = setup_bus();
    let budget = Budget {
        max_iterations: 2,
        ..Budget::default()
    };
    let mut enforcer = BudgetEnforcer::new(budget, bus.clone(), "run-1");

    enforcer.record_iteration();
    enforcer.record_iteration();
    enforcer.record_iteration();
    let _ = enforcer.check();

    insta::assert_yaml_snapshot!(event_type_sequence(&listener), @r"
    - BudgetExceeded
    ");
}

// --------------------------------------------------------------------------
// Scenario 6 — Cancellation tree: root cancel propagates to all child
// tokens. No events are emitted (cancellation is an internal signal, not
// a DomainEvent), but the observable tokens flip.
// --------------------------------------------------------------------------
#[tokio::test]
async fn scenario_cancellation_tree_root_cancel_propagates_silently() {
    let (bus, listener) = setup_bus();
    let tree = CancellationTree::new();
    let c1 = tree.child("a");
    let c2 = tree.child("b");

    tree.cancel_all();
    // Token propagation is synchronous — observable immediately.
    assert!(c1.is_cancelled() && c2.is_cancelled() && tree.is_cancelled());

    // No DomainEvents are emitted as part of cancellation alone.
    let events = listener.captured();
    assert!(
        events.is_empty(),
        "cancellation must not spam the event bus"
    );
    drop(bus); // keep it alive for the listener subscription
}

// --------------------------------------------------------------------------
// Scenario 7 — Full task+tool lifecycle combined: create task, enqueue
// a tool call, run it, transition the task to Completed. Used as a
// pin for the canonical "single-tool happy path" shape.
// --------------------------------------------------------------------------
#[tokio::test]
async fn scenario_task_plus_tool_lifecycle_combined_sequence() {
    let (bus, listener) = setup_bus();
    let tm = TaskManager::new(bus.clone());
    let tcm = ToolCallManager::new(bus.clone());
    let registry = create_default_registry();

    let task_id = tm.create_task(SessionId::new("s"), AgentType::Coder, "t".into());
    tm.transition(&task_id, TaskState::Ready).unwrap();
    tm.transition(&task_id, TaskState::Running).unwrap();

    let call_id = tcm.enqueue(
        task_id.clone(),
        "read".into(),
        serde_json::json!({"filePath": "/nonexistent"}),
    );
    let ctx = ToolContext {
        session_id: SessionId::new("s"),
        message_id: MessageId::new("m"),
        call_id: call_id.as_str().to_string().into(),
        agent: "test".to_string(),
        abort: tokio::sync::watch::channel(false).1,
        project_dir: PathBuf::from("/tmp"),
        graph_context: None,
        stdout_tx: None,
    };
    let _ = tcm.dispatch_and_execute(&call_id, &registry, &ctx).await;

    tm.transition(&task_id, TaskState::Completed).unwrap();

    insta::assert_yaml_snapshot!(event_type_sequence(&listener), @r"
    - TaskCreated
    - TaskStateChanged
    - TaskStateChanged
    - ToolCallQueued
    - ToolCallDispatched
    - ToolCallCompleted
    - TaskStateChanged
    ");
}

// --------------------------------------------------------------------------
// Scenario 8a — Task waiting-tool cycle: Pending → Ready → Running →
// WaitingTool → Running → Completed. Pins the recommended convergence
// path used by the agent loop when a tool call is in flight.
// --------------------------------------------------------------------------
#[test]
fn scenario_task_waiting_tool_cycle_emits_full_lifecycle_sequence() {
    let (bus, listener) = setup_bus();
    let tm = TaskManager::new(bus.clone());

    let id = tm.create_task(SessionId::new("s-wt"), AgentType::Coder, "loop".into());
    tm.transition(&id, TaskState::Ready).unwrap();
    tm.transition(&id, TaskState::Running).unwrap();
    tm.transition(&id, TaskState::WaitingTool).unwrap();
    tm.transition(&id, TaskState::Running).unwrap();
    tm.transition(&id, TaskState::Completed).unwrap();

    insta::assert_yaml_snapshot!(event_type_sequence(&listener), @r"
    - TaskCreated
    - TaskStateChanged
    - TaskStateChanged
    - TaskStateChanged
    - TaskStateChanged
    - TaskStateChanged
    ");
}

// --------------------------------------------------------------------------
// Scenario 8b — Cancellation tree nested-child propagation. A cancel
// of the root token must propagate to a manually-derived grandchild,
// not just direct children. Pins the inheritance contract.
// --------------------------------------------------------------------------
#[tokio::test]
async fn scenario_cancellation_tree_nested_child_inherits_root_cancel() {
    use std::time::Duration;

    let tree = Arc::new(CancellationTree::new());
    // Root → child → grandchild via tokio_util::sync::CancellationToken.
    let parent = tree.child("subagent-parent");
    let grandchild = parent.child_token();

    // Independent watcher tasks for each level.
    let parent_watch = tokio::spawn({
        let p = parent.clone();
        async move {
            p.cancelled().await;
            "parent_cancelled"
        }
    });
    let grand_watch = tokio::spawn({
        let g = grandchild.clone();
        async move {
            g.cancelled().await;
            "grandchild_cancelled"
        }
    });

    // Cancel from the root — both watchers must fire within a
    // short deadline (the inheritance is synchronous in tokio_util).
    tree.cancel_all();

    let parent_outcome =
        tokio::time::timeout(Duration::from_secs(1), parent_watch)
            .await
            .expect("parent cancel deadline")
            .expect("task ok");
    let grand_outcome =
        tokio::time::timeout(Duration::from_secs(1), grand_watch)
            .await
            .expect("grandchild cancel deadline")
            .expect("task ok");

    insta::assert_yaml_snapshot!(
        vec![parent_outcome, grand_outcome],
        @r"
    - parent_cancelled
    - grandchild_cancelled
    "
    );
}

// --------------------------------------------------------------------------
// Scenario 8c — ToolCallManager terminal-state purge after replay.
// 3 enqueues → 3 dispatches → all terminate → purge with cutoff in the
// far future removes exactly 3, leaving record_count=0. Pins the
// purge_completed contract used by record_session_exit.
// --------------------------------------------------------------------------
#[tokio::test]
async fn scenario_tool_call_manager_purges_terminal_records_after_replay() {
    let (bus, _listener) = setup_bus();
    let manager = ToolCallManager::new(bus);
    let registry = create_default_registry();
    let ctx = ToolContext::test_context(PathBuf::from("/tmp"));
    let _ = MessageId::new("ignored"); // reused import, silence linter

    let mut ids = Vec::new();
    for i in 0..3 {
        let id = manager.enqueue(
            theo_domain::identifiers::TaskId::new(format!("t-{i}")),
            "read".into(),
            serde_json::json!({"filePath": format!("/nonexistent/{i}")}),
        );
        ids.push(id);
    }
    for id in &ids {
        let _ = manager.dispatch_and_execute(id, &registry, &ctx).await;
    }
    assert_eq!(manager.record_count(), 3, "all 3 dispatched");

    let far_future = theo_domain::clock::now_millis() + 1_000_000;
    let purged = manager.purge_completed(far_future, 0);

    insta::assert_yaml_snapshot!(
        format!("purged={purged} remaining={}", manager.record_count()),
        @"purged=3 remaining=0"
    );
}

// --------------------------------------------------------------------------
// Scenario 9 — EventBus preserves insertion order under bounded-log
// rotation. This pins the FIFO contract the observability pipeline
// assumes.
// --------------------------------------------------------------------------
#[test]
fn scenario_event_bus_preserves_order_under_bounded_rotation() {
    let bus = EventBus::with_max_events(3);
    let listener = Arc::new(CapturingListener::new());
    bus.subscribe(listener.clone());

    for i in 0..5 {
        bus.publish(DomainEvent::new(
            EventType::TaskCreated,
            format!("t-{i}"),
            serde_json::Value::Null,
        ));
    }

    let entities: Vec<String> = bus
        .events()
        .into_iter()
        .map(|e| e.entity_id)
        .collect();

    // Bus capacity is 3 → oldest 2 dropped, order preserved for the rest.
    insta::assert_yaml_snapshot!(entities, @r"
    - t-2
    - t-3
    - t-4
    ");
}
