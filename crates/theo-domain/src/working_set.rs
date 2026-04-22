//! Working set — the agent's active context scope.
//!
//! Represents what information is currently "hot" for the running task.
//! Stored as a field in `RunSnapshot` for checkpoint/restore.
//! Supports multi-agent isolation via `agent_id` and `merge_from()`.

use serde::{Deserialize, Serialize};

/// Isolation mode for multi-agent WorkingSet.
///
/// Determines how a sub-agent's context relates to its parent.
/// Reference: OpenDev SubAgentSpec IsolationMode, Anthropic Managed Agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[derive(Default)]
pub enum WorkingSetIsolation {
    /// Inherits parent's WorkingSet (current default behavior).
    #[default]
    Shared,
    /// Has its own private WorkingSet, merges results back to parent.
    Owned,
    /// Can read parent's context but cannot modify it.
    ReadOnly,
}


/// The agent's active context scope during execution.
///
/// Tracks what files, events, hypotheses, and plan steps are currently
/// relevant. Used by the Context Assembler to build the prompt context.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct WorkingSet {
    /// Files currently being worked on or recently accessed.
    pub hot_files: Vec<String>,
    /// IDs of recent events relevant to the current step.
    pub recent_event_ids: Vec<String>,
    /// The agent's current working hypothesis (if any).
    pub active_hypothesis: Option<String>,
    /// The current step in the execution plan.
    pub current_plan_step: Option<String>,
    /// Constraints the agent has learned during this run.
    pub constraints: Vec<String>,
    /// Agent ID for multi-agent isolation. None = main agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Isolation mode for multi-agent context.
    #[serde(default)]
    pub isolation: WorkingSetIsolation,
}

impl WorkingSet {
    /// Creates an empty working set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a file as "hot" (recently accessed or modified).
    /// Deduplicates and keeps most recent at the end.
    pub fn touch_file(&mut self, path: impl Into<String>) {
        let path = path.into();
        self.hot_files.retain(|f| f != &path);
        self.hot_files.push(path);
    }

    /// Records a recent event ID. Keeps only the last `limit` entries.
    pub fn record_event(&mut self, event_id: impl Into<String>, limit: usize) {
        self.recent_event_ids.push(event_id.into());
        if self.recent_event_ids.len() > limit {
            let excess = self.recent_event_ids.len() - limit;
            self.recent_event_ids.drain(..excess);
        }
    }

    /// Merge another WorkingSet into this one (e.g., sub-agent results into parent).
    ///
    /// Combines hot_files (deduped), constraints (deduped), recent events.
    /// Child hypothesis wins if parent has none.
    pub fn merge_from(&self, other: &WorkingSet) -> WorkingSet {
        let mut merged = self.clone();
        for f in &other.hot_files {
            merged.touch_file(f.clone());
        }
        for eid in &other.recent_event_ids {
            if !merged.recent_event_ids.contains(eid) {
                merged.recent_event_ids.push(eid.clone());
            }
        }
        // Child hypothesis wins if parent has none
        if merged.active_hypothesis.is_none() {
            merged.active_hypothesis = other.active_hypothesis.clone();
        }
        // Merge constraints (dedup)
        for c in &other.constraints {
            if !merged.constraints.contains(c) {
                merged.constraints.push(c.clone());
            }
        }
        merged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_set_default_is_empty() {
        let ws = WorkingSet::new();
        assert!(ws.hot_files.is_empty());
        assert!(ws.recent_event_ids.is_empty());
        assert!(ws.active_hypothesis.is_none());
        assert!(ws.current_plan_step.is_none());
        assert!(ws.constraints.is_empty());
        assert!(ws.agent_id.is_none());
    }

    #[test]
    fn touch_file_deduplicates() {
        let mut ws = WorkingSet::new();
        ws.touch_file("src/a.rs");
        ws.touch_file("src/b.rs");
        ws.touch_file("src/a.rs");
        assert_eq!(ws.hot_files, vec!["src/b.rs", "src/a.rs"]);
    }

    #[test]
    fn record_event_respects_limit() {
        let mut ws = WorkingSet::new();
        for i in 0..20 {
            ws.record_event(format!("evt-{}", i), 10);
        }
        assert_eq!(ws.recent_event_ids.len(), 10);
        assert_eq!(ws.recent_event_ids[0], "evt-10");
        assert_eq!(ws.recent_event_ids[9], "evt-19");
    }

    #[test]
    fn working_set_serde_roundtrip() {
        let ws = WorkingSet {
            hot_files: vec!["src/lib.rs".into()],
            recent_event_ids: vec!["evt-1".into()],
            active_hypothesis: Some("jwt decode bug".into()),
            current_plan_step: Some("run tests".into()),
            constraints: vec!["no unwrap in auth".into()],
            agent_id: Some("agent-sub-1".into()),
            isolation: WorkingSetIsolation::Owned,
        };
        let json = serde_json::to_string(&ws).unwrap();
        let back: WorkingSet = serde_json::from_str(&json).unwrap();
        assert_eq!(ws, back);
    }

    #[test]
    fn working_set_deserialize_empty_json_uses_defaults() {
        let json = "{}";
        let ws: WorkingSet = serde_json::from_str(json).unwrap();
        assert!(ws.hot_files.is_empty());
        assert!(ws.active_hypothesis.is_none());
        assert!(ws.agent_id.is_none());
    }

    // --- WorkingSetIsolation tests ---

    #[test]
    fn isolation_default_is_shared() {
        let ws = WorkingSet::new();
        assert_eq!(ws.isolation, WorkingSetIsolation::Shared);
    }

    #[test]
    fn isolation_serde_roundtrip() {
        for iso in &[
            WorkingSetIsolation::Shared,
            WorkingSetIsolation::Owned,
            WorkingSetIsolation::ReadOnly,
        ] {
            let json = serde_json::to_string(iso).unwrap();
            let back: WorkingSetIsolation = serde_json::from_str(&json).unwrap();
            assert_eq!(*iso, back);
        }
    }

    #[test]
    fn isolation_backward_compat() {
        let json = "{}";
        let ws: WorkingSet = serde_json::from_str(json).unwrap();
        assert_eq!(ws.isolation, WorkingSetIsolation::Shared);
    }

    #[test]
    fn owned_isolation_preserves_on_clone() {
        let mut ws = WorkingSet::new();
        ws.isolation = WorkingSetIsolation::Owned;
        let cloned = ws.clone();
        assert_eq!(cloned.isolation, WorkingSetIsolation::Owned);
    }

    // --- P1.5: Multi-agent isolation tests ---

    #[test]
    fn working_set_has_agent_id() {
        let mut ws = WorkingSet::new();
        ws.agent_id = Some("agent-main".into());
        assert_eq!(ws.agent_id, Some("agent-main".to_string()));
    }

    #[test]
    fn working_set_clone_is_independent() {
        let mut parent = WorkingSet::new();
        parent.touch_file("a.rs");
        let mut child = parent.clone();
        child.touch_file("b.rs");
        child.active_hypothesis = Some("child hypothesis".into());
        assert!(!parent.hot_files.contains(&"b.rs".to_string()));
        assert!(parent.active_hypothesis.is_none());
    }

    #[test]
    fn merge_combines_hot_files() {
        let parent = WorkingSet {
            hot_files: vec!["a.rs".into()],
            ..Default::default()
        };
        let child = WorkingSet {
            hot_files: vec!["b.rs".into()],
            ..Default::default()
        };
        let merged = parent.merge_from(&child);
        assert_eq!(merged.hot_files.len(), 2);
        assert!(merged.hot_files.contains(&"a.rs".to_string()));
        assert!(merged.hot_files.contains(&"b.rs".to_string()));
    }

    #[test]
    fn merge_preserves_parent_constraints() {
        let parent = WorkingSet {
            constraints: vec!["no unwrap".into()],
            ..Default::default()
        };
        let child = WorkingSet {
            constraints: vec!["use Result".into()],
            ..Default::default()
        };
        let merged = parent.merge_from(&child);
        assert!(merged.constraints.contains(&"no unwrap".to_string()));
        assert!(merged.constraints.contains(&"use Result".to_string()));
    }

    #[test]
    fn merge_child_hypothesis_fills_empty_parent() {
        let parent = WorkingSet::default();
        let child = WorkingSet {
            active_hypothesis: Some("bug in auth".into()),
            ..Default::default()
        };
        let merged = parent.merge_from(&child);
        assert_eq!(merged.active_hypothesis, Some("bug in auth".to_string()));
    }

    #[test]
    fn merge_parent_hypothesis_preserved_over_child() {
        let parent = WorkingSet {
            active_hypothesis: Some("parent hyp".into()),
            ..Default::default()
        };
        let child = WorkingSet {
            active_hypothesis: Some("child hyp".into()),
            ..Default::default()
        };
        let merged = parent.merge_from(&child);
        assert_eq!(merged.active_hypothesis, Some("parent hyp".to_string()));
    }

    #[test]
    fn merge_deduplicates_constraints() {
        let parent = WorkingSet {
            constraints: vec!["no unwrap".into()],
            ..Default::default()
        };
        let child = WorkingSet {
            constraints: vec!["no unwrap".into(), "test first".into()],
            ..Default::default()
        };
        let merged = parent.merge_from(&child);
        assert_eq!(merged.constraints.len(), 2);
    }
}
