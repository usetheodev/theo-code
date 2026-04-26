// T4.10h / find_p3_007 — public surface trimmed.
//
// Modules below are split into 3 visibility tiers based on a grep audit
// of `theo-application/src/` and `apps/*/src/`:
//
//   1. `pub mod`      — externally consumed (theo-application or apps).
//   2. `pub(crate) mod` — internal helpers; no outside consumer found.
//   3. `mod`          — file-local helpers (none promoted out of mod root).
//
// Re-exports at the bottom preserve the public API (`AgentLoop`,
// `AgentConfig`, `EventBus`, etc.) so this trim does not break callers.

// ── Public modules (consumed by theo-application or apps/*) ────────────
pub mod agent_loop;
pub mod agent_message;
pub mod cancellation;
pub mod capability_gate;
pub mod checkpoint;
pub mod compaction;
pub mod config;
pub mod constants;
pub mod convergence;
pub mod dlq;
pub mod event_bus;
pub mod evolution;
pub mod handoff_guardrail;
pub mod hooks;
pub mod lifecycle_hooks;
pub mod observability;
pub mod onboarding;
pub mod output_format;
pub mod pilot;
pub mod plan_findings;
pub mod plan_progress;
pub mod plan_store;
pub mod project_config;
pub mod result;
pub mod roadmap;
pub mod run_engine;
pub mod session_tree;
pub mod skill;
pub mod subagent;
pub mod subagent_runs;
pub mod transcript_indexer;

// ── Internal modules (pub(crate) — T4.10h) ─────────────────────────────
//
// Note: a subset is `pub` (not `pub(crate)`) because integration tests
// in `crates/theo-agent-runtime/tests/` reach into them. Integration
// tests are first-class consumers — they live in a separate compilation
// unit that sees the same surface as external crates.
pub mod autodream;          // tests/observability_e2e.rs
pub mod budget_enforcer;    // observability tests
pub(crate) mod compaction_stages;
pub(crate) mod compaction_summary;
pub(crate) mod extension;
pub(crate) mod failure_tracker;
pub mod frontmatter;        // doctest in frontmatter::split_frontmatter
pub(crate) mod fs_errors;
pub(crate) mod hypothesis_pipeline;
pub(crate) mod jit_instructions;
pub(crate) mod lesson_pipeline;
pub(crate) mod loop_state;
pub mod memory_lifecycle;   // tests/memory_*.rs
pub mod memory_reviewer;    // tests/memory_*.rs
pub(crate) mod persistence;
pub(crate) mod plugin;
pub(crate) mod reflector;
pub(crate) mod retry;
pub(crate) mod run_engine_auto_init;
pub(crate) mod run_engine_helpers;
pub(crate) mod run_engine_sandbox;
pub(crate) mod secret_scrubber;
pub(crate) mod sensor;
pub(crate) mod session_bootstrap;
pub(crate) mod skill_catalog;
pub mod skill_reviewer;     // tests/sota12_integration.rs
pub(crate) mod snapshot;
pub mod state_manager;      // tests/state_manager_failure.rs
pub(crate) mod system_prompt_composer;
pub mod task_manager;       // tests/observability_pipeline.rs
pub mod tool_bridge;        // tests/meta_tools_t7_3.rs
pub mod tool_call_manager;  // tests/observability_pipeline.rs
pub(crate) mod tool_pair_integrity;

// ── File-local modules ─────────────────────────────────────────────────
mod doom_loop;

// ── Re-exports preserving the legacy module paths ──────────────────────
// Consumers can keep using `theo_agent_runtime::metrics::*` and
// `theo_agent_runtime::context_metrics::*`.
pub use observability::context_metrics;
pub use observability::metrics;

// T1.2 deprecated alias `pub use tool_pair_integrity as sanitizer`
// REMOVED in T4.10h — `tool_pair_integrity` is now `pub(crate)` (no
// external consumers per grep audit) and a re-export of a crate-private
// item cannot be `pub`. The grep audit confirmed zero references to the
// `sanitizer::*` path outside this crate, so the deprecation cycle is
// considered complete.

// ── Public re-exports (the documented surface) ─────────────────────────
pub use agent_loop::{AgentLoop, AgentResult, SubAgentIntegrations};
pub use config::{AgentConfig, CompactionPolicy, MessageQueues, ToolExecutionMode};
pub use event_bus::{EventBus, EventListener};
pub use run_engine::AgentRunEngine;
