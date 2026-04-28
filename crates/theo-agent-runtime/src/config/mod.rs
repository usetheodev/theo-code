use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use theo_infra_llm::types::Message;

mod prompts;

pub use prompts::system_prompt_for_mode;

// ---------------------------------------------------------------------------
// Steering & Follow-up Message Queues
// ---------------------------------------------------------------------------

/// Async closure type for message queue resolution.
/// Returns `Vec<Message>` — empty vec means "nothing queued".
///
/// **Pi-mono ref:** `packages/agent/src/types.ts:163-183`
pub type MessageQueueFn =
    Arc<dyn Fn() -> Pin<Box<dyn Future<Output = Vec<Message>> + Send>> + Send + Sync>;

/// Message queues for steering (mid-run) and follow-up (post-convergence).
///
/// Steering messages are checked after each tool execution batch and injected
/// as user messages before the next LLM call. Follow-up messages are checked
/// when the agent would otherwise converge; if present, the agent continues.
///
/// Both queues are optional — when absent, behavior is unchanged.
///
/// **Pi-mono ref:** `packages/agent/src/agent-loop.ts:165-229`
#[derive(Default)]
pub struct MessageQueues {
    /// Messages injected mid-run between turns (e.g., user types while agent works).
    pub steering: Option<MessageQueueFn>,
    /// Messages checked after natural convergence (extends the run if present).
    pub follow_up: Option<MessageQueueFn>,
}


impl std::fmt::Debug for MessageQueues {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessageQueues")
            .field("steering", &self.steering.as_ref().map(|_| "Some(Fn)"))
            .field("follow_up", &self.follow_up.as_ref().map(|_| "Some(Fn)"))
            .finish()
    }
}

// ---------------------------------------------------------------------------
// AgentMode — interaction style
// ---------------------------------------------------------------------------

/// Interaction mode that controls how the agent approaches tasks.
/// Implemented via system prompt — zero changes to RunEngine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum AgentMode {
    /// Full autonomy: Read → Think → Act → Verify → Done.
    #[default]
    Agent,
    /// Creates a detailed plan FIRST, presents it, waits for user approval.
    Plan,
    /// Asks clarifying questions FIRST, waits for answers, then acts.
    Ask,
}


impl AgentMode {
    /// Parse mode from string (CLI --mode flag, /mode command).
    #[allow(clippy::should_implement_trait)] // Returns Option, not Result; intentional API.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "agent" => Some(AgentMode::Agent),
            "plan" => Some(AgentMode::Plan),
            "ask" => Some(AgentMode::Ask),
            _ => None,
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            AgentMode::Agent => "agent",
            AgentMode::Plan => "plan",
            AgentMode::Ask => "ask",
        }
    }
}

impl std::fmt::Display for AgentMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}


// ---------------------------------------------------------------------------
// ToolExecutionMode — parallel vs sequential tool execution
// ---------------------------------------------------------------------------

/// How tool calls within a single LLM response are executed.
///
/// **Sequential** (default): tools execute one at a time in the order the LLM
/// returned them. Each tool sees the side-effects of the previous one.
///
/// **Parallel**: all tool calls are prepared sequentially (argument parsing,
/// `prepare_arguments` hook, schema validation, doom-loop check), then executed
/// concurrently via `join_all`. Results are collected in original request order,
/// not completion order. Meta-tools (`done`, `subagent`, `subagent_parallel`,
/// `batch`, `skill`, `reflect`, `think`) are still executed sequentially before
/// the parallel batch — they cannot be parallelised safely.
///
/// **Integration point in `run_engine.rs`:** the `for call in tool_calls` loop
/// (around line 1421) that dispatches regular tools via `ToolCallManager`. In
/// `Parallel` mode, regular tool calls would be collected into a `Vec<Future>`
/// after preparation and dispatched with `futures::future::join_all`, mirroring
/// the existing `batch` tool implementation (lines 1270-1282).
///
/// **Pi-mono ref:** `packages/agent/src/agent-loop.ts:390-438`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ToolExecutionMode {
    /// Execute tool calls one at a time in order (current behavior).
    #[default]
    Sequential,
    /// Prepare all sequentially (validation, hooks), then execute concurrently.
    /// Results are collected in original request order, not completion order.
    Parallel,
}

impl std::fmt::Display for ToolExecutionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolExecutionMode::Sequential => write!(f, "sequential"),
            ToolExecutionMode::Parallel => write!(f, "parallel"),
        }
    }
}

impl ToolExecutionMode {
    /// Parse from string (CLI flag, config file).
    #[allow(clippy::should_implement_trait)] // Returns Option, not Result; intentional API.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "sequential" => Some(ToolExecutionMode::Sequential),
            "parallel" => Some(ToolExecutionMode::Parallel),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// CompactionPolicy — centralized compaction parameters
// ---------------------------------------------------------------------------

/// Centralized policy for context compaction parameters.
///
/// Replaces scattered module-level constants across `compaction.rs` and
/// `compaction_stages.rs`. All compaction functions receive `&CompactionPolicy`
/// so behavior is configurable per-agent without recompilation.
#[derive(Debug, Clone)]
pub struct CompactionPolicy {
    /// Number of recent messages to always preserve fully.
    pub preserve_tail: usize,
    /// Max chars to keep in truncated tool results.
    pub truncate_tool_result_chars: usize,
    /// Threshold: compact when tokens exceed this fraction of context window.
    pub compact_threshold: f64,
    /// How many recent tool results to preserve during Prune stage.
    pub prune_keep_recent: usize,
    /// How many recent tool observations to preserve during masking.
    /// Used by `apply_observation_mask` (Fase 1).
    pub observation_mask_window: usize,
    /// T11.1 — When `true`, the runtime uses the staged compaction
    /// pipeline (`compact_staged_with_policy`) which dispatches across
    /// Mask / Prune / Aggressive / Compact stages based on usage
    /// pressure. When `false` (default), the legacy single-stage Mask
    /// path is preserved — keeps existing behavior for users who don't
    /// opt in.
    pub staged_compaction: bool,
}

impl Default for CompactionPolicy {
    fn default() -> Self {
        Self {
            preserve_tail: 6,
            truncate_tool_result_chars: 200,
            compact_threshold: 0.80,
            prune_keep_recent: 3,
            observation_mask_window: 10,
            staged_compaction: false,
        }
    }
}

// ---------------------------------------------------------------------------
// AgentConfig
// ---------------------------------------------------------------------------

/// LLM connection / model configuration. T3.2 PR1 — owned nested
/// sub-config that replaces the 8 flat LLM-related fields previously
/// held on `AgentConfig` (find_p3_004).
///
/// `Debug` is implemented manually so that `api_key` renders as
/// `Some("[REDACTED]")` / `None` instead of the actual secret
/// (T4.3 / find_p6_009).
#[derive(Clone)]
pub struct LlmConfig {
    /// LLM base URL (OpenAI-compatible).
    pub base_url: String,
    /// API key (optional, for local models).
    pub api_key: Option<String>,
    /// Model name.
    pub model: String,
    /// Override the full endpoint URL (e.g., Codex endpoint).
    /// When set, requests go here instead of `{base_url}/v1/chat/completions`.
    pub endpoint_override: Option<String>,
    /// Extra headers sent with every LLM request.
    pub extra_headers: HashMap<String, String>,
    /// Maximum tokens for LLM response.
    pub max_tokens: u32,
    /// Temperature for LLM sampling.
    pub temperature: f32,
    /// Reasoning effort for LLM: "low", "medium", "high". None = model default.
    pub reasoning_effort: Option<String>,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:8000".to_string(),
            api_key: None,
            model: "default".to_string(),
            endpoint_override: None,
            extra_headers: HashMap::new(),
            max_tokens: 4096,
            temperature: 0.1,
            reasoning_effort: None,
        }
    }
}

impl std::fmt::Debug for LlmConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .field("model", &self.model)
            .field("endpoint_override", &self.endpoint_override)
            .field("max_tokens", &self.max_tokens)
            .field("temperature", &self.temperature)
            .field("reasoning_effort", &self.reasoning_effort)
            .finish_non_exhaustive()
    }
}

/// Context window / compaction sub-config. T3.2 PR3 — owned nested
/// sub-config that replaces the 4 flat context-related fields previously
/// held on `AgentConfig` (find_p3_004).
#[derive(Debug, Clone)]
pub struct ContextConfig {
    /// System prompt prepended to every conversation.
    pub system_prompt: String,
    /// Interval (in iterations) for context loop injection.
    pub context_loop_interval: usize,
    /// Context window size in tokens for the target model.
    pub context_window_tokens: usize,
    /// Compaction policy — centralized parameters for context compaction.
    pub compaction_policy: CompactionPolicy,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            system_prompt: prompts::default_system_prompt().to_string(),
            context_loop_interval: 5,
            context_window_tokens: 128_000,
            compaction_policy: CompactionPolicy::default(),
        }
    }
}

/// Memory subsystem sub-config. T3.2 PR4 — owned nested sub-config that
/// replaces the 5 flat memory-related fields previously held on
/// `AgentConfig` (find_p3_004). Field names were normalized to drop the
/// `memory_*` prefix since the parent struct already carries that scope.
#[derive(Debug, Clone)]
pub struct MemoryConfig {
    /// Master switch for the agent-memory subsystem. When `false`, every
    /// memory lifecycle hook short-circuits to the NullMemoryProvider —
    /// runtime behavior is identical to pre-RM0. Plan ref:
    /// `outputs/agent-memory-plan.md` RM-pre-5.
    pub enabled: bool,
    /// Optional memory provider. When `Some` AND `enabled == true`,
    /// the agent loop calls `prefetch` before each LLM call, `sync_turn`
    /// after each completed turn, `on_pre_compress` before compaction,
    /// and `on_session_end` at convergence/abort. Plan ref:
    /// `outputs/agent-memory-plan.md` RM0.
    pub provider: Option<MemoryHandle>,
    /// PLAN_AUTO_EVOLUTION_SOTA: number of user turns between background
    /// memory-reviewer spawns. `0` disables the nudge entirely. Default:
    /// 10 (matches Hermes `run_agent.py:1418`).
    pub review_nudge_interval: usize,
    /// PLAN_AUTO_EVOLUTION_SOTA: optional reviewer invoked when the nudge
    /// counter fires. When `None`, the nudge becomes a no-op even if
    /// `review_nudge_interval > 0`.
    pub reviewer: Option<crate::memory_reviewer::MemoryReviewerHandle>,
    /// PLAN_AUTO_EVOLUTION_SOTA: optional transcript indexer. `None`
    /// disables cross-session BM25 recall.
    pub transcript_indexer: Option<crate::transcript_indexer::TranscriptIndexerHandle>,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: None,
            review_nudge_interval: 10,
            reviewer: None,
            transcript_indexer: None,
        }
    }
}

/// Plugin / capability gate sub-config. T3.2 PR7 — owned nested
/// sub-config that replaces the 2 flat plugin/capability fields
/// previously held on `AgentConfig` (find_p3_004).
#[derive(Debug, Clone, Default)]
pub struct PluginConfig {
    /// Optional pinned set of plugin manifest SHA-256 hashes. When
    /// `Some`, a plugin is only loaded if its computed `manifest_sha256`
    /// is in the set. When `None`, every ownership-verified plugin is
    /// accepted. T1.3 supply-chain.
    ///
    /// Stored as `BTreeSet<String>` so serialization and diffing remain
    /// deterministic across reproducibility audits.
    pub allowlist: Option<std::collections::BTreeSet<String>>,
    /// Capability set for this agent. Controls which tools are allowed.
    /// `None` = unrestricted. Set by SubAgentManager for sub-agents.
    pub capability_set: Option<theo_domain::capability::CapabilitySet>,
}

/// Routing layer sub-config. T3.2 PR6 — owned nested sub-config that
/// replaces the single flat router field previously held on
/// `AgentConfig` (find_p3_004).
#[derive(Debug, Clone, Default)]
pub struct RoutingConfig {
    /// Optional model router. When `Some`, every ChatRequest consults
    /// the router for its model + reasoning effort. When `None`, the
    /// session uses `model` / `reasoning_effort` verbatim.
    pub router: Option<RouterHandle>,
}

/// PLAN_AUTO_EVOLUTION_SOTA sub-config. T3.2 PR5 — owned nested
/// sub-config that replaces the 5 flat evolution-related fields
/// previously held on `AgentConfig` (find_p3_004).
#[derive(Debug, Clone)]
pub struct EvolutionConfig {
    /// Enables post-session memory consolidation (autodream). The actual
    /// run still respects 24h cooldown, lock file, and minimum file
    /// count. Set to `false` to disable unconditionally.
    pub autodream_enabled: bool,
    /// Max wall time for a consolidation pass. Matches OpenDev's
    /// bounded background work pattern.
    pub autodream_timeout_secs: u64,
    /// Optional executor that runs the LLM consolidation step. When
    /// `None`, `run_autodream` becomes a no-op even if
    /// `autodream_enabled == true`.
    pub autodream: Option<crate::autodream::AutodreamHandle>,
    /// Tool iterations between background skill-reviewer spawns,
    /// provided no skill was created during the task. `0` disables.
    pub skill_review_nudge_interval: usize,
    /// Reviewer invoked when the skill nudge counter fires. `None`
    /// disables the feature even with a positive interval.
    pub skill_reviewer: Option<crate::skill_reviewer::SkillReviewerHandle>,
}

impl Default for EvolutionConfig {
    fn default() -> Self {
        Self {
            autodream_enabled: true,
            autodream_timeout_secs: 60,
            autodream: None,
            skill_review_nudge_interval: 10,
            skill_reviewer: None,
        }
    }
}

/// Run-loop policy. T3.2 PR2 — owned sub-config grouping the 6
/// run-loop fields previously held flat on AgentConfig.
#[derive(Debug, Clone)]
pub struct LoopConfig {
    /// Maximum number of iterations before stopping.
    pub max_iterations: usize,
    /// Agent interaction mode (Agent, Plan, Ask). Controls runtime guards.
    pub mode: AgentMode,
    /// Whether this agent is a sub-agent.
    pub is_subagent: bool,
    /// Doom loop detection threshold.
    pub doom_loop_threshold: Option<usize>,
    /// Use aggressive retry policy (5 retries, 10-120s) for rate limits.
    pub aggressive_retry: bool,
    /// How tool calls within a single LLM response are executed.
    pub tool_execution_mode: ToolExecutionMode,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            max_iterations: 200,
            mode: AgentMode::default(),
            is_subagent: false,
            doom_loop_threshold: Some(3),
            aggressive_retry: false,
            tool_execution_mode: ToolExecutionMode::default(),
        }
    }
}

/// Configuration for the agent loop.
///
/// T3.2 PR1 — `LlmConfig` extracted.
/// T3.2 PR2 — `LoopConfig` extracted.
/// T3.2 PR3 — `ContextConfig` extracted.
/// T3.2 PR4 — `MemoryConfig` extracted.
/// T3.2 PR5 — `EvolutionConfig` extracted.
/// T3.2 PR6 — `RoutingConfig` extracted.
/// T3.2 PR7 — `PluginConfig` extracted (final PR — `views.rs` deleted).
///
/// `Debug` is implemented manually so that `api_key` (now inside
/// `LlmConfig`) renders as `Some("[REDACTED]")` / `None` instead of
/// the actual secret. T4.3 / find_p6_009.
#[derive(Clone)]
pub struct AgentConfig {
    /// LLM connection + sampling sub-config. T3.2 PR1 / find_p3_004.
    pub llm: LlmConfig,
    /// Run-loop policy sub-config. T3.2 PR2 / find_p3_004.
    pub loop_cfg: LoopConfig,
    /// Context window / compaction sub-config. T3.2 PR3 / find_p3_004.
    pub context: ContextConfig,
    /// Plugin / capability gate sub-config. T3.2 PR7 / find_p3_004.
    pub plugin: PluginConfig,
    /// Memory subsystem sub-config. T3.2 PR4 / find_p3_004.
    pub memory: MemoryConfig,
    /// Routing layer sub-config. T3.2 PR6 / find_p3_004.
    pub routing: RoutingConfig,
    /// PLAN_AUTO_EVOLUTION_SOTA sub-config. T3.2 PR5 / find_p3_004.
    pub evolution: EvolutionConfig,
    /// TTL (in seconds) applied to shadow-git checkpoints by
    /// `CheckpointManager::cleanup` at session shutdown. Default is
    /// 7 days (604800). Set to `0` to disable cleanup entirely.
    /// T3.5 / find_p5_005.
    pub checkpoint_ttl_seconds: u64,
}

/// Manual `Debug` impl that redacts `api_key`. T4.3 / find_p6_009.
///
/// The redaction preserves *presence* (`Some("[REDACTED]")` vs `None`)
/// because that signal is sometimes diagnostic ("did the user actually
/// configure a key?") without leaking the actual bytes. Every other
/// field uses its native Debug.
impl std::fmt::Debug for AgentConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentConfig")
            // T3.2 PR1+PR2+PR3 — LLM, run-loop, and context moved to
            // nested sub-configs. LlmConfig's own Debug impl redacts api_key.
            .field("llm", &self.llm)
            .field("loop_cfg", &self.loop_cfg)
            .field("context", &self.context)
            // The remaining fields are large and not security-sensitive;
            // we render them via a single non-exhaustive marker so this
            // Debug impl does not need to track every future field
            // addition. If `tracing::debug!(?config)` is needed for
            // detailed troubleshooting, the caller already has access
            // to the typed fields directly.
            .finish_non_exhaustive()
    }
}

/// Debug-friendly wrapper around `Arc<dyn MemoryProvider>` so `AgentConfig`
/// keeps its `#[derive(Debug, Clone)]` without forcing a `Debug` bound
/// into the `MemoryProvider` trait.
#[derive(Clone)]
pub struct MemoryHandle(pub Arc<dyn theo_domain::memory::MemoryProvider>);

impl MemoryHandle {
    pub fn new(provider: Arc<dyn theo_domain::memory::MemoryProvider>) -> Self {
        Self(provider)
    }
    pub fn as_provider(&self) -> &dyn theo_domain::memory::MemoryProvider {
        self.0.as_ref()
    }
}

impl std::fmt::Debug for MemoryHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("MemoryHandle")
            .field(&self.0.name())
            .finish()
    }
}

/// Debug-friendly wrapper around `Arc<dyn ModelRouter>` so `AgentConfig`
/// keeps its `#[derive(Debug, Clone)]` without leaking a `Debug` bound
/// into the router trait surface.
#[derive(Clone)]
pub struct RouterHandle(pub Arc<dyn theo_domain::routing::ModelRouter>);

impl RouterHandle {
    pub fn new(router: Arc<dyn theo_domain::routing::ModelRouter>) -> Self {
        Self(router)
    }

    pub fn as_router(&self) -> &dyn theo_domain::routing::ModelRouter {
        self.0.as_ref()
    }
}

impl std::fmt::Debug for RouterHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Delegate to the router's `name()` so Debug output is actually
        // useful in logs (T8.4). Previously the literal string
        // "<dyn ModelRouter>" was printed, which tells the reader nothing.
        f.debug_tuple("RouterHandle").field(&self.0.name()).finish()
    }
}

/// Sub-config accessors. After T3.2 PR1-PR7 every logical group lives in
/// its own owned struct under `AgentConfig`; these accessors return
/// `&XConfig` so call sites can use `cfg.x().field` syntax uniformly.
impl AgentConfig {
    pub fn llm(&self) -> &LlmConfig {
        &self.llm
    }
    pub fn loop_cfg(&self) -> &LoopConfig {
        &self.loop_cfg
    }
    pub fn context(&self) -> &ContextConfig {
        &self.context
    }
    pub fn memory(&self) -> &MemoryConfig {
        &self.memory
    }
    pub fn evolution(&self) -> &EvolutionConfig {
        &self.evolution
    }
    pub fn routing(&self) -> &RoutingConfig {
        &self.routing
    }
    pub fn plugin(&self) -> &PluginConfig {
        &self.plugin
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            llm: LlmConfig::default(),
            loop_cfg: LoopConfig::default(),
            context: ContextConfig::default(),
            plugin: PluginConfig::default(),
            memory: MemoryConfig::default(),
            routing: RoutingConfig::default(),
            evolution: EvolutionConfig::default(),
            // 7 days of shadow checkpoints. T3.5 / find_p5_005.
            checkpoint_ttl_seconds: 7 * 24 * 60 * 60,
        }
    }
}



#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
