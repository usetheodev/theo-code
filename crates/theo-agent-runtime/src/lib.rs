pub mod agent_loop;
pub mod agent_message;
pub mod budget_enforcer;
pub mod capability_gate;
pub mod compaction;
pub mod compaction_stages;
pub mod compaction_summary;
pub mod config;
pub mod context_metrics;
pub mod convergence;
#[doc(hidden)] // Dead code: exported but never instantiated externally
pub mod correction;
pub mod dlq;
pub mod event_bus;
pub mod evolution;
pub mod extension;
pub mod failure_tracker;
pub mod hooks;
pub mod loop_state;
pub mod metrics;
#[doc(hidden)] // Dead code: exported but never instantiated externally
pub mod observability;
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
pub mod snapshot;
pub mod subagent;
pub mod task_manager;
pub mod tool_bridge;
pub mod tool_call_manager;

pub use agent_loop::{AgentLoop, AgentResult};
pub use config::{AgentConfig, MessageQueues, ToolExecutionMode};
pub use event_bus::{EventBus, EventListener};
pub use run_engine::AgentRunEngine;
