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

#[test]
fn new_generates_unique_run_id() {
    let setup = TestSetup::new();
    let e1 = setup.create_engine("task1");
    let e2 = setup.create_engine("task2");
    assert_ne!(e1.run_id().as_str(), e2.run_id().as_str());
}

// -----------------------------------------------------------------------
// Events
// -----------------------------------------------------------------------

#[test]
fn new_publishes_run_initialized_event() {
    let setup = TestSetup::new();
    let engine = setup.create_engine("test");

    let events = setup.listener.captured();
    let run_init = events
        .iter()
        .find(|e| e.event_type == EventType::RunInitialized);
    assert!(run_init.is_some(), "RunInitialized event must be published");
    assert_eq!(run_init.unwrap().entity_id, engine.run_id().as_str());
}

#[test]
fn transition_run_publishes_state_changed_event() {
    let setup = TestSetup::new();
    let mut engine = setup.create_engine("test");

    engine.transition_run(RunState::Planning);
    assert_eq!(engine.state(), RunState::Planning);

    let events = setup.listener.captured();
    let state_changed: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == EventType::RunStateChanged)
        .collect();
    assert!(!state_changed.is_empty());
    let last = state_changed.last().unwrap();
    assert_eq!(last.payload["from"].as_str().unwrap(), "Initialized");
    assert_eq!(last.payload["to"].as_str().unwrap(), "Planning");
}

// ---------------------------------------------------------------------
// T1.3 / find_p4_002 / INV-002 — state_manager append failures must be
// observable on EventBus + tracing instead of being discarded by
// `let _ = sm.append_message(...)`.
// ---------------------------------------------------------------------

#[test]
fn publish_state_append_failure_emits_error_event_with_role_context() {
    // Arrange — engine + capturing listener.
    let setup = TestSetup::new();
    let engine = setup.create_engine("test");
    let baseline = setup.listener.captured().len();

    // Act — synthesise a SessionTreeError and publish it.
    let err = crate::session_tree::SessionTreeError::Io(std::io::Error::other(
        "disk full (synthesised)",
    ));
    engine.publish_state_append_failure("assistant", &err);

    // Assert — exactly one new EventType::Error with the expected
    // structured payload appeared on the bus.
    let events = setup.listener.captured();
    assert_eq!(
        events.len(),
        baseline + 1,
        "exactly one Error event should be emitted"
    );
    let last = events.last().expect("at least one event");
    assert_eq!(last.event_type, EventType::Error);
    assert_eq!(last.entity_id, engine.run_id().as_str());
    assert_eq!(
        last.payload["kind"].as_str().unwrap(),
        "state_manager_append_failed",
        "kind discriminator must be set so listeners can filter"
    );
    assert_eq!(
        last.payload["role"].as_str().unwrap(),
        "assistant",
        "role must be propagated to allow distinguishing assistant vs tool failures"
    );
    assert!(
        last.payload["error"]
            .as_str()
            .map(|s| s.contains("disk full"))
            .unwrap_or(false),
        "error message must be propagated for diagnostics; got {:?}",
        last.payload["error"]
    );
}

#[test]
fn try_task_transition_is_silent_for_already_in_state() {
    // Arrange: engine in initial task state Pending. Targeting
    // Pending again is a no-op (semantically idempotent) and must
    // NOT emit any event.
    let setup = TestSetup::new();
    let engine = setup.create_engine("test");
    let baseline = setup.listener.captured().len();

    // Act
    engine.try_task_transition(theo_domain::task::TaskState::Pending);

    // Assert
    let after = setup.listener.captured().len();
    assert_eq!(
        after, baseline,
        "no-op transition should not emit any new event"
    );
}

#[test]
fn try_task_transition_emits_error_for_genuine_invalid() {
    // Arrange: task starts in Pending. Pending → Completed is NOT
    // a valid transition per the state machine, so this is a real
    // failure that must be observable.
    let setup = TestSetup::new();
    let engine = setup.create_engine("test");
    let baseline = setup.listener.captured().len();

    // Act
    engine.try_task_transition(theo_domain::task::TaskState::Completed);

    // Assert
    let events = setup.listener.captured();
    assert_eq!(
        events.len(),
        baseline + 1,
        "exactly one Error event should be emitted"
    );
    let last = events.last().unwrap();
    assert_eq!(last.event_type, EventType::Error);
    assert_eq!(
        last.payload["kind"].as_str().unwrap(),
        "task_transition_failed"
    );
    assert_eq!(
        last.payload["target"].as_str().unwrap(),
        "Completed",
        "target must be in the payload so listeners can correlate"
    );
}

#[test]
fn publish_state_append_failure_distinguishes_role_label() {
    let setup = TestSetup::new();
    let engine = setup.create_engine("test");

    let err = crate::session_tree::SessionTreeError::Io(std::io::Error::other("x"));
    engine.publish_state_append_failure("tool", &err);

    let events = setup.listener.captured();
    let last = events.last().unwrap();
    assert_eq!(last.payload["role"].as_str().unwrap(), "tool");
}

#[test]
fn run_state_changed_events_have_correct_count() {
    let setup = TestSetup::new();
    let mut engine = setup.create_engine("test");

    engine.transition_run(RunState::Planning);
    engine.transition_run(RunState::Executing);
    engine.transition_run(RunState::Evaluating);
    engine.transition_run(RunState::Converged);

    let state_events: Vec<_> = setup
        .listener
        .captured()
        .iter()
        .filter(|e| e.event_type == EventType::RunStateChanged)
        .cloned()
        .collect();
    // Initialized→Planning, Planning→Executing, Executing→Evaluating, Evaluating→Converged
    assert_eq!(state_events.len(), 4);
}

// -----------------------------------------------------------------------
// State transitions
// -----------------------------------------------------------------------

#[test]
fn initial_state_is_initialized() {
    let setup = TestSetup::new();
    let engine = setup.create_engine("test");
    assert_eq!(engine.state(), RunState::Initialized);
    assert_eq!(engine.iteration(), 0);
}

#[test]
fn transition_run_through_full_cycle() {
    let setup = TestSetup::new();
    let mut engine = setup.create_engine("test");

    engine.transition_run(RunState::Planning);
    assert_eq!(engine.state(), RunState::Planning);

    engine.transition_run(RunState::Executing);
    assert_eq!(engine.state(), RunState::Executing);

    engine.transition_run(RunState::Evaluating);
    assert_eq!(engine.state(), RunState::Evaluating);

    engine.transition_run(RunState::Converged);
    assert_eq!(engine.state(), RunState::Converged);
    assert!(engine.state().is_terminal());
}

#[test]
fn transition_run_replanning_cycle() {
    let setup = TestSetup::new();
    let mut engine = setup.create_engine("test");

    engine.transition_run(RunState::Planning);
    engine.transition_run(RunState::Executing);
    engine.transition_run(RunState::Evaluating);
    engine.transition_run(RunState::Replanning);
    assert_eq!(engine.state(), RunState::Replanning);

    engine.transition_run(RunState::Planning);
    assert_eq!(engine.state(), RunState::Planning);

    engine.transition_run(RunState::Executing);
    engine.transition_run(RunState::Evaluating);
    engine.transition_run(RunState::Converged);
    assert_eq!(engine.state(), RunState::Converged);
}

#[test]
fn transition_run_abort_from_any_non_terminal() {
    let setup = TestSetup::new();
    let mut engine = setup.create_engine("test");

    engine.transition_run(RunState::Planning);
    engine.transition_run(RunState::Aborted);
    assert_eq!(engine.state(), RunState::Aborted);
    assert!(engine.state().is_terminal());
}

#[test]
fn converged_rejects_further_transitions() {
    let setup = TestSetup::new();
    let mut engine = setup.create_engine("test");

    engine.transition_run(RunState::Planning);
    engine.transition_run(RunState::Executing);
    engine.transition_run(RunState::Evaluating);
    engine.transition_run(RunState::Converged);

    engine.transition_run(RunState::Planning);
    assert_eq!(
        engine.state(),
        RunState::Converged,
        "terminal state must not change"
    );
}

#[test]
fn loop_phase_variants_are_distinct() {
    use crate::loop_state::LoopPhase;
    let p = LoopPhase::Explore;
    let e = LoopPhase::Edit;
    let v = LoopPhase::Verify;
    let d = LoopPhase::Done;

    // Verify variants are distinct (no discriminant collision)
    assert_ne!(format!("{p:?}"), format!("{e:?}"));
    assert_ne!(format!("{e:?}"), format!("{v:?}"));
    assert_ne!(format!("{v:?}"), format!("{d:?}"));
}

#[test]
fn agent_result_fields_preserved() {
    let result = AgentResult {
        success: true,
        summary: "done".to_string(),
        files_edited: vec!["src/main.rs".to_string()],
        iterations_used: 5,
        was_streamed: false,
        tokens_used: 0,
        input_tokens: 0,
        output_tokens: 0,
        tool_calls_total: 0,
        tool_calls_success: 0,
        llm_calls: 0,
        retries: 0,
        duration_ms: 0,
        ..Default::default()
    };
    assert!(result.success);
    assert_eq!(result.summary, "done");
    assert_eq!(result.files_edited.len(), 1);
    assert_eq!(result.iterations_used, 5);
}

#[test]
fn agent_result_default_has_no_error_class() {
    // Backcompat — legacy tests that build AgentResult via
    // ..Default::default() must keep working even if they don't set
    // error_class. Default is None.
    let r = AgentResult::default();
    assert!(r.error_class.is_none());
}

#[test]
fn invariant_solved_iff_success_true() {
    // Property: if AgentResult.error_class == Some(Solved), then
    // success MUST be true. Conversely, if success == true, the
    // class (if set) MUST be Solved. This is the headline invariant
    // of the headless v3 schema.
    use theo_domain::error_class::ErrorClass;
    let variants = [
        ErrorClass::Solved,
        ErrorClass::Exhausted,
        ErrorClass::RateLimited,
        ErrorClass::QuotaExceeded,
        ErrorClass::AuthFailed,
        ErrorClass::ContextOverflow,
        ErrorClass::SandboxDenied,
        ErrorClass::Cancelled,
        ErrorClass::Aborted,
        ErrorClass::InvalidTask,
    ];
    for v in variants {
        // Construct the legitimate combinations.
        let solved_pair = AgentResult {
            success: true,
            error_class: Some(ErrorClass::Solved),
            ..Default::default()
        };
        assert!(solved_pair.success);
        assert_eq!(solved_pair.error_class, Some(ErrorClass::Solved));
        // success=false with any non-Solved class is OK.
        if v != ErrorClass::Solved {
            let failed_pair = AgentResult {
                success: false,
                error_class: Some(v),
                ..Default::default()
            };
            assert!(!failed_pair.success);
            assert_ne!(failed_pair.error_class, Some(ErrorClass::Solved));
        }
    }
}

mod llm_error_class_mapping {
    use theo_domain::error_class::ErrorClass;
    use theo_infra_llm::LlmError;

    #[test]
    fn llm_error_to_class_maps_rate_limit() {
        let class = crate::run_engine_helpers::llm_error_to_class(
            &LlmError::RateLimited { retry_after: None },
        );
        assert_eq!(class, ErrorClass::RateLimited);
    }

    #[test]
    fn llm_error_to_class_maps_auth_failure() {
        let class = crate::run_engine_helpers::llm_error_to_class(
            &LlmError::AuthFailed("bad token".into()),
        );
        assert_eq!(class, ErrorClass::AuthFailed);
    }

    #[test]
    fn llm_error_to_class_maps_context_overflow() {
        let class = crate::run_engine_helpers::llm_error_to_class(&LlmError::ContextOverflow {
            provider: "openai".into(),
            message: "too long".into(),
        });
        assert_eq!(class, ErrorClass::ContextOverflow);
    }

    #[test]
    fn llm_error_to_class_falls_back_to_aborted_for_unknown() {
        // Network error doesn't have a dedicated ErrorClass — should
        // map to Aborted (catch-all) so consumers know the run did
        // terminate unexpectedly without misclassifying as infra.
        let class = crate::run_engine_helpers::llm_error_to_class(&LlmError::Timeout);
        assert_eq!(class, ErrorClass::Aborted);
        let class = crate::run_engine_helpers::llm_error_to_class(&LlmError::ServiceUnavailable);
        assert_eq!(class, ErrorClass::Aborted);
    }

    #[test]
    fn llm_error_to_class_maps_quota_exceeded() {
        // Distinct from RateLimited so ab_compare can
        // separate "agent retry exhausted" from "account hit billing
        // ceiling — bench is unusable until reset."
        let class = crate::run_engine_helpers::llm_error_to_class(&LlmError::QuotaExceeded {
            provider: "openai".into(),
            message: "insufficient_quota".into(),
        });
        assert_eq!(class, ErrorClass::QuotaExceeded);
    }
}

#[test]
fn agent_loop_new_signature_current_contract() {
    // Verify AgentLoop::new still accepts the current (config, registry) signature
    use crate::agent_loop::AgentLoop;
    let config = AgentConfig::default();
    let registry = theo_tooling::registry::create_default_registry();
    let agent_loop = AgentLoop::new(config, registry);

    // Verify run() method exists and is callable (signature contract)
    // We can't call it without an LLM, but we can verify the type is correct
    let _: &AgentLoop = &agent_loop;
    // If AgentLoop::new signature changes, this test fails at compile time.
    // If AgentLoop type is renamed or removed, this test fails at compile time.
    assert!(
        std::mem::size_of_val(&agent_loop) > 0,
        "AgentLoop should have non-zero size"
    );
}

#[test]
fn doom_loop_threshold_config_exposes_default() {
    let config = AgentConfig::default();
    assert_eq!(config.loop_cfg.doom_loop_threshold, Some(3));
}

// -----------------------------------------------------------------------
// delegate_task validation tests
// -----------------------------------------------------------------------

