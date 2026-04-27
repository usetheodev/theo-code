//! Projection layer — transforms a stream of trajectory envelopes into a
//! normalized list of `ProjectedStep`s plus an IntegrityReport.
//!
//! Properties (from the ADR):
//! - **P1** (determinism): same input → same output (except random id).
//! - **P2** (idempotence): project → serialize → deserialize → project
//!   yields identical fields.
//! - **P3** (tolerance): missing events degrade confidence, never panic.
//! - **P4** (out-of-order): events are sorted by (timestamp, sequence).

use serde::{Deserialize, Serialize};

use theo_domain::event::EventKind;
use theo_domain::identifiers::TrajectoryId;

use crate::observability::envelope::{EnvelopeKind, TrajectoryEnvelope};
use crate::observability::reader::IntegrityReport;

/// Outcome of a projected step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StepOutcome {
    Success,
    Failure { retryable: bool },
    Timeout,
    Skipped,
}

/// A normalized step in a trajectory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectedStep {
    pub sequence: u64,
    pub event_type: String,
    pub event_kind: Option<EventKind>,
    pub timestamp: u64,
    pub entity_id: String,
    pub payload_summary: String,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub outcome: Option<StepOutcome>,
}

/// A deterministic projection of a run's events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryProjection {
    pub run_id: String,
    pub trajectory_id: String,
    pub steps: Vec<ProjectedStep>,
    pub integrity: IntegrityReport,
}

const PAYLOAD_SUMMARY_MAX: usize = 500;

fn truncate_payload(v: &serde_json::Value) -> String {
    let s = match v {
        serde_json::Value::String(s) => s.clone(),
        _ => v.to_string(),
    };
    if s.len() > PAYLOAD_SUMMARY_MAX {
        format!("{}…", &s[..PAYLOAD_SUMMARY_MAX])
    } else {
        s
    }
}

fn extract_tool_name(payload: &serde_json::Value) -> Option<String> {
    payload
        .get("tool_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| payload.get("name").and_then(|v| v.as_str()).map(|s| s.to_string()))
}

fn extract_outcome(event_type: &str, payload: &serde_json::Value) -> Option<StepOutcome> {
    match event_type {
        "ToolCallCompleted" => {
            let state = payload
                .get("state")
                .and_then(|v| v.as_str())
                .unwrap_or("Succeeded");
            match state {
                "Succeeded" | "Success" => Some(StepOutcome::Success),
                "Failed" => Some(StepOutcome::Failure {
                    retryable: payload
                        .get("retryable")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                }),
                "Timeout" => Some(StepOutcome::Timeout),
                "Cancelled" | "Skipped" => Some(StepOutcome::Skipped),
                _ => Some(StepOutcome::Success),
            }
        }
        _ => None,
    }
}

/// Produce a `TrajectoryProjection` from envelopes and an integrity report.
///
/// This function is pure — no I/O, no randomness besides `TrajectoryId::generate`
/// which is only used for the top-level `trajectory_id` field. All other fields
/// are deterministic over the input.
pub fn project(
    run_id: &str,
    envelopes: Vec<TrajectoryEnvelope>,
    integrity: IntegrityReport,
) -> TrajectoryProjection {
    // Filter event-kind envelopes and sort by (timestamp, sequence).
    let mut events: Vec<TrajectoryEnvelope> = envelopes
        .into_iter()
        .filter(|e| matches!(e.kind, EnvelopeKind::Event))
        .collect();
    events.sort_by_key(|e| (e.ts, e.seq));

    // Build steps and pair durations between ToolCallQueued/Dispatched and
    // ToolCallCompleted on the same entity_id.
    let mut start_by_entity: std::collections::HashMap<String, u64> =
        std::collections::HashMap::new();
    let mut steps = Vec::with_capacity(events.len());

    for env in &events {
        let entity_id = env.entity_id.clone().unwrap_or_default();
        let event_type = env.event_type.clone().unwrap_or_default();
        let payload_summary = truncate_payload(&env.payload);
        let tool_name = extract_tool_name(&env.payload);
        let outcome = extract_outcome(&event_type, &env.payload);

        // Track start timestamps for tool-call pairing.
        if event_type == "ToolCallQueued" || event_type == "ToolCallDispatched" {
            start_by_entity.entry(entity_id.clone()).or_insert(env.ts);
        }

        let duration_ms = if event_type == "ToolCallCompleted" {
            start_by_entity.remove(&entity_id).map(|start| env.ts.saturating_sub(start))
        } else {
            None
        };

        steps.push(ProjectedStep {
            sequence: env.seq,
            event_type,
            event_kind: env.event_kind,
            timestamp: env.ts,
            entity_id,
            payload_summary,
            duration_ms,
            tool_name,
            outcome,
        });
    }

    TrajectoryProjection {
        run_id: run_id.to_string(),
        trajectory_id: TrajectoryId::generate().as_str().to_string(),
        steps,
        integrity,
    }
}

/// Build a stable projection (no random trajectory_id) — for tests and deterministic output.
pub fn project_with_id(
    run_id: &str,
    trajectory_id: &str,
    envelopes: Vec<TrajectoryEnvelope>,
    integrity: IntegrityReport,
) -> TrajectoryProjection {
    let mut p = project(run_id, envelopes, integrity);
    p.trajectory_id = trajectory_id.to_string();
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observability::envelope::{EnvelopeKind, TrajectoryEnvelope};
    use theo_domain::event::EventKind;

    fn event_env(seq: u64, ts: u64, et: &str, entity: &str, payload: serde_json::Value) -> TrajectoryEnvelope {
        TrajectoryEnvelope {
            v: 1,
            seq,
            ts,
            run_id: "r".into(),
            kind: EnvelopeKind::Event,
            event_type: Some(et.into()),
            event_kind: Some(EventKind::Tooling),
            entity_id: Some(entity.into()),
            payload,
            dropped_since_last: 0,
        }
    }

    fn empty_integrity() -> IntegrityReport {
        IntegrityReport::default()
    }

    #[test]
    fn test_projected_step_serde_roundtrip() {
        let s = ProjectedStep {
            sequence: 1,
            event_type: "ToolCallCompleted".into(),
            event_kind: Some(EventKind::Tooling),
            timestamp: 0,
            entity_id: "e".into(),
            payload_summary: "p".into(),
            duration_ms: Some(50),
            tool_name: Some("read".into()),
            outcome: Some(StepOutcome::Success),
        };
        let j = serde_json::to_string(&s).unwrap();
        let back: ProjectedStep = serde_json::from_str(&j).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn test_step_outcome_all_variants_serialize() {
        for o in &[
            StepOutcome::Success,
            StepOutcome::Failure { retryable: true },
            StepOutcome::Timeout,
            StepOutcome::Skipped,
        ] {
            let j = serde_json::to_string(o).unwrap();
            let back: StepOutcome = serde_json::from_str(&j).unwrap();
            assert_eq!(*o, back);
        }
    }

    #[test]
    fn test_trajectory_projection_serde_roundtrip() {
        let p = TrajectoryProjection {
            run_id: "r".into(),
            trajectory_id: "t".into(),
            steps: vec![],
            integrity: empty_integrity(),
        };
        let j = serde_json::to_string(&p).unwrap();
        let back: TrajectoryProjection = serde_json::from_str(&j).unwrap();
        assert_eq!(p.run_id, back.run_id);
    }

    #[test]
    fn test_payload_summary_truncated_at_500_chars() {
        let big = "x".repeat(1000);
        let env = event_env(0, 0, "X", "e", serde_json::Value::String(big.clone()));
        let proj = project("r", vec![env], empty_integrity());
        // 500 chars + 1 ellipsis char
        assert!(proj.steps[0].payload_summary.chars().count() <= 501);
    }

    #[test]
    fn test_projection_deterministic() {
        let events = vec![
            event_env(0, 10, "RunInitialized", "run", serde_json::json!({})),
            event_env(1, 20, "ToolCallCompleted", "c1", serde_json::json!({"state": "Succeeded"})),
        ];
        let p1 = project_with_id("r", "t", events.clone(), empty_integrity());
        let p2 = project_with_id("r", "t", events.clone(), empty_integrity());
        assert_eq!(p1.steps, p2.steps);
    }

    #[test]
    fn test_projection_sorts_by_timestamp_then_sequence() {
        let events = vec![
            event_env(2, 30, "A", "e", serde_json::json!({})),
            event_env(0, 10, "B", "e", serde_json::json!({})),
            event_env(1, 20, "C", "e", serde_json::json!({})),
        ];
        let p = project("r", events, empty_integrity());
        assert_eq!(p.steps[0].timestamp, 10);
        assert_eq!(p.steps[1].timestamp, 20);
        assert_eq!(p.steps[2].timestamp, 30);
    }

    #[test]
    fn test_projection_extracts_tool_name_from_tool_call_events() {
        let ev = event_env(0, 0, "ToolCallCompleted", "c1", serde_json::json!({"tool_name": "bash", "state": "Succeeded"}));
        let p = project("r", vec![ev], empty_integrity());
        assert_eq!(p.steps[0].tool_name.as_deref(), Some("bash"));
    }

    #[test]
    fn test_projection_computes_duration_for_tool_calls() {
        let events = vec![
            event_env(0, 100, "ToolCallQueued", "c1", serde_json::json!({})),
            event_env(1, 250, "ToolCallCompleted", "c1", serde_json::json!({"state": "Succeeded"})),
        ];
        let p = project("r", events, empty_integrity());
        assert_eq!(p.steps[1].duration_ms, Some(150));
    }

    #[test]
    fn test_projection_maps_step_outcome_from_tool_state() {
        let ok = event_env(0, 0, "ToolCallCompleted", "a", serde_json::json!({"state":"Succeeded"}));
        let fail = event_env(1, 1, "ToolCallCompleted", "b", serde_json::json!({"state":"Failed"}));
        let to = event_env(2, 2, "ToolCallCompleted", "c", serde_json::json!({"state":"Timeout"}));
        let p = project("r", vec![ok, fail, to], empty_integrity());
        assert_eq!(p.steps[0].outcome, Some(StepOutcome::Success));
        assert!(matches!(p.steps[1].outcome, Some(StepOutcome::Failure { .. })));
        assert_eq!(p.steps[2].outcome, Some(StepOutcome::Timeout));
    }

    #[test]
    fn test_projection_empty_events_returns_empty_steps() {
        let p = project("r", vec![], empty_integrity());
        assert!(p.steps.is_empty());
    }

    #[test]
    fn test_projection_idempotent_through_serde() {
        let events = vec![
            event_env(0, 0, "RunInitialized", "run", serde_json::json!({})),
            event_env(1, 1, "ToolCallCompleted", "c1", serde_json::json!({"state": "Succeeded"})),
        ];
        let p = project_with_id("r", "t", events, empty_integrity());
        let j = serde_json::to_string(&p).unwrap();
        let back: TrajectoryProjection = serde_json::from_str(&j).unwrap();
        assert_eq!(p.steps, back.steps);
    }

    // --- T2.4: proptest idempotence (P2) ---

    use proptest::prelude::*;

    fn arb_event_env() -> impl Strategy<Value = TrajectoryEnvelope> {
        (
            0u64..1000,
            0u64..100_000,
            prop::sample::select(vec![
                "RunInitialized",
                "ToolCallQueued",
                "ToolCallCompleted",
                "LlmCallStart",
                "HypothesisFormed",
            ]),
            "[a-z]{1,10}",
            prop::sample::select(vec!["Succeeded", "Failed", "Timeout"]),
        )
            .prop_map(|(seq, ts, et, entity, state)| {
                event_env(
                    seq,
                    ts,
                    et,
                    &entity,
                    serde_json::json!({"state": state, "tool_name": "bash"}),
                )
            })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]

        #[test]
        fn prop_projection_deterministic(events in prop::collection::vec(arb_event_env(), 0..20)) {
            let p1 = project_with_id("r", "t", events.clone(), empty_integrity());
            let p2 = project_with_id("r", "t", events, empty_integrity());
            prop_assert_eq!(p1.steps, p2.steps);
        }

        #[test]
        fn prop_projection_idempotent(events in prop::collection::vec(arb_event_env(), 0..20)) {
            let p = project_with_id("r", "t", events, empty_integrity());
            let j = serde_json::to_string(&p).unwrap();
            let back: TrajectoryProjection = serde_json::from_str(&j).unwrap();
            prop_assert_eq!(p.steps, back.steps);
            prop_assert_eq!(p.run_id, back.run_id);
        }
    }
}
