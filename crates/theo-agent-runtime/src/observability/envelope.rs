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
#[non_exhaustive]
pub enum EnvelopeKind {
    Event,
    DropSentinel,
    WriterRecovered,
    Summary,
    /// T16.1 — Human-provided rating attached to a turn or completed run.
    /// Payload: `{ "rating": i8, "turn_index": u64, "comment": Option<String> }`.
    Rating,
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

    /// T16.1 — Build a `Rating` envelope capturing human feedback on a turn.
    ///
    /// `rating` follows the `-1` / `0` / `+1` convention (👎 / neutral / 👍)
    /// but the type is `i8` so multi-step scoring (-3..=+3) is also supported.
    /// `turn_index` references the LLM turn the human is rating.
    pub fn rating(
        run_id: &str,
        seq: u64,
        ts: u64,
        rating: i8,
        turn_index: u64,
        comment: Option<&str>,
    ) -> Self {
        let mut payload = serde_json::json!({
            "rating": rating,
            "turn_index": turn_index,
        });
        if let Some(c) = comment {
            payload["comment"] = serde_json::Value::String(c.to_string());
        }
        Self {
            v: ENVELOPE_SCHEMA_VERSION,
            seq,
            ts,
            run_id: run_id.to_string(),
            kind: EnvelopeKind::Rating,
            event_type: None,
            event_kind: None,
            entity_id: None,
            payload,
            dropped_since_last: 0,
        }
    }

    /// T16.1 — Returns the rating value for this envelope, or None if it's
    /// not a Rating-kind line.
    pub fn rating_value(&self) -> Option<i8> {
        if self.kind != EnvelopeKind::Rating {
            return None;
        }
        self.payload
            .get("rating")
            .and_then(|v| v.as_i64())
            .and_then(|i| i8::try_from(i).ok())
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

    // ---- T16.1 — Rating envelope ----

    #[test]
    fn t161_rating_envelope_has_rating_kind() {
        let env = TrajectoryEnvelope::rating("run-1", 11, 4000, 1, 5, None);
        assert_eq!(env.kind, EnvelopeKind::Rating);
        assert_eq!(env.payload["rating"], 1);
        assert_eq!(env.payload["turn_index"], 5);
    }

    #[test]
    fn t161_rating_envelope_supports_negative_scores() {
        let env = TrajectoryEnvelope::rating("r", 0, 0, -1, 3, None);
        assert_eq!(env.rating_value(), Some(-1));
    }

    #[test]
    fn t161_rating_envelope_with_comment_includes_it() {
        let env = TrajectoryEnvelope::rating("r", 0, 0, 1, 2, Some("nice"));
        assert_eq!(env.payload["comment"], "nice");
    }

    #[test]
    fn t161_rating_envelope_without_comment_omits_it() {
        let env = TrajectoryEnvelope::rating("r", 0, 0, 0, 1, None);
        assert!(env.payload.get("comment").is_none());
    }

    #[test]
    fn t161_rating_value_returns_none_for_non_rating_kind() {
        let e = DomainEvent::new(EventType::ToolCallCompleted, "c", serde_json::json!({}));
        let env = TrajectoryEnvelope::from_event(&e, "r", 0);
        assert!(env.rating_value().is_none());
    }

    #[test]
    fn t161_rating_envelope_serde_roundtrip() {
        let env = TrajectoryEnvelope::rating("r", 7, 1234, 1, 9, Some("good"));
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("\"kind\":\"rating\""));
        let back: TrajectoryEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(back.kind, EnvelopeKind::Rating);
        assert_eq!(back.rating_value(), Some(1));
        assert_eq!(back.payload["comment"], "good");
    }

    /// Backward-compat regression guard for the `sota-tier1-tier2-plan`
    /// global DoD: a `.theo/trajectories/*.jsonl` line written by a
    /// theo build BEFORE T16.1 (so `EnvelopeKind` only had the original
    /// 4 variants — `event`/`drop_sentinel`/`writer_recovered`/`summary`)
    /// MUST still parse under the current `#[non_exhaustive]` enum that
    /// also contains `Rating`. Locks the wire-format contract.
    #[test]
    fn pre_t161_legacy_trajectory_envelope_loads_each_original_kind() {
        // Canonical pre-T16.1 lines for each of the 4 original kinds.
        let cases = [
            (
                "event",
                r#"{"v":1,"seq":0,"ts":1700000000,"run_id":"run-a",
                    "kind":"event","event_type":"ToolCallCompleted",
                    "event_kind":"Tooling","entity_id":"call-1",
                    "payload":{"status":"ok"}}"#,
                EnvelopeKind::Event,
            ),
            (
                "drop_sentinel",
                r#"{"v":1,"seq":5,"ts":1700000010,"run_id":"run-a",
                    "kind":"drop_sentinel",
                    "payload":{"dropped_count":3}}"#,
                EnvelopeKind::DropSentinel,
            ),
            (
                "writer_recovered",
                r#"{"v":1,"seq":7,"ts":1700000020,"run_id":"run-a",
                    "kind":"writer_recovered",
                    "payload":{"buffered_events":12,"error":"disk_full"}}"#,
                EnvelopeKind::WriterRecovered,
            ),
            (
                "summary",
                r#"{"v":1,"seq":10,"ts":1700000030,"run_id":"run-a",
                    "kind":"summary",
                    "payload":{"final_status":"ok","metric":0.42}}"#,
                EnvelopeKind::Summary,
            ),
        ];
        for (name, json, expected_kind) in cases {
            let env: TrajectoryEnvelope = serde_json::from_str(json)
                .unwrap_or_else(|e| panic!("legacy `{name}` envelope failed to parse: {e}"));
            assert_eq!(env.kind, expected_kind, "kind mismatch for `{name}`");
            assert_eq!(env.v, 1, "schema version preserved for `{name}`");
            assert_eq!(
                env.dropped_since_last, 0,
                "`dropped_since_last` defaults to 0 on legacy envelopes (`{name}`)"
            );
            // Roundtrip: serialise and deserialise once more — the modern
            // type must produce wire-format equivalent under all original
            // kinds (no rename / no field renumber).
            let s = serde_json::to_string(&env).expect("modern envelope serialises");
            let back: TrajectoryEnvelope =
                serde_json::from_str(&s).expect("modern envelope round-trips");
            assert_eq!(back.kind, expected_kind);
        }
    }
}
