use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

use theo_domain::agent_run::AgentRun;
use theo_domain::budget::BudgetUsage;
use theo_domain::event::DomainEvent;
use theo_domain::task::Task;
use theo_domain::tool_call::{ToolCallRecord, ToolResultRecord};
use theo_domain::working_set::WorkingSet;

use crate::dlq::DeadLetter;

/// A complete snapshot of an agent run's state for persistence and resume.
///
/// Invariant 7: every resume must start from a consistent snapshot.
/// Checksum validates integrity on load.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSnapshot {
    pub run: AgentRun,
    pub task: Task,
    pub tool_calls: Vec<ToolCallRecord>,
    pub tool_results: Vec<ToolResultRecord>,
    pub events: Vec<DomainEvent>,
    pub budget_usage: BudgetUsage,
    /// LLM conversation history (serialized messages).
    pub messages: Vec<serde_json::Value>,
    pub dlq: Vec<DeadLetter>,
    pub snapshot_at: u64,
    /// Hash of serialized state (excluding the checksum field itself).
    pub checksum: String,
    /// Schema version for forward/backward compatibility.
    /// Defaults to 0 for legacy snapshots that lack this field.
    #[serde(default)]
    pub schema_version: u32,
    /// Active context scope for the running task.
    /// None for legacy snapshots or runs that don't use working sets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_set: Option<WorkingSet>,
}

impl RunSnapshot {
    /// Current schema version. Increment when changing the snapshot format.
    pub const CURRENT_SCHEMA_VERSION: u32 = 1;

    /// Computes a deterministic checksum of the snapshot data.
    ///
    /// Uses a stable hash of the JSON-serialized content (excluding the checksum field).
    /// This is for integrity validation, not cryptographic security.
    pub fn compute_checksum(&self) -> String {
        let hashable = SnapshotHashable {
            run: &self.run,
            task: &self.task,
            tool_calls: &self.tool_calls,
            tool_results: &self.tool_results,
            events: &self.events,
            budget_usage: &self.budget_usage,
            messages: &self.messages,
            dlq: &self.dlq,
            snapshot_at: self.snapshot_at,
            schema_version: self.schema_version,
            working_set: &self.working_set,
        };

        let json = serde_json::to_string(&hashable).expect("snapshot serialization failed");
        let mut hasher = DefaultHasher::new();
        json.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    /// Validates that the stored checksum matches the computed one.
    pub fn validate_checksum(&self) -> bool {
        self.checksum == self.compute_checksum()
    }

    /// Creates a snapshot with auto-computed checksum and current timestamp.
    pub fn new(
        run: AgentRun,
        task: Task,
        tool_calls: Vec<ToolCallRecord>,
        tool_results: Vec<ToolResultRecord>,
        events: Vec<DomainEvent>,
        budget_usage: BudgetUsage,
        messages: Vec<serde_json::Value>,
        dlq: Vec<DeadLetter>,
    ) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            .as_millis() as u64;

        let mut snapshot = Self {
            run,
            task,
            tool_calls,
            tool_results,
            events,
            budget_usage,
            messages,
            dlq,
            snapshot_at: now,
            checksum: String::new(),
            schema_version: Self::CURRENT_SCHEMA_VERSION,
            working_set: None,
        };
        snapshot.checksum = snapshot.compute_checksum();
        snapshot
    }
}

/// Helper struct for hashing — excludes the checksum field.
#[derive(Serialize)]
struct SnapshotHashable<'a> {
    run: &'a AgentRun,
    task: &'a Task,
    tool_calls: &'a [ToolCallRecord],
    tool_results: &'a [ToolResultRecord],
    events: &'a [DomainEvent],
    budget_usage: &'a BudgetUsage,
    messages: &'a [serde_json::Value],
    dlq: &'a [DeadLetter],
    snapshot_at: u64,
    schema_version: u32,
    working_set: &'a Option<WorkingSet>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use theo_domain::agent_run::RunState;
    use theo_domain::identifiers::{RunId, TaskId};
    use theo_domain::session::SessionId;
    use theo_domain::task::{AgentType, TaskState};

    fn make_snapshot() -> RunSnapshot {
        let run = AgentRun {
            run_id: RunId::new("r-1"),
            task_id: TaskId::new("t-1"),
            state: RunState::Executing,
            iteration: 5,
            max_iterations: 30,
            created_at: 1000,
            updated_at: 2000,
        };
        let task = Task {
            task_id: TaskId::new("t-1"),
            session_id: SessionId::new("s-1"),
            state: TaskState::Running,
            agent_type: AgentType::Coder,
            objective: "fix bug".into(),
            artifacts: vec![],
            created_at: 1000,
            updated_at: 2000,
            completed_at: None,
        };
        RunSnapshot::new(
            run, task, vec![], vec![], vec![],
            BudgetUsage::default(), vec![], vec![],
        )
    }

    #[test]
    fn serde_roundtrip_preserves_all_fields() {
        let snapshot = make_snapshot();
        let json = serde_json::to_string(&snapshot).unwrap();
        let back: RunSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.run.run_id, snapshot.run.run_id);
        assert_eq!(back.task.task_id, snapshot.task.task_id);
        assert_eq!(back.task.objective, "fix bug");
        assert_eq!(back.snapshot_at, snapshot.snapshot_at);
        assert_eq!(back.checksum, snapshot.checksum);
    }

    #[test]
    fn compute_checksum_deterministic() {
        let snapshot = make_snapshot();
        let c1 = snapshot.compute_checksum();
        let c2 = snapshot.compute_checksum();
        assert_eq!(c1, c2);
    }

    #[test]
    fn validate_checksum_returns_true_for_valid() {
        let snapshot = make_snapshot();
        assert!(snapshot.validate_checksum());
    }

    #[test]
    fn validate_checksum_returns_false_for_corrupted() {
        let mut snapshot = make_snapshot();
        snapshot.checksum = "corrupted_checksum".to_string();
        assert!(!snapshot.validate_checksum());
    }

    #[test]
    fn snapshot_with_empty_collections() {
        let snapshot = make_snapshot();
        assert!(snapshot.tool_calls.is_empty());
        assert!(snapshot.tool_results.is_empty());
        assert!(snapshot.events.is_empty());
        assert!(snapshot.dlq.is_empty());
        assert!(snapshot.messages.is_empty());
        // Should still serialize fine
        let json = serde_json::to_string(&snapshot).unwrap();
        let back: RunSnapshot = serde_json::from_str(&json).unwrap();
        assert!(back.validate_checksum());
    }

    #[test]
    fn snapshot_with_dlq_entries_preserved() {
        use theo_domain::identifiers::CallId;

        let run = AgentRun {
            run_id: RunId::new("r-1"),
            task_id: TaskId::new("t-1"),
            state: RunState::Executing,
            iteration: 3,
            max_iterations: 30,
            created_at: 1000,
            updated_at: 2000,
        };
        let task = Task {
            task_id: TaskId::new("t-1"),
            session_id: SessionId::new("s-1"),
            state: TaskState::Running,
            agent_type: AgentType::Coder,
            objective: "test".into(),
            artifacts: vec![],
            created_at: 1000,
            updated_at: 2000,
            completed_at: None,
        };

        let dlq = vec![
            DeadLetter {
                call_id: CallId::new("c-1"),
                tool_name: "bash".into(),
                input: serde_json::json!({"command": "ls"}),
                error: "timeout".into(),
                attempts: 3,
                created_at: 1500,
            },
        ];

        let snapshot = RunSnapshot::new(
            run, task, vec![], vec![], vec![],
            BudgetUsage::default(), vec![], dlq,
        );

        let json = serde_json::to_string(&snapshot).unwrap();
        let back: RunSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.dlq.len(), 1);
        assert_eq!(back.dlq[0].tool_name, "bash");
        assert_eq!(back.dlq[0].attempts, 3);
        assert!(back.validate_checksum());
    }

    // --- Schema version tests (S0-T2) ---

    #[test]
    fn snapshot_has_schema_version() {
        let snapshot = make_snapshot();
        assert_eq!(snapshot.schema_version, RunSnapshot::CURRENT_SCHEMA_VERSION);
        assert!(snapshot.schema_version > 0, "Schema version must be positive");
    }

    #[test]
    fn snapshot_schema_version_included_in_checksum() {
        let s1 = make_snapshot();
        // Manually change schema_version → checksum should differ
        let mut s2 = make_snapshot();
        s2.schema_version = 999;
        s2.checksum = s2.compute_checksum();
        assert_ne!(s1.checksum, s2.checksum, "Schema version must affect checksum");
    }

    #[test]
    fn legacy_snapshot_without_schema_version_defaults_to_zero() {
        // Simulate a legacy JSON that doesn't have schema_version field
        let snapshot = make_snapshot();
        let mut json_val: serde_json::Value = serde_json::to_value(&snapshot).unwrap();
        json_val.as_object_mut().unwrap().remove("schema_version");
        let json_str = serde_json::to_string(&json_val).unwrap();

        let restored: RunSnapshot = serde_json::from_str(&json_str).unwrap();
        assert_eq!(restored.schema_version, 0, "Legacy snapshots should default to version 0");
    }

    // --- S1-T3: WorkingSet tests ---

    #[test]
    fn working_set_included_in_snapshot() {
        use theo_domain::working_set::WorkingSet;
        let mut snapshot = make_snapshot();
        let ws = WorkingSet {
            hot_files: vec!["src/auth.rs".into()],
            recent_event_ids: vec!["evt-1".into()],
            active_hypothesis: Some("jwt decode bug".into()),
            current_plan_step: Some("run tests".into()),
            constraints: vec!["no unwrap".into()],
            ..WorkingSet::default()
        };
        snapshot.working_set = Some(ws.clone());
        snapshot.checksum = snapshot.compute_checksum();

        assert_eq!(snapshot.working_set.as_ref().unwrap().hot_files, ws.hot_files);
        assert!(snapshot.validate_checksum());
    }

    #[test]
    fn working_set_survives_serde_roundtrip() {
        use theo_domain::working_set::WorkingSet;
        let mut snapshot = make_snapshot();
        snapshot.working_set = Some(WorkingSet {
            hot_files: vec!["src/lib.rs".into()],
            current_plan_step: Some("step 1".into()),
            ..WorkingSet::default()
        });
        snapshot.checksum = snapshot.compute_checksum();

        let json = serde_json::to_string(&snapshot).unwrap();
        let restored: RunSnapshot = serde_json::from_str(&json).unwrap();
        assert!(restored.working_set.is_some());
        assert_eq!(restored.working_set.unwrap().current_plan_step, Some("step 1".into()));
    }

    #[test]
    fn working_set_none_for_legacy_snapshots() {
        let snapshot = make_snapshot();
        let mut json_val: serde_json::Value = serde_json::to_value(&snapshot).unwrap();
        json_val.as_object_mut().unwrap().remove("working_set");
        let json_str = serde_json::to_string(&json_val).unwrap();

        let restored: RunSnapshot = serde_json::from_str(&json_str).unwrap();
        assert!(restored.working_set.is_none(), "Legacy snapshots should have no working_set");
    }

    #[test]
    fn working_set_affects_checksum() {
        use theo_domain::working_set::WorkingSet;
        let s1 = make_snapshot();

        let mut s2 = make_snapshot();
        s2.working_set = Some(WorkingSet {
            hot_files: vec!["changed.rs".into()],
            ..WorkingSet::default()
        });
        s2.checksum = s2.compute_checksum();

        assert_ne!(s1.checksum, s2.checksum, "WorkingSet must affect checksum");
    }

    #[test]
    fn different_data_produces_different_checksum() {
        let s1 = make_snapshot();
        let run2 = AgentRun {
            run_id: RunId::new("r-2"),
            task_id: TaskId::new("t-2"),
            state: RunState::Planning,
            iteration: 1,
            max_iterations: 10,
            created_at: 3000,
            updated_at: 4000,
        };
        let task2 = Task {
            task_id: TaskId::new("t-2"),
            session_id: SessionId::new("s-2"),
            state: TaskState::Pending,
            agent_type: AgentType::Reviewer,
            objective: "review code".into(),
            artifacts: vec![],
            created_at: 3000,
            updated_at: 4000,
            completed_at: None,
        };
        let s2 = RunSnapshot::new(
            run2, task2, vec![], vec![], vec![],
            BudgetUsage::default(), vec![], vec![],
        );
        assert_ne!(s1.checksum, s2.checksum);
    }
}
