//! Loop detector — tracks consecutive identical tool invocations and emits
//! escalating verdicts (Warning → Correct → HardStop).
//!
//! Keeps a sliding window (W=10) of normalized tool calls and maintains a
//! consecutive-counter that resets when a different call appears.
//!
//! Result-aware: the fingerprint includes a hash of the normalized output so
//! that calls with identical args but different outputs are not conflated.

use std::collections::hash_map::DefaultHasher;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};

const WINDOW_SIZE: usize = 10;

/// Expected tool-pair whitelist (A immediately followed by B is benign).
const EXPECTED_SEQUENCES: &[(&str, &str)] = &[
    ("write_file", "read_file"),
    ("edit_file", "bash"),
    ("edit_file", "read_file"),
    ("bash", "read_file"),
    ("grep", "read_file"),
];

/// Verdict returned by [`LoopDetector::record`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopVerdict {
    /// No repetition detected.
    Ok,
    /// 2 consecutive identical calls — weak signal.
    Warning,
    /// 3-4 consecutive identical — emit corrective guidance.
    Correct,
    /// 5+ consecutive identical — abort / hard stop.
    HardStop,
}

#[derive(Debug, Clone)]
struct WindowEntry {
    tool_name: String,
    #[allow(dead_code)]
    fingerprint: u64,
}

/// Detector with sliding window and consecutive counter.
pub struct LoopDetector {
    window: VecDeque<WindowEntry>,
    consecutive: u32,
    last_fingerprint: Option<u64>,
}

impl Default for LoopDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl LoopDetector {
    pub fn new() -> Self {
        Self {
            window: VecDeque::with_capacity(WINDOW_SIZE),
            consecutive: 0,
            last_fingerprint: None,
        }
    }

    /// Record a normalized tool invocation. Returns the current verdict.
    pub fn record(
        &mut self,
        tool_name: &str,
        normalized_args: &serde_json::Value,
        normalized_output: &str,
    ) -> LoopVerdict {
        let mut h = DefaultHasher::new();
        tool_name.hash(&mut h);
        normalized_args.to_string().hash(&mut h);
        normalized_output.hash(&mut h);
        let fp = h.finish();

        // Whitelist check: if previous entry forms a known benign pair, reset counter.
        let prev_tool = self.window.back().map(|e| e.tool_name.clone());
        let is_whitelisted = match &prev_tool {
            Some(prev) => EXPECTED_SEQUENCES
                .iter()
                .any(|(a, b)| *a == prev.as_str() && *b == tool_name),
            None => false,
        };

        if is_whitelisted {
            self.consecutive = 0;
            self.last_fingerprint = None;
        } else if self.last_fingerprint == Some(fp) {
            self.consecutive += 1;
        } else {
            self.consecutive = 1;
            self.last_fingerprint = Some(fp);
        }

        self.window.push_back(WindowEntry {
            tool_name: tool_name.to_string(),
            fingerprint: fp,
        });
        while self.window.len() > WINDOW_SIZE {
            self.window.pop_front();
        }

        match self.consecutive {
            0 | 1 => LoopVerdict::Ok,
            2 => LoopVerdict::Warning,
            3..=4 => LoopVerdict::Correct,
            _ => LoopVerdict::HardStop,
        }
    }

    pub fn reset(&mut self) {
        self.window.clear();
        self.consecutive = 0;
        self.last_fingerprint = None;
    }
}

// ---------------------------------------------------------------------------
// T4.4: EventListener wrapper that feeds tool-call events to the LoopDetector
// ---------------------------------------------------------------------------

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use theo_domain::event::{DomainEvent, EventType};

use crate::event_bus::EventListener;

/// Listener that records each `ToolCallCompleted` event into a shared
/// `LoopDetector` and exposes counters of verdicts.
///
/// Consumers can subscribe this to the `EventBus` to enable continuous
/// loop detection without plumbing the detector through the agent loop.
pub struct LoopDetectingListener {
    detector: Arc<Mutex<LoopDetector>>,
    warning_count: Arc<AtomicU64>,
    correct_count: Arc<AtomicU64>,
    hard_stop_count: Arc<AtomicU64>,
}

impl LoopDetectingListener {
    pub fn new(detector: Arc<Mutex<LoopDetector>>) -> Self {
        Self {
            detector,
            warning_count: Arc::new(AtomicU64::new(0)),
            correct_count: Arc::new(AtomicU64::new(0)),
            hard_stop_count: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn warnings(&self) -> u64 {
        self.warning_count.load(Ordering::Relaxed)
    }
    pub fn corrections(&self) -> u64 {
        self.correct_count.load(Ordering::Relaxed)
    }
    pub fn hard_stops(&self) -> u64 {
        self.hard_stop_count.load(Ordering::Relaxed)
    }
}

impl EventListener for LoopDetectingListener {
    fn on_event(&self, event: &DomainEvent) {
        if event.event_type != EventType::ToolCallCompleted {
            return;
        }
        let tool_name = event
            .payload
            .get("tool_name")
            .and_then(|v| v.as_str())
            .unwrap_or("<unknown>");
        let output_preview = event
            .payload
            .get("output_preview")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let args = event.payload.get("args").cloned().unwrap_or(serde_json::Value::Null);
        let verdict = self
            .detector
            .lock()
            .expect("loop detector mutex poisoned")
            .record(tool_name, &args, output_preview);
        match verdict {
            LoopVerdict::Ok => {}
            LoopVerdict::Warning => {
                self.warning_count.fetch_add(1, Ordering::Relaxed);
            }
            LoopVerdict::Correct => {
                self.correct_count.fetch_add(1, Ordering::Relaxed);
            }
            LoopVerdict::HardStop => {
                self.hard_stop_count.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_no_loop_with_distinct_calls() {
        let mut d = LoopDetector::new();
        for i in 0..10 {
            let v = d.record(&format!("t{}", i), &json!({"x": i}), "");
            assert_eq!(v, LoopVerdict::Ok);
        }
    }

    #[test]
    fn test_warning_at_2_consecutive() {
        let mut d = LoopDetector::new();
        d.record("grep", &json!({"q": "a"}), "");
        let v = d.record("grep", &json!({"q": "a"}), "");
        assert_eq!(v, LoopVerdict::Warning);
    }

    #[test]
    fn test_correct_at_3_consecutive() {
        let mut d = LoopDetector::new();
        d.record("grep", &json!({"q": "a"}), "");
        d.record("grep", &json!({"q": "a"}), "");
        let v = d.record("grep", &json!({"q": "a"}), "");
        assert_eq!(v, LoopVerdict::Correct);
    }

    #[test]
    fn test_hard_stop_at_5_consecutive() {
        let mut d = LoopDetector::new();
        for _ in 0..4 {
            d.record("grep", &json!({"q": "a"}), "");
        }
        let v = d.record("grep", &json!({"q": "a"}), "");
        assert_eq!(v, LoopVerdict::HardStop);
    }

    #[test]
    fn test_counter_resets_on_different_call() {
        let mut d = LoopDetector::new();
        d.record("grep", &json!({"q": "a"}), "");
        d.record("grep", &json!({"q": "a"}), ""); // Warning
        d.record("grep", &json!({"q": "b"}), ""); // reset
        let v = d.record("grep", &json!({"q": "c"}), "");
        assert_ne!(v, LoopVerdict::Correct);
    }

    #[test]
    fn test_window_size_is_10() {
        let mut d = LoopDetector::new();
        for i in 0..15 {
            d.record(&format!("t{}", i), &json!({"i": i}), "");
        }
        assert_eq!(d.window.len(), WINDOW_SIZE);
    }

    #[test]
    fn test_result_aware_detection() {
        let mut d = LoopDetector::new();
        let v1 = d.record("bash", &json!({"cmd": "ls"}), "out1");
        let v2 = d.record("bash", &json!({"cmd": "ls"}), "out2");
        assert_eq!(v1, LoopVerdict::Ok);
        assert_eq!(v2, LoopVerdict::Ok, "different output → not a loop");
    }

    // --- T4.3 whitelist ---

    #[test]
    fn test_write_then_read_not_flagged() {
        let mut d = LoopDetector::new();
        d.record("write_file", &json!({"path": "a"}), "");
        let v = d.record("read_file", &json!({"path": "a"}), "");
        assert_eq!(v, LoopVerdict::Ok);
    }

    #[test]
    fn test_edit_then_bash_not_flagged() {
        let mut d = LoopDetector::new();
        d.record("edit_file", &json!({"path": "a"}), "");
        let v = d.record("bash", &json!({"cmd": "cargo test"}), "");
        assert_eq!(v, LoopVerdict::Ok);
    }

    #[test]
    fn test_unknown_pair_still_flagged() {
        let mut d = LoopDetector::new();
        d.record("grep", &json!({"q": "a"}), "");
        let v = d.record("grep", &json!({"q": "a"}), "");
        assert_eq!(v, LoopVerdict::Warning);
    }

    #[test]
    fn test_loop_detecting_listener_counts_verdicts() {
        use theo_domain::event::{DomainEvent, EventType};
        let detector = Arc::new(Mutex::new(LoopDetector::new()));
        let listener = LoopDetectingListener::new(detector);
        for _ in 0..5 {
            let ev = DomainEvent::new(
                EventType::ToolCallCompleted,
                "e",
                json!({"tool_name": "grep", "args": {"q": "x"}, "output_preview": ""}),
            );
            listener.on_event(&ev);
        }
        assert!(listener.warnings() >= 1);
        assert!(listener.corrections() >= 1);
        assert!(listener.hard_stops() >= 1);
    }
}
