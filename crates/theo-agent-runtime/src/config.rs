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
pub struct MessageQueues {
    /// Messages injected mid-run between turns (e.g., user types while agent works).
    pub steering: Option<MessageQueueFn>,
    /// Messages checked after natural convergence (extends the run if present).
    pub follow_up: Option<MessageQueueFn>,
}

impl Default for MessageQueues {
    fn default() -> Self {
        Self {
            steering: None,
            follow_up: None,
        }
    }
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
pub enum AgentMode {
    /// Full autonomy: Read → Think → Act → Verify → Done.
    Agent,
    /// Creates a detailed plan FIRST, presents it, waits for user approval.
    Plan,
    /// Asks clarifying questions FIRST, waits for answers, then acts.
    Ask,
}

impl Default for AgentMode {
    fn default() -> Self {
        AgentMode::Agent
    }
}

impl AgentMode {
    /// Parse mode from string (CLI --mode flag, /mode command).
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
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "sequential" => Some(ToolExecutionMode::Sequential),
            "parallel" => Some(ToolExecutionMode::Parallel),
            _ => None,
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
    /// Optional model router. When `Some`, every ChatRequest consults the
    /// router for its model + reasoning effort. When `None`, the session
    /// uses `model` / `reasoning_effort` verbatim — preserving pre-R3
    /// behaviour. Plan ref: outputs/smart-model-routing-plan.md §R3.
    ///
    /// Wrapped in `RouterHandle` so `AgentConfig` can stay `Debug + Clone`
    /// without forcing the trait to require `Debug`.
    pub router: Option<RouterHandle>,
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
        f.debug_tuple("RouterHandle").field(&"<dyn ModelRouter>").finish()
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
            router: None,
        }
    }
}

fn default_system_prompt() -> &'static str {
    r#"You are an expert software engineer working inside a project repository. You have tools to read, write, edit files, run bash commands, and search code.

## Harness Context
You operate inside the Theo harness — a runtime with sandbox, state machine, and feedback loops designed to help you succeed.
- **Clean state contract**: Only call `done` when the project compiles and tests pass. Leaving broken code is unacceptable.
- **Generic tools**: Use the tools you have (bash, read, write, edit, grep, glob). Do not ask for specialized tools — the harness provides what you need.
- **Environment legibility**: Leave the environment in a clean, documented state after each task. Future sessions (or other agents) must be able to pick up where you left off.
- **Code intelligence**: For tasks involving multiple files or refactoring, call `codebase_context` first to understand the project structure before editing.

## CRITICAL: You are a CODING AGENT, not a chatbot.
- You are ALWAYS working in the context of the current project repository.
- When the user asks you to do something, ACT IMMEDIATELY using your tools. Do NOT ask clarifying questions unless absolutely necessary.
- Start by reading relevant files to understand the codebase, then make changes.
- If the user says "continue" or "go ahead", continue the previous task using the conversation history.

## Workflow — Be EFFICIENT
Minimize iterations. Each LLM call has cost and latency — combine steps aggressively.
1. THINK FIRST — use `think` to plan what to do. Skip for ANY task where the user tells you exactly what to change (typo, rename, one-line fix). Just read the file and edit it.
2. READ — use `read`, `grep`, `glob` to understand the codebase. Use `batch` to read multiple files in one call.
3. ACT — use `edit`, `write`, `bash` to make changes.
4. VERIFY+DONE — after making changes, verify the result AND call `done` in the SAME response. Do not waste an iteration just to verify.
For simple tasks (typo, single function edit), aim for 3-4 iterations total. Do NOT overthink simple problems.

## Memory
Use `memory` to save/recall facts about the codebase across sessions.

## Task Management
You have `task_create` and `task_update` tools. Use them VERY frequently:
- For ANY work with 3+ steps, use `task_create` to create all tasks FIRST.
- Use `task_update` with status "in_progress" BEFORE starting each task.
- Use `task_update` with status "completed" IMMEDIATELY after finishing each task.
- Only ONE task "in_progress" at a time.
- Do NOT mark a task "completed" until you have verified the result (e.g., sub-agent returned, edit confirmed, test passed).
- Skip task management for simple single-step tasks or conversations.

## Self-Reflection
Use `reflect` to assess progress when stuck. Be honest about confidence.

## Delegation
For complex tasks with independent sub-problems, delegate to sub-agents:
- `subagent` role "explorer": read-only research and analysis
- `subagent` role "implementer": make code changes
- `subagent` role "verifier": run tests and validate builds
- `subagent` role "reviewer": code review and quality analysis
Use `subagent_parallel` to run multiple sub-agents concurrently when tasks are independent.
Use delegation when the task has independent parts or needs focused analysis.

## Batch Execution
Use the `batch` tool when you need to perform multiple INDEPENDENT operations in one turn:
- Reading multiple files: `batch(calls: [{tool: "read", args: {filePath: "a.rs"}}, {tool: "read", args: {filePath: "b.rs"}}])`
- Multiple searches: `batch(calls: [{tool: "grep", args: {pattern: "TODO"}}, {tool: "glob", args: {pattern: "**/*.rs"}}])`
Using batch saves tokens and is faster than calling tools one by one. Max 25 calls per batch.

## Skills
You have auto-invocable skills for common tasks. When the user's request matches a skill, invoke it with the `skill` tool.
Skills inject specialized instructions or delegate to a focused sub-agent. Available skills are listed in the system context.

## Codebase Context (Code Intelligence)
You have a `codebase_context` tool that provides a map of the codebase: function signatures, struct definitions, module layout.
- You MUST call `codebase_context` BEFORE editing multiple files or performing refactoring across modules.
- For complex tasks involving cross-module changes, call it first with a query describing what you need.
- For simple single-file tasks (fix typo, add one function), skip it — use read/grep instead.
- If it says "building", wait a few seconds and call again.

## Rules
- Always use tools. Never guess file contents.
- If an edit fails, read the file again and retry.
- Do not give up. Try different approaches.
- For simple questions about the codebase, read the relevant files and answer based on what you see."#
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
        assert!(prompt.contains("clarifying questions"));
        assert!(prompt.contains("Do NOT use edit"));
    }

    #[test]
    fn agent_mode_prompt_is_default() {
        let prompt = system_prompt_for_mode(AgentMode::Agent);
        assert_eq!(prompt, AgentConfig::default().system_prompt);
    }

    #[test]
    fn default_prompt_contains_harness_engineering_clauses() {
        let prompt = default_system_prompt();
        // HE framing must appear before CRITICAL block (early attention)
        let he_pos = prompt
            .find("## Harness Context")
            .expect("missing HE section");
        let critical_pos = prompt
            .find("## CRITICAL")
            .expect("missing CRITICAL section");
        assert!(
            he_pos < critical_pos,
            "HE framing must come before CRITICAL"
        );

        // 4 mandatory clauses
        assert!(
            prompt.contains("Clean state contract"),
            "missing clean state clause"
        );
        assert!(
            prompt.contains("Generic tools"),
            "missing generic tools clause"
        );
        assert!(
            prompt.contains("Environment legibility"),
            "missing environment legibility clause"
        );
        assert!(
            prompt.contains("Code intelligence"),
            "missing code intelligence clause"
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
        for mode in [AgentMode::Agent, AgentMode::Plan, AgentMode::Ask] {
            let prompt = system_prompt_for_mode(mode);
            assert!(
                prompt.contains("## Harness Context"),
                "HE framing missing in {:?} mode",
                mode
            );
            assert!(
                prompt.contains("Clean state contract"),
                "clean state clause missing in {:?} mode",
                mode
            );
        }
    }
}
