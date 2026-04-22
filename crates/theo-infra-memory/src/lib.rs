//! Infrastructure layer for the agent memory subsystem.
//!
//! Houses:
//! - `MemoryEngine` — fan-out coordinator over multiple `MemoryProvider`s.
//! - `fs_util::atomic_write` — temp-file-plus-rename helper used by every
//!   memory writer to avoid torn files on crash.
//!
//! Plan: `outputs/agent-memory-plan.md` §RM1.
//! Ref: `referencias/hermes-agent/agent/memory_manager.py:97-206`.

pub mod builtin;
pub mod engine;
pub mod fs_util;
pub mod lint;
pub mod retrieval;
pub mod security;
pub mod session_search_fs;
pub mod wiki;

pub use builtin::BuiltinMemoryProvider;
pub use engine::{EngineStats, MemoryEngine};
pub use fs_util::atomic_write;
pub use lint::{LessonMetric, LintInputs, LintIssue, LintThresholds, Severity, render_json, run_lint};
pub use retrieval::{
    MemoryRetrieval, RetrievalBackedMemory, ScoredMemory, SourceType, ThresholdConfig,
    pack_within_budget,
};
pub use security::{InjectionReason, scan as security_scan};
pub use session_search_fs::{FsSessionSearch, render_hits};
pub use wiki::{HashManifest, SourceHash, lint_pages, parse_page};
