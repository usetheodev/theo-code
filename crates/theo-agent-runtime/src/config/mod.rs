use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use theo_infra_llm::types::Message;

mod prompts;
mod views;

pub use views::{
    EvolutionView, MemoryView, PluginView, RoutingView,
};
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
}

impl Default for CompactionPolicy {
    fn default() -> Self {
        Self {
            preserve_tail: 6,
            truncate_tool_result_chars: 200,
            compact_threshold: 0.80,
            prune_keep_recent: 3,
            observation_mask_window: 10,
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
    /// Capability set for this agent. Controls which tools are allowed.
    /// None = unrestricted (all tools allowed). Set by SubAgentManager for sub-agents.
    pub capability_set: Option<theo_domain::capability::CapabilitySet>,
    /// Master switch for the agent-memory subsystem. When `false`, every
    /// memory lifecycle hook (`prefetch`, `sync_turn`, `on_pre_compress`,
    /// `on_session_end`) short-circuits to the NullMemoryProvider — runtime
    /// behavior is identical to pre-RM0. When `true`, the configured
    /// `MemoryEngine` is consulted at every hook. Plan ref:
    /// `outputs/agent-memory-plan.md` RM-pre-5. Default: `false`.
    pub memory_enabled: bool,
    /// Optional model router. When `Some`, every ChatRequest consults the
    /// router for its model + reasoning effort. When `None`, the session
    /// uses `model` / `reasoning_effort` verbatim — preserving pre-R3
    /// behaviour. Plan ref: outputs/smart-model-routing-plan.md §R3.
    ///
    /// Wrapped in `RouterHandle` so `AgentConfig` can stay `Debug + Clone`
    /// without forcing the trait to require `Debug`.
    pub router: Option<RouterHandle>,
    /// Optional memory provider. When `Some` AND `memory_enabled == true`,
    /// the agent loop calls `prefetch` before each LLM call, `sync_turn`
    /// after each completed turn, `on_pre_compress` before compaction, and
    /// `on_session_end` at convergence/abort. When `None` OR
    /// `memory_enabled == false`, memory hooks short-circuit to a
    /// NullMemoryProvider (runtime behaviour identical to pre-RM0). Plan
    /// ref: `outputs/agent-memory-plan.md` RM0.
    pub memory_provider: Option<MemoryHandle>,
    /// PLAN_AUTO_EVOLUTION_SOTA: number of user turns between
    /// background memory-reviewer spawns. `0` disables the nudge entirely.
    /// Default: 10 (matches Hermes `run_agent.py:1418` and mitigates
    /// Issue #8506 by design — `AtomicUsize` on `RunEngine` persists
    /// across turns).
    pub memory_review_nudge_interval: usize,
    /// PLAN_AUTO_EVOLUTION_SOTA: optional reviewer invoked
    /// when the nudge counter fires. When `None`, the nudge becomes a
    /// no-op even if `memory_review_nudge_interval > 0`.
    pub memory_reviewer: Option<crate::memory_reviewer::MemoryReviewerHandle>,
    /// PLAN_AUTO_EVOLUTION_SOTA: enables post-session
    /// memory consolidation (autodream). Default: `true` — the actual
    /// run still respects 24h cooldown, lock file, and minimum file
    /// count. Set to `false` to disable unconditionally.
    pub autodream_enabled: bool,
    /// PLAN_AUTO_EVOLUTION_SOTA: max wall time for a
    /// consolidation pass. Default: 60s. Matches OpenDev's bounded
    /// background work pattern.
    pub autodream_timeout_secs: u64,
    /// PLAN_AUTO_EVOLUTION_SOTA: optional executor that
    /// runs the LLM consolidation step. When `None`, `run_autodream`
    /// becomes a no-op even if `autodream_enabled == true`.
    pub autodream: Option<crate::autodream::AutodreamHandle>,
    /// PLAN_AUTO_EVOLUTION_SOTA: tool iterations between
    /// background skill-reviewer spawns, provided no skill was
    /// created during the task. `0` disables. Default: 10
    /// (`referencias/hermes-agent/run_agent.py:1517-1520`).
    pub skill_review_nudge_interval: usize,
    /// PLAN_AUTO_EVOLUTION_SOTA: reviewer invoked when the
    /// skill nudge counter fires. `None` disables the feature even
    /// with a positive interval.
    pub skill_reviewer: Option<crate::skill_reviewer::SkillReviewerHandle>,
    /// PLAN_AUTO_EVOLUTION_SOTA: optional transcript
    /// indexer. `None` disables cross-session BM25 recall; the
    /// concrete Tantivy-backed impl lives in `theo-application` to
    /// keep bounded contexts intact.
    pub transcript_indexer: Option<crate::transcript_indexer::TranscriptIndexerHandle>,
    /// T1.3 supply-chain: optional pinned set of plugin manifest
    /// SHA-256 hashes. When `Some`, a plugin is only loaded if its
    /// computed `manifest_sha256` is in the set — a typo in one
    /// plugin.toml byte fails the load. When `None`, every ownership-
    /// verified plugin is accepted (backward-compatible default).
    ///
    /// Stored as `BTreeSet<String>` rather than `HashSet` so
    /// serialization and diffing remain deterministic across
    /// reproducibility audits.
    pub plugin_allowlist: Option<std::collections::BTreeSet<String>>,
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

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            llm: LlmConfig::default(),
            loop_cfg: LoopConfig::default(),
            context: ContextConfig::default(),
            capability_set: None,
            memory_enabled: false,
            memory_provider: None,
            router: None,
            memory_review_nudge_interval: 10,
            memory_reviewer: None,
            autodream_enabled: true,
            autodream_timeout_secs: 60,
            autodream: None,
            skill_review_nudge_interval: 10,
            skill_reviewer: None,
            transcript_indexer: None,
            plugin_allowlist: None,
            // 7 days of shadow checkpoints. T3.5 / find_p5_005.
            checkpoint_ttl_seconds: 7 * 24 * 60 * 60,
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use super::prompts::default_system_prompt;

    // -----------------------------------------------------------------
    // T4.3 / find_p6_009 — `api_key` must NEVER appear in Debug output.
    // -----------------------------------------------------------------

    #[test]
    fn t43_debug_redacts_api_key_when_present() {
        let mut cfg = AgentConfig::default();
        cfg.llm.api_key = Some("sk-ant-real-secret-do-not-leak".into());
        let dbg = format!("{:?}", cfg);
        assert!(
            !dbg.contains("sk-ant-real-secret-do-not-leak"),
            "raw api_key value leaked into Debug output: {dbg}"
        );
        assert!(
            dbg.contains("[REDACTED]"),
            "expected `[REDACTED]` marker in Debug output: {dbg}"
        );
    }

    #[test]
    fn t43_debug_shows_none_when_api_key_absent() {
        let cfg = AgentConfig::default();
        let dbg = format!("{:?}", cfg);
        // `api_key: None` should still be visible — presence/absence
        // is a useful diagnostic signal.
        assert!(
            dbg.contains("api_key: None"),
            "expected `api_key: None` in Debug output: {dbg}"
        );
    }

    #[test]
    fn t43_debug_pretty_print_does_not_leak_api_key() {
        let mut cfg = AgentConfig::default();
        cfg.llm.api_key = Some("sk-ant-pretty-print-secret".into());
        let pretty = format!("{:#?}", cfg);
        assert!(
            !pretty.contains("sk-ant-pretty-print-secret"),
            "raw api_key value leaked into Debug pretty output: {pretty}"
        );
    }

    /// T4.1 AC literal: each sub-config view exposes ≤ 10 fields. The
    /// AC stays satisfied as long as no future PR overgrows a view.
    /// Counts via reflection-on-source — no runtime field count exists.
    #[test]
    fn each_sub_config_view_has_at_most_10_fields() {
        let src = include_str!("views.rs");

        fn count_struct_fields(src: &str, struct_name: &str) -> usize {
            let needle = format!("pub struct {} ", struct_name);
            let alt = format!("pub struct {}<", struct_name);
            let start = src
                .find(&needle)
                .or_else(|| src.find(&alt))
                .unwrap_or_else(|| panic!("struct {struct_name} not found"));
            let body_start = src[start..]
                .find('{')
                .expect("struct body missing")
                + start;
            let body_end = src[body_start..]
                .find('}')
                .expect("struct close missing")
                + body_start;
            src[body_start..body_end]
                .lines()
                .filter(|l| l.trim_start().starts_with("pub "))
                .count()
        }

        // T3.2 PR1 — LlmView removed (see LlmConfig in mod.rs).
        // T3.2 PR2 — LoopView removed (see LoopConfig in mod.rs).
        // T3.2 PR3 — ContextView removed (see ContextConfig in mod.rs).
        for view in [
            "MemoryView",
            "EvolutionView",
            "RoutingView",
            "PluginView",
        ] {
            let n = count_struct_fields(src, view);
            assert!(
                n <= 10,
                "{view} has {n} fields — T4.1 AC requires <=10"
            );
            assert!(n >= 1, "{view} should expose at least one field");
        }
    }

    #[test]
    fn test_default_config() {
        let config = AgentConfig::default();
        assert_eq!(config.loop_cfg.max_iterations, 200);
        assert_eq!(config.llm.temperature, 0.1);
        assert_eq!(config.context.context_loop_interval, 5);
        assert!(config.llm.endpoint_override.is_none());
        assert!(config.llm.extra_headers.is_empty());
    }

    #[test]
    fn is_subagent_false_by_default() {
        let config = AgentConfig::default();
        assert!(
            !config.loop_cfg.is_subagent,
            "main agents must NOT be marked as sub-agents"
        );
    }

    #[test]
    fn agent_mode_default_is_agent() {
        assert_eq!(AgentMode::default(), AgentMode::Agent);
    }

    #[test]
    fn agent_mode_from_str_parses_all_modes() {
        assert_eq!(AgentMode::from_str("agent"), Some(AgentMode::Agent));
        assert_eq!(AgentMode::from_str("plan"), Some(AgentMode::Plan));
        assert_eq!(AgentMode::from_str("ask"), Some(AgentMode::Ask));
        assert_eq!(AgentMode::from_str("PLAN"), Some(AgentMode::Plan));
        assert_eq!(AgentMode::from_str("invalid"), None);
    }

    #[test]
    fn system_prompts_are_distinct_per_mode() {
        let agent = system_prompt_for_mode(AgentMode::Agent);
        let plan = system_prompt_for_mode(AgentMode::Plan);
        let ask = system_prompt_for_mode(AgentMode::Ask);
        assert_ne!(agent, plan);
        assert_ne!(agent, ask);
        assert_ne!(plan, ask);
    }

    #[test]
    fn plan_mode_prompt_requires_visible_text() {
        let prompt = system_prompt_for_mode(AgentMode::Plan);
        assert!(prompt.contains("PLAN MODE"), "missing mode header");
        assert!(
            prompt.contains("visible markdown text"),
            "must instruct the model to write visible text"
        );
        assert!(
            prompt.contains("`think` tool"),
            "must explicitly forbid the think tool"
        );
        assert!(prompt.contains(".theo/plans/"), "missing plan output path");
        assert!(prompt.contains("Tasks"), "missing tasks section");
        assert!(prompt.contains("Risks"), "missing risks section");
    }

    #[test]
    fn ask_mode_prompt_contains_ask_instructions() {
        let prompt = system_prompt_for_mode(AgentMode::Ask);
        assert!(prompt.contains("MODE: ASK"));
        // SOTA prompt rewrite: original literal "clarifying questions" was
        // replaced with the semantically equivalent "questions to clarify
        // requirements". Lock the SEMANTIC contract: the prompt instructs
        // the model to ASK QUESTIONS for clarification.
        assert!(
            prompt.contains("clarify"),
            "ask-mode prompt must instruct the model to clarify"
        );
        assert!(
            prompt.contains("questions"),
            "ask-mode prompt must instruct the model to ask questions"
        );
        assert!(prompt.contains("Do NOT use edit"));
    }

    #[test]
    fn agent_mode_prompt_is_default() {
        let prompt = system_prompt_for_mode(AgentMode::Agent);
        assert_eq!(prompt, AgentConfig::default().context.system_prompt);
    }

    #[test]
    fn default_prompt_contains_harness_engineering_clauses() {
        // SOTA prompt rewrite: the original 4 HE clauses (Clean state
        // contract, Generic tools, Environment legibility, Code
        // intelligence) were replaced with the more comprehensive SOTA
        // structure synthesized from Codex/Claude/Gemini. The CONCEPTS
        // are preserved — this test now locks the semantic contract.
        let prompt = default_system_prompt();

        // Identity: the agent knows it operates inside theo's harness
        assert!(
            prompt.contains("Theo Code") || prompt.contains("Theo agentic harness"),
            "missing harness identity"
        );

        // Clean state contract → verification before done
        assert!(
            prompt.contains("VERIFY") && prompt.contains("done"),
            "missing verification-before-done invariant"
        );

        // Generic tools → tool catalog mentions the core surface
        for tool in &["read", "write", "edit", "bash", "grep", "glob"] {
            assert!(
                prompt.contains(tool),
                "tool catalog missing core tool: {tool}"
            );
        }

        // Environment legibility → memory + persistent state mentioned
        assert!(
            prompt.contains("memory"),
            "missing memory/persistence mention"
        );

        // Code intelligence → codebase_context mentioned
        assert!(
            prompt.contains("codebase_context"),
            "missing codebase_context mention"
        );

        // SOTA invariants added by the rewrite
        assert!(
            prompt.contains("EXECUTE") || prompt.contains("execute"),
            "missing execution emphasis (the SOTA fix for tests_disagree)"
        );
        assert!(
            prompt.contains("git reset --hard") || prompt.contains("force"),
            "missing git safety absolutes"
        );
    }

    #[test]
    fn default_prompt_within_token_budget() {
        // SOTA prompt budget: 3500 tokens max. We approximate at 4 chars
        // per token (conservative for English+code). 3500 tokens ≈ 14000
        // chars. Tighter budget than the previous 2000-token estimate.
        let prompt = default_system_prompt();
        let approx_tokens = prompt.len() / 4;
        assert!(
            approx_tokens <= 3500,
            "default prompt exceeds 3500-token budget: ~{approx_tokens} tokens ({} chars)",
            prompt.len()
        );
    }

    #[test]
    fn default_prompt_mentions_sota_doctrines() {
        // SOTA doctrines synthesized from frontier scaffolds (Codex 5.4,
        // Claude Code 2.1, Gemini CLI). Each is a behavior we know
        // correlates with high pass rates.
        let p = default_system_prompt();
        // Persist until verified (Codex+Gemini)
        assert!(
            p.contains("Persist") || p.contains("persist"),
            "missing persistence doctrine"
        );
        // Action bias — implement, don't propose (Codex)
        assert!(
            p.contains("Never claim success") || p.contains("never propose"),
            "missing action-bias doctrine"
        );
        // Empirical bug reproduction (Gemini)
        assert!(
            p.contains("reproduce") || p.contains("repro"),
            "missing empirical-reproduction doctrine"
        );
        // No over-engineering (Claude)
        assert!(
            p.contains("over-engineer") || p.contains("Don't add"),
            "missing no-over-engineering doctrine"
        );
        // Parallelize independent tools (Codex+Claude)
        assert!(
            p.contains("batch") && p.contains("parallel"),
            "missing parallelization doctrine"
        );
    }

    #[test]
    fn tool_execution_mode_default_is_sequential() {
        assert_eq!(ToolExecutionMode::default(), ToolExecutionMode::Sequential);
    }

    #[test]
    fn tool_execution_mode_from_str_parses_all_modes() {
        assert_eq!(
            ToolExecutionMode::from_str("sequential"),
            Some(ToolExecutionMode::Sequential)
        );
        assert_eq!(
            ToolExecutionMode::from_str("parallel"),
            Some(ToolExecutionMode::Parallel)
        );
        assert_eq!(
            ToolExecutionMode::from_str("PARALLEL"),
            Some(ToolExecutionMode::Parallel)
        );
        assert_eq!(ToolExecutionMode::from_str("invalid"), None);
    }

    #[test]
    fn tool_execution_mode_display() {
        assert_eq!(ToolExecutionMode::Sequential.to_string(), "sequential");
        assert_eq!(ToolExecutionMode::Parallel.to_string(), "parallel");
    }

    #[test]
    fn agent_config_default_uses_sequential_tool_execution() {
        let config = AgentConfig::default();
        assert_eq!(
            config.loop_cfg.tool_execution_mode,
            ToolExecutionMode::Sequential,
            "default config must use sequential tool execution for backward compatibility"
        );
    }

    #[test]
    fn he_clauses_survive_all_modes() {
        // SOTA prompt rewrite: original tested for legacy literal "##
        // Harness Context" + "Clean state contract" headers. The new
        // prompt expresses these CONCEPTS differently. Lock the SEMANTIC
        // contract: every mode mentions the harness identity AND the
        // verification-before-done invariant.
        for mode in [AgentMode::Agent, AgentMode::Plan, AgentMode::Ask] {
            let prompt = system_prompt_for_mode(mode);
            assert!(
                prompt.contains("Theo") || prompt.contains("harness"),
                "harness identity missing in {:?} mode",
                mode
            );
            assert!(
                prompt.contains("done") || prompt.contains("Done"),
                "done-tool contract missing in {:?} mode",
                mode
            );
        }
    }
}
