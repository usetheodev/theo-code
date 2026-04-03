//! Pilot — autonomous development loop.
//!
//! Orchestrates AgentLoop in continuous cycles until a "promise" is fulfilled.
//! Inspired by Ralph patterns: dual-condition exit gate, circuit breaker,
//! git-based progress tracking, rate limiting.
//!
//! Pilot is a pure addition — zero changes to RunEngine or AgentLoop.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;

use crate::agent_loop::{AgentLoop, AgentResult};
use crate::config::AgentConfig;
use crate::event_bus::EventBus;
#[allow(deprecated)]
use crate::events::NullEventSink;
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
}

fn default_max_total_calls() -> usize { 50 }
fn default_max_loops_per_hour() -> usize { 100 }
fn default_exit_signal_threshold() -> usize { 2 }
fn default_cb_no_progress() -> usize { 3 }
fn default_cb_same_error() -> usize { 5 }
fn default_cb_cooldown() -> u64 { 300 }

impl Default for PilotConfig {
    fn default() -> Self {
        Self {
            max_total_calls: default_max_total_calls(),
            max_loops_per_hour: default_max_loops_per_hour(),
            exit_signal_threshold: default_exit_signal_threshold(),
            circuit_breaker_no_progress: default_cb_no_progress(),
            circuit_breaker_same_error: default_cb_same_error(),
            circuit_breaker_cooldown_secs: default_cb_cooldown(),
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
// CircuitBreakerState
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum CircuitBreakerState {
    Closed,
    Open,
    HalfOpen,
}

// ---------------------------------------------------------------------------
// ExitReason + PilotResult
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum ExitReason {
    PromiseFulfilled,
    FixPlanComplete,
    RateLimitExhausted,
    CircuitBreakerOpen(String),
    MaxCallsReached,
    UserInterrupt,
    Error(String),
}

impl std::fmt::Display for ExitReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExitReason::PromiseFulfilled => write!(f, "Promise fulfilled"),
            ExitReason::FixPlanComplete => write!(f, "Fix plan complete"),
            ExitReason::RateLimitExhausted => write!(f, "Rate limit exhausted"),
            ExitReason::CircuitBreakerOpen(reason) => write!(f, "Circuit breaker: {reason}"),
            ExitReason::MaxCallsReached => write!(f, "Max calls reached"),
            ExitReason::UserInterrupt => write!(f, "User interrupt"),
            ExitReason::Error(e) => write!(f, "Error: {e}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PilotResult {
    pub success: bool,
    pub reason: ExitReason,
    pub loops_completed: usize,
    pub total_tokens: u64,
    pub files_edited: Vec<String>,
    pub promise: String,
}

// ---------------------------------------------------------------------------
// GitProgress
// ---------------------------------------------------------------------------

struct GitProgress {
    sha_changed: bool,
    files_changed: usize,
}

async fn detect_git_progress(project_dir: &Path, previous_sha: &Option<String>) -> GitProgress {
    let current_sha = get_git_sha(project_dir).await;

    let sha_changed = match (previous_sha, &current_sha) {
        (Some(prev), Some(curr)) => prev != curr,
        _ => false,
    };

    // Count changed files (staged + unstaged + untracked)
    let files_changed = get_changed_file_count(project_dir).await;

    GitProgress { sha_changed, files_changed }
}

async fn get_git_sha(project_dir: &Path) -> Option<String> {
    let output = tokio::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(project_dir)
        .output()
        .await
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

async fn get_changed_file_count(project_dir: &Path) -> usize {
    let output = tokio::process::Command::new("git")
        .args(["diff", "--stat"])
        .current_dir(project_dir)
        .output()
        .await;
    match output {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout);
            text.lines().filter(|l| !l.trim().is_empty()).count().saturating_sub(1) // last line is summary
        }
        Err(_) => 0,
    }
}

// ---------------------------------------------------------------------------
// Fix Plan Parser
// ---------------------------------------------------------------------------

fn parse_fix_plan(project_dir: &Path) -> (usize, usize) {
    let path = project_dir.join(".theo").join("fix_plan.md");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return (0, 0),
    };

    let mut completed = 0;
    let mut total = 0;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]") {
            completed += 1;
            total += 1;
        } else if trimmed.starts_with("- [ ]") {
            total += 1;
        }
    }
    (completed, total)
}

// ---------------------------------------------------------------------------
// Promise Loader
// ---------------------------------------------------------------------------

/// Load promise from .theo/PROMPT.md if no inline promise provided.
pub fn load_promise(project_dir: &Path) -> Option<String> {
    let path = project_dir.join(".theo").join("PROMPT.md");
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
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
        }
    }

    /// Returns a clone of the interrupt flag for external signal handlers.
    pub fn interrupt_flag(&self) -> Arc<std::sync::atomic::AtomicBool> {
        self.interrupted.clone()
    }

    /// Run the autonomous pilot loop.
    pub async fn run(&mut self) -> PilotResult {
        // Record initial git SHA
        self.last_git_sha = get_git_sha(&self.project_dir).await;

        loop {
            // Check interrupt
            if self.interrupted.load(std::sync::atomic::Ordering::Acquire) {
                return self.build_result(ExitReason::UserInterrupt);
            }

            // Check max calls
            if self.pilot_config.max_total_calls > 0
                && self.loop_count >= self.pilot_config.max_total_calls
            {
                return self.build_result(ExitReason::MaxCallsReached);
            }

            // Check rate limit
            if !self.check_rate_limit() {
                return self.build_result(ExitReason::RateLimitExhausted);
            }

            // Check circuit breaker
            if let Some(reason) = self.check_circuit_breaker() {
                return self.build_result(ExitReason::CircuitBreakerOpen(reason));
            }

            // Check fix plan
            let (completed, total) = parse_fix_plan(&self.project_dir);
            if total > 0 && completed == total {
                return self.build_result(ExitReason::FixPlanComplete);
            }

            self.loop_count += 1;

            // Record git SHA before
            let sha_before = get_git_sha(&self.project_dir).await;

            // Build the loop prompt
            let task = self.build_loop_prompt(completed, total);

            // Create fresh EventBus per iteration (isolation)
            let loop_bus = Arc::new(EventBus::new());
            // Forward events to parent bus for rendering
            let forwarder = Arc::new(EventForwarder {
                target: self.parent_event_bus.clone(),
            });
            loop_bus.subscribe(forwarder);

            // Create fresh agent per iteration
            #[allow(deprecated)]
            let event_sink = Arc::new(NullEventSink);
            let registry = create_default_registry();
            let agent = AgentLoop::new(self.agent_config.clone(), registry, event_sink);

            // Execute
            let result = agent
                .run_with_history(
                    &task,
                    &self.project_dir,
                    self.session_messages.clone(),
                    Some(loop_bus),
                )
                .await;

            // Track tokens
            self.total_tokens += result.tokens_used;

            // Track files
            for file in &result.files_edited {
                if !self.total_files_edited.contains(file) {
                    self.total_files_edited.push(file.clone());
                }
            }

            // Record exchange in session (promise is System, not in rotative history)
            self.session_messages.push(Message::user(&task));
            self.session_messages.push(Message::assistant(&result.summary));
            if self.session_messages.len() > MAX_SESSION_MESSAGES {
                let excess = self.session_messages.len() - MAX_SESSION_MESSAGES;
                self.session_messages.drain(..excess);
            }

            // Detect git progress
            let progress = detect_git_progress(&self.project_dir, &sha_before).await;
            self.last_git_sha = get_git_sha(&self.project_dir).await;

            // Update counters based on result
            self.update_counters(&result, &progress);

            // Evaluate exit
            if let Some(reason) = self.evaluate_exit(&result) {
                return self.build_result(reason);
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
                // Check if cooldown elapsed → transition to HalfOpen
                let cooldown_elapsed = self.circuit_open_since
                    .map(|since| since.elapsed().as_secs() >= self.pilot_config.circuit_breaker_cooldown_secs)
                    .unwrap_or(false);

                if cooldown_elapsed {
                    self.circuit_state = CircuitBreakerState::HalfOpen;
                    None // Allow one try
                } else {
                    Some("no progress detected, waiting for cooldown".to_string())
                }
            }
            CircuitBreakerState::HalfOpen => None, // Allow one try
        }
    }

    fn update_counters(&mut self, result: &AgentResult, progress: &GitProgress) {
        let has_real_progress = !result.files_edited.is_empty() || progress.sha_changed || progress.files_changed > 0;

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

        // Corrective guidance
        if let Some(guidance) = self.build_corrective_guidance() {
            prompt.push_str(&format!("\n## Corrective Guidance\n{guidance}\n"));
        }

        // Instructions
        if self.complete.is_some() {
            prompt.push_str(
                "\n## Instructions\n\
                 Continue working on the promise. Only call done() when ALL criteria in the Definition of Done are met.\n\
                 If you encounter a blocker you cannot resolve, call done() and explain what is blocking.\n"
            );
        } else {
            prompt.push_str(
                "\n## Instructions\n\
                 Continue working on the promise. When ALL work is done, call done() with a summary.\n\
                 If you encounter a blocker you cannot resolve, call done() and explain in the summary.\n"
            );
        }

        prompt
    }

    fn build_corrective_guidance(&self) -> Option<String> {
        if self.consecutive_no_progress >= 2 {
            return Some(format!(
                "WARNING: You have not made file changes in {} consecutive loops. \
                 Focus on EDITING code, not just reading. Make concrete changes.",
                self.consecutive_no_progress
            ));
        }

        if self.consecutive_same_error >= 2 {
            if let Some(ref err) = self.last_error {
                let err_preview = if err.len() > 200 { &err[..200] } else { err };
                return Some(format!(
                    "WARNING: You keep getting the same error ({} times): {}...\n\
                     Stop retrying the same approach. Try something DIFFERENT.",
                    self.consecutive_same_error, err_preview
                ));
            }
        }

        None
    }

    fn build_result(&self, reason: ExitReason) -> PilotResult {
        let success = matches!(reason, ExitReason::PromiseFulfilled | ExitReason::FixPlanComplete);
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- PilotConfig --

    #[test]
    fn pilot_config_defaults() {
        let config = PilotConfig::default();
        assert_eq!(config.max_total_calls, 50);
        assert_eq!(config.max_loops_per_hour, 100);
        assert_eq!(config.exit_signal_threshold, 2);
        assert_eq!(config.circuit_breaker_no_progress, 3);
        assert_eq!(config.circuit_breaker_same_error, 5);
        assert_eq!(config.circuit_breaker_cooldown_secs, 300);
    }

    #[test]
    fn pilot_config_from_toml() {
        let dir = tempfile::tempdir().unwrap();
        let theo_dir = dir.path().join(".theo");
        std::fs::create_dir_all(&theo_dir).unwrap();
        std::fs::write(
            theo_dir.join("config.toml"),
            r#"
[pilot]
max_total_calls = 100
max_loops_per_hour = 50
exit_signal_threshold = 3
"#,
        ).unwrap();

        let config = PilotConfig::load(dir.path());
        assert_eq!(config.max_total_calls, 100);
        assert_eq!(config.max_loops_per_hour, 50);
        assert_eq!(config.exit_signal_threshold, 3);
        // Defaults for unset fields
        assert_eq!(config.circuit_breaker_no_progress, 3);
    }

    #[test]
    fn pilot_config_missing_section_uses_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let theo_dir = dir.path().join(".theo");
        std::fs::create_dir_all(&theo_dir).unwrap();
        std::fs::write(theo_dir.join("config.toml"), "model = \"gpt-4\"\n").unwrap();

        let config = PilotConfig::load(dir.path());
        assert_eq!(config.max_total_calls, 50);
    }

    // -- CircuitBreaker --

    #[test]
    fn circuit_breaker_starts_closed() {
        let pilot = make_test_pilot("test");
        assert!(matches!(pilot.circuit_state, CircuitBreakerState::Closed));
    }

    #[test]
    fn circuit_breaker_opens_after_no_progress_threshold() {
        let mut pilot = make_test_pilot("test");
        let no_progress = GitProgress { sha_changed: false, files_changed: 0 };
        let fail_result = AgentResult {
            success: true, summary: "nothing".into(),
            files_edited: vec![], iterations_used: 1,
            was_streamed: false, tokens_used: 0,
        };

        for _ in 0..3 {
            pilot.update_counters(&fail_result, &no_progress);
        }
        assert!(matches!(pilot.circuit_state, CircuitBreakerState::Open));
    }

    #[test]
    fn circuit_breaker_opens_after_same_error_threshold() {
        let mut pilot = make_test_pilot("test");
        let no_progress = GitProgress { sha_changed: false, files_changed: 0 };
        let err_result = AgentResult {
            success: false, summary: "same error".into(),
            files_edited: vec![], iterations_used: 1,
            was_streamed: false, tokens_used: 0,
        };

        for _ in 0..5 {
            pilot.update_counters(&err_result, &no_progress);
        }
        assert!(matches!(pilot.circuit_state, CircuitBreakerState::Open));
    }

    #[test]
    fn circuit_breaker_closes_on_progress_in_halfopen() {
        let mut pilot = make_test_pilot("test");
        pilot.circuit_state = CircuitBreakerState::HalfOpen;

        let progress = GitProgress { sha_changed: true, files_changed: 2 };
        let ok_result = AgentResult {
            success: true, summary: "done".into(),
            files_edited: vec!["a.rs".into()], iterations_used: 1,
            was_streamed: false, tokens_used: 100,
        };

        pilot.update_counters(&ok_result, &progress);
        assert!(matches!(pilot.circuit_state, CircuitBreakerState::Closed));
    }

    #[test]
    fn circuit_breaker_reopens_on_failure_in_halfopen() {
        let mut pilot = make_test_pilot("test");
        pilot.circuit_state = CircuitBreakerState::HalfOpen;

        let no_progress = GitProgress { sha_changed: false, files_changed: 0 };
        let fail_result = AgentResult {
            success: true, summary: "nothing".into(),
            files_edited: vec![], iterations_used: 1,
            was_streamed: false, tokens_used: 0,
        };

        pilot.update_counters(&fail_result, &no_progress);
        assert!(matches!(pilot.circuit_state, CircuitBreakerState::Open));
    }

    // -- Exit Detection --

    #[test]
    fn exit_promise_fulfilled_requires_threshold_signals() {
        let mut pilot = make_test_pilot("test");
        let progress = GitProgress { sha_changed: true, files_changed: 1 };
        let ok_result = AgentResult {
            success: true, summary: "done".into(),
            files_edited: vec!["a.rs".into()], iterations_used: 1,
            was_streamed: false, tokens_used: 100,
        };

        // First signal — not enough
        pilot.update_counters(&ok_result, &progress);
        assert!(pilot.evaluate_exit(&ok_result).is_none());

        // Second signal — triggers exit
        pilot.update_counters(&ok_result, &progress);
        let exit = pilot.evaluate_exit(&ok_result);
        assert!(matches!(exit, Some(ExitReason::PromiseFulfilled)));
    }

    #[test]
    fn exit_completion_signal_requires_real_progress() {
        let mut pilot = make_test_pilot("test");
        let no_progress = GitProgress { sha_changed: false, files_changed: 0 };
        let empty_done = AgentResult {
            success: true, summary: "done".into(),
            files_edited: vec![], iterations_used: 1,
            was_streamed: false, tokens_used: 0,
        };

        // done() without files_edited does NOT count as completion signal
        pilot.update_counters(&empty_done, &no_progress);
        pilot.update_counters(&empty_done, &no_progress);
        assert!(pilot.evaluate_exit(&empty_done).is_none());
    }

    #[test]
    fn exit_max_calls_checked_in_loop() {
        let pilot = make_test_pilot("test");
        // max_total_calls=50, loop_count=0 → no exit yet
        assert!(pilot.loop_count < pilot.pilot_config.max_total_calls);
    }

    // -- Rate Limit --

    #[test]
    fn rate_limit_allows_within_threshold() {
        let mut pilot = make_test_pilot("test");
        for _ in 0..100 {
            assert!(pilot.check_rate_limit());
        }
    }

    #[test]
    fn rate_limit_blocks_over_threshold() {
        let mut pilot = make_test_pilot("test");
        for _ in 0..100 {
            pilot.check_rate_limit();
        }
        assert!(!pilot.check_rate_limit()); // 101st call blocked
    }

    // -- Fix Plan --

    #[test]
    fn fix_plan_parser_counts_checkboxes() {
        let dir = tempfile::tempdir().unwrap();
        let theo_dir = dir.path().join(".theo");
        std::fs::create_dir_all(&theo_dir).unwrap();
        std::fs::write(
            theo_dir.join("fix_plan.md"),
            "# Tasks\n- [x] Done item\n- [ ] Pending item\n- [x] Another done\n",
        ).unwrap();

        let (completed, total) = parse_fix_plan(dir.path());
        assert_eq!(completed, 2);
        assert_eq!(total, 3);
    }

    #[test]
    fn fix_plan_missing_returns_zero() {
        let (completed, total) = parse_fix_plan(Path::new("/nonexistent"));
        assert_eq!(completed, 0);
        assert_eq!(total, 0);
    }

    // -- Corrective Guidance --

    #[test]
    fn corrective_guidance_after_no_progress() {
        let mut pilot = make_test_pilot("test");
        pilot.consecutive_no_progress = 2;
        let guidance = pilot.build_corrective_guidance();
        assert!(guidance.is_some());
        assert!(guidance.unwrap().contains("not made file changes"));
    }

    #[test]
    fn corrective_guidance_after_same_error() {
        let mut pilot = make_test_pilot("test");
        pilot.consecutive_same_error = 2;
        pilot.last_error = Some("compile error".into());
        let guidance = pilot.build_corrective_guidance();
        assert!(guidance.is_some());
        assert!(guidance.unwrap().contains("same error"));
    }

    // -- Promise Loader --

    #[test]
    fn load_promise_from_prompt_md() {
        let dir = tempfile::tempdir().unwrap();
        let theo_dir = dir.path().join(".theo");
        std::fs::create_dir_all(&theo_dir).unwrap();
        std::fs::write(theo_dir.join("PROMPT.md"), "Build the auth system\n").unwrap();

        let promise = load_promise(dir.path());
        assert_eq!(promise.as_deref(), Some("Build the auth system"));
    }

    #[test]
    fn load_promise_missing_returns_none() {
        assert!(load_promise(Path::new("/nonexistent")).is_none());
    }

    // -- Helper --

    fn make_test_pilot(promise: &str) -> PilotLoop {
        PilotLoop::new(
            AgentConfig::default(),
            PilotConfig::default(),
            PathBuf::from("/tmp"),
            promise.to_string(),
            None,
            Arc::new(EventBus::new()),
        )
    }
}
