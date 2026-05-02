//! REMEDIATION_PLAN T0.2 — Sub-agent spawn characterization.
//!
//! Snapshots the observable `EventType` sequence + key payload fields
//! emitted by `SubAgentManager::spawn_with_spec` for the spawn paths
//! that exit BEFORE the LLM hot path (so the tests run without an HTTP
//! mock):
//!
//!   1. Pre-run cancellation — root token already cancelled.
//!   2. SubagentStart hook returns `Block`.
//!   3. SubagentStart hook returns `Allow` but pre-run cancellation
//!      tree triggers (combined gate behavior).
//!
//! These pin the contract every future refactor of `spawn_helpers.rs`
//! must preserve. The full happy path (LLM responds, tools run,
//! SubagentCompleted with success=true) requires an LLM mock and is
//! tracked as remaining work in T0.1/T0.2.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;

use theo_agent_runtime::cancellation::CancellationTree;
use theo_agent_runtime::config::AgentConfig;
use theo_agent_runtime::event_bus::{EventBus, EventListener};
use theo_agent_runtime::lifecycle_hooks::{HookEvent, HookManager, HookMatcher, HookResponse};
use theo_agent_runtime::subagent::SubAgentManager;
use theo_domain::agent_spec::AgentSpec;
use theo_domain::event::{DomainEvent, EventType};

// ────────────────────────────────────────────────────────────────────
// Test harness
// ────────────────────────────────────────────────────────────────────

struct Capture {
    events: Mutex<Vec<DomainEvent>>,
}

impl Capture {
    fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }
    fn types(&self) -> Vec<String> {
        self.events
            .lock()
            .iter()
            .map(|e| format!("{:?}", e.event_type))
            .collect()
    }
    fn last_completion_payload(&self) -> Option<serde_json::Value> {
        self.events
            .lock()
            .iter()
            .rfind(|e| e.event_type == EventType::SubagentCompleted)
            .map(|e| e.payload.clone())
    }
}

impl EventListener for Capture {
    fn on_event(&self, e: &DomainEvent) {
        self.events.lock().push(e.clone());
    }
}

fn setup() -> (Arc<EventBus>, Arc<Capture>) {
    let bus = Arc::new(EventBus::new());
    let capture = Arc::new(Capture::new());
    bus.subscribe(capture.clone() as Arc<dyn EventListener>);
    (bus, capture)
}

// ────────────────────────────────────────────────────────────────────
// Scenario 1 — pre-run cancellation: root token cancelled BEFORE the
// spawn. The bus must see SubagentStarted (emitted right before the
// cancellation check) followed by SubagentCompleted with cancelled=true.
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn subagent_pre_run_cancellation_emits_started_then_completed_cancelled() {
    let (bus, capture) = setup();
    let tree = Arc::new(CancellationTree::new());
    tree.cancel_all(); // every child token is born already cancelled

    let manager = SubAgentManager::with_builtins(
        AgentConfig::default(),
        bus,
        PathBuf::from("/tmp"),
    )
    .with_cancellation(tree);

    let spec = AgentSpec::on_demand("scout", "noop");
    let result = manager.spawn_with_spec(&spec, "noop", None).await;

    insta::assert_yaml_snapshot!(capture.types(), @r"
    - SubagentStarted
    - SubagentCompleted
    ");

    let payload = capture.last_completion_payload().expect("completed event");
    assert_eq!(payload["agent_name"], "scout");
    assert_eq!(payload["cancelled"], true);
    assert_eq!(payload["success"], false);
    assert!(
        result.summary.contains("cancelled before start"),
        "summary should mention pre-run cancellation, got: {}",
        result.summary
    );
}

// ────────────────────────────────────────────────────────────────────
// Scenario 2 — SubagentStart hook returns `Block`. The hook fires
// BEFORE `emit_subagent_started`, so the bus must NOT see
// SubagentStarted at all — only the SubagentCompleted from the early
// `publish_completed` call.
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn subagent_start_hook_block_emits_only_completed_no_started() {
    let (bus, capture) = setup();

    let mut hooks = HookManager::new();
    hooks.add(
        HookEvent::SubagentStart,
        HookMatcher {
            matcher: None,
            response: HookResponse::Block {
                reason: "T0.2 characterization block".into(),
            },
            timeout_secs: 60,
        },
    );

    let manager = SubAgentManager::with_builtins(
        AgentConfig::default(),
        bus,
        PathBuf::from("/tmp"),
    )
    .with_hooks(Arc::new(hooks));

    let spec = AgentSpec::on_demand("blocked", "noop");
    let result = manager.spawn_with_spec(&spec, "noop", None).await;

    insta::assert_yaml_snapshot!(capture.types(), @r"
    - SubagentCompleted
    ");

    assert!(!result.success, "blocked spawn must report failure");
    assert!(
        result.summary.contains("blocked"),
        "summary should mention block, got: {}",
        result.summary
    );

    let payload = capture.last_completion_payload().expect("completed event");
    assert_eq!(payload["agent_name"], "blocked");
    assert_eq!(payload["success"], false);
}

// ────────────────────────────────────────────────────────────────────
// Scenario 3 — combined gates: hook ALLOWS but cancellation tree is
// pre-cancelled. Order matters — the hook fires before the
// cancellation check, so the SubagentStarted event still flows. The
// cancellation gate then short-circuits.
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn subagent_hook_allows_then_cancellation_short_circuits() {
    let (bus, capture) = setup();

    // Hook: configured but no SubagentStart matcher → defaults to
    // Allow. Verifies the hook plumbing doesn't accidentally swallow
    // the spawn even when configured.
    let hooks = HookManager::new();

    let tree = Arc::new(CancellationTree::new());
    tree.cancel_all();

    let manager = SubAgentManager::with_builtins(
        AgentConfig::default(),
        bus,
        PathBuf::from("/tmp"),
    )
    .with_hooks(Arc::new(hooks))
    .with_cancellation(tree);

    let spec = AgentSpec::on_demand("composite", "noop");
    let result = manager.spawn_with_spec(&spec, "noop", None).await;

    insta::assert_yaml_snapshot!(capture.types(), @r"
    - SubagentStarted
    - SubagentCompleted
    ");

    assert!(result.cancelled, "cancellation must propagate to result");
    let payload = capture.last_completion_payload().expect("completed event");
    assert_eq!(payload["cancelled"], true);
}

// ────────────────────────────────────────────────────────────────────
// Scenario 4 — SubagentCompleted payload always carries the canonical
// fields (agent_name, agent_source, success, cancelled, duration_ms,
// tokens_used, llm_calls, iterations_used, otel) so dashboards and
// metrics consumers don't need defensive-default branches.
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn subagent_completed_payload_carries_canonical_fields() {
    let (bus, capture) = setup();
    let tree = Arc::new(CancellationTree::new());
    tree.cancel_all();

    let manager = SubAgentManager::with_builtins(
        AgentConfig::default(),
        bus,
        PathBuf::from("/tmp"),
    )
    .with_cancellation(tree);

    let spec = AgentSpec::on_demand("payload-shape", "noop");
    let _ = manager.spawn_with_spec(&spec, "noop", None).await;

    let payload = capture.last_completion_payload().expect("completed event");
    let canonical_fields = [
        "agent_name",
        "agent_source",
        "success",
        "summary",
        "duration_ms",
        "tokens_used",
        "input_tokens",
        "output_tokens",
        "llm_calls",
        "iterations_used",
        "cancelled",
        "otel",
    ];
    let present: Vec<&str> = canonical_fields
        .iter()
        .copied()
        .filter(|f| payload.get(f).is_some())
        .collect();
    insta::assert_yaml_snapshot!(present, @r#"
    - agent_name
    - agent_source
    - success
    - summary
    - duration_ms
    - tokens_used
    - input_tokens
    - output_tokens
    - llm_calls
    - iterations_used
    - cancelled
    - otel
    "#);
}
