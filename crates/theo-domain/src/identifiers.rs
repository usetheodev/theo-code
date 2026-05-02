use std::fmt;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

// ---------------------------------------------------------------------------
// Planning IDs (sequential u32, scoped within a single Plan)
// ---------------------------------------------------------------------------
//
// Unlike `TaskId`/`RunId` (string-based, globally unique via timestamp+random),
// planning IDs are *small sequential integers* assigned by the LLM when it
// drafts a plan. They are scoped to a single `Plan` document — uniqueness is
// enforced by `Plan::validate()`, not by the type itself.
//
// We deliberately keep them as transparent newtypes (no `generate()`,
// no entropy) because plans are authored, not minted. See SOTA Planning
// System plan, Fase 1.1.

/// Sequential identifier for a `PlanTask` within a single `Plan`.
///
/// Display format: `T{n}` (e.g., `T1`, `T42`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct PlanTaskId(pub u32);

impl PlanTaskId {
    /// Returns the underlying `u32`.
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

impl fmt::Display for PlanTaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "T{}", self.0)
    }
}

impl From<u32> for PlanTaskId {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

/// Sequential identifier for a `Phase` within a single `Plan`.
///
/// Display format: `P{n}` (e.g., `P1`, `P3`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct PhaseId(pub u32);

impl PhaseId {
    /// Returns the underlying `u32`.
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

impl fmt::Display for PhaseId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "P{}", self.0)
    }
}

impl From<u32> for PhaseId {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

/// Collision-safe random `u64` derived from a fresh UUID v4.
///
/// T4.6 / find_p4_010 / find_p5_008 — the original implementation mixed
/// wall-clock nanos + thread id + stack address through `DefaultHasher`,
/// which is not a CSPRNG and shows measurable collision pressure on
/// fast hardware. The plan AC mandates `uuid::Uuid::new_v4()`, which
/// uses the OS CSPRNG (`getrandom` on Linux). We extract 64 bits from
/// the v4 UUID — that is plenty of entropy for the `*Id` chronological
/// suffix and remains a drop-in for every existing caller.
///
/// Exposed as `pub` so other crates (notably `theo-agent-runtime`'s
/// `subagent::generate_run_id` and `session_tree::EntryId::generate`)
/// can reuse the same entropy source.
pub fn random_u64() -> u64 {
    // Take the low 64 bits of a fresh v4 UUID. The high 64 bits encode
    // the version + variant nibbles and a few zero bits; the low 64
    // bits are full random entropy from `getrandom`.
    Uuid::new_v4().as_u64_pair().1
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

    // ------------------------------------------------------------------
    // T4.6 AC literal — "Property test passa" against UUID v4-backed
    // `random_u64`. The plan mandates 10_000 concurrent generates with
    // 0 collisions; the previous DefaultHasher mix could (and did)
    // collide under the same load. We test both the raw helper AND
    // both downstream `*Id::generate()` consumers so a future
    // regression in any of the three surfaces is caught.
    // ------------------------------------------------------------------

    #[test]
    fn t46_random_u64_is_collision_free_under_concurrent_generate() {
        use std::sync::Mutex;
        use std::thread;

        let collected: Mutex<HashSet<u64>> = Mutex::new(HashSet::new());
        thread::scope(|s| {
            for _ in 0..16 {
                s.spawn(|| {
                    let mut local: Vec<u64> =
                        (0..625).map(|_| random_u64()).collect();
                    let mut guard = collected.lock().unwrap();
                    for v in local.drain(..) {
                        guard.insert(v);
                    }
                });
            }
        });
        let total = collected.into_inner().unwrap();
        assert_eq!(
            total.len(),
            10_000,
            "UUID v4-backed random_u64 must produce 10_000 unique values \
             under concurrent generation; got {} (collision detected)",
            total.len()
        );
    }

    // ------------------------------------------------------------------
    // Planning IDs — sequential u32 newtypes
    // ------------------------------------------------------------------

    #[test]
    fn plan_task_id_display_uses_t_prefix() {
        assert_eq!(format!("{}", PlanTaskId(42)), "T42");
        assert_eq!(format!("{}", PlanTaskId(1)), "T1");
    }

    #[test]
    fn phase_id_display_uses_p_prefix() {
        assert_eq!(format!("{}", PhaseId(3)), "P3");
        assert_eq!(format!("{}", PhaseId(0)), "P0");
    }

    #[test]
    fn plan_task_id_serde_roundtrip() {
        let id = PlanTaskId(42);
        let json = serde_json::to_string(&id).unwrap();
        // Transparent newtype → serializes as the raw u32.
        assert_eq!(json, "42");
        let back: PlanTaskId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn phase_id_serde_roundtrip() {
        let id = PhaseId(7);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "7");
        let back: PhaseId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn plan_task_id_hashable_and_comparable() {
        let mut set = HashSet::new();
        set.insert(PlanTaskId(1));
        set.insert(PlanTaskId(2));
        set.insert(PlanTaskId(1));
        assert_eq!(set.len(), 2);
        assert!(PlanTaskId(1) < PlanTaskId(2));
    }

    #[test]
    fn plan_task_id_from_u32() {
        let id: PlanTaskId = 99u32.into();
        assert_eq!(id.as_u32(), 99);
    }

    #[test]
    fn phase_id_from_u32() {
        let id: PhaseId = 5u32.into();
        assert_eq!(id.as_u32(), 5);
    }

    #[test]
    fn t46_run_id_uniqueness_under_concurrent_generate() {
        use std::sync::Mutex;
        use std::thread;

        let collected: Mutex<HashSet<String>> = Mutex::new(HashSet::new());
        thread::scope(|s| {
            for _ in 0..16 {
                s.spawn(|| {
                    let mut local: Vec<String> = (0..625)
                        .map(|_| RunId::generate().as_str().to_string())
                        .collect();
                    let mut guard = collected.lock().unwrap();
                    for v in local.drain(..) {
                        guard.insert(v);
                    }
                });
            }
        });
        assert_eq!(
            collected.into_inner().unwrap().len(),
            10_000,
            "RunId::generate must be collision-free under concurrent load"
        );
    }
}
