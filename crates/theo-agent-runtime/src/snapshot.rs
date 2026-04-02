use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

use theo_domain::agent_run::AgentRun;
use theo_domain::budget::BudgetUsage;
use theo_domain::event::DomainEvent;
use theo_domain::task::Task;
use theo_domain::tool_call::{ToolCallRecord, ToolResultRecord};

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
}

impl RunSnapshot {
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
