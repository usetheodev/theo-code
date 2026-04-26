pub mod facade;
pub mod use_cases;

/// CLI-facing re-exports of `theo-agent-runtime` types.
///
/// T3.3 / find_p3_009 / ADR-023 — `apps/theo-cli/` previously imported
/// `theo-agent-runtime` directly, violating the apps/* → theo-application
/// layer rule. This module surfaces the runtime types the CLI needs
/// behind the `theo-application` namespace so the CLI can switch to
/// `use theo_application::cli_runtime::...` and the temporary
/// allowlist exception in `scripts/check-arch-contract.sh` can be
/// retired.
pub mod cli_runtime {
    pub use theo_agent_runtime::cancellation::CancellationTree;
    pub use theo_agent_runtime::checkpoint::CheckpointManager;
    pub use theo_agent_runtime::config::AgentConfig;
    pub use theo_agent_runtime::event_bus::EventBus;
    pub use theo_agent_runtime::subagent::{
        approval, builtins, watcher, ApprovalMode, ReloadableRegistry,
        ResumeError, Resumer, SubAgentManager, SubAgentRegistry,
    };
    pub use theo_agent_runtime::subagent_runs::{
        FileSubagentRunStore, RunStatus, SubagentRun,
    };
}
