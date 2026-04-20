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
pub mod security;

pub use builtin::BuiltinMemoryProvider;
pub use engine::{EngineStats, MemoryEngine};
pub use fs_util::atomic_write;
pub use security::{InjectionReason, scan as security_scan};
