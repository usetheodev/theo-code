//! Meta-tool `ToolDefinition` factories.
//!
//! Every meta-tool's OpenAI-compatible schema lives here as a named factory
//! function. This keeps `registry_to_definitions{,_for_subagent}` focused on
//! assembly, not on multi-hundred-line inline JSON blobs.
//!
//! Fase 4 (REMEDIATION_PLAN T4.6). Extracted from `tool_bridge.rs`.

use theo_infra_llm::types::ToolDefinition;

/// `tool_search` — keyword lookup over deferred (rarely-used) tools.
/// See Anthropic principle 12 (minimize context overhead). OpenDev
/// equivalent: `search_hint` + registry discovery (traits.rs:547-575).
pub(super) fn tool_search() -> ToolDefinition {
    ToolDefinition::new(
        "tool_search",
        concat!(
            "Search for deferred (rarely-used) tools by keyword. Returns a list of `(id, hint)` \
             pairs the agent can invoke by name. Use this only when none of the visible tools ",
            "fit the task. Example: tool_search({query: 'wiki lookup'})."
        ),
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Keyword to match against deferred tool ids and search hints"
                }
            },
            "required": ["query"]
        }),
    )
}

/// `batch_execute` — Programmatic Tool Calling (minimum-viable form).
/// Ordered list of `{tool, args}`, executed serially, early-exits on failure.
pub(super) fn batch_execute() -> ToolDefinition {
    ToolDefinition::new(
        "batch_execute",
        concat!(
            "Execute an ordered list of tool calls in one assistant turn. ",
            "Runs each step serially and stops at the first failure. Use this to collapse N round-trips ",
            "(e.g. 'fetch 3 URLs then summarise') into a single LLM generation — cuts ~30-50% of tokens ",
            "on parallelisable workflows. Each step is `{tool: string, args: object}` matching that ",
            "tool's own schema. The aggregated result is returned as a JSON block with per-step ",
            "`ok: bool`, `name`, and `result`/`error` fields. ",
            "Example: batch_execute({calls: [",
            "{tool: 'read', args: {filePath: 'Cargo.toml'}}, ",
            "{tool: 'grep', args: {pattern: 'version', path: '.'}} ]})."
        ),
        serde_json::json!({
            "type": "object",
            "properties": {
                "calls": {
                    "type": "array",
                    "description": "Ordered list of tool invocations to execute serially.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "tool": {
                                "type": "string",
                                "description": "Name of a visible tool (not a meta-tool). Cannot be `done`, `subagent`, `subagent_parallel`, `skill`, `tool_search`, or `batch_execute` itself."
                            },
                            "args": {
                                "type": "object",
                                "description": "Arguments for the invoked tool, matching its JSON schema."
                            }
                        },
                        "required": ["tool", "args"]
                    }
                }
            },
            "required": ["calls"]
        }),
    )
}

/// `done` — task-complete signal. Required summary for postmortem.
pub(super) fn done() -> ToolDefinition {
    ToolDefinition::new(
        "done",
        "Call when the task is complete. Requires a summary of what was accomplished.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "summary": {
                    "type": "string",
                    "description": "Brief summary of what was accomplished"
                }
            },
            "required": ["summary"]
        }),
    )
}

/// `skill` — invoke a packaged workflow (commit, test, review, build, explain).
pub(super) fn skill() -> ToolDefinition {
    ToolDefinition::new(
        "skill",
        "Invoke a specialized skill workflow. Skills provide expert instructions for common tasks like commit, test, review, build, explain. Use when the task matches a skill's trigger.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Skill name to invoke (e.g., 'commit', 'test', 'review', 'build', 'explain')"
                }
            },
            "required": ["name"]
        }),
    )
}

/// `delegate_task_single` — spawn ONE sub-agent (fixed required-shape schema
/// for weaker tool-callers like Codex that can't parse one-of).
pub(super) fn delegate_task_single() -> ToolDefinition {
    ToolDefinition::new(
        "delegate_task_single",
        concat!(
            "Spawn ONE sub-agent. Both `agent` and `objective` are REQUIRED. ",
            "Built-in agents: explorer, implementer, verifier, reviewer. ",
            "Custom agents loadable from .theo/agents/*.md. ",
            "Unknown names create on-demand READ-ONLY agents (max 10 iterations, 120s timeout)."
        ),
        serde_json::json!({
            "type": "object",
            "properties": {
                "agent": {
                    "type": "string",
                    "description": "Name of a registered agent OR an arbitrary name."
                },
                "objective": {
                    "type": "string",
                    "description": "What the agent should accomplish."
                },
                "context": {
                    "type": "string",
                    "description": "Optional background info, file paths, or constraints."
                }
            },
            "required": ["agent", "objective"]
        }),
    )
}

/// `delegate_task_parallel` — spawn multiple sub-agents concurrently.
pub(super) fn delegate_task_parallel() -> ToolDefinition {
    ToolDefinition::new(
        "delegate_task_parallel",
        concat!(
            "Spawn multiple sub-agents concurrently. `tasks` is REQUIRED. ",
            "Each task has `agent` and `objective` (both required) plus optional `context`."
        ),
        serde_json::json!({
            "type": "object",
            "properties": {
                "tasks": {
                    "type": "array",
                    "description": "List of agents to spawn in parallel.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "agent": { "type": "string" },
                            "objective": { "type": "string" },
                            "context": { "type": "string" }
                        },
                        "required": ["agent", "objective"]
                    }
                }
            },
            "required": ["tasks"]
        }),
    )
}

/// Legacy `delegate_task` — unified one-of schema kept for backward compat.
pub(super) fn delegate_task_legacy() -> ToolDefinition {
    ToolDefinition::new(
        "delegate_task",
        concat!(
            "DEPRECATED: prefer `delegate_task_single` or `delegate_task_parallel`. ",
            "Delegate work to a specialized sub-agent. Single-mode: pass `agent` + `objective`. ",
            "Parallel-mode: pass `parallel: [{agent, objective, context}, ...]`. ",
            "Built-in agents: explorer, implementer, verifier, reviewer."
        ),
        serde_json::json!({
            "type": "object",
            "properties": {
                "agent": {
                    "type": "string",
                    "description": "Mutually exclusive with `parallel`."
                },
                "objective": { "type": "string" },
                "context": { "type": "string" },
                "parallel": {
                    "type": "array",
                    "description": "Mutually exclusive with `agent`/`objective`.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "agent": { "type": "string" },
                            "objective": { "type": "string" },
                            "context": { "type": "string" }
                        },
                        "required": ["agent", "objective"]
                    }
                }
            }
        }),
    )
}

/// `batch` — parallel execution meta-tool (CodeAct-inspired). Max 25 calls.
pub(super) fn batch() -> ToolDefinition {
    ToolDefinition::new(
        "batch",
        "Execute multiple tool calls in a single turn. Use for independent operations like reading multiple files. Max 25 calls. Cannot include batch/done/delegate_task/skill inside.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "calls": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "tool": {
                                "type": "string",
                                "description": "Tool name (e.g., 'read', 'grep', 'glob', 'bash')"
                            },
                            "args": {
                                "type": "object",
                                "description": "Arguments for the tool"
                            }
                        },
                        "required": ["tool", "args"]
                    },
                    "description": "Array of tool calls to execute (max 25)"
                }
            },
            "required": ["calls"]
        }),
    )
}

/// `batch` meta-tool for sub-agents — narrower description (no meta-tool
/// nesting warning; sub-agents cannot delegate anyway).
pub(super) fn batch_for_subagent() -> ToolDefinition {
    ToolDefinition::new(
        "batch",
        "Execute multiple tool calls in a single turn. Use for independent operations like reading multiple files. Max 25 calls.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "calls": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "tool": { "type": "string" },
                            "args": { "type": "object" }
                        },
                        "required": ["tool", "args"]
                    }
                }
            },
            "required": ["calls"]
        }),
    )
}
