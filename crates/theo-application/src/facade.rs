//! Application-layer facade that re-exports the subset of lower-layer
//! types that `apps/*` legitimately needs.
//!
//! Per `.claude/rules/architecture.md` + ADR-010, apps must depend only on
//! `theo-application` and `theo-api-contracts`. This module is the
//! migration vehicle for T1.2 / T1.3: apps rewrite their
//! `use theo_agent_runtime::…` / `use theo_infra_llm::…` /
//! `use theo_tooling::…` imports to `use theo_application::facade::…`,
//! preserving binary compatibility while the decoupling lands.
//!
//! The facade is intentionally **narrow** — we only expose types already
//! consumed by existing apps (cf. 43 violations reported by
//! `scripts/check-arch-contract.sh`). New apps SHOULD NOT reach for the
//! facade; they should consume `theo-application::use_cases::*`
//! directly.

/// Types that apps pulled in from `theo-agent-runtime` before T1.2.
///
/// Each re-export lists the concrete app(s) that depend on it. Dropping
/// a re-export must be paired with removing the corresponding app import.
pub mod agent {
    // Config — consumed by theo-cli (pilot, tui) and theo-desktop (state).
    pub use theo_agent_runtime::AgentConfig;
    // Event bus — consumed by theo-cli (renderer, pilot, tui), theo-desktop.
    pub use theo_agent_runtime::event_bus::{EventBus, EventListener};
    // AgentLoop — consumed by theo-cli (tui, main).
    pub use theo_agent_runtime::AgentLoop;
    // Pilot — theo-cli autonomous mode.
    pub use theo_agent_runtime::pilot::{
        self, PilotConfig, PilotLoop, PilotResult, load_promise,
    };
    // Roadmap — theo-cli pilot subcommand (legacy markdown plans).
    pub use theo_agent_runtime::roadmap::{find_latest_roadmap, parse_roadmap};
    // Plan store — SOTA Planning System (canonical JSON plans).
    pub use theo_agent_runtime::plan_store::{find_latest_plan, load_plan, save_plan};
    // Observability listener types — theo-desktop.
    pub use theo_agent_runtime::observability;
    // Agent config helpers — theo-cli main.
    pub use theo_agent_runtime::config::{self, AgentMode, system_prompt_for_mode};
    // Project config — theo-cli main, theo-desktop.
    pub use theo_agent_runtime::project_config;
    // Skill registry — theo-cli TUI.
    pub use theo_agent_runtime::skill;
    // T9.1 — Skill catalog CRUD primitives consumed by the
    // `theo skill` CLI subcommand. The high-level use case lives at
    // `theo_application::use_cases::skills` (list/view/theo_home).
    pub use theo_agent_runtime::skill_catalog::{
        delete_skill, SkillCatalogError, SkillMetadata, SkillView,
    };
}

/// Types that apps pulled in from `theo-infra-llm` before T1.2.
pub mod llm {
    pub use theo_infra_llm::provider::registry as provider_registry;
    pub use theo_infra_llm::types::Message;

    /// Routing infrastructure (Phase 14 + 27 sota-gaps-followup gap #4).
    /// Re-exported so the CLI can wire `AutomaticModelRouter` into
    /// `AgentConfig.router` without breaking ADR-016.
    pub mod routing {
        pub use theo_infra_llm::routing::{
            AutomaticModelRouter, RoutingConfig, RuleBasedRouter,
            env_disables_routing,
        };
        pub use theo_infra_llm::routing::auto::RoutingMetricsRecorder;
    }
}

/// Types that apps pulled in from `theo-tooling` before T1.2.
pub mod tooling {
    pub use theo_tooling::registry::{
        create_default_registry, create_default_registry_with_project,
    };
    // File-backed memory store — theo-cli TUI persists session memory via this.
    pub use theo_tooling::memory;
}

/// MCP types that apps need for the discovery cache + admin CLI.
/// Phase 21 (sota-gaps-followup): exposes the minimal surface that
/// `apps/theo-cli/src/mcp_admin.rs` consumes (DiscoveryCache, registry,
/// timeout constant, server config).
pub mod mcp {
    pub use theo_infra_mcp::{
        effective_default_timeout, DEFAULT_PER_SERVER_TIMEOUT, DiscoveryCache,
        DiscoveryReport, McpRegistry, McpServerConfig, McpTool,
    };
}

/// Phase 41 (otlp-exporter-plan) — OTLP exporter surface for the CLI.
/// Gated by feature `otel`; default builds skip this module entirely
/// and `theo-cli` skips wiring (no-op).
#[cfg(feature = "otel")]
pub mod observability {
    pub use theo_agent_runtime::observability::otel_exporter::{
        init_otlp_exporter, OtelInitError, OtlpExporterConfig, OtlpGuard, OtlpProtocol,
    };
}

/// Handoff guardrail types for the apps layer (Phase 18 + 23).
pub mod handoff_guardrail {
    pub use theo_agent_runtime::handoff_guardrail::{
        GuardrailChain, GuardrailDecision, HandoffContext, HandoffGuardrail,
    };
}

/// Types that apps pulled in from `theo-infra-auth` before T1.2.
///
/// Kept narrow — expose only what theo-cli + theo-desktop currently wire
/// up for OAuth / API-key login flows.
pub mod auth {
    pub use theo_infra_auth::{OpenAIAuth, CopilotAuth, CopilotConfig};
    pub use theo_infra_auth::device_flow;
    // Auth store — cli & desktop persist tokens here.
    pub use theo_infra_auth::store::{self, AuthStore};
    // Provider-specific device-code modules used by the desktop shim.
    pub use theo_infra_auth::{openai, copilot, anthropic};
    // Anthropic auth types are only compiled when the corresponding
    // feature is enabled in theo-infra-auth. Wrap in a re-export that
    // mirrors the source crate's surface.
    pub use theo_infra_auth::{AnthropicAuth, AnthropicConfig};
}
