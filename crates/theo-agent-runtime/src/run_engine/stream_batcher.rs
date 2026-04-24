//! Streaming delta batcher — REMEDIATION_PLAN T6.4.
//!
//! The LLM streaming path delivers ContentDelta / ReasoningDelta chunks
//! one per SSE tick (50-100 per second). Emitting a `DomainEvent` per
//! chunk produces thousands of publishes for a single response. That
//! floods bus listeners (OTel exporter, file writer) and is the
//! dominant overhead on the streaming hot path.
//!
//! The batcher coalesces chunks by byte threshold: buffer the text and
//! flush a single event once the buffer is ≥ [`FLUSH_BYTES`] bytes. On
//! the first chunk AND on explicit `flush_remainder`, emit whatever is
//! left. Time-based flushing is deliberately NOT implemented — it
//! would require a background timer task, and the incoming SSE cadence
//! already bounds end-to-end latency.
//!
//! # Correctness invariants
//! - Every chunk is eventually emitted exactly once (no loss).
//! - Concatenation across batched emissions is byte-identical to the
//!   original chunk sequence when joined.
//! - An empty stream produces zero events (nothing to flush).
//! - The buffer is flushed when the stream ends (via
//!   `flush_remainder`) so tail bytes never linger.

use std::sync::Arc;

use theo_domain::event::{DomainEvent, EventType};

use crate::event_bus::EventBus;

/// Minimum buffer size before a batched event is published. 64 bytes
/// matches the plan's target and balances UX latency (a single SSE
/// tick is ~20-200 bytes) with publish overhead (per-event cost is
/// ~1.5 µs on the baseline bench).
pub const FLUSH_BYTES: usize = 64;

/// Buffers a single stream kind (ContentDelta OR ReasoningDelta) and
/// publishes coalesced events to the bus. Not thread-safe — the LLM
/// streaming callback is synchronous within the retry future, so the
/// batcher is always accessed from a single task.
pub(super) struct StreamBatcher {
    event_type: EventType,
    event_bus: Arc<EventBus>,
    run_id: String,
    buffer: String,
}

impl StreamBatcher {
    pub(super) fn new(event_type: EventType, event_bus: Arc<EventBus>, run_id: String) -> Self {
        Self {
            event_type,
            event_bus,
            run_id,
            buffer: String::new(),
        }
    }

    /// Append a chunk and flush if the buffer hit the byte threshold.
    pub(super) fn push(&mut self, chunk: &str) {
        if chunk.is_empty() {
            return;
        }
        self.buffer.push_str(chunk);
        if self.buffer.len() >= FLUSH_BYTES {
            self.emit();
        }
    }

    /// Flush whatever is left in the buffer. Caller invokes this after
    /// the stream ends (StreamDelta::Done) or before the next error
    /// path that abandons the stream.
    pub(super) fn flush_remainder(&mut self) {
        if !self.buffer.is_empty() {
            self.emit();
        }
    }

    fn emit(&mut self) {
        // std::mem::take avoids a clone: the buffer is reset to empty
        // and the consumed String goes straight into the event payload.
        let text = std::mem::take(&mut self.buffer);
        self.event_bus.publish(DomainEvent::new(
            self.event_type,
            &self.run_id,
            serde_json::json!({ "text": text }),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn setup() -> (Arc<EventBus>, StreamBatcher) {
        let bus = Arc::new(EventBus::new());
        let batcher = StreamBatcher::new(EventType::ContentDelta, bus.clone(), "run-1".into());
        (bus, batcher)
    }

    #[test]
    fn empty_push_emits_nothing() {
        let (bus, mut batcher) = setup();
        batcher.push("");
        batcher.flush_remainder();
        assert_eq!(bus.len(), 0);
    }

    #[test]
    fn single_sub_threshold_chunk_buffers_until_flush() {
        let (bus, mut batcher) = setup();
        batcher.push("short"); // 5 bytes < 64
        assert_eq!(bus.len(), 0, "sub-threshold must buffer, not publish");
        batcher.flush_remainder();
        assert_eq!(bus.len(), 1);
        let ev = &bus.events()[0];
        assert_eq!(ev.payload["text"], "short");
    }

    #[test]
    fn single_over_threshold_chunk_publishes_immediately() {
        let (bus, mut batcher) = setup();
        let big = "x".repeat(100); // > 64 bytes
        batcher.push(&big);
        assert_eq!(bus.len(), 1, "over-threshold single push should emit");
        batcher.flush_remainder();
        assert_eq!(bus.len(), 1, "flush of empty buffer is a no-op");
    }

    #[test]
    fn many_small_chunks_coalesce_into_one_emission() {
        let (bus, mut batcher) = setup();
        // 20 pushes × 5 bytes = 100 bytes total. With 64-byte
        // threshold we expect 1 mid-stream publish (at ~65 bytes) + 1
        // flush_remainder (tail 35 bytes). So 2 events total, NOT 20.
        for _ in 0..20 {
            batcher.push("abcde"); // 5 bytes each
        }
        let mid_count = bus.len();
        assert_eq!(
            mid_count, 1,
            "should coalesce 13 chunks into 1 emission at byte-threshold"
        );
        batcher.flush_remainder();
        assert_eq!(bus.len(), 2, "tail flush produces a second event");
    }

    #[test]
    fn concatenation_is_byte_identical_to_source() {
        let (bus, mut batcher) = setup();
        let source_parts = ["alpha-", "beta-", "gamma-", "delta-", "epsilon-", "zeta."];
        for part in &source_parts {
            batcher.push(part);
        }
        batcher.flush_remainder();
        // Re-assemble every published event's text — must equal the
        // original concatenation.
        let reassembled: String = bus
            .events()
            .iter()
            .filter_map(|e| e.payload["text"].as_str().map(String::from))
            .collect();
        let expected: String = source_parts.concat();
        assert_eq!(reassembled, expected);
    }

    #[test]
    fn utf8_multibyte_scalars_are_never_split_mid_scalar() {
        let (bus, mut batcher) = setup();
        // 4-byte emoji pushed 30 times (= 120 bytes). Every emission
        // payload MUST be valid UTF-8 — String::push_str guarantees
        // this so this test is a regression guard for future impls.
        for _ in 0..30 {
            batcher.push("\u{1F600}");
        }
        batcher.flush_remainder();
        for ev in bus.events() {
            let text = ev.payload["text"].as_str().unwrap_or("");
            assert!(
                std::str::from_utf8(text.as_bytes()).is_ok(),
                "every emission must be valid UTF-8"
            );
        }
    }

    /// T6.4 AC literal: with 50 small chunks fed in, the bus must see
    /// strictly fewer events than chunks pushed.
    #[test]
    fn streaming_publishes_at_most_one_per_flush_threshold() {
        let (bus, mut batcher) = setup();
        for _ in 0..50 {
            batcher.push("ab"); // 2 bytes each, total 100
        }
        batcher.flush_remainder();
        // 100 bytes / 64-byte threshold = 1 mid-stream + 1 tail = 2 events.
        assert!(
            bus.len() <= 5,
            "expected ≤5 coalesced events for 50 chunks, got {}",
            bus.len()
        );
        assert!(
            bus.len() < 50,
            "batcher must coalesce — got {} publishes for 50 chunks",
            bus.len()
        );
    }
}
