//! Sibling test body of `run_engine/mod.rs` — split per-area (T3.2 of code-hygiene-5x5).
//!
//! Test-only file; gates use the inner `cfg(test)` attribute below to
//! classify every line as test code.

#![cfg(test)]
#![allow(unused_imports)]

use super::*;
use crate::event_bus::CapturingListener;
use theo_domain::session::SessionId;
use theo_domain::task::AgentType;

use crate::run_engine::test_helpers::TestSetup;

#[tokio::test]
async fn delegate_task_rejects_both_agent_and_parallel() {
    let setup = TestSetup::new();
    let mut engine = setup.create_engine("test");
    let args = serde_json::json!({
        "agent": "explorer",
        "objective": "x",
        "parallel": [{"agent": "verifier", "objective": "y"}]
    });
    let result = engine.handle_delegate_task(args).await;
    assert!(result.starts_with("Error:"));
    assert!(result.contains("not both"));
}

#[tokio::test]
async fn delegate_task_rejects_neither_agent_nor_parallel() {
    let setup = TestSetup::new();
    let mut engine = setup.create_engine("test");
    let args = serde_json::json!({});
    let result = engine.handle_delegate_task(args).await;
    assert!(result.starts_with("Error:"));
}

#[tokio::test]
async fn delegate_task_rejects_empty_agent_name() {
    let setup = TestSetup::new();
    let mut engine = setup.create_engine("test");
    let args = serde_json::json!({"agent": "", "objective": "x"});
    let result = engine.handle_delegate_task(args).await;
    assert!(result.starts_with("Error:"));
    assert!(result.contains("non-empty"));
}

#[tokio::test]
async fn delegate_task_rejects_empty_objective() {
    let setup = TestSetup::new();
    let mut engine = setup.create_engine("test");
    let args = serde_json::json!({"agent": "explorer", "objective": ""});
    let result = engine.handle_delegate_task(args).await;
    assert!(result.starts_with("Error:"));
    assert!(result.contains("required"));
}

#[tokio::test]
async fn delegate_task_rejects_empty_parallel_array() {
    let setup = TestSetup::new();
    let mut engine = setup.create_engine("test");
    let args = serde_json::json!({"parallel": []});
    let result = engine.handle_delegate_task(args).await;
    assert!(result.starts_with("Error:"));
    assert!(result.contains("non-empty"));
}

#[tokio::test]
async fn delegate_task_unknown_agent_creates_on_demand() {
    // We can't actually run a real LLM here. We can verify that the
    // dispatch path PICKS the on-demand spec by inspecting the registry
    // build behavior: when an unknown agent name is passed, the spec
    // returned is on-demand (read-only).
    // This is implicitly tested above through `spawn_with_spec` semantics.
    // The integration test runs against a real LLM (out of scope here).
    let setup = TestSetup::new();
    let mut engine = setup.create_engine("test");
    // Use a name that won't be in any registry. Fast-fail because
    // there's no LLM at localhost:9999, but we should at least see the
    // delegation prefix prove the routing executed.
    let args = serde_json::json!({"agent": "nonexistent-zzzz", "objective": "do x"});
    let result = engine.handle_delegate_task(args).await;
    // Either succeed (unlikely without LLM) or fail with the agent name
    // prefix proving the dispatch reached spawn_with_spec.
    assert!(
        result.contains("nonexistent-zzzz"),
        "expected agent name in result, got: {}",
        result
    );
}

#[test]
fn delegate_task_tool_def_is_registered() {
    let registry = theo_tooling::registry::create_default_registry();
    let defs = crate::tool_bridge::registry_to_definitions(&registry);
    let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
    assert!(
        names.contains(&"delegate_task"),
        "delegate_task must be in tool definitions"
    );
}

// ── Split tool variants ──

#[test]
fn delegate_task_single_tool_def_is_registered() {
    let registry = theo_tooling::registry::create_default_registry();
    let defs = crate::tool_bridge::registry_to_definitions(&registry);
    let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
    assert!(names.contains(&"delegate_task_single"));
    assert!(names.contains(&"delegate_task_parallel"));
    assert!(names.contains(&"delegate_task")); // legacy alias
}

#[tokio::test]
async fn delegate_task_uses_injected_registry_and_run_store() {
    use crate::subagent_runs::FileSubagentRunStore;
    let setup = TestSetup::new();
    let mut engine = setup.create_engine("test");

    // Inject a custom registry with a known agent + a run store
    let mut reg = crate::subagent::SubAgentRegistry::with_builtins();
    reg.register(theo_domain::agent_spec::AgentSpec::on_demand(
        "scout",
        "test purpose",
    ));
    let tempdir = tempfile::TempDir::new().unwrap();
    let store = std::sync::Arc::new(FileSubagentRunStore::new(tempdir.path()));

    engine = engine
        .with_subagent_registry(std::sync::Arc::new(reg))
        .with_subagent_run_store(store.clone());

    let args = serde_json::json!({"agent": "scout", "objective": "look around"});
    let _result = engine.handle_delegate_task(args).await;

    // Run store must have persisted the run
    let runs = store.list().unwrap();
    assert_eq!(runs.len(), 1, "registry-resolved spawn must persist");
}

// ── Handoff guardrails integration ──

#[tokio::test]
async fn delegate_task_redirects_when_explorer_asked_to_implement() {
    // Built-in `ReadOnlyAgentMustNotMutate` now redirects (instead of
    // blocking): explorer is read-only, "implement" is a mutation verb,
    // therefore the spawn should target `implementer` and the result
    // should carry a `[handoff redirected …]` prefix.
    let setup = TestSetup::new();
    let reg = crate::subagent::SubAgentRegistry::with_builtins();
    let _ = reg.get("explorer").expect("explorer builtin must exist");
    let engine = setup
        .create_engine("test")
        .with_subagent_registry(std::sync::Arc::new(reg));

    let mut engine = engine;
    let args = serde_json::json!({
        "agent": "explorer",
        "objective": "implement caching layer"
    });
    let result = engine.handle_delegate_task(args).await;
    assert!(
        result.contains("handoff redirected"),
        "expected redirect prefix, got: {}",
        result
    );
    assert!(
        result.contains("implementer"),
        "expected redirect target name in result, got: {}",
        result
    );
}

#[tokio::test]
async fn delegate_task_redirect_emits_handoff_evaluated_with_decision_redirect() {
    use crate::event_bus::EventListener;
    use std::sync::Mutex;
    use theo_domain::event::{DomainEvent, EventType};

    struct Capture(Mutex<Vec<DomainEvent>>);
    impl EventListener for Capture {
        fn on_event(&self, e: &DomainEvent) {
            self.0.lock().unwrap().push(e.clone());
        }
    }

    let setup = TestSetup::new();
    let capture = std::sync::Arc::new(Capture(Mutex::new(Vec::new())));
    setup
        .bus
        .subscribe(capture.clone() as std::sync::Arc<dyn EventListener>);
    let mut engine = setup.create_engine("test");
    let args = serde_json::json!({
        "agent": "explorer",
        "objective": "implement evil mutation"
    });
    let _ = engine.handle_delegate_task(args).await;
    let events = capture.0.lock().unwrap().clone();
    let evt = events
        .iter()
        .find(|e| e.event_type == EventType::HandoffEvaluated)
        .expect("HandoffEvaluated must be emitted");
    assert_eq!(
        evt.payload.get("decision").and_then(|v| v.as_str()),
        Some("redirect"),
        "decision label must be redirect; payload={}",
        evt.payload
    );
    assert_eq!(
        evt.payload.get("redirect_to").and_then(|v| v.as_str()),
        Some("implementer")
    );
}

#[tokio::test]
async fn delegate_task_rewrite_uses_new_objective() {
    use crate::handoff_guardrail::{
        GuardrailChain, GuardrailDecision, HandoffContext, HandoffGuardrail,
    };

    #[derive(Debug)]
    struct ScopeRewriter;
    impl HandoffGuardrail for ScopeRewriter {
        fn id(&self) -> &str { "test.scope_rewriter" }
        fn evaluate(&self, _ctx: &HandoffContext<'_>) -> GuardrailDecision {
            GuardrailDecision::RewriteObjective {
                new_objective: "scoped: list crates only".into(),
            }
        }
    }

    let setup = TestSetup::new();
    let mut chain = GuardrailChain::new();
    chain.add(std::sync::Arc::new(ScopeRewriter));
    let mut engine = setup
        .create_engine("test")
        .with_subagent_handoff_guardrails(std::sync::Arc::new(chain));
    let args = serde_json::json!({
        "agent": "explorer",
        "objective": "list everything in the universe"
    });
    let result = engine.handle_delegate_task(args).await;
    assert!(
        result.contains("handoff objective rewritten"),
        "expected rewrite prefix, got: {}",
        result
    );
    assert!(
        result.contains("test.scope_rewriter"),
        "expected guardrail id in prefix, got: {}",
        result
    );
}

#[tokio::test]
async fn delegate_task_block_keeps_returning_refusal_when_chain_blocks() {
    use crate::handoff_guardrail::{
        GuardrailChain, GuardrailDecision, HandoffContext, HandoffGuardrail,
    };

    #[derive(Debug)]
    struct AlwaysBlock;
    impl HandoffGuardrail for AlwaysBlock {
        fn id(&self) -> &str { "test.always_block" }
        fn evaluate(&self, _ctx: &HandoffContext<'_>) -> GuardrailDecision {
            GuardrailDecision::Block { reason: "policy".into() }
        }
    }

    let setup = TestSetup::new();
    let mut chain = GuardrailChain::new();
    chain.add(std::sync::Arc::new(AlwaysBlock));
    let mut engine = setup
        .create_engine("test")
        .with_subagent_handoff_guardrails(std::sync::Arc::new(chain));
    let args = serde_json::json!({
        "agent": "implementer",
        "objective": "anything"
    });
    let result = engine.handle_delegate_task(args).await;
    assert!(result.contains("handoff refused"), "got: {}", result);
    assert!(result.contains("test.always_block"), "got: {}", result);
}

#[tokio::test]
async fn delegate_task_allowed_when_implementer_asked_to_implement() {
    let setup = TestSetup::new();
    let mut engine = setup.create_engine("test");
    let args = serde_json::json!({
        "agent": "implementer",
        "objective": "implement caching layer"
    });
    let result = engine.handle_delegate_task(args).await;
    assert!(
        !result.contains("handoff refused"),
        "implementer must be allowed; got: {}",
        result
    );
}

#[tokio::test]
async fn delegate_task_emits_handoff_evaluated_event_with_block_payload() {
    use crate::event_bus::EventListener;
    use crate::handoff_guardrail::{
        GuardrailChain, GuardrailDecision, HandoffContext, HandoffGuardrail,
    };
    use std::sync::Mutex;
    use theo_domain::event::{DomainEvent, EventType};

    struct Capture(Mutex<Vec<DomainEvent>>);
    impl EventListener for Capture {
        fn on_event(&self, e: &DomainEvent) {
            self.0.lock().unwrap().push(e.clone());
        }
    }

    #[derive(Debug)]
    struct AlwaysBlock;
    impl HandoffGuardrail for AlwaysBlock {
        fn id(&self) -> &str { "test.always_block_for_audit" }
        fn evaluate(&self, _ctx: &HandoffContext<'_>) -> GuardrailDecision {
            GuardrailDecision::Block {
                reason: "audit-test".into(),
            }
        }
    }

    let setup = TestSetup::new();
    let capture = std::sync::Arc::new(Capture(Mutex::new(Vec::new())));
    setup
        .bus
        .subscribe(capture.clone() as std::sync::Arc<dyn EventListener>);
    let mut chain = GuardrailChain::new();
    chain.add(std::sync::Arc::new(AlwaysBlock));
    let mut engine = setup
        .create_engine("test")
        .with_subagent_handoff_guardrails(std::sync::Arc::new(chain));
    let args = serde_json::json!({
        "agent": "explorer",
        "objective": "anything"
    });
    let _ = engine.handle_delegate_task(args).await;
    let events = capture.0.lock().unwrap().clone();
    let evt = events
        .iter()
        .find(|e| e.event_type == EventType::HandoffEvaluated)
        .expect("HandoffEvaluated must be emitted");
    assert_eq!(
        evt.payload.get("decision").and_then(|v| v.as_str()),
        Some("block")
    );
    assert!(
        evt.payload
            .get("blocked_by")
            .and_then(|v| v.as_str())
            .is_some()
    );
}

#[test]
fn delegate_task_excluded_from_subagent_tools() {
    let registry = theo_tooling::registry::create_default_registry();
    let defs = crate::tool_bridge::registry_to_definitions_for_subagent(&registry);
    let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
    assert!(
        !names.contains(&"delegate_task"),
        "sub-agents must NOT see delegate_task (no recursive delegation)"
    );
}

// -----------------------------------------------------------------------
// Resume-runtime-wiring dispatch wiring
// -----------------------------------------------------------------------

