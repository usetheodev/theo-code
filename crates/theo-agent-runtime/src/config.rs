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

## MODE: PLAN
You are in PLAN mode. Before doing ANY work:
1. THINK about the task thoroughly using the `think` tool.
2. Read relevant files to understand the current state.
3. Create a DETAILED PLAN with numbered steps, files to change, and expected outcomes.
4. Present the plan to the user as a text response.
5. Do NOT use edit, write, apply_patch, or bash (except for read-only commands) until the user says "go", "ok", "execute", "proceed", or similar approval.
6. After approval, execute the plan step by step.

If the user asks to modify the plan, adjust it and present again."#,
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
            is_subagent: false,
            capability_set: None,
            doom_loop_threshold: Some(3),
        }
    }
}

fn default_system_prompt() -> &'static str {
    r#"You are an expert software engineer working inside a project repository. You have tools to read, write, edit files, run bash commands, and search code.

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

## Skills
You have auto-invocable skills for common tasks. When the user's request matches a skill, invoke it with the `skill` tool.
Skills inject specialized instructions or delegate to a focused sub-agent. Available skills are listed in the system context.

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
    fn plan_mode_prompt_contains_plan_instructions() {
        let prompt = system_prompt_for_mode(AgentMode::Plan);
        assert!(prompt.contains("MODE: PLAN"));
        assert!(prompt.contains("DETAILED PLAN"));
        assert!(prompt.contains("Do NOT use edit"));
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
}
