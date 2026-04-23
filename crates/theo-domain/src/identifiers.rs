use std::fmt;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

macro_rules! define_identifier {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        pub struct $name(String);

        impl $name {
            /// Creates a new identifier from a non-empty string.
            ///
            /// # Panics
            ///
            /// Panics if `id` is empty.
            pub fn new(id: impl Into<String>) -> Self {
                let id = id.into();
                assert!(
                    !id.is_empty(),
                    concat!(stringify!($name), " must not be empty")
                );
                Self(id)
            }

            /// Generates a unique identifier using timestamp + random entropy.
            ///
            /// Format: `{timestamp_millis_hex}_{random_u64_hex}`
            pub fn generate() -> Self {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("system clock before UNIX epoch")
                    .as_millis() as u64;
                let random = random_u64();
                Self(format!("{:013x}_{:016x}", ts, random))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl Hash for $name {
            fn hash<H: Hasher>(&self, state: &mut H) {
                self.0.hash(state);
            }
        }
    };
}

define_identifier!(
    TaskId,
    "Unique identifier for a task in the agent lifecycle."
);
define_identifier!(CallId, "Unique identifier for a tool call.");
define_identifier!(RunId, "Unique identifier for an agent run.");
define_identifier!(EventId, "Unique identifier for a domain event.");
define_identifier!(
    TrajectoryId,
    "Unique identifier for an observability trajectory (derived projection of a run)."
);

/// Simple random u64 using system entropy without external crates.
fn random_u64() -> u64 {
    use std::collections::hash_map::DefaultHasher;

    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_nanos();

    let thread_id = format!("{:?}", std::thread::current().id());

    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    thread_id.hash(&mut hasher);
    // Mix in the address of a stack variable for extra entropy
    let stack_var: u8 = 0;
    let addr = std::ptr::addr_of!(stack_var) as u64;
    addr.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn task_id_new_accepts_non_empty() {
        let id = TaskId::new("task-123");
        assert_eq!(id.as_str(), "task-123");
    }

    #[test]
    #[should_panic(expected = "must not be empty")]
    fn task_id_new_rejects_empty() {
        TaskId::new("");
    }

    #[test]
    #[should_panic(expected = "must not be empty")]
    fn call_id_new_rejects_empty() {
        CallId::new("");
    }

    #[test]
    #[should_panic(expected = "must not be empty")]
    fn run_id_new_rejects_empty() {
        RunId::new("");
    }

    #[test]
    #[should_panic(expected = "must not be empty")]
    fn event_id_new_rejects_empty() {
        EventId::new("");
    }

    #[test]
    fn task_id_generate_produces_unique_ids() {
        let mut ids = HashSet::new();
        for _ in 0..1000 {
            let id = TaskId::generate();
            assert!(!id.as_str().is_empty());
            ids.insert(id.as_str().to_string());
        }
        assert_eq!(ids.len(), 1000, "expected 1000 unique IDs");
    }

    #[test]
    fn call_id_generate_produces_unique_ids() {
        let mut ids = HashSet::new();
        for _ in 0..1000 {
            ids.insert(CallId::generate().as_str().to_string());
        }
        assert_eq!(ids.len(), 1000);
    }

    #[test]
    fn run_id_generate_produces_unique_ids() {
        let mut ids = HashSet::new();
        for _ in 0..1000 {
            ids.insert(RunId::generate().as_str().to_string());
        }
        assert_eq!(ids.len(), 1000);
    }

    #[test]
    fn event_id_generate_produces_unique_ids() {
        let mut ids = HashSet::new();
        for _ in 0..1000 {
            ids.insert(EventId::generate().as_str().to_string());
        }
        assert_eq!(ids.len(), 1000);
    }

    // --- T0.1: TrajectoryId tests ---

    #[test]
    fn test_trajectory_id_generate_is_unique() {
        let a = TrajectoryId::generate();
        let b = TrajectoryId::generate();
        assert_ne!(a, b, "two generated TrajectoryIds must differ");
    }

    #[test]
    #[should_panic(expected = "must not be empty")]
    fn test_trajectory_id_new_rejects_empty() {
        TrajectoryId::new("");
    }

    #[test]
    fn trajectory_id_generate_produces_unique_ids() {
        let mut ids = HashSet::new();
        for _ in 0..1000 {
            ids.insert(TrajectoryId::generate().as_str().to_string());
        }
        assert_eq!(ids.len(), 1000);
    }

    #[test]
    fn serde_roundtrip_trajectory_id() {
        let id = TrajectoryId::new("traj-42");
        let json = serde_json::to_string(&id).unwrap();
        let back: TrajectoryId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn display_shows_inner_value() {
        let id = TaskId::new("abc-123");
        assert_eq!(format!("{}", id), "abc-123");
    }

    #[test]
    fn serde_roundtrip_task_id() {
        let id = TaskId::new("task-42");
        let json = serde_json::to_string(&id).unwrap();
        let back: TaskId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn serde_roundtrip_call_id() {
        let id = CallId::new("call-99");
        let json = serde_json::to_string(&id).unwrap();
        let back: CallId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn serde_roundtrip_run_id() {
        let id = RunId::new("run-007");
        let json = serde_json::to_string(&id).unwrap();
        let back: RunId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn serde_roundtrip_event_id() {
        let id = EventId::new("evt-1");
        let json = serde_json::to_string(&id).unwrap();
        let back: EventId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn hash_consistent_with_eq() {
        use std::collections::hash_map::DefaultHasher;

        let a = TaskId::new("same");
        let b = TaskId::new("same");
        assert_eq!(a, b);

        let hash_a = {
            let mut h = DefaultHasher::new();
            a.hash(&mut h);
            h.finish()
        };
        let hash_b = {
            let mut h = DefaultHasher::new();
            b.hash(&mut h);
            h.finish()
        };
        assert_eq!(hash_a, hash_b, "equal values must have equal hashes");
    }

    #[test]
    fn different_ids_not_equal() {
        let a = TaskId::new("one");
        let b = TaskId::new("two");
        assert_ne!(a, b);
    }

    #[test]
    fn clone_produces_equal_value() {
        let original = CallId::new("clone-me");
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn as_str_returns_inner() {
        let id = RunId::new("inner-value");
        assert_eq!(id.as_str(), "inner-value");
    }
}
