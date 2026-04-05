use std::collections::HashMap;

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
        AgentMode::Plan => format!(
            r#"{}

## MODE: PLAN (Governance-First)
You are in PLAN mode. Every task goes through a governance process before execution.

CRITICAL RULES:
- You MUST call the `write` tool to create a roadmap file in `.theo/plans/`. This is MANDATORY.
- Do NOT present the roadmap as text. WRITE IT TO THE FILE using the `write` tool.
- Do NOT edit source code until the user approves.
- A text-only response WITHOUT writing the roadmap file is a FAILURE.
- Be FAST. Read only what matters. Do NOT explore exhaustively. Aim for 10-15 iterations total.

### PHASE 1 — ENTENDIMENTO (3-5 iterations)
1. Use `think` to analyze the task: O que, Por que, Escopo, Risco.
2. Read key files with `read`, `grep`, `glob`. Focus on structure, not every file.
3. Identify existing patterns, dependencies, and risks.
4. Do this YOURSELF — do not delegate to sub-agents for small/medium projects.

For LARGE projects only (10+ modules, 50+ files), you MAY use `subagent_parallel` with Explorer + Reviewer. For smaller projects, analyze directly — it's faster.

### PHASE 2 — WRITE THE ROADMAP FILE (1-2 iterations)
This is the most important phase. You MUST:
1. Check `.theo/plans/` for existing files to determine next number (01, 02, etc.)
2. Call `write` with filePath `.theo/plans/NN-slug.md` using the EXACT template below.
3. Do NOT skip this step. Do NOT present text instead. CALL THE WRITE TOOL.

TEMPLATE — copy this structure exactly, fill in the brackets:

---BEGIN TEMPLATE---
# Roadmap: [Title]

## Entendimento
- **O que**: [objective]
- **Por que**: [motivation]
- **Escopo**: [files/modules affected]
- **Risco**: [what could go wrong]

## Análises
### Explorer
[findings]

### Reviewer
[findings]

## Conflitos
[disagreements or risks — ALWAYS list at least one]

## Microtasks

### Task 1: [title]
- **Arquivo(s)**: [file paths]
- **O que fazer**: [concrete description — what to create/change]
- **Critério de aceite**: [how to verify — specific command or check]
- **DoD**: [definition of done — measurable, not vague]

### Task 2: [title]
- **Arquivo(s)**: [file paths]
- **O que fazer**: [description]
- **Critério de aceite**: [verification]
- **DoD**: [measurable]

[repeat for all tasks — aim for 3-10 tasks]

## Riscos
| # | Risco | Severidade | Mitigação |
|---|-------|-----------|-----------|
| 1 | [risk] | low/medium/high | [mitigation] |

## Verificação Final
- [ ] Todos os testes passam (`cargo test`)
- [ ] Nenhum warning novo (`cargo check`)
- [ ] Código revisado
---END TEMPLATE---

Rules for microtasks:
- ATOMIC: one focused change per task, not a grab bag
- ORDERED: by dependency (task 2 may depend on task 1)
- DoD is SPECIFIC: "cargo test passes" not "tests work"
- Critério is HOW TO VERIFY: "run cargo test", "read file X, confirm function Y exists"
- 3-10 tasks. Fewer = too vague. More = over-engineered.

### PHASE 3 — PRESENT SUMMARY & WAIT
After the `write` tool succeeds, call `done` with a brief summary:
- How many microtasks
- Key files affected
- Path to the roadmap file
- Say: "Roadmap salvo. Use `theo pilot` para executar."

Do NOT execute any source code changes. Your job in Plan mode is ONLY the roadmap."#,
            default_system_prompt()
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

## Workflow
1. THINK FIRST — you MUST use the `think` tool as your FIRST action for any non-trivial task. Plan what files to read, what changes to make, and in what order.
2. READ — use `read`, `grep`, `glob` to understand the codebase.
3. ACT — use `edit`, `write`, `bash` to make changes.
4. VERIFY — read the changed files to confirm correctness.
5. DONE — call `done` with a summary when the task is complete.

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
        assert!(!config.is_subagent, "main agents must NOT be marked as sub-agents");
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
    fn plan_mode_prompt_contains_governance_phases() {
        let prompt = system_prompt_for_mode(AgentMode::Plan);
        assert!(prompt.contains("MODE: PLAN"), "missing mode header");
        assert!(prompt.contains("ENTENDIMENTO"), "missing phase 1");
        assert!(prompt.contains("WRITE THE ROADMAP FILE"), "missing write phase enforcement");
        assert!(prompt.contains(".theo/plans/"), "missing roadmap output path");
        assert!(prompt.contains("Microtasks"), "missing microtasks section");
        assert!(prompt.contains("DoD"), "missing definition of done");
        assert!(prompt.contains("CALL THE WRITE TOOL"), "missing write enforcement");
        assert!(prompt.contains("BEGIN TEMPLATE"), "missing template");
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
        let he_pos = prompt.find("## Harness Context").expect("missing HE section");
        let critical_pos = prompt.find("## CRITICAL").expect("missing CRITICAL section");
        assert!(he_pos < critical_pos, "HE framing must come before CRITICAL");

        // 4 mandatory clauses
        assert!(prompt.contains("Clean state contract"), "missing clean state clause");
        assert!(prompt.contains("Generic tools"), "missing generic tools clause");
        assert!(prompt.contains("Environment legibility"), "missing environment legibility clause");
        assert!(prompt.contains("Code intelligence"), "missing code intelligence clause");
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
