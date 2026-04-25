//! Trajectory envelope — schema versioned JSONL line format.
//!
//! Each line written to `.theo/trajectories/{run_id}.jsonl` is a JSON object
//! with a fixed envelope providing: schema version, sequence number,
//! timestamp, run id, kind discriminator, and structured payload.

use serde::{Deserialize, Serialize};

use theo_domain::event::{DomainEvent, EventKind};

/// Current schema version for the trajectory JSONL format.
pub const ENVELOPE_SCHEMA_VERSION: u32 = 1;

/// Kind discriminator for trajectory lines. Allows a single JSONL to mix
/// events, drop sentinels, writer-recovery markers, and summary lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvelopeKind {
    Event,
    DropSentinel,
    WriterRecovered,
    Summary,
}

/// Wire format used to write a single line into the trajectory JSONL.
///
/// Fields are tagged so that we can parse lines regardless of the kind
/// without knowing the payload shape upfront.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryEnvelope {
    /// Schema version of the envelope format.
    pub v: u32,
    /// Monotonic sequence number per-run.
    pub seq: u64,
    /// Unix timestamp in milliseconds.
    pub ts: u64,
    /// The run this envelope belongs to.
    pub run_id: String,
    /// Discriminator for the kind of line (event, sentinel, summary...).
    pub kind: EnvelopeKind,
    /// Event type name (present only for event kinds).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub event_type: Option<String>,
    /// High-level event classification (present only for event kinds).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub event_kind: Option<EventKind>,
    /// Entity the event refers to (present only for event kinds).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub entity_id: Option<String>,
    /// Structured payload — shape depends on `kind`.
    pub payload: serde_json::Value,
    /// Number of events dropped since the last written envelope.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub dropped_since_last: u64,
}

fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}

impl TrajectoryEnvelope {
    /// Build an `Event` envelope from a DomainEvent.
    pub fn from_event(event: &DomainEvent, run_id: &str, seq: u64) -> Self {
        Self {
            v: ENVELOPE_SCHEMA_VERSION,
            seq,
            ts: event.timestamp,
            run_id: run_id.to_string(),
            kind: EnvelopeKind::Event,
            event_type: Some(format!("{}", event.event_type)),
            event_kind: Some(event.event_type.kind()),
            entity_id: Some(event.entity_id.clone()),
            payload: event.payload.clone(),
            dropped_since_last: 0,
        }
    }

    /// Build a `DropSentinel` envelope reporting dropped events.
    pub fn drop_sentinel(run_id: &str, seq: u64, ts: u64, dropped_count: u64) -> Self {
        Self {
            v: ENVELOPE_SCHEMA_VERSION,
            seq,
            ts,
            run_id: run_id.to_string(),
            kind: EnvelopeKind::DropSentinel,
            event_type: None,
            event_kind: None,
            entity_id: None,
            payload: serde_json::json!({ "dropped_count": dropped_count }),
            dropped_since_last: 0,
        }
    }

    /// Build a `WriterRecovered` envelope after flushing retry queue.
    pub fn writer_recovered(
        run_id: &str,
        seq: u64,
        ts: u64,
        buffered_events: u64,
        error: &str,
    ) -> Self {
        Self {
            v: ENVELOPE_SCHEMA_VERSION,
            seq,
            ts,
            run_id: run_id.to_string(),
            kind: EnvelopeKind::WriterRecovered,
            event_type: None,
            event_kind: None,
            entity_id: None,
            payload: serde_json::json!({
                "buffered_events": buffered_events,
                "error": error,
            }),
            dropped_since_last: 0,
        }
    }

    /// Build a `Summary` envelope — written as last line of each JSONL.
    pub fn summary(run_id: &str, seq: u64, ts: u64, payload: serde_json::Value) -> Self {
        Self {
            v: ENVELOPE_SCHEMA_VERSION,
            seq,
            ts,
            run_id: run_id.to_string(),
            kind: EnvelopeKind::Summary,
            event_type: None,
            event_kind: None,
            entity_id: None,
            payload,
            dropped_since_last: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theo_domain::event::{DomainEvent, EventType};

    #[test]
    fn envelope_from_event_has_required_fields() {
        let e = DomainEvent::new(EventType::ToolCallCompleted, "call-1", serde_json::json!({}));
        let env = TrajectoryEnvelope::from_event(&e, "run-xyz", 0);
        assert_eq!(env.v, 1);
        assert_eq!(env.seq, 0);
        assert_eq!(env.run_id, "run-xyz");
        assert_eq!(env.kind, EnvelopeKind::Event);
        assert_eq!(env.event_type.as_deref(), Some("ToolCallCompleted"));
        assert_eq!(env.event_kind, Some(EventKind::Tooling));
    }

    #[test]
    fn envelope_serde_roundtrip() {
        let e = DomainEvent::new(EventType::RunInitialized, "run-1", serde_json::json!({"x": 1}));
        let env = TrajectoryEnvelope::from_event(&e, "run-1", 3);
        let json = serde_json::to_string(&env).unwrap();
        let back: TrajectoryEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(back.seq, 3);
        assert_eq!(back.kind, EnvelopeKind::Event);
    }

    #[test]
    fn drop_sentinel_envelope_has_dropped_count() {
        let env = TrajectoryEnvelope::drop_sentinel("run-1", 5, 1000, 42);
        assert_eq!(env.kind, EnvelopeKind::DropSentinel);
        assert_eq!(env.payload["dropped_count"], 42);
    }

    #[test]
    fn writer_recovered_envelope_carries_error() {
        let env = TrajectoryEnvelope::writer_recovered("run-1", 7, 2000, 12, "disk full");
        assert_eq!(env.kind, EnvelopeKind::WriterRecovered);
        assert_eq!(env.payload["buffered_events"], 12);
        assert_eq!(env.payload["error"], "disk full");
    }

    #[test]
    fn summary_envelope_has_summary_kind() {
        let env = TrajectoryEnvelope::summary("run-1", 10, 3000, serde_json::json!({"metric": 0.5}));
        assert_eq!(env.kind, EnvelopeKind::Summary);
        assert_eq!(env.payload["metric"], 0.5);
    }
}
