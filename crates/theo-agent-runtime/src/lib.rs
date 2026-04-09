pub mod budget_enforcer;
pub mod capability_gate;
pub mod compaction;
pub mod context_metrics;
pub mod config;
pub mod convergence;
#[doc(hidden)] // Dead code: exported but never instantiated externally
pub mod correction;
pub mod dlq;
pub mod event_bus;
pub mod failure_tracker;
pub mod hooks;
pub mod events;
pub mod metrics;
#[doc(hidden)] // Dead code: exported but never instantiated externally
pub mod observability;
pub mod persistence;
pub mod pilot;
pub mod reflector;
pub mod plugin;
pub mod project_config;
pub mod roadmap;
pub mod retry;
pub mod snapshot;
pub mod run_engine;
pub mod session_bootstrap;
#[doc(hidden)] // Dead code: exported but never instantiated externally
pub mod scheduler;
pub mod skill;
pub mod subagent;
pub mod state;
pub mod task_manager;
pub mod tool_call_manager;
pub mod tool_bridge;
pub mod agent_loop;

pub use config::AgentConfig;
#[allow(deprecated)]
pub use events::{AgentEvent, EventSink};
pub use event_bus::{EventBus, EventListener};
pub use run_engine::AgentRunEngine;
#[allow(deprecated)]
pub use state::{AgentState, Phase};
pub use agent_loop::{AgentLoop, AgentResult};
