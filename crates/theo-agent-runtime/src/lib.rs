pub mod agent_loop;
pub mod agent_message;
pub mod budget_enforcer;
pub mod capability_gate;
pub mod compaction;
pub mod compaction_stages;
pub mod compaction_summary;
pub mod config;
pub mod convergence;
#[doc(hidden)] // Dead code: exported but never instantiated externally
pub mod correction;
pub mod dlq;
pub mod event_bus;
pub mod evolution;
pub mod extension;
pub mod failure_tracker;
pub mod frontmatter;
pub mod hooks;
pub mod hypothesis_pipeline;
pub mod jit_instructions;
pub mod lesson_pipeline;
pub mod loop_state;
pub mod autodream;
mod doom_loop;
pub mod memory_lifecycle;
pub mod memory_reviewer;
pub mod onboarding;
pub mod observability;

// Re-exports preserving the legacy module paths. Consumers can keep using
// `theo_agent_runtime::metrics::*` and `theo_agent_runtime::context_metrics::*`.
pub use observability::context_metrics;
pub use observability::metrics;
pub mod persistence;
pub mod pilot;
pub mod plugin;
pub mod project_config;
pub mod reflector;
pub mod retry;
pub mod roadmap;
pub mod sanitizer;
pub mod run_engine;
pub mod sensor;
#[doc(hidden)] // Dead code: exported but never instantiated externally
pub mod scheduler;
pub mod session_tree;
pub mod session_bootstrap;
pub mod state_manager;
pub mod system_prompt_composer;
pub mod skill;
pub mod skill_catalog;
pub mod skill_reviewer;
pub mod transcript_indexer;
pub mod snapshot;
pub mod subagent;
pub mod task_manager;
pub mod tool_bridge;
pub mod tool_call_manager;

pub use agent_loop::{AgentLoop, AgentResult};
pub use config::{AgentConfig, CompactionPolicy, MessageQueues, ToolExecutionMode};
pub use event_bus::{EventBus, EventListener};
pub use run_engine::AgentRunEngine;
