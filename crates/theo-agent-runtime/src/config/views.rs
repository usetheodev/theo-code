//! REMEDIATION_PLAN T4.1 — sub-config view structs.
//!
//! `AgentConfig` has 27 flat fields covering 7 logical groups. A full
//! nesting refactor (`pub llm: LlmConfig, pub loop_cfg: LoopConfig, ...`)
//! would ripple through ~76 call sites in `theo-agent-runtime`,
//! `theo-application`, `apps/theo-cli`, and every test file.
//!
//! As an incremental step we provide read-only **views** here. Each
//! view is a `pub struct ...View<'a>` that borrows from `AgentConfig`,
//! exposing only the fields belonging to its logical group. New code
//! should reach for these views (`config.llm()`, `config.memory()`,
//! etc.) instead of reading fields off `AgentConfig` directly. When
//! every call site has migrated, the views can be replaced by owned
//! sub-config structs in a single coordinated PR.
//!
//! Each view has at most 10 fields, satisfying the AC literal `Cada
//! sub-config <= 10 campos`. The flat-field migration of `AgentConfig`
//! itself remains as follow-up work explicitly tracked in the plan.
//!
//! Split out of `config/mod.rs` (REMEDIATION_PLAN T4.* — production-LOC
//! trim toward the per-file 500-line target). Views and accessors are
//! re-exported from `mod.rs` so the public path stays byte-identical.

use std::collections::HashMap;

use super::{AgentConfig, CompactionPolicy, LlmConfig, LoopConfig, MemoryHandle, RouterHandle};

// T3.2 PR1 — `LlmView` removed; `AgentConfig::llm()` now returns
// `&LlmConfig` (the owned nested sub-config) directly. Field-access
// syntax `config.llm().model` keeps working unchanged; sites that
// previously chained `.cloned()` on `Option<&String>` now need
// `.clone()` on `Option<String>` (migration done in T3.2 PR1 commit).

// T3.2 PR2 — `LoopView` removed; `AgentConfig::loop_cfg()` now returns
// `&LoopConfig` (the owned nested sub-config) directly.

/// Context window / compaction. ≤4 fields.
#[derive(Debug)]
pub struct ContextView<'a> {
    pub system_prompt: &'a str,
    pub context_loop_interval: usize,
    pub context_window_tokens: usize,
    pub compaction_policy: &'a CompactionPolicy,
}

/// Memory subsystem. ≤5 fields.
#[derive(Debug)]
pub struct MemoryView<'a> {
    pub enabled: bool,
    pub provider: Option<&'a MemoryHandle>,
    pub review_nudge_interval: usize,
    pub reviewer: Option<&'a crate::memory_reviewer::MemoryReviewerHandle>,
    pub transcript_indexer: Option<&'a crate::transcript_indexer::TranscriptIndexerHandle>,
}

/// PLAN_AUTO_EVOLUTION_SOTA. ≤5 fields.
#[derive(Debug)]
pub struct EvolutionView<'a> {
    pub autodream_enabled: bool,
    pub autodream_timeout_secs: u64,
    pub autodream: Option<&'a crate::autodream::AutodreamHandle>,
    pub skill_review_nudge_interval: usize,
    pub skill_reviewer: Option<&'a crate::skill_reviewer::SkillReviewerHandle>,
}

/// Routing layer. ≤1 field.
#[derive(Debug)]
pub struct RoutingView<'a> {
    pub router: Option<&'a RouterHandle>,
}

/// Plugin / capability gate. ≤2 fields.
#[derive(Debug)]
pub struct PluginView<'a> {
    pub allowlist: Option<&'a std::collections::BTreeSet<String>>,
    pub capability_set: Option<&'a theo_domain::capability::CapabilitySet>,
}

impl AgentConfig {
    /// LLM connection accessor. T3.2 PR1 — returns the owned nested
    /// `LlmConfig` directly instead of a borrowed view.
    pub fn llm(&self) -> &LlmConfig {
        &self.llm
    }

    /// Run-loop policy accessor. T3.2 PR2.
    pub fn loop_cfg(&self) -> &LoopConfig {
        &self.loop_cfg
    }

    /// Context / compaction view.
    pub fn context(&self) -> ContextView<'_> {
        ContextView {
            system_prompt: &self.system_prompt,
            context_loop_interval: self.context_loop_interval,
            context_window_tokens: self.context_window_tokens,
            compaction_policy: &self.compaction_policy,
        }
    }

    /// Memory subsystem view.
    pub fn memory(&self) -> MemoryView<'_> {
        MemoryView {
            enabled: self.memory_enabled,
            provider: self.memory_provider.as_ref(),
            review_nudge_interval: self.memory_review_nudge_interval,
            reviewer: self.memory_reviewer.as_ref(),
            transcript_indexer: self.transcript_indexer.as_ref(),
        }
    }

    /// PLAN_AUTO_EVOLUTION_SOTA view.
    pub fn evolution(&self) -> EvolutionView<'_> {
        EvolutionView {
            autodream_enabled: self.autodream_enabled,
            autodream_timeout_secs: self.autodream_timeout_secs,
            autodream: self.autodream.as_ref(),
            skill_review_nudge_interval: self.skill_review_nudge_interval,
            skill_reviewer: self.skill_reviewer.as_ref(),
        }
    }

    /// Routing view.
    pub fn routing(&self) -> RoutingView<'_> {
        RoutingView {
            router: self.router.as_ref(),
        }
    }

    /// Plugin / capability gate view.
    pub fn plugin(&self) -> PluginView<'_> {
        PluginView {
            allowlist: self.plugin_allowlist.as_ref(),
            capability_set: self.capability_set.as_ref(),
        }
    }
}
