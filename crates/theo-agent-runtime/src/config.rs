use std::collections::HashMap;

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
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_iterations: 15,
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
1. READ first — use `read`, `grep`, `glob` to understand the codebase.
2. PLAN — use the `think` tool to plan your approach for complex tasks.
3. ACT — use `edit`, `write`, `bash` to make changes.
4. VERIFY — read the changed files to confirm correctness.
5. DONE — call `done` with a summary when the task is complete.

## Memory
Use `memory` to save/recall facts about the codebase across sessions.

## Self-Reflection
Use `reflect` to assess progress when stuck. Be honest about confidence.

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
        assert_eq!(config.max_iterations, 15);
        assert_eq!(config.temperature, 0.1);
        assert_eq!(config.context_loop_interval, 5);
        assert!(config.endpoint_override.is_none());
        assert!(config.extra_headers.is_empty());
    }
}
