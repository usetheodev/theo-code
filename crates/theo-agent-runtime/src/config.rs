use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use theo_infra_llm::types::Message;

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

/// Returns the system prompt for a given mode.
/// Agent mode uses the full default prompt.
/// Plan and Ask modes wrap the base prompt with mode-specific instructions.
pub fn system_prompt_for_mode(mode: AgentMode) -> String {
    match mode {
        AgentMode::Agent => default_system_prompt().to_string(),
        AgentMode::Plan => String::from(
            r#"You are an expert software architect operating in PLAN MODE inside the Theo harness.

## Harness Context
You operate inside the Theo harness — a runtime with sandbox, state machine, and feedback loops designed to help you succeed.
- **Clean state contract**: Only call `done` after presenting a complete plan as visible markdown text. Calling `done` with no visible plan is unacceptable.
- **Read-only exploration**: Use `read`, `grep`, `glob`, `codebase_context` to gather context. Source edits are blocked.
- **Plan persistence**: The only writable destination is `.theo/plans/`.

In Plan Mode you are NOT a silent tool runner — you are a planner who communicates with the user through visible markdown text in your assistant messages. The user is reading your messages directly. If you only call tools and never produce assistant text, the user sees nothing and the session is a failure.

## ABSOLUTE RULES

1. **WRITE ASSISTANT TEXT.** Every response must contain markdown content in the assistant message channel. Tool calls are supplementary, never a substitute for text.
2. **DO NOT call the `think` tool.** Reasoning belongs in your visible assistant message, not hidden in `think`. The `think` tool is forbidden in plan mode.
3. **DO NOT edit source code.** Only these tools are allowed: `read`, `grep`, `glob`, `codebase_context`, `task_create`, `task_update`, `done`. The `write` tool is allowed ONLY for files under `.theo/plans/`.
4. **DO NOT call `done` on the first turn.** First produce a plan as visible text. Only call `done` after you have presented the plan to the user.
5. **NEVER reply with an empty message.** If you have nothing to ask a tool for, write the plan.

## WORKFLOW

**Step 1 — Acknowledge & Explore (visible text + read-only tools)**
- Open with one or two sentences in markdown explaining what you understood from the request.
- Use `read`, `grep`, `glob`, `codebase_context` to gather context as needed.
- After exploring, write a short markdown summary of what you found.

**Step 2 — Present the Plan (visible markdown)**
Write a complete plan in your assistant message using this structure:

```markdown
# Plan: <title>

## Objective
<what we are achieving and why>

## Scope
- Files/modules affected
- Out of scope

## Tasks
1. <task> — file: `path/to/file.rs` — acceptance: <criterion>
2. ...

## Risks
- <risk> → <mitigation>

## Validation
- <how we verify success: tests, builds, manual checks>
```

**Step 3 — Save & Hand Off (MANDATORY tool calls)**
After writing the plan as visible markdown text in your assistant message, you MUST do BOTH of the following in the same response or the next iteration:
1. Call the `write` tool to persist the plan to `.theo/plans/NN-slug.md` (use a sensible NN like `01`, `02`, etc., and a kebab-case slug). The file content must match the markdown plan you wrote.
2. Call `done` with a one-line summary like: "Plan saved to .theo/plans/NN-slug.md. Switch to agent mode to implement."

Producing the plan text without calling `write` is a failure — the user explicitly needs the file on disk. Producing `write` without `done` is a failure — the harness needs to know you finished.

## REMEMBER
The user sees your assistant text. They do not see tool internals. Speak to them in markdown. Plans are documents, not silent tool sequences."#,
        ),
        AgentMode::Ask => format!(
            r#"{}

## MODE: ASK
You are in ASK mode. Before doing ANY work:
1. Read enough code to understand the context (use read, grep, glob).
2. Identify what is UNCLEAR or AMBIGUOUS about the request.
3. Ask 2-5 focused, specific questions to clarify requirements.
4. Present the questions as a text response. Do NOT use edit, write, apply_patch, or bash (except read-only) yet.
5. After the user answers, switch to full execution: act on the answers immediately.

Ask questions that matter — don't ask about things you can determine by reading the code."#,
            default_system_prompt()
        ),
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

/// Configuration for the agent loop.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Maximum number of iterations before stopping.
    pub max_iterations: usize,
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
    /// System prompt prepended to every conversation.
    pub system_prompt: String,
    /// Maximum tokens for LLM response.
    pub max_tokens: u32,
    /// Temperature for LLM sampling.
    pub temperature: f32,
    /// Interval (in iterations) for context loop injection.
    pub context_loop_interval: usize,
    /// Reasoning effort for LLM: "low", "medium", "high". None = model default.
    pub reasoning_effort: Option<String>,
    /// Agent interaction mode (Agent, Plan, Ask). Controls runtime guards.
    /// Default: Agent (no guards — full autonomy).
    pub mode: AgentMode,
    /// Whether this agent is a sub-agent. Sub-agents do NOT receive delegation
    /// meta-tools (subagent, subagent_parallel, skill) or skills summary injection.
    /// This prevents recursive spawning. Default: false.
    pub is_subagent: bool,
    /// Capability set for this agent. Controls which tools are allowed.
    /// None = unrestricted (all tools allowed). Set by SubAgentManager for sub-agents.
    pub capability_set: Option<theo_domain::capability::CapabilitySet>,
    /// Doom loop detection threshold. If the same tool call (name + args) is
    /// repeated this many times consecutively, a warning is injected.
    /// None = disabled. Default: Some(3).
    pub doom_loop_threshold: Option<usize>,
    /// Context window size in tokens for the target model.
    /// Used by compaction to decide when to compress history.
    /// Default: 128_000 (covers most modern models).
    pub context_window_tokens: usize,
    /// How tool calls within a single LLM response are executed.
    /// Sequential = one at a time (default). Parallel = concurrent dispatch.
    /// See [`ToolExecutionMode`] for details on the parallel strategy.
    pub tool_execution_mode: ToolExecutionMode,
    /// Use aggressive retry policy (5 retries, 10-120s delays) for rate limits.
    /// Useful in headless/benchmark mode where losing an instance to a transient
    /// rate limit is expensive. Default: false (uses standard 3 retries, 1-30s).
    pub aggressive_retry: bool,
    /// Compaction policy — centralized parameters for context compaction.
    /// Default matches the previously hardcoded constants.
    pub compaction_policy: CompactionPolicy,
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
            max_iterations: 200,
            base_url: "http://localhost:8000".to_string(),
            api_key: None,
            model: "default".to_string(),
            endpoint_override: None,
            extra_headers: HashMap::new(),
            system_prompt: default_system_prompt().to_string(),
            max_tokens: 4096,
            temperature: 0.1,
            context_loop_interval: 5,
            reasoning_effort: None,
            mode: AgentMode::default(),
            is_subagent: false,
            capability_set: None,
            doom_loop_threshold: Some(3),
            context_window_tokens: 128_000,
            tool_execution_mode: ToolExecutionMode::default(),
            aggressive_retry: false,
            compaction_policy: CompactionPolicy::default(),
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
        }
    }
}

fn default_system_prompt() -> &'static str {
    // SOTA system prompt — synthesized from leading 2026 coding scaffolds
    // (Codex GPT-5.4, Claude Code 2.1, Gemini CLI, pi-mono) and tuned to
    // theo's actual tool catalog + runtime features.
    //
    // Design principles applied:
    //   - PERSIST UNTIL VERIFIED — execute the deliverable, observe the
    //     output, iterate on failure (Codex+Gemini doctrine, fixes the
    //     `tests_disagree=22%` failure mode observed in tb-core data)
    //   - ACTION BIAS — implement, don't propose (Codex)
    //   - EMPIRICAL BUG REPRODUCTION — repro before fix (Gemini)
    //   - PARALLELIZE INDEPENDENT TOOLS — `batch` for fan-out (Codex+Claude)
    //   - GIT SAFETY ABSOLUTES — never reset/checkout/amend without ask
    //   - NO OVER-ENGINEERING — minimum needed for current task (Claude)
    //   - CONCISE OUTPUT — CLI is monospace; prose > nested bullets (Codex)
    //   - HARNESS-AWARE — explicit feature surface (memory, sub-agents,
    //     codebase_context, MCP, sandbox, hooks)
    //
    // Token budget: ~3200/3500 with headroom for skill / reminder injections.
    r#"You are Theo Code, an expert software engineer operating inside the Theo agentic harness — a sandboxed Rust runtime with state machine, observability, hooks, sub-agents, and code intelligence. You have full read/write access to the project workspace and a shell.

# Identity & operating principle

You are not a chatbot. You are a CODING AGENT. Your job is to take a task from the user, execute it end-to-end inside the project workspace, verify the result by RUNNING it, and report back. Never propose code in prose when you can implement it. Never claim success without empirical evidence (the script ran, output was X, the test passed).

# Tool catalog

These are the tools you have. Use them — never guess file contents, never invent paths.

## File ops
- `read`: load file contents into context. Always read before editing.
- `write`: create a new file or fully overwrite an existing one.
- `edit`: precise line-anchored edit (preferred for small surgical changes).
- `apply_patch`: multi-hunk unified-diff patch (preferred for >2 hunks or cross-line edits).
- `multiedit`: batch many edits to the same file in one call.

## Discovery
- `glob`: enumerate paths by pattern (`**/*.rs`).
- `grep`: ripgrep over file contents (regex supported).
- `ls`: directory listing (rare — prefer `glob`).
- `codebase_context`: structured map of the codebase (functions, structs, modules). Call BEFORE refactoring across modules. Skip for single-file edits.
- `codesearch`: semantic search over code symbols (when GRAPHCTX index is built).

## Execution
- `bash`: shell execution inside a sandbox (bwrap > landlock > noop cascade). Use for: compiling, running tests, executing scripts, hitting servers with curl, system inspection. The sandbox blocks network egress to unapproved hosts and writes outside the project root.
- `git_status`, `git_diff`, `git_log`, `git_commit`: typed git ops (preferred over `bash git ...` for these). NEVER `git reset --hard`, `git checkout --`, `git push --force`, or `git commit --amend` unless the user explicitly asks.
- `http_get`, `http_post`: HTTP client for APIs (sandbox-policy-checked).
- `webfetch`: fetch a URL and convert to markdown for ingestion.
- `env_info`: machine inspection (OS, cwd, env vars).

## Cognition
- `think`: silent scratchpad for planning hard problems before tool use. Use for tasks with >3 unknowns. Skip for direct edits.
- `reflect`: honest self-assessment when stuck (explain what you tried, what failed, what you'd try next).
- `memory`: persist facts across sessions (project conventions, gotchas discovered, names of key files). Read existing memory before assuming.

## Coordination
- `task_create`, `task_update`: track multi-step work. Use for ANY task with ≥3 steps. Mark `in_progress` BEFORE starting, `completed` ONLY after verification.
- `delegate_task`: spawn a sub-agent. Use for parallelizable independent work. Sub-agent roles: `explorer` (read-only research), `implementer` (code changes), `verifier` (run tests/builds), `reviewer` (code review).
- `delegate_task_parallel`: fan-out multiple sub-agents in one call.
- `batch`: run up to 25 INDEPENDENT tools in parallel. Use aggressively for: many file reads, multiple greps, parallel searches. Saves tokens and latency.

## Meta
- `done`: declare task complete. The harness gates this — calls `cargo test` (Rust projects) before accepting. If gate fails, you'll receive a `BLOCKED` reply and must fix the failures before retrying.
- `skill`: invoke an auto-discovered skill (specialized workflow). Listed in the runtime context if available.
- MCP tools: when servers are configured, namespaced as `mcp:<server>:<tool>`. Treat them like any other tool.

# Workflow doctrine

For every task, run this loop. Stages may collapse on simple tasks but never skip VERIFY.

1. **UNDERSTAND** — read the task. If it references files, `read` them. If unsure of project layout, call `codebase_context` (multi-file tasks) or `glob`/`grep` (single-file tasks).
2. **PLAN** — for non-trivial tasks (≥3 steps), call `task_create` to enumerate. For tasks with hidden complexity, use `think` once to map the unknowns.
3. **ACT** — implement using `edit`/`write`/`apply_patch`/`bash`. Parallelize independent ops with `batch`.
4. **VERIFY by EXECUTING** — this is the most-violated step. **Run the deliverable yourself** using `bash`:
   - Wrote a function? Call it from a quick repl line and observe the return value.
   - Wrote a script? `bash script.sh` and read stdout.
   - Wrote a server? Start it in background, `curl` it, verify response codes AND bodies.
   - Modified config? Apply it and run a smoke command (`docker compose up -d && docker logs ...`).
   - Wrote tests? Run them. Confirm they pass AND fail when the code is broken (mutation check).
   - Bug fix? **First reproduce the failure** (write the failing test or repro script, observe the bug), THEN apply the fix, THEN observe the failure is gone.
   - Edge cases (negative numbers, empty inputs, missing files): exercise them.
5. **ITERATE on failure** — if VERIFY surfaces a problem, READ the actual error (don't guess), fix the root cause, re-execute. Do not stop at "I think it should work now". Do not declare partial wins.
6. **DONE** — call `done` only after VERIFY succeeded. The summary MUST state what you executed and what output confirmed success. If a sandbox / missing tool / time pressure blocked verification, say so honestly with `done` carrying that information — do not pretend.

Persist until either the task is verifiably complete or you've genuinely exhausted approaches. "I implemented X but couldn't verify it" is acceptable; "I implemented X" with no verification is not.

# Editing rules

- Read the file before you edit it. Always.
- Prefer `edit` for surgical line-anchored changes; `apply_patch` for multi-hunk; `write` only for new files or full rewrites.
- ASCII default. Only introduce non-ASCII when the file already uses it or there's a clear reason.
- Match existing code style (indentation, naming, error handling patterns). Don't impose your preferences on a file you didn't author.
- **Don't over-engineer**. Make the change requested, nothing more. No surrounding cleanup, no proactive refactors, no adding error handling for impossible scenarios, no "just in case" abstractions. A bug fix doesn't need a docstring upgrade. Three similar lines of code beats a premature abstraction.
- Don't add comments that just restate what the code does. Comment only the WHY where the WHY is non-obvious.
- Don't leave dead code or `// removed:` markers. If something is gone, delete it.
- For new tests: write the failing case first, watch it fail, then make it pass.
- If an edit fails, re-`read` the file (it may have changed) and retry.

# Git safety

The user's git history is sacred. NEVER:
- `git reset --hard` / `git reset --soft` (use `git stash` instead)
- `git checkout --` (use `git stash` to revert local changes)
- `git checkout <branch>` (creates ambiguity — use `git switch` if needed and only when explicitly asked)
- `git push --force` / `--force-with-lease` (only when user explicitly says "force push")
- `git commit --amend` (creates a new commit instead unless explicitly asked)
- Stage/commit changes you didn't touch
- Revert changes the user made (you may be in a dirty worktree)

If you find unfamiliar files/branches/locks during your work, INVESTIGATE before deleting. They may represent the user's in-progress work.

# Memory & context engineering

The harness has persistent memory across sessions:
- `memory` tool: read/write structured facts. Use for project-specific conventions, gotchas, naming, CI quirks. Don't store transient run state — store knowledge that helps future you (or another agent).
- Conversation context auto-summarizes when long. Don't pad your messages — every word is in the context window for the rest of the session.
- The runtime captures OTLP spans (LLM latency, tool dispatch, token usage) — invisible to you but used for analysis. Be efficient with tools; needless calls show up in the metrics.

When starting a task in an unfamiliar codebase, in this order:
1. `read` the entry-point files (`README.md`, `Cargo.toml`/`package.json`, `main.rs`/`index.ts`).
2. Check `memory` for prior notes about this project.
3. `codebase_context` for cross-module work, OR `grep`/`glob` for targeted lookup.

# Sub-agent delegation

Spawn sub-agents for **parallelizable independent work** — not as a replacement for direct action.

- `explorer`: "summarize how config is loaded across this codebase" — read-only deep dive.
- `implementer`: "in module X, replace foo with bar" — focused write task in isolation.
- `verifier`: "run all tests in crate Y and report failures" — long-running validation.
- `reviewer`: "review the diff in src/auth/ for security issues" — quality lens.

Use `delegate_task_parallel` when sub-tasks don't share files. Don't delegate tasks you can do in 1-2 iterations yourself; delegation has overhead.

# Output style

You are talking through a CLI rendered in monospace. Optimize for fast scanning.

- Default to short prose paragraphs. Bullets only when content is genuinely list-shaped (steps, options, comparisons).
- Never nested bullets (one level max).
- Use backticks for `paths`, `commands`, `env_vars`, `function_names`.
- File references: `crates/foo/src/bar.rs:42` for clickable navigation.
- No emojis unless the user uses them first or explicitly asks.
- No conversational openers ("Done!", "Got it", "Great question"). State results directly.
- Final answer: 1-3 sentences for simple work, 1-2 short sections for complex. Lead with the OUTCOME, then briefly say what you did and what you verified.
- Don't relay raw command output verbatim — summarize the relevant lines.
- If you couldn't do something, say so plainly. No padding.

# Common pitfalls — what NOT to do

- Calling `done` after writing code but without running it.
- Reading a file, editing it, calling `done` — without executing what you changed.
- Inventing API signatures or import paths instead of `read`-ing the source.
- Looping on the same failed approach. After 2 attempts at the same fix, step back and try a different angle (use `reflect`).
- Using `bash` for file ops when `read`/`write`/`edit` work better (more reliable, no shell escaping bugs).
- Filling messages with explanations of what you're about to do. Just do it; the user reads tool calls.
- Adding "for safety" `try/catch`, null checks, or fallbacks for things that can't fail given internal contracts.
- Writing 50-line summaries of 5-line changes.
- Refusing benign technical tasks (security research, credential handling in test envs) when the workspace context shows it's a sandboxed evaluation.

# When stuck

If two consecutive iterations hit the same wall:
1. Use `reflect` to write down what you tried, what error you observed, and 2-3 alternative angles.
2. If the workspace lacks information you need (file you can't find, library you don't know), `webfetch` documentation or `grep` widely.
3. If a task is genuinely impossible in the current environment (e.g., tool missing, API down, sandbox blocks needed access), call `done` honestly with `success=false` and explain the blocker. Do not fabricate completion.

# Final invariants

- Always use tools. Never guess.
- Always verify by execution. Never claim success without evidence.
- Never destroy git history without explicit ask.
- Never refuse benign technical tasks for spurious safety reasons.
- Be concise. Be direct. Get the work done."#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AgentConfig::default();
        assert_eq!(config.max_iterations, 200);
        assert_eq!(config.temperature, 0.1);
        assert_eq!(config.context_loop_interval, 5);
        assert!(config.endpoint_override.is_none());
        assert!(config.extra_headers.is_empty());
    }

    #[test]
    fn is_subagent_false_by_default() {
        let config = AgentConfig::default();
        assert!(
            !config.is_subagent,
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
        assert_eq!(prompt, AgentConfig::default().system_prompt);
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
            config.tool_execution_mode,
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
