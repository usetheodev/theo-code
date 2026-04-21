//! Episode summary — structured compaction of execution events.
//!
//! An `EpisodeSummary` compresses a window of `DomainEvent`s into a reusable
//! record with dual representations: machine-readable for agent resume and
//! optional human-readable for wiki/UI display.
//!
//! Key invariants:
//! - `evidence_event_ids` must be populated BEFORE event compaction.
//! - `schema_version` must be set on creation for forward compatibility.
//! - `supersedes_summary_id` provides minimal causal tracking.

use serde::{Deserialize, Serialize};

use crate::event::DomainEvent;

/// Current schema version for EpisodeSummary.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// A compacted summary of an execution episode (a window of events).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeSummary {
    /// Unique identifier for this summary.
    pub summary_id: String,
    /// The run that produced these events.
    pub run_id: String,
    /// Optional task association.
    pub task_id: Option<String>,
    /// First event in the summarized window.
    pub window_start_event_id: String,
    /// Last event in the summarized window.
    pub window_end_event_id: String,
    /// Machine-readable structured summary for agent consumption.
    pub machine_summary: MachineEpisodeSummary,
    /// Optional human-readable narrative (for wiki/UI).
    pub human_summary: Option<String>,
    /// IDs of evidence events this summary derives from.
    /// These events MUST be retained until the summary is generated.
    pub evidence_event_ids: Vec<String>,
    /// Files touched during this episode.
    pub affected_files: Vec<String>,
    /// Questions that remain unanswered.
    pub open_questions: Vec<String>,
    /// Hypotheses that were not resolved.
    pub unresolved_hypotheses: Vec<String>,
    /// GraphCTX community IDs that were assembled into context during this episode.
    /// Used for context usefulness tracking (P0-T1).
    #[serde(default)]
    pub referenced_community_ids: Vec<String>,
    /// Optional reference to a summary this one replaces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes_summary_id: Option<String>,
    /// Schema version for forward compatibility.
    pub schema_version: u32,
    /// When this summary was created (Unix ms).
    pub created_at: u64,
    /// Time-to-live policy for this summary.
    pub ttl_policy: TtlPolicy,
    /// Memory lifecycle tier (Active → Cooling → Archived).
    #[serde(default)]
    pub lifecycle: MemoryLifecycle,
    /// What kind of knowledge this summary represents.
    #[serde(default)]
    pub memory_kind: MemoryKind,
    /// Token usage accumulated across LLM calls for this episode.
    /// Added in Phase 1 T1.1 — serde default keeps legacy JSONs readable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<crate::budget::TokenUsage>,
}

/// Machine-readable episode summary for agent resume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineEpisodeSummary {
    /// What the agent was trying to accomplish.
    pub objective: String,
    /// Key actions taken during this episode.
    pub key_actions: Vec<String>,
    /// Episode outcome classification.
    pub outcome: EpisodeOutcome,
    /// Steps that succeeded.
    pub successful_steps: Vec<String>,
    /// Attempts that failed (with brief reason).
    pub failed_attempts: Vec<String>,
    /// Constraints learned during execution.
    pub learned_constraints: Vec<String>,
    /// Files modified during this episode.
    pub files_touched: Vec<String>,
}

/// Classification of episode outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EpisodeOutcome {
    Success,
    Failure,
    Partial,
    Inconclusive,
}

/// What kind of knowledge this memory represents.
///
/// Orthogonal to `MemoryLifecycle` (lifecycle = when to transition, kind = what type).
/// Eviction policy and assembly priority vary by kind.
///
/// Reference: MemGPT/Letta three-tier (Core/Recall/Archival),
/// Knowledge Objects [arxiv 2603.17781], MemArchitect [arxiv 2603.18330].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum MemoryKind {
    /// Runtime-only context, never persisted across episodes.
    /// Evicts on turn/episode boundary.
    Ephemeral,
    /// Session-scoped learnings from execution. Evicts by LRU + staleness.
    Episodic,
    /// Cross-session knowledge, persists, retrieved on demand.
    Reusable,
    /// Permanent facts (hash-keyed). Never auto-evicted.
    /// Inspired by Knowledge Objects: facts must not live only in context window.
    Canonical,
}

impl Default for MemoryKind {
    fn default() -> Self {
        MemoryKind::Episodic
    }
}

impl MemoryKind {
    /// Whether this kind of memory should survive episode compaction.
    pub fn survives_compaction(&self) -> bool {
        matches!(self, MemoryKind::Reusable | MemoryKind::Canonical)
    }

    /// Whether this kind can be auto-evicted by the assembler.
    pub fn auto_evictable(&self) -> bool {
        matches!(self, MemoryKind::Ephemeral | MemoryKind::Episodic)
    }
}

/// Memory lifecycle tier for episode summaries.
///
/// Defines retrieval eligibility and eviction behavior per tier.
/// Transitions: Active → Cooling → Archived (one-way, except LRU re-promotion).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryLifecycle {
    /// Runtime: always eligible for assembler, high priority.
    Active,
    /// Post-episode: eligible only if usefulness > threshold.
    Cooling,
    /// Long-term: never auto-assembled, lookup-only.
    Archived,
}

impl Default for MemoryLifecycle {
    fn default() -> Self {
        MemoryLifecycle::Active
    }
}

impl MemoryLifecycle {
    /// Whether this tier is eligible for automatic context assembly.
    pub fn eligible_for_assembly(&self) -> bool {
        matches!(self, MemoryLifecycle::Active)
    }

    /// Whether this tier requires a minimum usefulness score to be assembled.
    pub fn requires_usefulness_gate(&self) -> bool {
        matches!(self, MemoryLifecycle::Cooling)
    }

    /// Usefulness threshold for conditional assembly.
    pub fn usefulness_threshold(&self) -> f64 {
        match self {
            MemoryLifecycle::Active => 0.0,
            MemoryLifecycle::Cooling => 0.3,
            MemoryLifecycle::Archived => 1.0, // effectively never auto-assembled
        }
    }

    /// Transition to the next lifecycle stage.
    pub fn next(&self) -> Self {
        match self {
            MemoryLifecycle::Active => MemoryLifecycle::Cooling,
            MemoryLifecycle::Cooling => MemoryLifecycle::Archived,
            MemoryLifecycle::Archived => MemoryLifecycle::Archived,
        }
    }
}

/// Status of a tracked hypothesis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HypothesisStatus {
    /// In use, enters assembler context.
    Active,
    /// Unused for N iterations, assembler ignores.
    Stale,
    /// Contradicted/replaced, never enters assembler.
    Superseded,
}

/// Source of a hypothesis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HypothesisSource {
    /// Model explicitly emitted HypothesisFormed event.
    Explicit,
    /// System inferred from repeated action patterns.
    Inferred,
}

/// A tracked hypothesis with confidence scoring and lifecycle.
///
/// Evidence tracking: `evidence_for` and `evidence_against` count supporting
/// and contradicting observations. Auto-prunes when against > for * 2.
/// Reference: LATS [arxiv 2310.04406] — confidence as accumulated reward.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hypothesis {
    pub id: String,
    pub description: String,
    pub rationale: String,
    pub confidence: f64,
    pub status: HypothesisStatus,
    pub evidence_event_ids: Vec<String>,
    pub superseded_by: Option<String>,
    pub created_at: u64,
    pub last_accessed_iteration: usize,
    pub source: HypothesisSource,
    /// Count of observations supporting this hypothesis.
    #[serde(default)]
    pub evidence_for: u32,
    /// Count of observations contradicting this hypothesis.
    #[serde(default)]
    pub evidence_against: u32,
}

impl Hypothesis {
    pub fn new(id: &str, description: &str, rationale: &str) -> Self {
        Self {
            id: id.to_string(),
            description: description.to_string(),
            rationale: rationale.to_string(),
            confidence: 0.5,
            status: HypothesisStatus::Active,
            evidence_event_ids: vec![],
            superseded_by: None,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            last_accessed_iteration: 0,
            source: HypothesisSource::Explicit,
            evidence_for: 0,
            evidence_against: 0,
        }
    }

    pub fn inferred(id: &str, description: &str, rationale: &str) -> Self {
        let mut h = Self::new(id, description, rationale);
        h.confidence = 0.3;
        h.source = HypothesisSource::Inferred;
        h
    }

    pub fn mark_stale(&mut self) {
        self.status = HypothesisStatus::Stale;
    }

    pub fn supersede(&mut self, by: &str) {
        self.status = HypothesisStatus::Superseded;
        self.superseded_by = Some(by.to_string());
    }

    /// Record supporting evidence. Updates confidence upward.
    pub fn record_support(&mut self, event_id: &str) {
        self.evidence_for += 1;
        self.evidence_event_ids.push(event_id.to_string());
        self.update_confidence();
    }

    /// Record contradicting evidence. Updates confidence downward.
    /// Auto-prunes to Stale when evidence_against > evidence_for * 2.
    pub fn record_contradiction(&mut self, event_id: &str) {
        self.evidence_against += 1;
        self.evidence_event_ids.push(event_id.to_string());
        self.update_confidence();
        if self.should_auto_prune() {
            self.status = HypothesisStatus::Stale;
        }
    }

    /// Recompute confidence from evidence counts.
    /// confidence = (evidence_for + 1) / (evidence_for + evidence_against + 2)
    /// Laplace smoothing: starts at 0.5 with no evidence.
    fn update_confidence(&mut self) {
        let total = self.evidence_for + self.evidence_against + 2;
        self.confidence = (self.evidence_for + 1) as f64 / total as f64;
    }

    /// Whether this hypothesis should be auto-pruned.
    /// Threshold: evidence_against > evidence_for * 2 AND total evidence >= 3.
    pub fn should_auto_prune(&self) -> bool {
        let total = self.evidence_for + self.evidence_against;
        total >= 3 && self.evidence_against > self.evidence_for * 2
    }

    pub fn is_eligible_for_assembly(&self) -> bool {
        self.status == HypothesisStatus::Active
    }
}

/// Outcome of a causal link between assembled context and a tool call.
///
/// Tracks whether context that was assembled actually contributed to
/// a successful tool call. Reference: CausalTrace pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CausalOutcome {
    /// Tool call succeeded and referenced this context.
    Used,
    /// Tool call succeeded but did not reference this context.
    Unused,
    /// Tool call failed.
    Failed,
}

/// A link between assembled context and a tool call outcome.
///
/// Tracks which community's context was assembled before a tool call
/// and whether it contributed to the outcome.
/// Reference: AgentRx trajectory normalization [Microsoft].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalLink {
    pub community_id: String,
    pub tool_call_id: String,
    pub outcome: CausalOutcome,
    pub iteration: usize,
}

/// Classification of agent errors for failure fingerprinting.
///
/// Reference: AgentDebug [ICLR 2026, arxiv 2509.25370].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ErrorClass {
    /// Error in memory/context management.
    Memory,
    /// Error in planning or reasoning.
    Planning,
    /// Error in tool execution or action.
    Action,
    /// System-level error (timeout, resource limit).
    System,
}

/// Fingerprint of a recurring failure pattern.
///
/// Tracks tool name + error class + argument hash for cycle detection.
/// When a fingerprint recurs ≥ threshold times, auto-generates a constraint.
/// Reference: AgentAssay behavioral fingerprinting [arxiv 2603.02601],
/// OpenDev DoomLoopDetector fingerprint pattern.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct FailureFingerprint {
    pub error_class: ErrorClass,
    pub tool_name: String,
    pub args_hash: u64,
}

impl FailureFingerprint {
    pub fn new(error_class: ErrorClass, tool_name: &str, args: &str) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        args.hash(&mut hasher);
        Self {
            error_class,
            tool_name: tool_name.to_string(),
            args_hash: hasher.finish(),
        }
    }

    /// Generate a constraint message from this fingerprint.
    pub fn to_constraint(&self, count: usize) -> String {
        format!(
            "Avoid {:?} error with tool '{}' (seen {} times)",
            self.error_class, self.tool_name, count
        )
    }
}

/// Time-to-live policy for episode summaries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TtlPolicy {
    /// Deleted when the originating run ends.
    RunScoped,
    /// Deleted after the specified number of seconds.
    TimeScoped { seconds: u64 },
    /// Never automatically deleted.
    Permanent,
}

impl Default for TtlPolicy {
    fn default() -> Self {
        TtlPolicy::RunScoped
    }
}

/// Infer TTL policy from constraint scopes in the event stream.
///
/// - workspace-local constraints → Permanent (must survive indefinitely)
/// - task-local constraints → TimeScoped(24h)
/// - run-local only → RunScoped (default, deleted at run end)
pub fn infer_ttl_policy(events: &[crate::event::DomainEvent]) -> TtlPolicy {
    let has_workspace = events
        .iter()
        .filter(|e| e.event_type == crate::event::EventType::ConstraintLearned)
        .any(|e| e.payload.get("scope").and_then(|s| s.as_str()) == Some("workspace-local"));
    let has_task = events
        .iter()
        .filter(|e| e.event_type == crate::event::EventType::ConstraintLearned)
        .any(|e| e.payload.get("scope").and_then(|s| s.as_str()) == Some("task-local"));

    if has_workspace {
        TtlPolicy::Permanent
    } else if has_task {
        TtlPolicy::TimeScoped { seconds: 86400 }
    } else {
        TtlPolicy::RunScoped
    }
}

impl EpisodeSummary {
    /// Creates an EpisodeSummary from a slice of DomainEvents.
    ///
    /// Extracts structured information deterministically (no LLM needed).
    /// The `objective` must be provided externally (from the Task).
    pub fn from_events(
        run_id: &str,
        task_id: Option<&str>,
        objective: &str,
        events: &[DomainEvent],
    ) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            .as_millis() as u64;

        let evidence_ids: Vec<String> = events
            .iter()
            .map(|e| e.event_id.as_str().to_string())
            .collect();

        let window_start = events
            .first()
            .map(|e| e.event_id.as_str().to_string())
            .unwrap_or_default();
        let window_end = events
            .last()
            .map(|e| e.event_id.as_str().to_string())
            .unwrap_or_default();

        // Extract key actions from tool calls
        let key_actions: Vec<String> = events
            .iter()
            .filter(|e| e.event_type == crate::event::EventType::ToolCallCompleted)
            .filter_map(|e| {
                e.payload
                    .get("tool_name")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
            .collect();

        // Extract affected files from tool payloads
        let affected_files: Vec<String> = events
            .iter()
            .filter_map(|e| {
                e.payload
                    .get("file")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        // Extract learned constraints from ConstraintLearned events
        let mut learned_constraints: Vec<String> = events
            .iter()
            .filter(|e| e.event_type == crate::event::EventType::ConstraintLearned)
            .filter_map(|e| {
                e.payload
                    .get("constraint")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
            .collect();

        // Extract unresolved hypotheses (formed but not invalidated)
        let invalidated_refs: std::collections::HashSet<String> = events
            .iter()
            .filter(|e| e.event_type == crate::event::EventType::HypothesisInvalidated)
            .filter_map(|e| {
                e.payload
                    .get("prior_event_id")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
            .collect();
        let unresolved: Vec<String> = events
            .iter()
            .filter(|e| e.event_type == crate::event::EventType::HypothesisFormed)
            .filter(|e| !invalidated_refs.contains(e.event_id.as_str()))
            .filter_map(|e| {
                e.payload
                    .get("hypothesis")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
            .collect();

        // Extract successful steps (tool calls with success: true)
        let successful_steps: Vec<String> = events
            .iter()
            .filter(|e| e.event_type == crate::event::EventType::ToolCallCompleted)
            .filter(|e| {
                e.payload
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            })
            .map(|e| {
                let tool = e
                    .payload
                    .get("tool_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let file = e.payload.get("file").and_then(|v| v.as_str()).unwrap_or("");
                if file.is_empty() {
                    tool.to_string()
                } else {
                    format!("{}: {}", tool, file)
                }
            })
            .collect();

        // Extract failed attempts (failed tool calls + error events)
        let mut failed_attempts: Vec<String> = events
            .iter()
            .filter(|e| e.event_type == crate::event::EventType::ToolCallCompleted)
            .filter(|e| e.payload.get("success").and_then(|v| v.as_bool()) == Some(false))
            .map(|e| {
                let tool = e
                    .payload
                    .get("tool_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let err = e
                    .payload
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("failed");
                format!("{}: {}", tool, err)
            })
            .collect();
        // Also include Error events
        for e in events
            .iter()
            .filter(|e| e.event_type == crate::event::EventType::Error)
        {
            let msg = e
                .payload
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("error");
            failed_attempts.push(msg.to_string());
        }

        // Determine outcome
        let has_errors = events
            .iter()
            .any(|e| e.event_type == crate::event::EventType::Error);
        let has_failures = events.iter().any(|e| {
            e.event_type == crate::event::EventType::ToolCallCompleted
                && e.payload.get("success").and_then(|v| v.as_bool()) == Some(false)
        });
        let outcome = if has_errors || has_failures {
            EpisodeOutcome::Partial
        } else {
            EpisodeOutcome::Success
        };

        // Extract failure-derived constraints (threshold ≥ 3)
        let failure_constraints = extract_failure_constraints(events, 3);
        learned_constraints.extend(failure_constraints);

        // Infer TTL from constraint scopes
        let ttl_policy = infer_ttl_policy(events);
        let memory_kind = infer_memory_kind(&ttl_policy);

        EpisodeSummary {
            summary_id: format!("ep-{}", uuid_v4_simple()),
            run_id: run_id.to_string(),
            task_id: task_id.map(String::from),
            window_start_event_id: window_start,
            window_end_event_id: window_end,
            machine_summary: MachineEpisodeSummary {
                objective: objective.to_string(),
                key_actions,
                outcome,
                successful_steps,
                failed_attempts,
                learned_constraints,
                files_touched: affected_files.clone(),
            },
            human_summary: None,
            evidence_event_ids: evidence_ids,
            affected_files,
            open_questions: vec![],
            unresolved_hypotheses: unresolved,
            referenced_community_ids: vec![],
            supersedes_summary_id: None,
            schema_version: CURRENT_SCHEMA_VERSION,
            created_at: now,
            ttl_policy,
            lifecycle: MemoryLifecycle::Active,
            memory_kind,
            token_usage: None,
        }
    }
}

/// Infer the memory kind from the TTL policy.
///
/// Permanent → Canonical (cross-session facts)
/// TimeScoped → Reusable (task-scoped knowledge)
/// RunScoped → Episodic (session learnings)
pub fn infer_memory_kind(ttl: &TtlPolicy) -> MemoryKind {
    match ttl {
        TtlPolicy::Permanent => MemoryKind::Canonical,
        TtlPolicy::TimeScoped { .. } => MemoryKind::Reusable,
        TtlPolicy::RunScoped => MemoryKind::Episodic,
    }
}

/// Infer hypotheses from repeated action patterns.
///
/// Detects when the agent repeats the same tool+args 3+ times,
/// inferring an implicit hypothesis with low confidence (0.3).
pub fn infer_hypotheses_from_patterns(events: &[crate::event::DomainEvent]) -> Vec<Hypothesis> {
    let mut action_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for e in events
        .iter()
        .filter(|e| e.event_type == crate::event::EventType::ToolCallCompleted)
    {
        let tool = e
            .payload
            .get("tool_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let target = e.payload.get("file").and_then(|v| v.as_str()).unwrap_or("");
        let key = format!("{}:{}", tool, target);
        *action_counts.entry(key).or_insert(0) += 1;
    }

    action_counts
        .into_iter()
        .filter(|(_, count)| *count >= 3)
        .map(|(action, count)| {
            let id = format!("inferred-{}", uuid_v4_simple());
            let mut h = Hypothesis::inferred(
                &id,
                &format!("Agent repeatedly attempted {} ({} times)", action, count),
                "Detected from repeated action pattern",
            );
            h.confidence = 0.3;
            h
        })
        .collect()
}

/// Extract failure-derived constraints from recurring error patterns.
///
/// Errors appearing ≥ threshold times generate automatic constraints.
pub fn extract_failure_constraints(
    events: &[crate::event::DomainEvent],
    threshold: usize,
) -> Vec<String> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for e in events
        .iter()
        .filter(|e| e.event_type == crate::event::EventType::Error)
    {
        if let Some(msg) = e.payload.get("message").and_then(|v| v.as_str()) {
            let normalized = normalize_error_for_constraint(msg);
            *counts.entry(normalized).or_insert(0) += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(_, count)| *count >= threshold)
        .map(|(msg, count)| format!("Avoid: {} (seen {} times)", msg, count))
        .collect()
}

/// Normalize an error message for pattern matching (strip line numbers, lowercase).
fn normalize_error_for_constraint(error: &str) -> String {
    let mut s = error.to_string();
    // Strip line:col patterns like ":42:5"
    while let Some(pos) = s.find(':') {
        let after = &s[pos + 1..];
        if after.starts_with(|c: char| c.is_ascii_digit()) {
            if let Some(end) = after.find(|c: char| !c.is_ascii_digit() && c != ':') {
                s = format!("{}{}", &s[..pos], &after[end..]);
            } else {
                s = s[..pos].to_string();
                break;
            }
        } else {
            break;
        }
    }
    s.trim().to_lowercase()
}

/// Simple UUID v4 generator (no external dep).
fn uuid_v4_simple() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    std::time::SystemTime::now().hash(&mut hasher);
    std::thread::current().id().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{DomainEvent, EventType};
    use crate::identifiers::EventId;

    fn make_event(event_type: EventType, payload: serde_json::Value) -> DomainEvent {
        DomainEvent::new(event_type, "run-1", payload)
    }

    #[test]
    fn episode_summary_created_from_events() {
        // Arrange
        let events = vec![
            make_event(
                EventType::ToolCallCompleted,
                serde_json::json!({"tool_name": "read", "file": "src/lib.rs"}),
            ),
            make_event(
                EventType::ToolCallCompleted,
                serde_json::json!({"tool_name": "edit", "file": "src/lib.rs"}),
            ),
            make_event(
                EventType::ConstraintLearned,
                serde_json::json!({"constraint": "no unwrap in auth", "scope": "workspace-local"}),
            ),
        ];

        // Act
        let summary = EpisodeSummary::from_events("run-1", Some("task-1"), "fix auth bug", &events);

        // Assert
        assert_eq!(summary.run_id, "run-1");
        assert_eq!(summary.task_id, Some("task-1".to_string()));
        assert_eq!(summary.evidence_event_ids.len(), 3);
        assert_eq!(summary.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(summary.machine_summary.objective, "fix auth bug");
        assert_eq!(summary.machine_summary.key_actions.len(), 2);
        assert!(
            summary
                .machine_summary
                .learned_constraints
                .contains(&"no unwrap in auth".to_string())
        );
        assert_eq!(summary.machine_summary.outcome, EpisodeOutcome::Success);
    }

    #[test]
    fn episode_summary_machine_part_has_structured_fields() {
        let events = vec![make_event(
            EventType::ToolCallCompleted,
            serde_json::json!({"tool_name": "bash"}),
        )];
        let summary = EpisodeSummary::from_events("r-1", None, "test", &events);

        assert!(!summary.machine_summary.objective.is_empty());
        assert_eq!(summary.machine_summary.outcome, EpisodeOutcome::Success);
    }

    #[test]
    fn episode_summary_serde_roundtrip() {
        let events = vec![make_event(
            EventType::RunStateChanged,
            serde_json::json!({"from": "Planning", "to": "Executing"}),
        )];
        let summary = EpisodeSummary::from_events("r-1", None, "plan", &events);

        let json = serde_json::to_string(&summary).unwrap();
        let restored: EpisodeSummary = serde_json::from_str(&json).unwrap();

        assert_eq!(summary.summary_id, restored.summary_id);
        assert_eq!(summary.schema_version, restored.schema_version);
        assert_eq!(summary.run_id, restored.run_id);
        assert_eq!(summary.ttl_policy, restored.ttl_policy);
    }

    #[test]
    fn episode_summary_detects_partial_outcome_on_errors() {
        let events = vec![
            make_event(
                EventType::ToolCallCompleted,
                serde_json::json!({"tool_name": "edit"}),
            ),
            make_event(
                EventType::Error,
                serde_json::json!({"message": "compile error"}),
            ),
        ];
        let summary = EpisodeSummary::from_events("r-1", None, "fix bug", &events);

        assert_eq!(summary.machine_summary.outcome, EpisodeOutcome::Partial);
    }

    #[test]
    fn episode_summary_tracks_unresolved_hypotheses() {
        let h1 = make_event(
            EventType::HypothesisFormed,
            serde_json::json!({
                "hypothesis": "bug in jwt.rs", "rationale": "test fails"
            }),
        );
        let h1_id = h1.event_id.as_str().to_string();

        let h2 = make_event(
            EventType::HypothesisFormed,
            serde_json::json!({
                "hypothesis": "race condition", "rationale": "flaky test"
            }),
        );

        let invalidation = DomainEvent {
            event_id: EventId::generate(),
            event_type: EventType::HypothesisInvalidated,
            entity_id: "run-1".into(),
            timestamp: 1000,
            payload: serde_json::json!({"prior_event_id": h1_id, "reason": "test passed after revert"}),
            supersedes_event_id: Some(h1.event_id.clone()),
        };

        let events = vec![h1, h2, invalidation];
        let summary = EpisodeSummary::from_events("r-1", None, "investigate", &events);

        // h1 was invalidated, h2 remains unresolved
        assert_eq!(summary.unresolved_hypotheses.len(), 1);
        assert_eq!(summary.unresolved_hypotheses[0], "race condition");
    }

    #[test]
    fn episode_summary_empty_events() {
        let summary = EpisodeSummary::from_events("r-1", None, "empty", &[]);
        assert!(summary.evidence_event_ids.is_empty());
        assert!(summary.machine_summary.key_actions.is_empty());
        assert_eq!(summary.window_start_event_id, "");
        assert_eq!(summary.window_end_event_id, "");
    }

    #[test]
    fn ttl_policy_default_is_run_scoped() {
        assert_eq!(TtlPolicy::default(), TtlPolicy::RunScoped);
    }

    #[test]
    fn ttl_policy_serde_roundtrip() {
        for policy in &[
            TtlPolicy::RunScoped,
            TtlPolicy::TimeScoped { seconds: 3600 },
            TtlPolicy::Permanent,
        ] {
            let json = serde_json::to_string(policy).unwrap();
            let back: TtlPolicy = serde_json::from_str(&json).unwrap();
            assert_eq!(*policy, back);
        }
    }

    #[test]
    fn episode_outcome_serde_roundtrip() {
        for outcome in &[
            EpisodeOutcome::Success,
            EpisodeOutcome::Failure,
            EpisodeOutcome::Partial,
            EpisodeOutcome::Inconclusive,
        ] {
            let json = serde_json::to_string(outcome).unwrap();
            let back: EpisodeOutcome = serde_json::from_str(&json).unwrap();
            assert_eq!(*outcome, back);
        }
    }

    // --- P-1 BF1: TTL promotion tests ---

    #[test]
    fn ttl_promoted_to_permanent_when_workspace_constraint() {
        let events = vec![make_event(
            EventType::ConstraintLearned,
            serde_json::json!({
                "constraint": "no unwrap in auth", "scope": "workspace-local"
            }),
        )];
        let summary = EpisodeSummary::from_events("r-1", None, "task", &events);
        assert_eq!(
            summary.ttl_policy,
            TtlPolicy::Permanent,
            "Workspace constraints must survive run end"
        );
    }

    #[test]
    fn ttl_stays_run_scoped_when_only_run_local() {
        let events = vec![make_event(
            EventType::ConstraintLearned,
            serde_json::json!({
                "constraint": "retry 3 times", "scope": "run-local"
            }),
        )];
        let summary = EpisodeSummary::from_events("r-1", None, "task", &events);
        assert_eq!(summary.ttl_policy, TtlPolicy::RunScoped);
    }

    #[test]
    fn ttl_time_scoped_when_task_local() {
        let events = vec![make_event(
            EventType::ConstraintLearned,
            serde_json::json!({
                "constraint": "auth module fragile", "scope": "task-local"
            }),
        )];
        let summary = EpisodeSummary::from_events("r-1", None, "task", &events);
        assert_eq!(summary.ttl_policy, TtlPolicy::TimeScoped { seconds: 86400 });
    }

    // --- P-1 BF3: successful_steps / failed_attempts ---

    #[test]
    fn from_events_populates_successful_steps() {
        let events = vec![
            make_event(
                EventType::ToolCallCompleted,
                serde_json::json!({
                    "tool_name": "edit", "file": "src/auth.rs", "success": true
                }),
            ),
            make_event(
                EventType::ToolCallCompleted,
                serde_json::json!({
                    "tool_name": "bash", "success": true
                }),
            ),
        ];
        let summary = EpisodeSummary::from_events("r-1", None, "fix", &events);
        assert!(
            !summary.machine_summary.successful_steps.is_empty(),
            "Should extract successful tool calls"
        );
    }

    #[test]
    fn from_events_populates_failed_attempts() {
        let events = vec![
            make_event(
                EventType::Error,
                serde_json::json!({"message": "compile error"}),
            ),
            make_event(
                EventType::ToolCallCompleted,
                serde_json::json!({
                    "tool_name": "edit", "success": false, "error": "file not found"
                }),
            ),
        ];
        let summary = EpisodeSummary::from_events("r-1", None, "fix", &events);
        assert!(
            !summary.machine_summary.failed_attempts.is_empty(),
            "Should extract failures"
        );
    }

    #[test]
    fn from_events_separates_success_from_failure() {
        let events = vec![
            make_event(
                EventType::ToolCallCompleted,
                serde_json::json!({
                    "tool_name": "read", "success": true, "file": "src/a.rs"
                }),
            ),
            make_event(
                EventType::ToolCallCompleted,
                serde_json::json!({
                    "tool_name": "edit", "success": false, "error": "permission denied"
                }),
            ),
        ];
        let summary = EpisodeSummary::from_events("r-1", None, "task", &events);
        assert_eq!(summary.machine_summary.successful_steps.len(), 1);
        assert_eq!(summary.machine_summary.failed_attempts.len(), 1);
    }

    // --- P0.5: MemoryLifecycle tests ---

    #[test]
    fn lifecycle_defaults_to_active() {
        let summary = EpisodeSummary::from_events("r-1", None, "task", &[]);
        assert_eq!(summary.lifecycle, MemoryLifecycle::Active);
    }

    #[test]
    fn lifecycle_serde_roundtrip() {
        for lc in &[
            MemoryLifecycle::Active,
            MemoryLifecycle::Cooling,
            MemoryLifecycle::Archived,
        ] {
            let json = serde_json::to_string(lc).unwrap();
            let back: MemoryLifecycle = serde_json::from_str(&json).unwrap();
            assert_eq!(*lc, back);
        }
    }

    #[test]
    fn lifecycle_active_eligible_for_assembly() {
        assert!(MemoryLifecycle::Active.eligible_for_assembly());
        assert!(!MemoryLifecycle::Cooling.eligible_for_assembly());
        assert!(!MemoryLifecycle::Archived.eligible_for_assembly());
    }

    #[test]
    fn lifecycle_cooling_requires_gate() {
        assert!(!MemoryLifecycle::Active.requires_usefulness_gate());
        assert!(MemoryLifecycle::Cooling.requires_usefulness_gate());
        assert!(!MemoryLifecycle::Archived.requires_usefulness_gate());
    }

    #[test]
    fn lifecycle_transitions() {
        assert_eq!(MemoryLifecycle::Active.next(), MemoryLifecycle::Cooling);
        assert_eq!(MemoryLifecycle::Cooling.next(), MemoryLifecycle::Archived);
        assert_eq!(MemoryLifecycle::Archived.next(), MemoryLifecycle::Archived);
    }

    #[test]
    fn lifecycle_backward_compat() {
        let mut val =
            serde_json::to_value(&EpisodeSummary::from_events("r-1", None, "t", &[])).unwrap();
        val.as_object_mut().unwrap().remove("lifecycle");
        let back: EpisodeSummary = serde_json::from_value(val).unwrap();
        assert_eq!(back.lifecycle, MemoryLifecycle::Active);
    }

    // --- P2: Hypothesis tests ---

    #[test]
    fn hypothesis_new_default_confidence() {
        let h = Hypothesis::new("h-1", "jwt bug", "test fails");
        assert_eq!(h.confidence, 0.5);
        assert_eq!(h.status, HypothesisStatus::Active);
        assert_eq!(h.source, HypothesisSource::Explicit);
    }

    #[test]
    fn hypothesis_inferred_low_confidence() {
        let h = Hypothesis::inferred("h-2", "repeated edit", "pattern detected");
        assert_eq!(h.confidence, 0.3);
        assert_eq!(h.source, HypothesisSource::Inferred);
    }

    #[test]
    fn hypothesis_degrades_to_stale() {
        let mut h = Hypothesis::new("h-1", "bug", "reason");
        h.mark_stale();
        assert_eq!(h.status, HypothesisStatus::Stale);
        assert!(!h.is_eligible_for_assembly());
    }

    #[test]
    fn hypothesis_superseded_not_eligible() {
        let mut h = Hypothesis::new("h-1", "bug", "reason");
        h.supersede("h-2");
        assert_eq!(h.status, HypothesisStatus::Superseded);
        assert_eq!(h.superseded_by, Some("h-2".to_string()));
        assert!(!h.is_eligible_for_assembly());
    }

    #[test]
    fn hypothesis_serde_roundtrip() {
        let h = Hypothesis::new("h-1", "test", "reason");
        let json = serde_json::to_string(&h).unwrap();
        let back: Hypothesis = serde_json::from_str(&json).unwrap();
        assert_eq!(h.id, back.id);
        assert_eq!(h.status, back.status);
        assert_eq!(h.confidence, back.confidence);
    }

    // --- P1: Failure learning tests ---

    #[test]
    fn recurring_error_generates_constraint() {
        let events = vec![
            make_event(
                EventType::Error,
                serde_json::json!({"message": "file not found"}),
            ),
            make_event(
                EventType::Error,
                serde_json::json!({"message": "file not found"}),
            ),
            make_event(
                EventType::Error,
                serde_json::json!({"message": "file not found"}),
            ),
        ];
        let constraints = extract_failure_constraints(&events, 3);
        assert!(
            !constraints.is_empty(),
            "Should generate constraint for recurring error"
        );
    }

    #[test]
    fn isolated_error_no_constraint() {
        let events = vec![make_event(
            EventType::Error,
            serde_json::json!({"message": "timeout"}),
        )];
        let constraints = extract_failure_constraints(&events, 3);
        assert!(constraints.is_empty());
    }

    #[test]
    fn from_events_includes_failure_constraints() {
        let events = vec![
            make_event(
                EventType::Error,
                serde_json::json!({"message": "compile error"}),
            ),
            make_event(
                EventType::Error,
                serde_json::json!({"message": "compile error"}),
            ),
            make_event(
                EventType::Error,
                serde_json::json!({"message": "compile error"}),
            ),
        ];
        let summary = EpisodeSummary::from_events("r-1", None, "fix", &events);
        assert!(
            summary
                .machine_summary
                .learned_constraints
                .iter()
                .any(|c| c.contains("compile error")),
            "Should include failure-derived constraint in learned_constraints"
        );
    }

    // --- MemoryKind tests ---

    #[test]
    fn memory_kind_default_is_episodic() {
        assert_eq!(MemoryKind::default(), MemoryKind::Episodic);
    }

    #[test]
    fn memory_kind_survives_compaction() {
        assert!(!MemoryKind::Ephemeral.survives_compaction());
        assert!(!MemoryKind::Episodic.survives_compaction());
        assert!(MemoryKind::Reusable.survives_compaction());
        assert!(MemoryKind::Canonical.survives_compaction());
    }

    #[test]
    fn memory_kind_auto_evictable() {
        assert!(MemoryKind::Ephemeral.auto_evictable());
        assert!(MemoryKind::Episodic.auto_evictable());
        assert!(!MemoryKind::Reusable.auto_evictable());
        assert!(!MemoryKind::Canonical.auto_evictable());
    }

    #[test]
    fn memory_kind_serde_roundtrip() {
        for kind in &[
            MemoryKind::Ephemeral,
            MemoryKind::Episodic,
            MemoryKind::Reusable,
            MemoryKind::Canonical,
        ] {
            let json = serde_json::to_string(kind).unwrap();
            let back: MemoryKind = serde_json::from_str(&json).unwrap();
            assert_eq!(*kind, back);
        }
    }

    #[test]
    fn memory_kind_inferred_from_ttl() {
        assert_eq!(
            infer_memory_kind(&TtlPolicy::RunScoped),
            MemoryKind::Episodic
        );
        assert_eq!(
            infer_memory_kind(&TtlPolicy::TimeScoped { seconds: 3600 }),
            MemoryKind::Reusable
        );
        assert_eq!(
            infer_memory_kind(&TtlPolicy::Permanent),
            MemoryKind::Canonical
        );
    }

    #[test]
    fn episode_summary_has_memory_kind() {
        let events = vec![make_event(
            EventType::ConstraintLearned,
            serde_json::json!({"constraint": "no unwrap", "scope": "workspace-local"}),
        )];
        let summary = EpisodeSummary::from_events("r-1", None, "task", &events);
        assert_eq!(summary.memory_kind, MemoryKind::Canonical);
    }

    #[test]
    fn episode_summary_default_memory_kind_is_episodic() {
        let summary = EpisodeSummary::from_events("r-1", None, "task", &[]);
        assert_eq!(summary.memory_kind, MemoryKind::Episodic);
    }

    #[test]
    fn memory_kind_backward_compat_deserialization() {
        let mut val =
            serde_json::to_value(&EpisodeSummary::from_events("r-1", None, "t", &[])).unwrap();
        val.as_object_mut().unwrap().remove("memory_kind");
        let back: EpisodeSummary = serde_json::from_value(val).unwrap();
        assert_eq!(back.memory_kind, MemoryKind::Episodic);
    }

    // --- Hypothesis evidence tracking tests ---

    #[test]
    fn hypothesis_record_support_increases_confidence() {
        let mut h = Hypothesis::new("h-1", "bug in auth", "test fails");
        let initial = h.confidence;
        h.record_support("evt-1");
        assert!(h.confidence > initial);
        assert_eq!(h.evidence_for, 1);
        assert_eq!(h.evidence_against, 0);
    }

    #[test]
    fn hypothesis_record_contradiction_decreases_confidence() {
        let mut h = Hypothesis::new("h-1", "bug in auth", "test fails");
        let initial = h.confidence;
        h.record_contradiction("evt-1");
        assert!(h.confidence < initial);
        assert_eq!(h.evidence_for, 0);
        assert_eq!(h.evidence_against, 1);
    }

    #[test]
    fn hypothesis_auto_prunes_on_heavy_contradiction() {
        let mut h = Hypothesis::new("h-1", "bug", "reason");
        // 0 for, 3 against → should auto-prune (3 > 0*2, total >= 3)
        h.record_contradiction("evt-1");
        h.record_contradiction("evt-2");
        assert_eq!(h.status, HypothesisStatus::Active); // not yet
        h.record_contradiction("evt-3");
        assert_eq!(h.status, HypothesisStatus::Stale); // auto-pruned
    }

    #[test]
    fn hypothesis_no_prune_with_balanced_evidence() {
        let mut h = Hypothesis::new("h-1", "bug", "reason");
        h.record_support("evt-1");
        h.record_support("evt-2");
        h.record_contradiction("evt-3");
        h.record_contradiction("evt-4");
        // 2 for, 2 against: 2 > 2*2=4? No → should NOT prune
        assert_eq!(h.status, HypothesisStatus::Active);
    }

    #[test]
    fn hypothesis_confidence_with_laplace_smoothing() {
        let h = Hypothesis::new("h-1", "test", "reason");
        // No evidence: (0+1)/(0+0+2) = 0.5
        assert!((h.confidence - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn hypothesis_evidence_backward_compat() {
        let h = Hypothesis::new("h-1", "test", "reason");
        let json = serde_json::to_string(&h).unwrap();
        // Remove the new fields to simulate old data
        let mut val: serde_json::Value = serde_json::from_str(&json).unwrap();
        val.as_object_mut().unwrap().remove("evidence_for");
        val.as_object_mut().unwrap().remove("evidence_against");
        let back: Hypothesis = serde_json::from_value(val).unwrap();
        assert_eq!(back.evidence_for, 0);
        assert_eq!(back.evidence_against, 0);
    }

    #[test]
    fn infer_hypotheses_from_repeated_actions() {
        let events = vec![
            make_event(
                EventType::ToolCallCompleted,
                serde_json::json!({"tool_name": "edit", "file": "src/auth.rs"}),
            ),
            make_event(
                EventType::ToolCallCompleted,
                serde_json::json!({"tool_name": "edit", "file": "src/auth.rs"}),
            ),
            make_event(
                EventType::ToolCallCompleted,
                serde_json::json!({"tool_name": "edit", "file": "src/auth.rs"}),
            ),
        ];
        let hypotheses = infer_hypotheses_from_patterns(&events);
        assert!(
            !hypotheses.is_empty(),
            "Should infer hypothesis from repeated pattern"
        );
        assert_eq!(hypotheses[0].source, HypothesisSource::Inferred);
        assert_eq!(hypotheses[0].confidence, 0.3);
    }
}
