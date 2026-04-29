//! Pilot — autonomous development loop.
//!
//! Orchestrates AgentLoop in continuous cycles until a "promise" is fulfilled.
//! Inspired by Ralph patterns: dual-condition exit gate, circuit breaker,
//! git-based progress tracking, rate limiting.
//!
//! Pilot is a pure addition — zero changes to RunEngine or AgentLoop.

mod git;
mod run_loop;
mod types;

use git::{GitProgress, detect_git_progress, get_git_sha};
pub use types::{CircuitBreakerState, ExitReason, PilotResult};

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;

use crate::agent_loop::{AgentLoop, AgentResult};
use crate::config::AgentConfig;
use crate::event_bus::EventBus;
use crate::plan_store;
use crate::roadmap;
use theo_domain::identifiers::PlanTaskId;
use theo_domain::plan::{Plan, PlanTaskStatus};
use theo_infra_llm::types::Message;
use theo_tooling::registry::create_default_registry;

// ---------------------------------------------------------------------------
// PilotConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct PilotConfig {
    /// Max loop iterations total. 0 = use default (50).
    #[serde(default = "default_max_total_calls")]
    pub max_total_calls: usize,
    /// Max loop iterations per hour.
    #[serde(default = "default_max_loops_per_hour")]
    pub max_loops_per_hour: usize,
    /// Consecutive completion signals needed for PromiseFulfilled exit.
    #[serde(default = "default_exit_signal_threshold")]
    pub exit_signal_threshold: usize,
    /// Consecutive loops without git progress before circuit breaker opens.
    #[serde(default = "default_cb_no_progress")]
    pub circuit_breaker_no_progress: usize,
    /// Consecutive loops with same error before circuit breaker opens.
    #[serde(default = "default_cb_same_error")]
    pub circuit_breaker_same_error: usize,
    /// Seconds before circuit breaker transitions Open → HalfOpen.
    #[serde(default = "default_cb_cooldown")]
    pub circuit_breaker_cooldown_secs: u64,
    /// T6.1 — Failure count at which a task is flagged for replan.
    /// When `record_failure` bumps a task to or past this threshold,
    /// the pilot logs a `replan_threshold_exceeded` event and the
    /// operator (or a future auto-replan advisor) can decide to
    /// mutate the plan via `plan_replan`. Default 3 — matches the
    /// SOTA-tier plan target.
    #[serde(default = "default_replan_failure_threshold")]
    pub replan_failure_threshold: u32,
}

fn default_max_total_calls() -> usize {
    50
}
fn default_max_loops_per_hour() -> usize {
    100
}
fn default_exit_signal_threshold() -> usize {
    2
}
fn default_cb_no_progress() -> usize {
    3
}
fn default_cb_same_error() -> usize {
    5
}
fn default_cb_cooldown() -> u64 {
    300
}
fn default_replan_failure_threshold() -> u32 {
    3
}

impl Default for PilotConfig {
    fn default() -> Self {
        Self {
            max_total_calls: default_max_total_calls(),
            max_loops_per_hour: default_max_loops_per_hour(),
            exit_signal_threshold: default_exit_signal_threshold(),
            circuit_breaker_no_progress: default_cb_no_progress(),
            circuit_breaker_same_error: default_cb_same_error(),
            circuit_breaker_cooldown_secs: default_cb_cooldown(),
            replan_failure_threshold: default_replan_failure_threshold(),
        }
    }
}

impl PilotConfig {
    /// Load from .theo/config.toml [pilot] section.
    pub fn load(project_dir: &Path) -> Self {
        let config_path = project_dir.join(".theo").join("config.toml");
        if !config_path.exists() {
            return Self::default();
        }

        #[derive(Deserialize, Default)]
        struct Wrapper {
            pilot: Option<PilotConfig>,
        }

        match std::fs::read_to_string(&config_path) {
            Ok(content) => match toml::from_str::<Wrapper>(&content) {
                Ok(w) => w.pilot.unwrap_or_default(),
                Err(_) => Self::default(),
            },
            Err(_) => Self::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Fix Plan Parser (delegates to roadmap::parse_checkbox_progress)
// ---------------------------------------------------------------------------

fn parse_fix_plan(project_dir: &Path) -> (usize, usize) {
    let path = project_dir.join(".theo").join("fix_plan.md");
    roadmap::parse_checkbox_progress_from_file(&path)
}

// ---------------------------------------------------------------------------
// Promise Loader
// ---------------------------------------------------------------------------

/// Hard cap (in bytes) on the promise loaded from `.theo/PROMPT.md`
/// before it joins the system prompt. 8 KiB is enough for a structured
/// task description (~5 pages) but small enough to bound the prompt
/// budget and limit the blast-radius of an attacker-controlled
/// repository (T2.5 / find_p6_004).
pub const MAX_PROMPT_MD_BYTES: usize = 8 * 1024;

/// Load promise from .theo/PROMPT.md if no inline promise provided.
///
/// **Security (T2.5 / find_p6_004 / D5):** the file is committer-
/// controlled and reaches the LLM verbatim today. We strip known
/// LLM-injection tokens and apply a [`MAX_PROMPT_MD_BYTES`] cap before
/// returning the string. Both the cap and the strip are silent —
/// callers cannot tell whether either fired.
pub fn load_promise(project_dir: &Path) -> Option<String> {
    let path = project_dir.join(".theo").join("PROMPT.md");
    let raw = std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())?;
    let stripped = theo_domain::prompt_sanitizer::strip_injection_tokens(&raw);
    let capped = theo_domain::prompt_sanitizer::char_boundary_truncate(
        &stripped,
        MAX_PROMPT_MD_BYTES,
    );
    Some(capped)
}

// ---------------------------------------------------------------------------
// PilotLoop
// ---------------------------------------------------------------------------

pub struct PilotLoop {
    agent_config: AgentConfig,
    pilot_config: PilotConfig,
    project_dir: PathBuf,
    promise: String,
    /// Definition of Done — criteria that must be met for the promise to be fulfilled.
    /// When set, the agent sees this as acceptance criteria and only calls done()
    /// when ALL criteria are satisfied.
    complete: Option<String>,
    parent_event_bus: Arc<EventBus>,
    session_messages: Vec<Message>,

    // Tracking
    loop_count: usize,
    total_tokens: u64,
    total_files_edited: Vec<String>,

    // Rate limiting
    calls_this_hour: usize,
    hour_start: std::time::Instant,

    // Exit detection
    consecutive_completion_signals: usize,
    consecutive_no_progress: usize,
    consecutive_same_error: usize,
    last_error: Option<String>,

    // Circuit breaker
    circuit_state: CircuitBreakerState,
    circuit_open_since: Option<std::time::Instant>,

    // Git progress
    last_git_sha: Option<String>,

    // Interrupt flag
    interrupted: Arc<std::sync::atomic::AtomicBool>,

    // GRAPHCTX — shared across pilot loops (read-only after init)
    graph_context: Option<Arc<dyn theo_domain::graph_context::GraphContextProvider>>,

    // Heuristic reflector for failure classification and corrective guidance
    reflector: crate::reflector::HeuristicReflector,

    // Evolution loop — structured retry with reflection between attempts
    evolution: crate::evolution::EvolutionLoop,

    /// T6.1 — Optional auto-replan advisor. When `Some` AND a task's
    /// failure_count breaches `pilot_config.replan_failure_threshold`,
    /// the pilot calls `advisor.propose(...)` and applies the
    /// returned `PlanPatch` automatically. `None` (the default)
    /// keeps the existing log-only behaviour: the operator (or the
    /// agent itself via `plan_failure_status` + `plan_replan`)
    /// decides what to do.
    replan_advisor: Option<Arc<dyn theo_domain::plan_patch::ReplanAdvisor>>,
}

const MAX_SESSION_MESSAGES: usize = 100;

impl PilotLoop {
    pub fn new(
        agent_config: AgentConfig,
        pilot_config: PilotConfig,
        project_dir: PathBuf,
        promise: String,
        complete: Option<String>,
        parent_event_bus: Arc<EventBus>,
    ) -> Self {
        Self {
            agent_config,
            pilot_config,
            project_dir,
            promise,
            complete,
            parent_event_bus,
            session_messages: Vec::new(),
            loop_count: 0,
            total_tokens: 0,
            total_files_edited: Vec::new(),
            calls_this_hour: 0,
            hour_start: std::time::Instant::now(),
            consecutive_completion_signals: 0,
            consecutive_no_progress: 0,
            consecutive_same_error: 0,
            last_error: None,
            circuit_state: CircuitBreakerState::Closed,
            circuit_open_since: None,
            last_git_sha: None,
            interrupted: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            graph_context: None,
            reflector: crate::reflector::HeuristicReflector::new(),
            evolution: crate::evolution::EvolutionLoop::new(),
            replan_advisor: None,
        }
    }

    /// Set the graph context provider for code intelligence.
    pub fn with_graph_context(
        mut self,
        provider: Arc<dyn theo_domain::graph_context::GraphContextProvider>,
    ) -> Self {
        self.graph_context = Some(provider);
        self
    }

    /// T6.1 — Wire an auto-replan advisor. When set, threshold
    /// breaches trigger an automatic LLM-driven `Plan::apply_patch`
    /// on the proposed `PlanPatch`. When unset, threshold breaches
    /// are logged and the agent / operator must call `plan_replan`
    /// manually (the existing safe default).
    pub fn with_replan_advisor(
        mut self,
        advisor: Arc<dyn theo_domain::plan_patch::ReplanAdvisor>,
    ) -> Self {
        self.replan_advisor = Some(advisor);
        self
    }

    /// Returns a clone of the interrupt flag for external signal handlers.
    pub fn interrupt_flag(&self) -> Arc<std::sync::atomic::AtomicBool> {
        self.interrupted.clone()
    }

    /// Run the autonomous pilot loop.
    pub async fn run(&mut self) -> PilotResult {
        self.last_git_sha = get_git_sha(&self.project_dir).await;

        loop {
            if let Some(reason) = self.check_pre_loop_guards() {
                return self.build_result(reason);
            }

            let (completed, total) = parse_fix_plan(&self.project_dir);
            self.loop_count += 1;

            let sha_before = get_git_sha(&self.project_dir).await;
            let task = self.build_loop_prompt(completed, total);

            // Fresh per-iteration event bus + agent (isolation).
            let loop_bus = self.build_iteration_bus();
            let registry = create_default_registry();
            let mut agent = AgentLoop::new(self.agent_config.clone(), registry);
            if let Some(ref gc) = self.graph_context {
                agent = agent.with_graph_context(gc.clone());
            }

            let result = agent
                .run_with_history(
                    &task,
                    &self.project_dir,
                    self.session_messages.clone(),
                    Some(loop_bus),
                )
                .await;

            self.track_tokens_and_files(&result);
            self.record_exchange(&task, &result);

            let progress = detect_git_progress(&self.project_dir, &sha_before).await;
            self.last_git_sha = get_git_sha(&self.project_dir).await;
            self.update_counters(&result, &progress);
            self.record_evolution_attempt(&result);
            self.publish_loop_summary(&result);

            if let Some(reason) = self.evaluate_exit(&result) {
                return self.build_result(reason);
            }
        }
    }

    /// Execute tasks from a roadmap file sequentially.
    /// Each task becomes one pilot loop iteration with the task prompt.
    /// After successful execution, the task is marked ✅ in the roadmap file.
    pub async fn run_from_roadmap(&mut self, roadmap_path: &Path) -> PilotResult {
        let tasks = match roadmap::parse_roadmap(roadmap_path) {
            Ok(t) => t,
            Err(e) => {
                return self
                    .build_result(ExitReason::Error(format!("Failed to parse roadmap: {e}")));
            }
        };

        let pending: Vec<_> = tasks.iter().filter(|t| !t.completed).collect();
        if pending.is_empty() {
            return self.build_result(ExitReason::FixPlanComplete);
        }

        self.last_git_sha = get_git_sha(&self.project_dir).await;

        for task in &pending {
            if let Some(reason) = self.check_core_guards() {
                return self.build_result(reason);
            }

            self.loop_count += 1;
            let sha_before = get_git_sha(&self.project_dir).await;
            let task_prompt = task.to_agent_prompt();

            let loop_bus = self.build_iteration_bus();
            let registry = create_default_registry();
            let mut agent = AgentLoop::new(self.agent_config.clone(), registry);
            if let Some(ref gc) = self.graph_context {
                agent = agent.with_graph_context(gc.clone());
            }

            let result = agent
                .run_with_history(
                    &task_prompt,
                    &self.project_dir,
                    self.session_messages.clone(),
                    Some(loop_bus),
                )
                .await;

            self.track_tokens_and_files(&result);
            self.record_exchange(&task_prompt, &result);

            let progress = detect_git_progress(&self.project_dir, &sha_before).await;
            self.last_git_sha = get_git_sha(&self.project_dir).await;
            self.update_counters(&result, &progress);

            if result.success {
                let _ = roadmap::mark_task_completed(roadmap_path, task.number);
            }
        }

        self.build_result(ExitReason::PromiseFulfilled)
    }

    /// Execute tasks from a JSON plan file sequentially, respecting the
    /// dependency DAG. Each `next_actionable_task()` becomes one pilot
    /// loop iteration; status transitions are persisted between iterations.
    ///
    /// SOTA Planning System replacement for `run_from_roadmap`.
    pub async fn run_from_plan(&mut self, plan_path: &Path) -> PilotResult {
        let mut plan = match plan_store::load_plan(plan_path) {
            Ok(p) => p,
            Err(e) => {
                return self.build_result(ExitReason::Error(format!(
                    "Failed to load plan: {e}"
                )));
            }
        };

        if plan.next_actionable_task().is_none() {
            // Plan is fully resolved (every task is terminal).
            return self.build_result(ExitReason::FixPlanComplete);
        }

        self.last_git_sha = get_git_sha(&self.project_dir).await;

        // Re-evaluate next-actionable each iteration. The status may have
        // moved forward (Pending → Completed) and a new task may now have
        // its dependencies satisfied.
        while let Some(task) = plan.next_actionable_task().cloned() {
            if let Some(reason) = self.check_core_guards() {
                return self.build_result(reason);
            }

            // Mark in_progress + persist before running the agent.
            update_task_status(&mut plan, task.id, PlanTaskStatus::InProgress);
            plan.updated_at = theo_domain::clock::now_millis();
            if let Err(e) = plan_store::save_plan(plan_path, &plan) {
                tracing::warn!("Failed to save plan progress: {e}");
            }

            self.loop_count += 1;
            let sha_before = get_git_sha(&self.project_dir).await;
            let prompt = plan.task_to_agent_prompt(&task);

            let loop_bus = self.build_iteration_bus();
            let registry = create_default_registry();
            let mut agent = AgentLoop::new(self.agent_config.clone(), registry);
            if let Some(ref gc) = self.graph_context {
                agent = agent.with_graph_context(gc.clone());
            }

            let result = agent
                .run_with_history(
                    &prompt,
                    &self.project_dir,
                    self.session_messages.clone(),
                    Some(loop_bus),
                )
                .await;

            self.track_tokens_and_files(&result);
            self.record_exchange(&prompt, &result);

            let progress = detect_git_progress(&self.project_dir, &sha_before).await;
            self.last_git_sha = get_git_sha(&self.project_dir).await;
            self.update_counters(&result, &progress);

            // Update task status based on result, preserving outcome summary.
            let new_status = if result.success {
                PlanTaskStatus::Completed
            } else {
                PlanTaskStatus::Failed
            };
            update_task_status(&mut plan, task.id, new_status);
            update_task_outcome(&mut plan, task.id, result.summary.clone());

            self.handle_failure_or_success(&mut plan, task.id, &result).await;

            plan.updated_at = theo_domain::clock::now_millis();
            if let Err(e) = plan_store::save_plan(plan_path, &plan) {
                tracing::warn!("Failed to save plan completion: {e}");
            }
        }

        self.build_result(ExitReason::PromiseFulfilled)
    }

    /// T6.1 — auto-replan trigger. On failure, bump `failure_count` and
    /// (optionally) call the advisor for a recovery patch. On success,
    /// reset the counter so flaky-but-eventually-green tasks don't carry
    /// their history forever.
    async fn handle_failure_or_success(
        &self,
        plan: &mut theo_domain::plan::Plan,
        task_id: theo_domain::identifiers::PlanTaskId,
        result: &AgentResult,
    ) {
        if result.success {
            plan.reset_failure_count(task_id);
            return;
        }
        let Some(count) = plan.record_failure(task_id) else {
            return;
        };
        let threshold = self.pilot_config.replan_failure_threshold;
        if threshold == 0 || count < threshold {
            return;
        }
        let summary = result.summary.clone();
        let auto_applied = self.try_auto_replan(plan, task_id, count, threshold, summary).await;
        if !auto_applied {
            tracing::warn!(
                task_id = task_id.0,
                failure_count = count,
                threshold = threshold,
                "replan_threshold_exceeded — task is stuck; consider plan_replan with SkipTask or EditTask",
            );
        }
    }

    async fn try_auto_replan(
        &self,
        plan: &mut theo_domain::plan::Plan,
        task_id: theo_domain::identifiers::PlanTaskId,
        count: u32,
        threshold: u32,
        summary: String,
    ) -> bool {
        let Some(advisor) = self.replan_advisor.clone() else {
            return false;
        };
        // 5 s timeout: unresponsive advisor shouldn't pin the pilot.
        let proposal = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            advisor.propose(plan, task_id, &summary),
        )
        .await;
        let Ok(Some(patch)) = proposal else {
            return false;
        };
        match plan.apply_patch(&patch) {
            Ok(()) => {
                tracing::info!(
                    task_id = task_id.0,
                    failure_count = count,
                    threshold = threshold,
                    "auto_replan_applied — advisor patch accepted; plan moved forward",
                );
                true
            }
            Err(e) => {
                tracing::warn!(
                    task_id = task_id.0,
                    error = %e,
                    "auto_replan_patch_invalid — falling back to manual replan",
                );
                false
            }
        }
    }

    fn check_rate_limit(&mut self) -> bool {
        // Reset hourly counter
        if self.hour_start.elapsed().as_secs() >= 3600 {
            self.calls_this_hour = 0;
            self.hour_start = std::time::Instant::now();
        }
        self.calls_this_hour += 1;
        self.calls_this_hour <= self.pilot_config.max_loops_per_hour
    }

    fn check_circuit_breaker(&mut self) -> Option<String> {
        match &self.circuit_state {
            CircuitBreakerState::Closed => None,
            CircuitBreakerState::Open => {
                let elapsed_secs = self
                    .circuit_open_since
                    .map(|since| since.elapsed().as_secs())
                    .unwrap_or(0);

                if should_transition_to_halfopen(
                    elapsed_secs,
                    self.pilot_config.circuit_breaker_cooldown_secs,
                ) {
                    self.circuit_state = CircuitBreakerState::HalfOpen;
                    None
                } else {
                    Some("no progress detected, waiting for cooldown".to_string())
                }
            }
            CircuitBreakerState::HalfOpen => None,
        }
    }

    fn update_counters(&mut self, result: &AgentResult, progress: &GitProgress) {
        // Defense in depth: filter empty strings from files_edited.
        // apply_patch with non-standard format can produce "" entries.
        let has_real_files = result.files_edited.iter().any(|f| !f.is_empty());
        let has_real_progress =
            has_real_files || progress.sha_changed || progress.files_changed > 0;

        // Completion signals: only count if there was real work
        if result.success && has_real_progress {
            self.consecutive_completion_signals += 1;
        } else if result.success && !has_real_progress {
            // done() without real work doesn't count as completion
            self.consecutive_completion_signals = 0;
        } else {
            self.consecutive_completion_signals = 0;
        }

        // No progress tracking
        if has_real_progress {
            self.consecutive_no_progress = 0;
            // Success in HalfOpen → close circuit breaker
            if matches!(self.circuit_state, CircuitBreakerState::HalfOpen) {
                self.circuit_state = CircuitBreakerState::Closed;
                self.circuit_open_since = None;
            }
        } else {
            self.consecutive_no_progress += 1;
            // Failure in HalfOpen → reopen
            if matches!(self.circuit_state, CircuitBreakerState::HalfOpen) {
                self.circuit_state = CircuitBreakerState::Open;
                self.circuit_open_since = Some(std::time::Instant::now());
            }
            // Threshold reached → open
            if self.consecutive_no_progress >= self.pilot_config.circuit_breaker_no_progress
                && matches!(self.circuit_state, CircuitBreakerState::Closed)
            {
                self.circuit_state = CircuitBreakerState::Open;
                self.circuit_open_since = Some(std::time::Instant::now());
            }
        }

        // Same error tracking
        if !result.success {
            let error = result.summary.clone();
            if self.last_error.as_ref() == Some(&error) {
                self.consecutive_same_error += 1;
                if self.consecutive_same_error >= self.pilot_config.circuit_breaker_same_error
                    && matches!(self.circuit_state, CircuitBreakerState::Closed)
                {
                    self.circuit_state = CircuitBreakerState::Open;
                    self.circuit_open_since = Some(std::time::Instant::now());
                }
            } else {
                self.consecutive_same_error = 1;
            }
            self.last_error = Some(error);
        } else {
            self.consecutive_same_error = 0;
            self.last_error = None;
        }
    }

    fn evaluate_exit(&self, result: &AgentResult) -> Option<ExitReason> {
        // Dual-condition: N completion signals with real progress
        if self.consecutive_completion_signals >= self.pilot_config.exit_signal_threshold
            && result.success
        {
            return Some(ExitReason::PromiseFulfilled);
        }

        // Fix plan complete
        let (completed, total) = parse_fix_plan(&self.project_dir);
        if total > 0 && completed == total {
            return Some(ExitReason::FixPlanComplete);
        }

        None
    }

    fn build_loop_prompt(&self, fix_completed: usize, fix_total: usize) -> String {
        let mut prompt = format!("## Promise\n{}\n", self.promise);

        // Definition of Done
        if let Some(ref dod) = self.complete {
            prompt.push_str(&format!(
                "\n## Definition of Done\nThe promise is ONLY fulfilled when ALL of these criteria are met:\n{dod}\n"
            ));
        }

        // Progress info
        prompt.push_str(&format!(
            "\n## Progress\nPilot loop {}. Total tokens: {}. Files changed: {}.\n",
            self.loop_count + 1,
            self.total_tokens,
            self.total_files_edited.len(),
        ));

        if fix_total > 0 {
            prompt.push_str(&format!(
                "Fix plan: {}/{} tasks completed.\n",
                fix_completed, fix_total
            ));
        }

        // Evolution context — prior attempt history and reflections
        let evolution_ctx = self.evolution.build_evolution_prompt();
        if !evolution_ctx.is_empty() {
            prompt.push_str(&format!("\n{evolution_ctx}\n"));
        }

        // Corrective guidance
        if let Some(guidance) = self.build_corrective_guidance() {
            prompt.push_str(&format!("\n## Corrective Guidance\n{guidance}\n"));
        }

        // Instructions
        if self.complete.is_some() {
            prompt.push_str(
                "\n## Instructions\n\
                 Continue working on the promise. Only call done() when ALL criteria in the Definition of Done are met.\n\
                 If you encounter a blocker you cannot resolve, call done() and explain what is blocking.\n\
                 IMPORTANT: Do NOT create tasks that already exist. Check task history before calling task_create.\n"
            );
        } else {
            prompt.push_str(
                "\n## Instructions\n\
                 Continue working on the promise. When ALL work is done, call done() with a summary.\n\
                 If you encounter a blocker you cannot resolve, call done() and explain in the summary.\n\
                 IMPORTANT: Do NOT create tasks that already exist. Check task history before calling task_create.\n"
            );
        }

        prompt
    }

    fn build_corrective_guidance(&self) -> Option<String> {
        self.reflector.corrective_guidance(
            self.consecutive_no_progress,
            self.consecutive_same_error,
            self.last_error.as_deref(),
            false, // Called during loop — not after success.
        )
    }

    fn build_result(&self, reason: ExitReason) -> PilotResult {
        let success = matches!(
            reason,
            ExitReason::PromiseFulfilled | ExitReason::FixPlanComplete
        );
        PilotResult {
            success,
            reason,
            loops_completed: self.loop_count,
            total_tokens: self.total_tokens,
            files_edited: self.total_files_edited.clone(),
            promise: self.promise.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// EventForwarder — forwards loop EventBus events to parent
// ---------------------------------------------------------------------------

use crate::event_bus::EventListener;
use theo_domain::event::DomainEvent;

struct EventForwarder {
    target: Arc<EventBus>,
}

impl EventListener for EventForwarder {
    fn on_event(&self, event: &DomainEvent) {
        self.target.publish(event.clone());
    }
}

/// Pure function: should circuit breaker transition from Open to HalfOpen?
/// Extracted for deterministic testing without wall-clock dependency.
fn should_transition_to_halfopen(elapsed_secs: u64, cooldown_secs: u64) -> bool {
    elapsed_secs >= cooldown_secs
}

/// Mutates a `Plan` in place: sets the status of the matching `PlanTask`.
/// No-op when the task ID is not found — caller already cloned by id.
fn update_task_status(plan: &mut Plan, id: PlanTaskId, status: PlanTaskStatus) {
    if let Some(task) = plan.find_task_mut(id) {
        task.status = status;
    }
}

/// Mutates a `Plan` in place: stores the agent's summary as the task outcome.
/// No-op when the task ID is not found.
fn update_task_outcome(plan: &mut Plan, id: PlanTaskId, outcome: String) {
    if let Some(task) = plan.find_task_mut(id) {
        task.outcome = Some(outcome);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
