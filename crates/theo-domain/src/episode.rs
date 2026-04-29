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
#[derive(Default)]
pub enum MemoryKind {
    /// Runtime-only context, never persisted across episodes.
    /// Evicts on turn/episode boundary.
    Ephemeral,
    /// Session-scoped learnings from execution. Evicts by LRU + staleness.
    #[default]
    Episodic,
    /// Cross-session knowledge, persists, retrieved on demand.
    Reusable,
    /// Permanent facts (hash-keyed). Never auto-evicted.
    /// Inspired by Knowledge Objects: facts must not live only in context window.
    Canonical,
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
#[derive(Default)]
pub enum MemoryLifecycle {
    /// Runtime: always eligible for assembler, high priority.
    #[default]
    Active,
    /// Post-episode: eligible only if usefulness > threshold.
    Cooling,
    /// Long-term: never auto-assembled, lookup-only.
    Archived,
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
#[derive(Default)]
pub enum TtlPolicy {
    /// Deleted when the originating run ends.
    #[default]
    RunScoped,
    /// Deleted after the specified number of seconds.
    TimeScoped { seconds: u64 },
    /// Never automatically deleted.
    Permanent,
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
        let (window_start, window_end) = window_bounds(events);
        let key_actions = extract_key_actions(events);
        let affected_files = extract_affected_files(events);
        let mut learned_constraints = extract_learned_constraints(events);
        let unresolved = extract_unresolved_hypotheses(events);
        let successful_steps = extract_successful_steps(events);
        let failed_attempts = extract_failed_attempts(events);
        let outcome = derive_outcome(events);

        learned_constraints.extend(extract_failure_constraints(events, 3));
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

/// First/last event IDs framing the summary window.
fn window_bounds(events: &[DomainEvent]) -> (String, String) {
    let start = events
        .first()
        .map(|e| e.event_id.as_str().to_string())
        .unwrap_or_default();
    let end = events
        .last()
        .map(|e| e.event_id.as_str().to_string())
        .unwrap_or_default();
    (start, end)
}

/// Tool names from every `ToolCallCompleted` event, in order.
fn extract_key_actions(events: &[DomainEvent]) -> Vec<String> {
    events
        .iter()
        .filter(|e| e.event_type == crate::event::EventType::ToolCallCompleted)
        .filter_map(|e| {
            e.payload
                .get("tool_name")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .collect()
}

/// Distinct file paths mentioned in any event payload's `file` field.
fn extract_affected_files(events: &[DomainEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| {
            e.payload
                .get("file")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect()
}

/// Constraints emitted explicitly via `ConstraintLearned` events.
fn extract_learned_constraints(events: &[DomainEvent]) -> Vec<String> {
    events
        .iter()
        .filter(|e| e.event_type == crate::event::EventType::ConstraintLearned)
        .filter_map(|e| {
            e.payload
                .get("constraint")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .collect()
}

/// `HypothesisFormed` events that have NO matching `HypothesisInvalidated`.
fn extract_unresolved_hypotheses(events: &[DomainEvent]) -> Vec<String> {
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
    events
        .iter()
        .filter(|e| e.event_type == crate::event::EventType::HypothesisFormed)
        .filter(|e| !invalidated_refs.contains(e.event_id.as_str()))
        .filter_map(|e| {
            e.payload
                .get("hypothesis")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .collect()
}

/// `ToolCallCompleted` events flagged success=true, formatted as `tool` or `tool: file`.
fn extract_successful_steps(events: &[DomainEvent]) -> Vec<String> {
    events
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
        .collect()
}

/// Failed tool calls plus standalone `Error` events, formatted for the summary.
fn extract_failed_attempts(events: &[DomainEvent]) -> Vec<String> {
    let mut out: Vec<String> = events
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
    for e in events
        .iter()
        .filter(|e| e.event_type == crate::event::EventType::Error)
    {
        let msg = e
            .payload
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("error");
        out.push(msg.to_string());
    }
    out
}

/// `Partial` if any `Error` event OR any `ToolCallCompleted` with success=false; else `Success`.
fn derive_outcome(events: &[DomainEvent]) -> EpisodeOutcome {
    let has_errors = events
        .iter()
        .any(|e| e.event_type == crate::event::EventType::Error);
    let has_failures = events.iter().any(|e| {
        e.event_type == crate::event::EventType::ToolCallCompleted
            && e.payload.get("success").and_then(|v| v.as_bool()) == Some(false)
    });
    if has_errors || has_failures {
        EpisodeOutcome::Partial
    } else {
        EpisodeOutcome::Success
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
#[path = "episode_tests.rs"]
mod tests;
