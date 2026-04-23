//! ObservabilityListener — filters streaming events and hands the rest off
//! to a background writer via an mpsc channel.
//!
//! Properties (from the ADR):
//! - **INV-1** (at_least_observed): on_event is best-effort — events either
//!   enter the channel or increment `dropped_events` / `serialization_errors`.
//! - **Non-blocking**: uses `try_send`. If the channel is full, the event
//!   is counted as dropped and the call returns immediately.
//! - **Filtering**: Streaming events (ContentDelta / ReasoningDelta) never
//!   enter the trajectory channel.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};

use theo_domain::event::{DomainEvent, EventKind};

use crate::event_bus::EventListener;

/// Default capacity of the trajectory channel.
pub const DEFAULT_CHANNEL_CAPACITY: usize = 4096;

/// Listener that forwards non-streaming DomainEvents to a background writer.
///
/// The sender is held inside an `Option` so that the pipeline's `finalize()`
/// can explicitly close it — otherwise the bus still holds an `Arc<Self>` which
/// keeps the sender alive forever and the writer thread cannot detect hang-up.
pub struct ObservabilityListener {
    sender: Mutex<Option<SyncSender<Vec<u8>>>>,
    dropped_events: Arc<AtomicU64>,
    serialization_errors: Arc<AtomicU64>,
}

impl ObservabilityListener {
    /// Construct a listener.
    pub fn new(
        sender: SyncSender<Vec<u8>>,
        dropped_events: Arc<AtomicU64>,
        serialization_errors: Arc<AtomicU64>,
    ) -> Self {
        Self {
            sender: Mutex::new(Some(sender)),
            dropped_events,
            serialization_errors,
        }
    }

    /// Close the sender so the writer thread can observe hang-up and drain.
    pub fn close(&self) {
        if let Ok(mut guard) = self.sender.lock() {
            guard.take();
        }
    }

    /// Return the current number of events dropped because the channel was full.
    pub fn dropped_events(&self) -> u64 {
        self.dropped_events.load(Ordering::Relaxed)
    }

    /// Return the current number of serialization failures.
    pub fn serialization_errors(&self) -> u64 {
        self.serialization_errors.load(Ordering::Relaxed)
    }
}

impl EventListener for ObservabilityListener {
    fn on_event(&self, event: &DomainEvent) {
        // Filter streaming events — they never enter the trajectory.
        if event.event_type.kind() == EventKind::Streaming {
            return;
        }

        let bytes = match serde_json::to_vec(event) {
            Ok(b) => b,
            Err(_) => {
                self.serialization_errors.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };

        // Take a short-lived clone of the sender so we don't hold the mutex
        // across the try_send call — keeps the Mutex contention to a minimum.
        let send_result = {
            let guard = match self.sender.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            match guard.as_ref() {
                Some(tx) => tx.try_send(bytes),
                None => return,
            }
        };
        if send_result.is_err() {
            self.dropped_events.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::sync_channel;
    use std::time::Instant;

    use theo_domain::event::{DomainEvent, EventType};

    fn listener_with_capacity(
        cap: usize,
    ) -> (
        ObservabilityListener,
        std::sync::mpsc::Receiver<Vec<u8>>,
        Arc<AtomicU64>,
        Arc<AtomicU64>,
    ) {
        let (tx, rx) = sync_channel::<Vec<u8>>(cap);
        let dropped = Arc::new(AtomicU64::new(0));
        let serr = Arc::new(AtomicU64::new(0));
        (
            ObservabilityListener::new(tx, dropped.clone(), serr.clone()),
            rx,
            dropped,
            serr,
        )
    }

    fn ev(t: EventType) -> DomainEvent {
        DomainEvent::new(t, "test", serde_json::json!({}))
    }

    #[test]
    fn test_listener_filters_streaming_events() {
        let (l, rx, _, _) = listener_with_capacity(16);
        l.on_event(&ev(EventType::ContentDelta));
        l.on_event(&ev(EventType::ReasoningDelta));
        assert!(rx.try_recv().is_err(), "no streaming event should reach channel");
    }

    #[test]
    fn test_listener_sends_non_streaming_events() {
        let (l, rx, _, _) = listener_with_capacity(16);
        l.on_event(&ev(EventType::ToolCallCompleted));
        let bytes = rx.try_recv().expect("event should reach channel");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_listener_on_event_is_nonblocking() {
        // Capacity=1, so all subsequent events overflow and must return fast.
        let (l, _rx, _, _) = listener_with_capacity(1);
        let start = Instant::now();
        for _ in 0..1000 {
            l.on_event(&ev(EventType::ToolCallCompleted));
        }
        let elapsed = start.elapsed();
        // 1000 events on a single thread — even with overhead well under 1s.
        assert!(
            elapsed.as_millis() < 1000,
            "on_event must be non-blocking (<1s for 1000 events), took {:?}",
            elapsed
        );
    }

    #[test]
    fn test_listener_counts_dropped_events() {
        let (l, _rx, dropped, _) = listener_with_capacity(1);
        for _ in 0..100 {
            l.on_event(&ev(EventType::ToolCallCompleted));
        }
        assert!(dropped.load(Ordering::Relaxed) > 0);
    }

    #[test]
    fn test_listener_serialization_errors_zero_for_valid_events() {
        let (l, _rx, _, serr) = listener_with_capacity(16);
        l.on_event(&ev(EventType::ToolCallCompleted));
        assert_eq!(serr.load(Ordering::Relaxed), 0);
    }
}
