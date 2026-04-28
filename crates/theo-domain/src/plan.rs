//! Schema-validated plan model — the SOTA Planning System core.
//!
//! Replaces `theo-agent-runtime::roadmap` (markdown string-matching parser)
//! with a typed JSON-canonical model:
//!
//! - `Plan` is the document root; serialized as JSON via serde.
//! - `Phase` groups related `PlanTask`s; one phase is "current" at any time.
//! - `PlanTask` is the executable unit with explicit `depends_on`.
//! - `Plan::validate()` enforces invariants (unique IDs, acyclic DAG).
//! - `Plan::topological_order()` orders tasks by dependencies (Kahn).
//! - `Plan::next_actionable_task()` returns the next pending task whose
//!   dependencies are all `Completed`.
//! - `Plan::to_markdown()` renders a *read-only* view; never parsed back.
//!
//! See `docs/plans/sota-planning-system.md` for the design rationale and the
//! meeting `20260426-122956-planning-system-sota-redesign.md` for decisions.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::identifiers::{PhaseId, PlanTaskId};

/// Current `Plan` schema version. Bump when an incompatible change is shipped.
///
/// Forward compatibility is preserved by `#[serde(default)]` on optional
/// fields. `load_plan` rejects any plan with `version > PLAN_FORMAT_VERSION`.
pub const PLAN_FORMAT_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Status enums
// ---------------------------------------------------------------------------

/// Lifecycle of a single `PlanTask`.
///
/// `#[non_exhaustive]` per `code-reviewer` D9 — adding new states (e.g.,
/// `Cancelled`) must not break downstream `match` arms in tools.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PlanTaskStatus {
    Pending,
    InProgress,
    Completed,
    Skipped,
    Blocked,
    Failed,
}

impl PlanTaskStatus {
    /// Returns `true` when the task is finished (success or otherwise).
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            PlanTaskStatus::Completed
                | PlanTaskStatus::Skipped
                | PlanTaskStatus::Failed
        )
    }

    /// Returns `true` when the task contributed a finished result.
    /// Used by `next_actionable_task` to decide whether a dependency is
    /// "satisfied" — `Skipped` and `Completed` both satisfy a dependency,
    /// `Failed` does not.
    pub fn satisfies_dependency(self) -> bool {
        matches!(
            self,
            PlanTaskStatus::Completed | PlanTaskStatus::Skipped
        )
    }
}

/// Lifecycle of a `Phase`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PhaseStatus {
    Pending,
    InProgress,
    Completed,
}

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

/// A decision recorded against a plan (rationale, ADR-style).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanDecision {
    pub decision: String,
    pub rationale: String,
    pub timestamp: u64,
}

/// One executable unit inside a `Phase`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanTask {
    pub id: PlanTaskId,
    pub title: String,
    pub status: PlanTaskStatus,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub dod: String,
    #[serde(default)]
    pub depends_on: Vec<PlanTaskId>,
    #[serde(default)]
    pub rationale: String,
    /// Free-form summary of what happened after the task ran. SOTA T1
    /// foundation for feedback-loop replanning.
    #[serde(default)]
    pub outcome: Option<String>,
    /// T7.1 — Run id of the agent that has reserved this task. `None`
    /// means the task is available to be claimed by any worker. Set by
    /// `Plan::claim_task` (CAS via `plan_store::save_plan_if_version`)
    /// and cleared by `Plan::release_task` once the worker finishes.
    #[serde(default)]
    pub assignee: Option<String>,
    /// T6.1 — Count of failed attempts on this task. Incremented by
    /// `Plan::record_failure`; reset by `Plan::reset_failure_count`.
    /// The auto-replan trigger compares this against a configurable
    /// threshold (default 3) — tasks exceeding it are surfaced via
    /// `Plan::tasks_exceeding_failure_threshold` so the agent loop
    /// can ask the LLM to mutate the plan via `plan_replan`.
    /// Backwards-compat: `#[serde(default)]` keeps existing plan.json
    /// files (without this field) loadable; they start at 0 attempts.
    #[serde(default)]
    pub failure_count: u32,
}

/// Group of tasks executed together; advances one at a time.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Phase {
    pub id: PhaseId,
    pub title: String,
    pub status: PhaseStatus,
    pub tasks: Vec<PlanTask>,
}

/// The schema-validated root document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Plan {
    pub version: u32,
    pub title: String,
    pub goal: String,
    pub current_phase: PhaseId,
    pub phases: Vec<Phase>,
    #[serde(default)]
    pub decisions: Vec<PlanDecision>,
    pub created_at: u64,
    pub updated_at: u64,
    /// T7.1 — Monotonic counter bumped on every successful save. Enables
    /// optimistic concurrency control: `plan_store::save_plan_if_version`
    /// rejects writes when the on-disk version is newer than the caller's
    /// last read, so two agents trying to claim the same task can be
    /// serialised by retrying.
    #[serde(default)]
    pub version_counter: u64,
}

// ---------------------------------------------------------------------------
// Multi-agent claim (T7.1)
// ---------------------------------------------------------------------------

/// Outcome of a `Plan::claim_task` attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClaimResult {
    /// Successfully reserved (or already owned by the same agent).
    Claimed,
    /// Another agent currently holds this task.
    AlreadyClaimed { by: String },
    /// The task id is not in the plan.
    NotFound,
    /// The task is already finished (Completed/Skipped/Failed/Blocked).
    Terminal,
}

impl ClaimResult {
    /// Returns true when the caller now owns the task (or already did).
    pub fn is_owned(&self) -> bool {
        matches!(self, ClaimResult::Claimed)
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Failures that can arise from the *content* of a Plan (independent of IO).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PlanValidationError {
    #[error("duplicate task ID: {0}")]
    DuplicateTaskId(PlanTaskId),
    #[error("duplicate phase ID: {0}")]
    DuplicatePhaseId(PhaseId),
    #[error("task {task_id} depends on non-existent task {missing_dep}")]
    InvalidDependency {
        task_id: PlanTaskId,
        missing_dep: PlanTaskId,
    },
    #[error("task {0} cannot depend on itself")]
    SelfDependency(PlanTaskId),
    #[error("dependency cycle detected")]
    CycleDetected,
    #[error("invalid phase reference: {0}")]
    InvalidPhaseRef(PhaseId),
    #[error("plan must contain at least one phase")]
    EmptyPlan,
    #[error("plan title must not be empty")]
    EmptyTitle,
}

/// Failures that can arise when loading or saving a plan.
///
/// `Io` wraps `std::io::Error` directly (D8 — code-reviewer) so callers
/// retain `ErrorKind` and source-chain context.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PlanError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid plan format: {0}")]
    InvalidFormat(String),
    #[error("unsupported plan version: found {found}, max supported {max_supported}")]
    UnsupportedVersion { found: u32, max_supported: u32 },
    #[error(transparent)]
    Validation(#[from] PlanValidationError),
    /// T7.1 — Compare-and-swap failure: on-disk plan was modified since
    /// the caller last read it. Caller should reload, re-apply the
    /// intent, and retry.
    #[error("version mismatch: expected {expected}, on-disk has {actual}")]
    VersionMismatch { expected: u64, actual: u64 },
}

// ---------------------------------------------------------------------------
// Plan: validation, traversal, rendering
// ---------------------------------------------------------------------------

impl Plan {
    /// Validates structural invariants:
    ///
    /// - Plan has at least one phase, non-empty title.
    /// - Phase IDs are unique.
    /// - Task IDs are unique across all phases.
    /// - Every `depends_on` reference points to an existing task.
    /// - No task depends on itself.
    /// - The dependency graph is acyclic.
    /// - `current_phase` references an existing phase.
    pub fn validate(&self) -> Result<(), PlanValidationError> {
        if self.title.trim().is_empty() {
            return Err(PlanValidationError::EmptyTitle);
        }
        if self.phases.is_empty() {
            return Err(PlanValidationError::EmptyPlan);
        }

        let mut phase_ids = HashSet::new();
        for phase in &self.phases {
            if !phase_ids.insert(phase.id) {
                return Err(PlanValidationError::DuplicatePhaseId(phase.id));
            }
        }

        if !phase_ids.contains(&self.current_phase) {
            return Err(PlanValidationError::InvalidPhaseRef(self.current_phase));
        }

        let mut task_ids: HashSet<PlanTaskId> = HashSet::new();
        for phase in &self.phases {
            for task in &phase.tasks {
                if !task_ids.insert(task.id) {
                    return Err(PlanValidationError::DuplicateTaskId(task.id));
                }
            }
        }

        for phase in &self.phases {
            for task in &phase.tasks {
                for dep in &task.depends_on {
                    if *dep == task.id {
                        return Err(PlanValidationError::SelfDependency(task.id));
                    }
                    if !task_ids.contains(dep) {
                        return Err(PlanValidationError::InvalidDependency {
                            task_id: task.id,
                            missing_dep: *dep,
                        });
                    }
                }
            }
        }

        // Cycle detection via Kahn's algorithm — `topological_order` returns
        // CycleDetected if the DAG is not acyclic.
        self.topological_order()?;
        Ok(())
    }

    /// Returns all tasks across phases, preserving authoring order.
    pub fn all_tasks(&self) -> Vec<&PlanTask> {
        self.phases
            .iter()
            .flat_map(|p| p.tasks.iter())
            .collect()
    }

    /// Returns all tasks across phases mutably (preserves authoring order).
    pub fn all_tasks_mut(&mut self) -> Vec<&mut PlanTask> {
        self.phases
            .iter_mut()
            .flat_map(|p| p.tasks.iter_mut())
            .collect()
    }

    /// Look up a task by id (read-only).
    pub fn find_task(&self, id: PlanTaskId) -> Option<&PlanTask> {
        self.phases
            .iter()
            .flat_map(|p| p.tasks.iter())
            .find(|t| t.id == id)
    }

    /// Look up a task by id (mutable).
    pub fn find_task_mut(&mut self, id: PlanTaskId) -> Option<&mut PlanTask> {
        self.phases
            .iter_mut()
            .flat_map(|p| p.tasks.iter_mut())
            .find(|t| t.id == id)
    }

    /// Look up a phase by id (read-only).
    pub fn find_phase(&self, id: PhaseId) -> Option<&Phase> {
        self.phases.iter().find(|p| p.id == id)
    }

    /// T7.1 — Reserve a task for a specific agent. Returns `ClaimResult`:
    ///
    /// - `Claimed` when the task was unclaimed and now has `assignee = Some(agent)`.
    /// - `AlreadyClaimed { by }` when another agent already holds it.
    /// - `NotFound` when the task id is unknown.
    /// - `Terminal` when the task is in a terminal state (Completed/Skipped/
    ///   Failed/Blocked) — claiming finished work is a no-op.
    ///
    /// The plan's `version_counter` is bumped on success so callers using
    /// `plan_store::save_plan_if_version` can detect concurrent writers.
    pub fn claim_task(
        &mut self,
        task_id: PlanTaskId,
        agent_id: impl Into<String>,
    ) -> crate::plan::ClaimResult {
        let agent_id = agent_id.into();
        let task = match self.find_task_mut(task_id) {
            Some(t) => t,
            None => return ClaimResult::NotFound,
        };
        if task.status.is_terminal() {
            return ClaimResult::Terminal;
        }
        if let Some(by) = &task.assignee {
            if by == &agent_id {
                return ClaimResult::Claimed; // idempotent self-claim
            }
            return ClaimResult::AlreadyClaimed { by: by.clone() };
        }
        task.assignee = Some(agent_id);
        self.version_counter = self.version_counter.saturating_add(1);
        ClaimResult::Claimed
    }

    /// T7.1 — Release a previously claimed task.
    ///
    /// Returns `true` if the assignee was cleared. The release is a no-op
    /// (returning `false`) when:
    /// - the task id is unknown
    /// - the task wasn't claimed
    /// - the claim belongs to a different agent (defensive — only the
    ///   owner can release)
    ///
    /// Bumps `version_counter` only on actual mutation.
    pub fn release_task(
        &mut self,
        task_id: PlanTaskId,
        agent_id: impl Into<String>,
    ) -> bool {
        let agent_id = agent_id.into();
        let task = match self.find_task_mut(task_id) {
            Some(t) => t,
            None => return false,
        };
        match &task.assignee {
            Some(by) if by == &agent_id => {
                task.assignee = None;
                self.version_counter = self.version_counter.saturating_add(1);
                true
            }
            _ => false,
        }
    }

    /// T6.1 — Increment the failure_count of `task_id` by 1 and bump
    /// the plan's `version_counter`. Returns the new failure_count, or
    /// `None` when the task id is unknown. Caller chains with
    /// `tasks_exceeding_failure_threshold` to decide whether the agent
    /// should ask the LLM for a `plan_replan` patch.
    ///
    /// Does NOT change the task's `status` — that's the caller's
    /// concern (some failures should keep the task pending for retry;
    /// others should mark it Failed). Decoupling lets the auto-replan
    /// trigger fire even on retryable failures.
    pub fn record_failure(&mut self, task_id: PlanTaskId) -> Option<u32> {
        let task = self.find_task_mut(task_id)?;
        task.failure_count = task.failure_count.saturating_add(1);
        let new_count = task.failure_count;
        self.version_counter = self.version_counter.saturating_add(1);
        Some(new_count)
    }

    /// T6.1 — Reset the failure_count of `task_id` back to 0 (e.g.
    /// after a successful retry). Returns true when the task existed.
    pub fn reset_failure_count(&mut self, task_id: PlanTaskId) -> bool {
        let Some(task) = self.find_task_mut(task_id) else {
            return false;
        };
        if task.failure_count != 0 {
            task.failure_count = 0;
            self.version_counter = self.version_counter.saturating_add(1);
        }
        true
    }

    /// T6.1 — List task ids whose `failure_count` is `>= threshold`.
    /// Used by the agent loop to decide which tasks need an LLM-
    /// generated `PlanPatch` (typically `SkipTask` with rationale).
    /// Returns ids in `all_tasks()` order so callers see deterministic
    /// output across runs.
    pub fn tasks_exceeding_failure_threshold(&self, threshold: u32) -> Vec<PlanTaskId> {
        if threshold == 0 {
            // Threshold 0 would match every task — not useful, return
            // empty so callers don't accidentally trigger replan on
            // every fresh task.
            return Vec::new();
        }
        self.all_tasks()
            .into_iter()
            .filter(|t| t.failure_count >= threshold)
            .map(|t| t.id)
            .collect()
    }

    /// T7.1 — Iterator over tasks that are unclaimed AND `Pending` AND
    /// have all dependencies satisfied. Used by parallel workers to pick
    /// the next task to claim.
    pub fn next_unclaimed_actionable_task(&self) -> Option<&PlanTask> {
        let order = self.topological_order().ok()?;
        let by_id: std::collections::HashMap<PlanTaskId, &PlanTask> = self
            .all_tasks()
            .into_iter()
            .map(|t| (t.id, t))
            .collect();

        for id in order {
            let task = by_id.get(&id)?;
            if task.status != PlanTaskStatus::Pending {
                continue;
            }
            if task.assignee.is_some() {
                continue;
            }
            let deps_ok = task.depends_on.iter().all(|d| {
                by_id
                    .get(d)
                    .map(|t| t.status.satisfies_dependency())
                    .unwrap_or(false)
            });
            if deps_ok {
                return Some(*task);
            }
        }
        None
    }

    /// T6.1 / D4 — Apply a `PlanPatch` to mutate the plan in place.
    ///
    /// On `Err`, the plan is **left unchanged** (atomicity guarantee). The
    /// operation works on a clone first, validates, and only swaps when the
    /// post-validation passes.
    pub fn apply_patch(
        &mut self,
        patch: &crate::plan_patch::PlanPatch,
    ) -> Result<(), crate::plan_patch::PatchError> {
        use crate::plan_patch::{InsertPosition, PatchError, PlanPatch};

        let mut updated = self.clone();
        match patch {
            PlanPatch::AddTask {
                phase,
                task,
                position,
            } => {
                let phase_obj = updated
                    .phases
                    .iter_mut()
                    .find(|p| p.id == *phase)
                    .ok_or(PatchError::PhaseNotFound(*phase))?;
                let pos = match position {
                    InsertPosition::End => phase_obj.tasks.len(),
                    InsertPosition::Begin => 0,
                    InsertPosition::AfterTask { id } => phase_obj
                        .tasks
                        .iter()
                        .position(|t| t.id == *id)
                        .map(|i| i + 1)
                        .ok_or(PatchError::AnchorNotInPhase {
                            anchor: *id,
                            phase: *phase,
                        })?,
                };
                phase_obj.tasks.insert(pos, task.clone());
            }
            PlanPatch::RemoveTask { id } => {
                // Orphan check: no other task may depend on the removed one.
                let dependents: Vec<PlanTaskId> = updated
                    .phases
                    .iter()
                    .flat_map(|p| p.tasks.iter())
                    .filter(|t| t.depends_on.contains(id))
                    .map(|t| t.id)
                    .collect();
                if !dependents.is_empty() {
                    return Err(PatchError::RemoveWouldOrphan(*id));
                }
                let mut removed = false;
                for phase in updated.phases.iter_mut() {
                    if let Some(idx) = phase.tasks.iter().position(|t| t.id == *id) {
                        phase.tasks.remove(idx);
                        removed = true;
                        break;
                    }
                }
                if !removed {
                    return Err(PatchError::TaskNotFound(*id));
                }
            }
            PlanPatch::EditTask { id, edits } => {
                if edits.is_empty() {
                    return Err(PatchError::Empty);
                }
                let task = updated
                    .find_task_mut(*id)
                    .ok_or(PatchError::TaskNotFound(*id))?;
                if let Some(t) = &edits.title {
                    task.title = t.clone();
                }
                if let Some(s) = edits.status {
                    task.status = s;
                }
                if let Some(d) = &edits.description {
                    task.description = d.clone();
                }
                if let Some(d) = &edits.dod {
                    task.dod = d.clone();
                }
                if let Some(r) = &edits.rationale {
                    task.rationale = r.clone();
                }
                if let Some(o) = &edits.outcome {
                    task.outcome = o.clone();
                }
                if let Some(f) = &edits.files {
                    task.files = f.clone();
                }
            }
            PlanPatch::ReorderDeps { id, new_deps } => {
                let task = updated
                    .find_task_mut(*id)
                    .ok_or(PatchError::TaskNotFound(*id))?;
                task.depends_on = new_deps.clone();
            }
            PlanPatch::SkipTask { id, rationale } => {
                let task = updated
                    .find_task_mut(*id)
                    .ok_or(PatchError::TaskNotFound(*id))?;
                task.status = PlanTaskStatus::Skipped;
                task.outcome = Some(rationale.clone());
            }
        }

        // Re-validate the patched plan; reject if it broke any invariant
        // (cycle introduced, orphan dep, duplicate id, etc.).
        updated.validate()?;
        *self = updated;
        Ok(())
    }

    /// Kahn's algorithm — yields task IDs in a valid execution order.
    ///
    /// Returns `Err(CycleDetected)` when the dependency graph contains a
    /// cycle. The order is deterministic: tasks with the lowest ID are
    /// dequeued first when multiple are ready, so two `Plan`s with the same
    /// shape produce the same ordering.
    pub fn topological_order(&self) -> Result<Vec<PlanTaskId>, PlanValidationError> {
        let tasks: Vec<&PlanTask> = self.all_tasks();
        let total = tasks.len();
        let mut indegree: HashMap<PlanTaskId, usize> = HashMap::with_capacity(total);
        // Adjacency: dep -> list of tasks that depend on it.
        let mut forward: HashMap<PlanTaskId, Vec<PlanTaskId>> = HashMap::with_capacity(total);

        for task in &tasks {
            indegree.entry(task.id).or_insert(0);
            forward.entry(task.id).or_default();
        }
        for task in &tasks {
            for dep in &task.depends_on {
                if !indegree.contains_key(dep) {
                    return Err(PlanValidationError::InvalidDependency {
                        task_id: task.id,
                        missing_dep: *dep,
                    });
                }
                *indegree.entry(task.id).or_insert(0) += 1;
                forward.entry(*dep).or_default().push(task.id);
            }
        }

        // Use a sorted ready-set so the resulting order is stable.
        let mut ready: VecDeque<PlanTaskId> = {
            let mut zero: Vec<PlanTaskId> = indegree
                .iter()
                .filter(|(_, deg)| **deg == 0)
                .map(|(id, _)| *id)
                .collect();
            zero.sort();
            zero.into()
        };

        let mut order: Vec<PlanTaskId> = Vec::with_capacity(total);
        while let Some(id) = ready.pop_front() {
            order.push(id);
            // Lowering indegree for everyone that depended on `id`.
            if let Some(downstream) = forward.get(&id) {
                let mut newly_ready: Vec<PlanTaskId> = Vec::new();
                for next_id in downstream {
                    if let Some(deg) = indegree.get_mut(next_id) {
                        *deg = deg.saturating_sub(1);
                        if *deg == 0 {
                            newly_ready.push(*next_id);
                        }
                    }
                }
                newly_ready.sort();
                for n in newly_ready {
                    ready.push_back(n);
                }
            }
        }

        if order.len() != total {
            return Err(PlanValidationError::CycleDetected);
        }
        Ok(order)
    }

    /// First `Pending` task whose dependencies are all
    /// `satisfies_dependency()`. Returns in the topological order produced
    /// by `topological_order()`. Returns `None` when no such task exists.
    ///
    /// `InProgress` tasks are *not* re-issued — caller is responsible for
    /// transitioning them out of that state on retry.
    pub fn next_actionable_task(&self) -> Option<&PlanTask> {
        let order = self.topological_order().ok()?;
        let by_id: HashMap<PlanTaskId, &PlanTask> = self
            .all_tasks()
            .into_iter()
            .map(|t| (t.id, t))
            .collect();

        for id in order {
            let task = by_id.get(&id)?;
            if task.status != PlanTaskStatus::Pending {
                continue;
            }
            let deps_ok = task.depends_on.iter().all(|d| {
                by_id
                    .get(d)
                    .map(|t| t.status.satisfies_dependency())
                    .unwrap_or(false)
            });
            if deps_ok {
                return Some(*task);
            }
        }
        None
    }

    /// Renders a read-only markdown view. **Never parsed back** — purely
    /// for terminal/UI display and for injection into the LLM system
    /// prompt (Manus principle: attention manipulation).
    pub fn to_markdown(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("# {}\n\n", self.title));
        if !self.goal.trim().is_empty() {
            s.push_str(&format!("**Goal:** {}\n\n", self.goal));
        }
        for phase in &self.phases {
            let marker = match phase.status {
                PhaseStatus::Completed => "✅",
                PhaseStatus::InProgress => "🔄",
                PhaseStatus::Pending => "⏳",
            };
            s.push_str(&format!(
                "## {} Phase {} — {}\n\n",
                marker, phase.id.as_u32(), phase.title
            ));
            for task in &phase.tasks {
                let box_marker = match task.status {
                    PlanTaskStatus::Completed => "[x]",
                    PlanTaskStatus::Skipped => "[~]",
                    PlanTaskStatus::Failed => "[!]",
                    PlanTaskStatus::Blocked => "[#]",
                    PlanTaskStatus::InProgress => "[>]",
                    PlanTaskStatus::Pending => "[ ]",
                };
                s.push_str(&format!(
                    "- {} **{}**: {}\n",
                    box_marker, task.id, task.title
                ));
                if !task.depends_on.is_empty() {
                    let deps: Vec<String> =
                        task.depends_on.iter().map(|d| format!("{}", d)).collect();
                    s.push_str(&format!("  - depends on: {}\n", deps.join(", ")));
                }
                if !task.dod.trim().is_empty() {
                    s.push_str(&format!("  - DoD: {}\n", task.dod));
                }
            }
            s.push('\n');
        }
        s
    }

    /// Builds the prompt fed to the agent for a specific task.
    ///
    /// Mirrors the existing `RoadmapTask::to_agent_prompt` semantics so the
    /// migration is transparent at the runtime layer.
    pub fn task_to_agent_prompt(&self, task: &PlanTask) -> String {
        let mut prompt = format!("## {}: {}\n", task.id, task.title);
        if !task.files.is_empty() {
            prompt.push_str(&format!("Files: {}\n", task.files.join(", ")));
        }
        if !task.description.is_empty() {
            prompt.push_str(&format!("\n{}\n", task.description));
        }
        if !task.depends_on.is_empty() {
            let deps: Vec<String> = task.depends_on.iter().map(|d| format!("{}", d)).collect();
            prompt.push_str(&format!("\n**Depends on**: {}\n", deps.join(", ")));
        }
        if !task.dod.trim().is_empty() {
            prompt.push_str(&format!(
                "\n**Definition of Done**: {}\n\
                 Verify this DoD is met before calling done().\n",
                task.dod
            ));
        }
        if !task.rationale.trim().is_empty() {
            prompt.push_str(&format!("\n**Rationale**: {}\n", task.rationale));
        }
        if !self.goal.trim().is_empty() {
            prompt.push_str(&format!("\n**Plan goal**: {}\n", self.goal));
        }
        prompt
    }
}

// ---------------------------------------------------------------------------
// Tests (TDD — RED-GREEN per docs/plans/sota-planning-system.md §TDD Plan)
// ---------------------------------------------------------------------------


// Sibling tests split per-feature (T3.4 of code-hygiene-5x5).
#[cfg(test)]
#[path = "plan_test_helpers.rs"]
mod plan_test_helpers;
#[cfg(test)]
#[path = "plan_schema_tests.rs"]
mod plan_schema_tests;
#[cfg(test)]
#[path = "plan_patch_tests.rs"]
mod plan_patch_tests;
#[cfg(test)]
#[path = "plan_failure_tests.rs"]
mod plan_failure_tests;
#[cfg(test)]
#[path = "plan_claim_tests.rs"]
mod plan_claim_tests;
#[cfg(test)]
#[path = "plan_legacy_tests.rs"]
mod plan_legacy_tests;
