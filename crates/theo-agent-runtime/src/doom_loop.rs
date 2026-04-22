//! Doom-loop tracker extracted from `run_engine.rs` to keep that
//! file under the workspace's 2500-line structural-hygiene cap.
//!
//! Tracks recent tool calls via a fixed-size ring buffer. Detects
//! when the last N calls are identical (same tool + same args hash).

use std::collections::VecDeque;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Tracks recent tool calls to detect doom loops (identical calls
/// repeated).
pub(crate) struct DoomLoopTracker {
    recent: VecDeque<(String, u64)>,
    threshold: usize,
    /// How many times the doom loop was detected consecutively.
    /// First detection = warning. Second detection (threshold*2) = hard abort.
    hit_count: usize,
}

impl DoomLoopTracker {
    pub fn new(threshold: usize) -> Self {
        Self {
            recent: VecDeque::with_capacity(threshold + 1),
            threshold,
            hit_count: 0,
        }
    }

    /// Returns true if a hard abort should happen (2× threshold
    /// consecutive identical calls).
    pub fn should_abort(&self) -> bool {
        self.hit_count >= 2
    }

    /// Record a tool call. Returns true if a doom loop is detected.
    pub fn record(&mut self, tool_name: &str, args: &serde_json::Value) -> bool {
        let mut hasher = DefaultHasher::new();
        tool_name.hash(&mut hasher);
        args.to_string().hash(&mut hasher);
        let hash = hasher.finish();

        self.recent.push_back((tool_name.to_string(), hash));
        if self.recent.len() > self.threshold {
            self.recent.pop_front();
        }

        if self.recent.len() == self.threshold {
            let first = &self.recent[0];
            let is_loop = self.recent.iter().all(|entry| entry.1 == first.1);
            if is_loop {
                self.hit_count += 1;
            } else {
                self.hit_count = 0;
            }
            is_loop
        } else {
            self.hit_count = 0;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doom_loop_detected_after_threshold_identical_calls() {
        let mut tracker = DoomLoopTracker::new(3);
        let args = serde_json::json!({"filePath": "/tmp/test"});
        assert!(!tracker.record("read", &args));
        assert!(!tracker.record("read", &args));
        assert!(
            tracker.record("read", &args),
            "3rd identical call should trigger doom loop"
        );
    }

    #[test]
    fn doom_loop_no_false_positive_same_tool_different_inputs() {
        let mut tracker = DoomLoopTracker::new(3);
        assert!(!tracker.record("read", &serde_json::json!({"filePath": "a.rs"})));
        assert!(!tracker.record("read", &serde_json::json!({"filePath": "b.rs"})));
        assert!(!tracker.record("read", &serde_json::json!({"filePath": "c.rs"})));
    }

    #[test]
    fn doom_loop_counter_resets_on_different_tool() {
        let mut tracker = DoomLoopTracker::new(3);
        let args = serde_json::json!({"filePath": "/tmp/test"});
        assert!(!tracker.record("read", &args));
        assert!(!tracker.record("read", &args));
        assert!(!tracker.record("bash", &serde_json::json!({"command": "ls"})));
        assert!(!tracker.record("read", &args));
    }

    #[test]
    fn doom_loop_threshold_configurable() {
        let mut tracker = DoomLoopTracker::new(5);
        let args = serde_json::json!({});
        for _ in 0..4 {
            assert!(!tracker.record("bash", &args));
        }
        assert!(
            tracker.record("bash", &args),
            "5th call should trigger with threshold=5"
        );
    }
}
