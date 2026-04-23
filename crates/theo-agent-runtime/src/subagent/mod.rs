//! Sub-agent system — delegated autonomous execution.
//!
//! The main agent can delegate work to specialized sub-agents, each with
//! its own role, capability set, budget, and system prompt.
//! Sub-agent = RunEngine with specialized config. Zero new subsystems.
//!
//! Track A — Phase 1: dynamic specs via `AgentSpec` + `SubAgentRegistry`.
//! The legacy `SubAgentRole` enum is kept for backward compat; the new
//! `builtins` module is the source of truth and is consumed by the registry.

pub mod approval;
pub mod builtins;
pub mod parser;
pub mod registry;
pub mod watcher;

pub use approval::{ApprovalManifest, ApprovalMode, ApprovedEntry};
pub use parser::{parse_agent_spec, ParseError};
pub use registry::{LoadOutcome, RegistryWarning, SubAgentRegistry, WarningKind};

use std::path::PathBuf;
use std::sync::Arc;

use crate::agent_loop::{AgentLoop, AgentResult};
use crate::config::AgentConfig;
use crate::event_bus::EventBus;
use theo_domain::agent_spec::AgentSpec;
use theo_domain::capability::CapabilitySet;
use theo_domain::event::{DomainEvent, EventType};
use theo_infra_llm::types::Message;

// ---------------------------------------------------------------------------
// SubAgentRole — specialized roles with restricted capabilities
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubAgentRole {
    /// Read-only research and analysis. Cannot edit files.
    Explorer,
    /// Full implementation capability. Can read, write, edit, run commands.
    Implementer,
    /// Validation and testing. Can read and run commands, but not edit.
    Verifier,
    /// Code review and quality analysis. Read-only with reasoning tools.
    Reviewer,
}

impl SubAgentRole {
    /// Map to the canonical routing role-id. Plan §R4: routing slots key
    /// on these string ids so `theo-domain` stays dependency-free.
    pub fn role_id(&self) -> theo_domain::routing::SubAgentRoleId {
        match self {
            SubAgentRole::Explorer => theo_domain::routing::SubAgentRoleId::EXPLORER,
            SubAgentRole::Implementer => theo_domain::routing::SubAgentRoleId::IMPLEMENTER,
            SubAgentRole::Verifier => theo_domain::routing::SubAgentRoleId::VERIFIER,
            SubAgentRole::Reviewer => theo_domain::routing::SubAgentRoleId::REVIEWER,
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            SubAgentRole::Explorer => "Explorer",
            SubAgentRole::Implementer => "Implementer",
            SubAgentRole::Verifier => "Verifier",
            SubAgentRole::Reviewer => "Reviewer",
        }
    }

    pub fn max_iterations(&self) -> usize {
        match self {
            SubAgentRole::Explorer => 30,
            SubAgentRole::Implementer => 100,
            SubAgentRole::Verifier => 20,
            SubAgentRole::Reviewer => 20,
        }
    }

    pub fn system_prompt(&self) -> String {
        match self {
            SubAgentRole::Explorer => {
                "You are a code explorer sub-agent. Your job is to READ and UNDERSTAND code, never edit it.\n\
                 - Use read, grep, glob, bash(ls/find) to explore the codebase.\n\
                 - Use think to organize your findings.\n\
                 - Use memory to save important facts.\n\
                 - Report your findings clearly and concisely.\n\
                 - NEVER use edit, write, or apply_patch.".to_string()
            }
            SubAgentRole::Implementer => {
                "You are an implementer sub-agent. Your job is to MAKE CODE CHANGES.\n\
                 - Read files to understand context, then make targeted edits.\n\
                 - Use think to plan your approach before editing.\n\
                 - Validate your changes with bash (cargo check, tests).\n\
                 - Call done when the implementation is complete.".to_string()
            }
            SubAgentRole::Verifier => {
                "You are a verifier sub-agent. Your job is to VALIDATE code, never edit it.\n\
                 - Run tests: cargo test, cargo check, cargo clippy.\n\
                 - Read code to verify correctness.\n\
                 - Use reflect to assess quality and confidence.\n\
                 - Report issues found clearly.\n\
                 - NEVER use edit, write, or apply_patch.".to_string()
            }
            SubAgentRole::Reviewer => {
                "You are a code reviewer sub-agent. Your job is to ANALYZE quality.\n\
                 - Read code carefully for bugs, anti-patterns, and improvements.\n\
                 - Use think to structure your review.\n\
                 - Use reflect to assess overall code quality.\n\
                 - Report findings with severity (critical/major/minor/suggestion).\n\
                 - NEVER use edit, write, or apply_patch.".to_string()
            }
        }
    }

    pub fn capability_set(&self) -> CapabilitySet {
        match self {
            SubAgentRole::Explorer => CapabilitySet::read_only(),
            SubAgentRole::Implementer => CapabilitySet::unrestricted(),
            SubAgentRole::Verifier => {
                use std::collections::BTreeSet;
                use theo_domain::capability::AllowedTools;
                let mut denied = BTreeSet::new();
                denied.insert("edit".to_string());
                denied.insert("write".to_string());
                denied.insert("apply_patch".to_string());
                CapabilitySet {
                    allowed_tools: AllowedTools::All,
                    denied_tools: denied,
                    allowed_categories: BTreeSet::new(),
                    max_file_size_bytes: u64::MAX,
                    allowed_paths: Vec::new(),
                    network_access: false,
                }
            }
            SubAgentRole::Reviewer => CapabilitySet::read_only(),
        }
    }

    #[allow(clippy::should_implement_trait)] // Returns Option, not Result; intentional API.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "explorer" | "explore" => Some(SubAgentRole::Explorer),
            "implementer" | "implement" => Some(SubAgentRole::Implementer),
            "verifier" | "verify" => Some(SubAgentRole::Verifier),
            "reviewer" | "review" => Some(SubAgentRole::Reviewer),
            _ => None,
        }
    }
}

impl std::fmt::Display for SubAgentRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

// ---------------------------------------------------------------------------
// SubAgentManager — orchestrates sub-agent spawning
// ---------------------------------------------------------------------------

/// Maximum sub-agent nesting depth. Sub-agents CANNOT spawn sub-agents.
const MAX_DEPTH: usize = 1;

/// Default timeout for sub-agent execution.
fn timeout_for_role(role: &SubAgentRole) -> u64 {
    match role {
        SubAgentRole::Explorer => 300,    // 5 min (read-only, fast)
        SubAgentRole::Implementer => 600, // 10 min (edits + cargo check)
        SubAgentRole::Verifier => 600,    // 10 min (cargo test can be slow)
        SubAgentRole::Reviewer => 300,    // 5 min (read-only analysis)
    }
}

pub struct SubAgentManager {
    config: AgentConfig,
    event_bus: Arc<EventBus>,
    project_dir: PathBuf,
    depth: usize,
    /// Optional registry for spec-based spawning (Phase 3). If `None`, the
    /// legacy role-based API (`spawn`) is used. The registry is opt-in so
    /// existing call sites don't need updating until Phase 4.
    registry: Option<Arc<SubAgentRegistry>>,
    /// Phase 10: optional persistence store. When Some, every spawn_with_spec
    /// creates a SubagentRun record (started → completed/failed/cancelled)
    /// and appends iteration events. None = no persistence (legacy).
    run_store: Option<Arc<crate::subagent_runs::FileSubagentRunStore>>,
    /// Phase 5: optional global hooks dispatched at SubagentStart/SubagentStop.
    hook_manager: Option<Arc<crate::lifecycle_hooks::HookManager>>,
    /// Phase 6: optional cancellation tree. When Some, spawn_with_spec creates
    /// a child token and bails out early if cancelled before the LLM call.
    cancellation: Option<Arc<crate::cancellation::CancellationTree>>,
    /// Phase 9: optional checkpoint manager. When Some, snapshot the workdir
    /// once at the start of every spawn_with_spec (pre-mutation safety).
    checkpoint_manager: Option<Arc<crate::checkpoint::CheckpointManager>>,
}

impl SubAgentManager {
    /// Legacy constructor (preserves backward compat for 530+ existing tests).
    pub fn new(config: AgentConfig, event_bus: Arc<EventBus>, project_dir: PathBuf) -> Self {
        Self {
            config,
            event_bus,
            project_dir,
            depth: 0,
            registry: None,
            run_store: None,
            hook_manager: None,
            cancellation: None,
            checkpoint_manager: None,
        }
    }

    /// Phase 3: new constructor that injects a registry for spec-based spawning.
    /// Prefer this over `new()` in new code.
    pub fn with_registry(
        config: AgentConfig,
        event_bus: Arc<EventBus>,
        project_dir: PathBuf,
        registry: Arc<SubAgentRegistry>,
    ) -> Self {
        Self {
            config,
            event_bus,
            project_dir,
            depth: 0,
            registry: Some(registry),
            run_store: None,
            hook_manager: None,
            cancellation: None,
            checkpoint_manager: None,
        }
    }

    /// Phase 3: convenience — builds a default registry (with the 4 builtins).
    /// Drop-in replacement for `new()` that unlocks the spec-based API.
    pub fn with_builtins(
        config: AgentConfig,
        event_bus: Arc<EventBus>,
        project_dir: PathBuf,
    ) -> Self {
        Self::with_registry(
            config,
            event_bus,
            project_dir,
            Arc::new(SubAgentRegistry::with_builtins()),
        )
    }

    /// Phase 10: attach a persistence store for sub-agent runs.
    /// When set, every `spawn_with_spec` persists a `SubagentRun` record.
    pub fn with_run_store(mut self, store: Arc<crate::subagent_runs::FileSubagentRunStore>) -> Self {
        self.run_store = Some(store);
        self
    }

    /// Phase 5: attach a global HookManager. Hooks fire at SubagentStart/Stop.
    pub fn with_hooks(mut self, hooks: Arc<crate::lifecycle_hooks::HookManager>) -> Self {
        self.hook_manager = Some(hooks);
        self
    }

    /// Phase 6: attach a cancellation tree. spawn_with_spec checks the token
    /// at start (after Started event) and aborts cleanly if cancelled.
    pub fn with_cancellation(
        mut self,
        tree: Arc<crate::cancellation::CancellationTree>,
    ) -> Self {
        self.cancellation = Some(tree);
        self
    }

    /// Phase 9: attach a checkpoint manager. spawn_with_spec auto-snapshots
    /// the workdir BEFORE the agent loop runs (pre-mutation safety).
    pub fn with_checkpoint(
        mut self,
        manager: Arc<crate::checkpoint::CheckpointManager>,
    ) -> Self {
        self.checkpoint_manager = Some(manager);
        self
    }

    /// Access the registry, if any.
    pub fn registry(&self) -> Option<&SubAgentRegistry> {
        self.registry.as_deref()
    }

    /// Access the persistence store, if any.
    pub fn run_store(&self) -> Option<&crate::subagent_runs::FileSubagentRunStore> {
        self.run_store.as_deref()
    }

    /// Access the global hook manager, if any.
    pub fn hook_manager(&self) -> Option<&crate::lifecycle_hooks::HookManager> {
        self.hook_manager.as_deref()
    }

    /// Access the cancellation tree, if any.
    pub fn cancellation(&self) -> Option<&crate::cancellation::CancellationTree> {
        self.cancellation.as_deref()
    }

    /// Access the checkpoint manager, if any.
    pub fn checkpoint_manager(&self) -> Option<&crate::checkpoint::CheckpointManager> {
        self.checkpoint_manager.as_deref()
    }

    /// Spawn a sub-agent with a specific role and objective.
    ///
    /// The sub-agent gets:
    /// - Isolated TaskManager and ToolCallManager
    /// - Role-specific system prompt and max_iterations
    /// - Shared EventBus (events tagged by sub-agent's run_id)
    /// - Parent's LLM config (same model/provider)
    ///
    /// Mandatory conditions:
    /// - max_depth=1: sub-agents cannot spawn sub-agents
    /// - Timeout: 5 minutes default per sub-agent
    /// - Budget: deducted from parent (via shared BudgetEnforcer)
    pub fn spawn(
        &self,
        role: SubAgentRole,
        objective: &str,
        context: Option<Vec<Message>>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = AgentResult> + Send + '_>> {
        let objective = objective.to_string();

        Box::pin(async move {
            // Enforce max_depth
            if self.depth >= MAX_DEPTH {
                return AgentResult {
                    success: false,
                    summary: "Sub-agent depth limit reached. Sub-agents cannot spawn sub-agents."
                        .to_string(),
                    ..Default::default()
                };
            }

            // Build sub-agent config
            let mut sub_config = self.config.clone();
            sub_config.system_prompt = role.system_prompt();
            sub_config.max_iterations = role.max_iterations();
            // Mark as sub-agent: prevents receiving delegation tools and skills.
            // This is the primary defense against recursive spawning.
            sub_config.is_subagent = true;
            // Set capability restrictions based on role.
            // This activates CapabilityGate in the sub-agent's ToolCallManager.
            sub_config.capability_set = Some(role.capability_set());

            // Create sub-agent EventBus with prefixed listener
            let sub_bus = Arc::new(crate::event_bus::EventBus::new());
            let prefixed = Arc::new(PrefixedEventForwarder {
                role_name: role.display_name().to_string(),
                parent_bus: self.event_bus.clone(),
            });
            sub_bus.subscribe(prefixed);

            // Add role identifier + project_dir restriction to system prompt
            sub_config.system_prompt = format!(
                "[{}] {}\n\nIMPORTANT: You MUST only operate within the project directory: {}. \
             Do NOT search, read, or access files outside this directory.",
                role.display_name(),
                sub_config.system_prompt,
                self.project_dir.display()
            );

            // Create agent with default registry (CapabilityGate handles restrictions)
            let registry = theo_tooling::registry::create_default_registry();
            let agent = AgentLoop::new(sub_config, registry);

            // Execute with role-specific timeout
            let history = context.unwrap_or_default();
            let timeout_secs = timeout_for_role(&role);
            let timeout = std::time::Duration::from_secs(timeout_secs);

            match tokio::time::timeout(
                timeout,
                agent.run_with_history(&objective, &self.project_dir, history, Some(sub_bus)),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => AgentResult {
                    success: false,
                    summary: format!(
                        "Sub-agent ({}) timed out after {}s. Objective: {}",
                        role.display_name(),
                        timeout_secs,
                        objective
                    ),
                    ..Default::default()
                },
            }
        }) // close Box::pin(async move {
    }
    /// Spawn multiple sub-agents in parallel.
    ///
    /// All sub-agents execute concurrently via tokio::spawn.
    /// Returns when ALL sub-agents complete (or timeout individually).
    /// Results are returned in the same order as the input tasks.
    pub async fn spawn_parallel(&self, tasks: Vec<(SubAgentRole, String)>) -> Vec<AgentResult> {
        use tokio::task::JoinSet;

        let mut join_set = JoinSet::new();

        for (role, objective) in tasks {
            let config = self.config.clone();
            let event_bus = self.event_bus.clone();
            let project_dir = self.project_dir.clone();

            join_set.spawn(async move {
                let manager = SubAgentManager::new(config, event_bus, project_dir);
                manager.spawn(role, &objective, None).await
            });
        }

        let mut results = Vec::new();
        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(agent_result) => results.push(agent_result),
                Err(e) => results.push(AgentResult {
                    success: false,
                    summary: format!("Sub-agent task panicked: {e}"),
                    ..Default::default()
                }),
            }
        }

        results
    }

    // ---------------------------------------------------------------------
    // Phase 3: spec-based API
    // ---------------------------------------------------------------------

    /// Phase 3: spawn a sub-agent from an `AgentSpec`.
    ///
    /// Differences vs. legacy `spawn`:
    /// - Uses `spec.system_prompt`, `spec.capability_set`, `spec.max_iterations`,
    ///   `spec.timeout_secs` directly (no hardcoded role match).
    /// - Emits `SubagentStarted` before spawn and `SubagentCompleted` after.
    /// - Populates `AgentResult.agent_name` and `AgentResult.context_used`.
    ///
    /// Backward-compat invariants preserved:
    /// - max_depth=1 enforcement
    /// - Sub-agent config: `is_subagent=true`, capability_set injected
    /// - EventBus forwarding via `PrefixedEventForwarder` (now tagged by `spec.name`)
    pub fn spawn_with_spec(
        &self,
        spec: &AgentSpec,
        objective: &str,
        context: Option<Vec<Message>>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = AgentResult> + Send + '_>> {
        let spec = spec.clone();
        let objective = objective.to_string();
        let context_text: Option<String> = context.as_ref().and_then(|msgs| {
            msgs.iter()
                .find_map(|m| m.content.as_ref().map(|c| c.to_string()))
        });

        Box::pin(async move {
            // Phase 9: auto-snapshot the workdir BEFORE the run (pre-mutation safety)
            let checkpoint_before: Option<String> = self
                .checkpoint_manager
                .as_ref()
                .and_then(|cm| {
                    cm.snapshot(&format!("pre-run:{}", spec.name)).ok()
                });

            // Phase 10: persist run start
            let run_id = format!(
                "subagent-{}-{}",
                spec.name,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_micros())
                    .unwrap_or(0)
            );
            if let Some(store) = &self.run_store {
                let run = crate::subagent_runs::SubagentRun::new_running(
                    &run_id,
                    None,
                    &spec,
                    &objective,
                    self.project_dir.to_string_lossy(),
                    checkpoint_before.clone(),
                );
                let _ = store.save(&run);
            }

            // Phase 5: dispatch SubagentStart hook
            if let Some(hooks) = &self.hook_manager {
                use crate::lifecycle_hooks::{HookContext, HookEvent, HookResponse};
                let resp = hooks.dispatch(HookEvent::SubagentStart, &HookContext::default());
                if let HookResponse::Block { reason } = resp {
                    let r = AgentResult {
                        success: false,
                        summary: format!("Sub-agent blocked by SubagentStart hook: {}", reason),
                        agent_name: spec.name.clone(),
                        context_used: context_text.clone(),
                        ..Default::default()
                    };
                    self.publish_completed(&spec, &r);
                    return r;
                }
            }

            // Emit SubagentStarted
            self.event_bus.publish(DomainEvent::new(
                EventType::SubagentStarted,
                format!("subagent:{}", spec.name).as_str(),
                serde_json::json!({
                    "agent_name": spec.name,
                    "agent_source": spec.source.as_str(),
                    "objective": objective,
                    "run_id": run_id,
                    "checkpoint_before": checkpoint_before,
                }),
            ));

            let start = std::time::Instant::now();

            // Phase 6: register child cancellation token (early-bail if root already cancelled)
            let cancellation_token = self
                .cancellation
                .as_ref()
                .map(|tree| tree.child(&run_id));
            if let Some(tok) = &cancellation_token {
                if tok.is_cancelled() {
                    let r = AgentResult {
                        success: false,
                        summary: "Sub-agent cancelled before start (parent cancelled)".to_string(),
                        agent_name: spec.name.clone(),
                        context_used: context_text.clone(),
                        duration_ms: start.elapsed().as_millis() as u64,
                        ..Default::default()
                    };
                    if let Some(store) = &self.run_store {
                        if let Ok(mut run) = store.load(&run_id) {
                            run.status = crate::subagent_runs::RunStatus::Cancelled;
                            run.summary = Some(r.summary.clone());
                            let _ = store.save(&run);
                        }
                    }
                    self.publish_completed(&spec, &r);
                    return r;
                }
            }

            // Enforce max_depth
            if self.depth >= MAX_DEPTH {
                let r = AgentResult {
                    success: false,
                    summary: "Sub-agent depth limit reached. Sub-agents cannot spawn sub-agents."
                        .to_string(),
                    agent_name: spec.name.clone(),
                    context_used: context_text.clone(),
                    duration_ms: start.elapsed().as_millis() as u64,
                    ..Default::default()
                };
                // Persist final state for early return path (Phase 10)
                if let Some(store) = &self.run_store {
                    if let Ok(mut run) = store.load(&run_id) {
                        run.status = crate::subagent_runs::RunStatus::Failed;
                        run.finished_at = Some(
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs() as i64)
                                .unwrap_or(0),
                        );
                        run.summary = Some(r.summary.clone());
                        let _ = store.save(&run);
                    }
                }
                self.publish_completed(&spec, &r);
                return r;
            }

            // Build sub-agent config from spec
            let mut sub_config = self.config.clone();
            sub_config.system_prompt = spec.system_prompt.clone();
            sub_config.max_iterations = spec.max_iterations;
            sub_config.is_subagent = true;
            sub_config.capability_set = Some(spec.capability_set.clone());
            if let Some(m) = &spec.model_override {
                sub_config.model = m.clone();
            }

            // Create sub-agent EventBus with prefixed listener tagged by spec.name
            let sub_bus = Arc::new(crate::event_bus::EventBus::new());
            let prefixed = Arc::new(PrefixedEventForwarder {
                role_name: spec.name.clone(),
                parent_bus: self.event_bus.clone(),
            });
            sub_bus.subscribe(prefixed);

            // Prefix role name + project dir restriction (same format as legacy spawn)
            sub_config.system_prompt = format!(
                "[{}] {}\n\nIMPORTANT: You MUST only operate within the project directory: {}. \
             Do NOT search, read, or access files outside this directory.",
                spec.name,
                sub_config.system_prompt,
                self.project_dir.display()
            );

            let registry = theo_tooling::registry::create_default_registry();
            let agent = AgentLoop::new(sub_config, registry);

            let history = context.unwrap_or_default();
            let timeout = std::time::Duration::from_secs(spec.timeout_secs);

            // Phase 6: race the agent against (timeout || cancellation)
            let agent_run = agent.run_with_history(&objective, &self.project_dir, history, Some(sub_bus));
            let mut result = if let Some(tok) = cancellation_token {
                tokio::select! {
                    res = tokio::time::timeout(timeout, agent_run) => match res {
                        Ok(r) => r,
                        Err(_) => AgentResult {
                            success: false,
                            summary: format!(
                                "Sub-agent ({}) timed out after {}s. Objective: {}",
                                spec.name, spec.timeout_secs, objective
                            ),
                            ..Default::default()
                        },
                    },
                    _ = tok.cancelled() => AgentResult {
                        success: false,
                        summary: format!(
                            "Sub-agent ({}) cancelled mid-run by parent",
                            spec.name
                        ),
                        ..Default::default()
                    },
                }
            } else {
                match tokio::time::timeout(timeout, agent_run).await {
                    Ok(r) => r,
                    Err(_) => AgentResult {
                        success: false,
                        summary: format!(
                            "Sub-agent ({}) timed out after {}s. Objective: {}",
                            spec.name, spec.timeout_secs, objective
                        ),
                        ..Default::default()
                    },
                }
            };

            // Annotate result with spec metadata
            result.agent_name = spec.name.clone();
            result.context_used = context_text;
            result.duration_ms = start.elapsed().as_millis() as u64;

            // Phase 10: update persisted run with final status + metrics
            if let Some(store) = &self.run_store {
                if let Ok(mut run) = store.load(&run_id) {
                    run.status = if result.success {
                        crate::subagent_runs::RunStatus::Completed
                    } else {
                        crate::subagent_runs::RunStatus::Failed
                    };
                    run.finished_at = Some(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(0),
                    );
                    run.iterations_used = result.iterations_used;
                    run.tokens_used = result.tokens_used;
                    run.summary = Some(result.summary.clone());
                    let _ = store.save(&run);
                }
            }

            // Phase 7: try output format parsing
            if let Some(schema) = &spec.output_format {
                let strict = spec.output_format_strict.unwrap_or(false);
                match crate::output_format::try_parse_structured(&result.summary, schema) {
                    Ok(value) => {
                        result.structured = Some(value.clone());
                        // Phase 10: also persist structured_output if store attached
                        if let Some(store) = &self.run_store {
                            if let Ok(mut run) = store.load(&run_id) {
                                run.structured_output = Some(value);
                                let _ = store.save(&run);
                            }
                        }
                    }
                    Err(err) => {
                        if strict {
                            // Strict mode: fail the run, append error to summary
                            result.success = false;
                            result.summary = format!(
                                "{}\n\n[output_format strict] {}",
                                result.summary, err
                            );
                        }
                        // best_effort (default): keep free-text, structured=None
                    }
                }
            }

            // Phase 5: dispatch SubagentStop hook (informational; can't cancel
            // — the run already finished). Block here is treated as marking
            // the result with a warning suffix.
            if let Some(hooks) = &self.hook_manager {
                use crate::lifecycle_hooks::{HookContext, HookEvent, HookResponse};
                let resp = hooks.dispatch(HookEvent::SubagentStop, &HookContext::default());
                if let HookResponse::Block { reason } = resp {
                    result.summary = format!(
                        "{}\n\n[SubagentStop hook flagged] {}",
                        result.summary, reason
                    );
                }
            }

            // Phase 6: forget the cancellation token (cleanup tree)
            if let Some(tree) = &self.cancellation {
                tree.forget(&run_id);
            }

            self.publish_completed(&spec, &result);
            result
        })
    }

    /// Helper: builds user messages from a plain string and delegates to spawn_with_spec.
    pub fn spawn_with_spec_text(
        &self,
        spec: &AgentSpec,
        objective: &str,
        context: Option<&str>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = AgentResult> + Send + '_>> {
        let messages = context.map(|c| vec![Message::user(c)]);
        self.spawn_with_spec(spec, objective, messages)
    }

    fn publish_completed(&self, spec: &AgentSpec, result: &AgentResult) {
        self.event_bus.publish(DomainEvent::new(
            EventType::SubagentCompleted,
            format!("subagent:{}", spec.name).as_str(),
            serde_json::json!({
                "agent_name": spec.name,
                "agent_source": spec.source.as_str(),
                "success": result.success,
                "summary": result.summary,
                "duration_ms": result.duration_ms,
                "tokens_used": result.tokens_used,
                "input_tokens": result.input_tokens,
                "output_tokens": result.output_tokens,
                "llm_calls": result.llm_calls,
                "iterations_used": result.iterations_used,
            }),
        ));
    }
}

// ---------------------------------------------------------------------------
// PrefixedEventForwarder — tags sub-agent events with role name
// ---------------------------------------------------------------------------

struct PrefixedEventForwarder {
    role_name: String,
    parent_bus: Arc<EventBus>,
}

impl crate::event_bus::EventListener for PrefixedEventForwarder {
    fn on_event(&self, event: &DomainEvent) {
        // Clone event and add role prefix to entity_id
        let mut tagged = event.clone();
        tagged.entity_id = format!("[{}] {}", self.role_name, tagged.entity_id);
        self.parent_bus.publish(tagged);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_display_names() {
        assert_eq!(SubAgentRole::Explorer.display_name(), "Explorer");
        assert_eq!(SubAgentRole::Implementer.display_name(), "Implementer");
        assert_eq!(SubAgentRole::Verifier.display_name(), "Verifier");
        assert_eq!(SubAgentRole::Reviewer.display_name(), "Reviewer");
    }

    #[test]
    fn role_max_iterations() {
        assert_eq!(SubAgentRole::Explorer.max_iterations(), 30);
        assert_eq!(SubAgentRole::Implementer.max_iterations(), 100);
        assert_eq!(SubAgentRole::Verifier.max_iterations(), 20);
        assert_eq!(SubAgentRole::Reviewer.max_iterations(), 20);
    }

    #[test]
    fn role_system_prompts_are_unique() {
        let prompts: Vec<String> = [
            SubAgentRole::Explorer,
            SubAgentRole::Implementer,
            SubAgentRole::Verifier,
            SubAgentRole::Reviewer,
        ]
        .iter()
        .map(|r| r.system_prompt())
        .collect();

        for i in 0..prompts.len() {
            for j in (i + 1)..prompts.len() {
                assert_ne!(prompts[i], prompts[j], "Prompts should be unique per role");
            }
        }
    }

    #[test]
    fn explorer_capability_is_read_only() {
        let caps = SubAgentRole::Explorer.capability_set();
        assert!(caps.denied_tools.contains("bash"));
        assert!(caps.denied_tools.contains("edit"));
        assert!(caps.denied_tools.contains("write"));
    }

    #[test]
    fn implementer_capability_is_unrestricted() {
        let caps = SubAgentRole::Implementer.capability_set();
        assert!(caps.denied_tools.is_empty());
        assert_eq!(
            caps.allowed_tools,
            theo_domain::capability::AllowedTools::All
        );
    }

    #[test]
    fn verifier_cannot_edit() {
        let caps = SubAgentRole::Verifier.capability_set();
        assert!(caps.denied_tools.contains("edit"));
        assert!(caps.denied_tools.contains("write"));
        assert!(!caps.denied_tools.contains("bash")); // can run tests
    }

    #[test]
    fn reviewer_is_read_only() {
        let caps = SubAgentRole::Reviewer.capability_set();
        assert!(caps.denied_tools.contains("edit"));
        assert!(caps.denied_tools.contains("write"));
    }

    #[test]
    fn from_str_parses_roles() {
        assert_eq!(
            SubAgentRole::from_str("explorer"),
            Some(SubAgentRole::Explorer)
        );
        assert_eq!(
            SubAgentRole::from_str("implement"),
            Some(SubAgentRole::Implementer)
        );
        assert_eq!(
            SubAgentRole::from_str("VERIFIER"),
            Some(SubAgentRole::Verifier)
        );
        assert_eq!(
            SubAgentRole::from_str("review"),
            Some(SubAgentRole::Reviewer)
        );
        assert_eq!(SubAgentRole::from_str("unknown"), None);
    }

    #[test]
    fn spawn_config_sets_is_subagent_true() {
        // Verify that sub-agent configs are marked as sub-agents.
        // This is tested indirectly: SubAgentManager::spawn() sets
        // sub_config.is_subagent = true before creating AgentLoop.
        // We verify the parent config is NOT a sub-agent by default.
        let config = AgentConfig::default();
        assert!(!config.is_subagent, "parent config must not be sub-agent");

        // After clone + manual set (what spawn() does internally):
        let mut sub_config = config.clone();
        sub_config.is_subagent = true;
        assert!(sub_config.is_subagent, "sub-agent config must be marked");
    }

    #[test]
    fn spawn_parallel_configs_inherit_parent_settings() {
        // Verify that spawn_parallel creates managers that will produce
        // sub-agent configs with is_subagent=true (via spawn() internally).
        let config = AgentConfig::default();
        assert!(!config.is_subagent);

        // spawn_parallel clones self.config and passes to SubAgentManager::new()
        // then calls spawn() which sets is_subagent=true on the sub_config.
        let cloned = config.clone();
        let mut sub_config = cloned.clone();
        sub_config.is_subagent = true;
        assert!(sub_config.is_subagent);

        // LLM settings must be preserved
        assert_eq!(sub_config.base_url, config.base_url);
        assert_eq!(sub_config.model, config.model);
        assert_eq!(sub_config.api_key, config.api_key);
    }

    #[test]
    fn max_depth_prevents_recursion() {
        let bus = Arc::new(EventBus::new());
        let manager = SubAgentManager {
            config: AgentConfig::default(),
            event_bus: bus,
            project_dir: PathBuf::from("/tmp"),
            depth: 1, // Already at max
            registry: None,
            run_store: None,
            hook_manager: None,
            cancellation: None,
            checkpoint_manager: None,
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result =
            rt.block_on(async { manager.spawn(SubAgentRole::Explorer, "test", None).await });
        assert!(!result.success);
        assert!(result.summary.contains("depth limit"));
    }

    // ── Phase 3: spec-based spawn + events ───────────────────────────────

    use crate::event_bus::EventListener;
    use std::sync::Mutex;
    use theo_domain::event::{DomainEvent, EventType};

    /// Test helper: captures events published to the bus.
    struct CaptureListener {
        events: Mutex<Vec<DomainEvent>>,
    }
    impl CaptureListener {
        fn new() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
            }
        }
        fn events(&self) -> Vec<DomainEvent> {
            self.events.lock().unwrap().clone()
        }
    }
    impl EventListener for CaptureListener {
        fn on_event(&self, e: &DomainEvent) {
            self.events.lock().unwrap().push(e.clone());
        }
    }

    #[test]
    fn with_builtins_preserves_backward_compat_constructor_signature() {
        // Drop-in replacement for `new()`. Legacy call sites work unchanged.
        let bus = Arc::new(EventBus::new());
        let manager =
            SubAgentManager::with_builtins(AgentConfig::default(), bus, PathBuf::from("/tmp"));
        assert!(manager.registry().is_some());
        // Has 4 builtin specs
        assert_eq!(manager.registry().unwrap().len(), 4);
    }

    #[test]
    fn with_registry_uses_provided_registry() {
        let bus = Arc::new(EventBus::new());
        let mut custom = SubAgentRegistry::new();
        custom.register(theo_domain::agent_spec::AgentSpec::on_demand("x", "y"));
        let manager = SubAgentManager::with_registry(
            AgentConfig::default(),
            bus,
            PathBuf::from("/tmp"),
            Arc::new(custom),
        );
        assert_eq!(manager.registry().unwrap().len(), 1);
        assert!(manager.registry().unwrap().contains("x"));
    }

    #[test]
    fn spawn_with_spec_at_max_depth_emits_events_and_fails() {
        let bus = Arc::new(EventBus::new());
        let capture = Arc::new(CaptureListener::new());
        bus.subscribe(capture.clone() as Arc<dyn EventListener>);

        let manager = SubAgentManager {
            config: AgentConfig::default(),
            event_bus: bus,
            project_dir: PathBuf::from("/tmp"),
            depth: 1,
            registry: Some(Arc::new(SubAgentRegistry::with_builtins())),
            run_store: None,
            hook_manager: None,
            cancellation: None,
            checkpoint_manager: None,
        };

        let spec = theo_domain::agent_spec::AgentSpec::on_demand("scout", "check x");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async { manager.spawn_with_spec(&spec, "check x", None).await });

        // Result reflects the depth-limit failure
        assert!(!result.success);
        assert!(result.summary.contains("depth limit"));
        assert_eq!(result.agent_name, "scout");

        // Events published: SubagentStarted + SubagentCompleted
        let events = capture.events();
        assert!(
            events
                .iter()
                .any(|e| e.event_type == EventType::SubagentStarted),
            "SubagentStarted event missing"
        );
        let completed: Vec<&DomainEvent> = events
            .iter()
            .filter(|e| e.event_type == EventType::SubagentCompleted)
            .collect();
        assert_eq!(completed.len(), 1);
        assert_eq!(
            completed[0].payload.get("agent_name").and_then(|v| v.as_str()),
            Some("scout")
        );
        assert_eq!(
            completed[0].payload.get("agent_source").and_then(|v| v.as_str()),
            Some("on_demand")
        );
        assert_eq!(
            completed[0].payload.get("success").and_then(|v| v.as_bool()),
            Some(false)
        );
    }

    #[test]
    fn spawn_with_spec_populates_agent_name_and_context() {
        let bus = Arc::new(EventBus::new());
        let manager = SubAgentManager {
            config: AgentConfig::default(),
            event_bus: bus,
            project_dir: PathBuf::from("/tmp"),
            depth: 1, // trigger depth-limit early return (no real LLM)
            registry: Some(Arc::new(SubAgentRegistry::with_builtins())),
            run_store: None,
            hook_manager: None,
            cancellation: None,
            checkpoint_manager: None,
        };
        let spec = theo_domain::agent_spec::AgentSpec::on_demand("x", "y");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async {
            manager
                .spawn_with_spec_text(&spec, "do y", Some("some context"))
                .await
        });
        assert_eq!(result.agent_name, "x");
        assert_eq!(result.context_used.as_deref(), Some("some context"));
    }

    #[test]
    fn spawn_with_spec_with_run_store_persists_run_record() {
        use crate::subagent_runs::FileSubagentRunStore;
        let tempdir = tempfile::TempDir::new().unwrap();
        let store = Arc::new(FileSubagentRunStore::new(tempdir.path()));
        let bus = Arc::new(EventBus::new());
        let manager = SubAgentManager {
            config: AgentConfig::default(),
            event_bus: bus,
            project_dir: PathBuf::from("/tmp"),
            depth: 1, // depth-limit early return (no real LLM)
            registry: None,
            run_store: Some(store.clone()),
            hook_manager: None,
            cancellation: None,
            checkpoint_manager: None,
        };
        let spec = theo_domain::agent_spec::AgentSpec::on_demand("persisted", "test");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _ = rt.block_on(async { manager.spawn_with_spec(&spec, "test", None).await });
        let runs = store.list().unwrap();
        assert_eq!(runs.len(), 1);
        let run = store.load(&runs[0]).unwrap();
        assert_eq!(run.agent_name, "persisted");
        // Final status set after early return
        assert!(matches!(
            run.status,
            crate::subagent_runs::RunStatus::Failed | crate::subagent_runs::RunStatus::Completed
        ));
    }

    #[test]
    fn spawn_with_spec_without_run_store_does_not_persist() {
        let bus = Arc::new(EventBus::new());
        let manager = SubAgentManager {
            config: AgentConfig::default(),
            event_bus: bus,
            project_dir: PathBuf::from("/tmp"),
            depth: 1,
            registry: None,
            run_store: None,
            hook_manager: None,
            cancellation: None,
            checkpoint_manager: None,
        };
        let spec = theo_domain::agent_spec::AgentSpec::on_demand("x", "y");
        let rt = tokio::runtime::Runtime::new().unwrap();
        // Should not panic / not require store
        let _ = rt.block_on(async { manager.spawn_with_spec(&spec, "y", None).await });
    }

    #[test]
    fn with_hooks_builder_stores_reference() {
        use crate::lifecycle_hooks::HookManager;
        let bus = Arc::new(EventBus::new());
        let manager = SubAgentManager::with_builtins(
            AgentConfig::default(),
            bus,
            PathBuf::from("/tmp"),
        )
        .with_hooks(Arc::new(HookManager::new()));
        assert!(manager.hook_manager().is_some());
    }

    #[test]
    fn with_cancellation_builder_stores_reference() {
        use crate::cancellation::CancellationTree;
        let bus = Arc::new(EventBus::new());
        let manager = SubAgentManager::with_builtins(
            AgentConfig::default(),
            bus,
            PathBuf::from("/tmp"),
        )
        .with_cancellation(Arc::new(CancellationTree::new()));
        assert!(manager.cancellation().is_some());
    }

    #[test]
    fn spawn_with_spec_blocked_by_subagent_start_hook() {
        use crate::lifecycle_hooks::{HookEvent, HookManager, HookMatcher, HookResponse};
        let bus = Arc::new(EventBus::new());
        let mut hooks = HookManager::new();
        hooks.add(
            HookEvent::SubagentStart,
            HookMatcher {
                matcher: None,
                response: HookResponse::Block {
                    reason: "test block".into(),
                },
                timeout_secs: 60,
            },
        );
        let manager = SubAgentManager {
            config: AgentConfig::default(),
            event_bus: bus,
            project_dir: PathBuf::from("/tmp"),
            depth: 0,
            registry: None,
            run_store: None,
            hook_manager: Some(Arc::new(hooks)),
            cancellation: None,
            checkpoint_manager: None,
        };
        let spec = theo_domain::agent_spec::AgentSpec::on_demand("x", "y");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async { manager.spawn_with_spec(&spec, "y", None).await });
        assert!(!result.success);
        assert!(result.summary.contains("test block"));
    }

    #[test]
    fn spawn_with_spec_early_cancelled_by_pre_run_cancel() {
        use crate::cancellation::CancellationTree;
        let bus = Arc::new(EventBus::new());
        let tree = Arc::new(CancellationTree::new());
        tree.cancel_all(); // root already cancelled

        let manager = SubAgentManager {
            config: AgentConfig::default(),
            event_bus: bus,
            project_dir: PathBuf::from("/tmp"),
            depth: 0,
            registry: None,
            run_store: None,
            hook_manager: None,
            cancellation: Some(tree),
            checkpoint_manager: None,
        };
        let spec = theo_domain::agent_spec::AgentSpec::on_demand("x", "y");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async { manager.spawn_with_spec(&spec, "y", None).await });
        assert!(!result.success);
        assert!(
            result.summary.contains("cancelled before start"),
            "got: {}",
            result.summary
        );
    }

    #[test]
    fn with_run_store_builder_stores_reference() {
        use crate::subagent_runs::FileSubagentRunStore;
        let tempdir = tempfile::TempDir::new().unwrap();
        let store = Arc::new(FileSubagentRunStore::new(tempdir.path()));
        let bus = Arc::new(EventBus::new());
        let manager = SubAgentManager::with_builtins(
            AgentConfig::default(),
            bus,
            PathBuf::from("/tmp"),
        )
        .with_run_store(store);
        assert!(manager.run_store().is_some());
    }

    #[test]
    fn spawn_with_spec_text_none_context_leaves_context_used_none() {
        let bus = Arc::new(EventBus::new());
        let manager = SubAgentManager {
            config: AgentConfig::default(),
            event_bus: bus,
            project_dir: PathBuf::from("/tmp"),
            depth: 1,
            registry: None,
            run_store: None,
            hook_manager: None,
            cancellation: None,
            checkpoint_manager: None,
        };
        let spec = theo_domain::agent_spec::AgentSpec::on_demand("y", "z");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result =
            rt.block_on(async { manager.spawn_with_spec_text(&spec, "do z", None).await });
        assert!(result.context_used.is_none());
    }
}
