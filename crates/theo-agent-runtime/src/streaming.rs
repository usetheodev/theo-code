//! T14.1 — Streaming partial-result helpers.
//!
//! Tools that support streaming (declared via `Tool::supports_streaming`)
//! emit `PartialToolResult` chunks during execution. This module provides
//! the runtime-side primitives consumers (TUI, log writers, OTel
//! exporters) build on top of:
//!
//! - [`StreamAccumulator`] — buffers chunks into a single growing string,
//!   optionally tracking max progress (some tools report 0..=1.0).
//! - [`debounce`] — yields the latest chunk when at least `min_interval`
//!   has elapsed since the previous emission. Caller drives time
//!   explicitly so we can test deterministically without sleeping.
//!
//! Pure code — no `tokio::sync::mpsc` dependency at this level. Wiring
//! the channel happens at integration points (`tool_context.stdout_tx`,
//! TUI consumer); these helpers are agnostic so both `Stream<Item =
//! PartialToolResult>` and `Vec<PartialToolResult>` work.

use std::time::Duration;

use theo_domain::tool::PartialToolResult;

/// Accumulator that grows a single `String` from successive partial
/// chunks AND tracks the highest reported progress value.
///
/// Tools may emit progress as monotonically increasing fractions; we
/// keep the **max** so a stale chunk arriving out of order (rare but
/// possible across MPSC reordering) doesn't cause the displayed
/// progress to regress.
#[derive(Debug, Default, Clone)]
pub struct StreamAccumulator {
    buffer: String,
    max_progress: Option<f32>,
    chunks: usize,
}

impl StreamAccumulator {
    /// Empty accumulator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a chunk's content + advance progress (max-of).
    pub fn push(&mut self, partial: &PartialToolResult) {
        self.buffer.push_str(&partial.content);
        if let Some(p) = partial.progress {
            // Clamp to [0,1] AND ignore NaN so the displayed value is
            // always sane.
            if p.is_finite() {
                let clamped = p.clamp(0.0, 1.0);
                self.max_progress = Some(match self.max_progress {
                    Some(prev) => prev.max(clamped),
                    None => clamped,
                });
            }
        }
        self.chunks += 1;
    }

    /// Current concatenated output.
    pub fn content(&self) -> &str {
        &self.buffer
    }

    /// Highest progress value seen so far. `None` until any chunk with
    /// `progress = Some(_)` arrives.
    pub fn progress(&self) -> Option<f32> {
        self.max_progress
    }

    /// Number of chunks pushed.
    pub fn chunk_count(&self) -> usize {
        self.chunks
    }

    /// True when the buffer is still empty (no chunks pushed or all
    /// chunks had empty content).
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Take the buffered content out, leaving the accumulator empty.
    /// Progress and chunk count are preserved (useful when paginating
    /// renders).
    pub fn drain_content(&mut self) -> String {
        std::mem::take(&mut self.buffer)
    }

    /// Reset everything — buffer, progress, chunk count.
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.max_progress = None;
        self.chunks = 0;
    }
}

/// Time-ordered debouncer for streaming renders. Caller drives the
/// "current time" explicitly via `now_millis` so tests stay
/// deterministic; production code passes `now_millis()` from
/// `theo_domain::clock`.
///
/// `min_interval_ms` is the gap — a chunk landing inside the gap from
/// the previous "kept" emission is discarded; the next chunk after the
/// gap closes is kept.
///
/// The first chunk is ALWAYS kept (caller wants to see something
/// immediately).
#[derive(Debug, Clone, Copy)]
pub struct Debouncer {
    last_keep_ms: Option<u64>,
    min_interval_ms: u64,
}

impl Debouncer {
    /// Build a debouncer with the given minimum interval between
    /// emissions.
    pub fn new(min_interval: Duration) -> Self {
        Self {
            last_keep_ms: None,
            min_interval_ms: min_interval.as_millis() as u64,
        }
    }

    /// Convenience for the canonical TUI default (50 ms).
    pub fn tui_default() -> Self {
        Self::new(Duration::from_millis(50))
    }

    /// Inspect whether a chunk arriving at `now_millis` should be kept
    /// (rendered) or discarded. Mutates the internal "last kept"
    /// timestamp on keep.
    pub fn should_keep(&mut self, now_millis: u64) -> bool {
        match self.last_keep_ms {
            None => {
                self.last_keep_ms = Some(now_millis);
                true
            }
            Some(prev) => {
                if now_millis.saturating_sub(prev) >= self.min_interval_ms {
                    self.last_keep_ms = Some(now_millis);
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Force the next chunk to be kept regardless of timing — useful
    /// for "final flush" semantics when the stream ends.
    pub fn flush(&mut self) {
        self.last_keep_ms = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(content: &str, progress: Option<f32>) -> PartialToolResult {
        PartialToolResult {
            content: content.into(),
            progress,
        }
    }

    // ---- StreamAccumulator ----

    #[test]
    fn t141_accumulator_new_is_empty() {
        let a = StreamAccumulator::new();
        assert!(a.is_empty());
        assert_eq!(a.chunk_count(), 0);
        assert!(a.progress().is_none());
    }

    #[test]
    fn t141_accumulator_push_concatenates_content() {
        let mut a = StreamAccumulator::new();
        a.push(&p("hel", None));
        a.push(&p("lo", None));
        assert_eq!(a.content(), "hello");
        assert_eq!(a.chunk_count(), 2);
    }

    #[test]
    fn t141_accumulator_progress_takes_max() {
        let mut a = StreamAccumulator::new();
        a.push(&p("", Some(0.3)));
        a.push(&p("", Some(0.7)));
        a.push(&p("", Some(0.5))); // out-of-order should NOT regress
        assert_eq!(a.progress(), Some(0.7));
    }

    #[test]
    fn t141_accumulator_progress_clamped_to_unit_range() {
        let mut a = StreamAccumulator::new();
        a.push(&p("", Some(-0.5)));
        a.push(&p("", Some(2.0)));
        // -0.5 → 0.0, 2.0 → 1.0; max = 1.0
        assert_eq!(a.progress(), Some(1.0));
    }

    #[test]
    fn t141_accumulator_ignores_nan_progress() {
        let mut a = StreamAccumulator::new();
        a.push(&p("", Some(f32::NAN)));
        a.push(&p("", Some(0.4)));
        assert_eq!(a.progress(), Some(0.4));
    }

    #[test]
    fn t141_accumulator_progress_none_when_no_progress_chunks() {
        let mut a = StreamAccumulator::new();
        a.push(&p("text", None));
        assert!(a.progress().is_none());
    }

    #[test]
    fn t141_accumulator_drain_content_returns_buffer_and_resets_buffer() {
        let mut a = StreamAccumulator::new();
        a.push(&p("hello", Some(0.5)));
        let drained = a.drain_content();
        assert_eq!(drained, "hello");
        assert_eq!(a.content(), "");
        // Progress + chunk count preserved.
        assert_eq!(a.progress(), Some(0.5));
        assert_eq!(a.chunk_count(), 1);
    }

    #[test]
    fn t141_accumulator_reset_clears_everything() {
        let mut a = StreamAccumulator::new();
        a.push(&p("x", Some(0.9)));
        a.reset();
        assert!(a.is_empty());
        assert_eq!(a.chunk_count(), 0);
        assert!(a.progress().is_none());
    }

    #[test]
    fn t141_accumulator_empty_chunk_still_counted() {
        let mut a = StreamAccumulator::new();
        a.push(&p("", None));
        assert!(a.is_empty()); // buffer empty
        assert_eq!(a.chunk_count(), 1); // but chunk arrived
    }

    // ---- Debouncer ----

    #[test]
    fn t141_debouncer_first_chunk_always_kept() {
        let mut d = Debouncer::new(Duration::from_millis(50));
        assert!(d.should_keep(0));
    }

    #[test]
    fn t141_debouncer_chunk_within_interval_discarded() {
        let mut d = Debouncer::new(Duration::from_millis(50));
        d.should_keep(0);
        assert!(!d.should_keep(20)); // 20ms < 50ms gap
        assert!(!d.should_keep(49));
    }

    #[test]
    fn t141_debouncer_chunk_at_or_past_interval_kept() {
        let mut d = Debouncer::new(Duration::from_millis(50));
        d.should_keep(0);
        assert!(d.should_keep(50));
        // Now t=50 was kept — next gate is at t=100.
        assert!(!d.should_keep(99));
        assert!(d.should_keep(100));
    }

    #[test]
    fn t141_debouncer_flush_resets_gating() {
        let mut d = Debouncer::new(Duration::from_millis(50));
        d.should_keep(0);
        // Inside the gap, should normally be discarded.
        assert!(!d.should_keep(20));
        d.flush();
        // After flush, the next chunk is kept regardless of time.
        assert!(d.should_keep(20));
    }

    #[test]
    fn t141_debouncer_tui_default_is_50ms() {
        let mut d = Debouncer::tui_default();
        d.should_keep(0);
        assert!(!d.should_keep(49));
        assert!(d.should_keep(50));
    }

    #[test]
    fn t141_debouncer_clock_skew_saturating_sub_does_not_panic() {
        // If the wall clock goes BACKWARDS (rare, e.g. NTP correction),
        // `saturating_sub` keeps the math non-negative. Without it we'd
        // overflow and the discarded-chunk branch would never fire
        // (or worse, panic in debug mode).
        let mut d = Debouncer::new(Duration::from_millis(50));
        d.should_keep(1000);
        // Now go back to 500. The gap should be reported as 0 (saturating),
        // so the chunk is discarded (still inside the original 50ms gap).
        assert!(!d.should_keep(500));
    }
}
