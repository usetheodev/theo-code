//! Backward-compat re-export of `AgentResult`.
//!
//! T4.10f / find_p3_005 — the type itself moved to `crate::result`
//! to break the `agent_loop` ↔ `run_engine` cycle. This file is kept
//! so the historical path `crate::agent_loop::result::AgentResult`
//! still resolves; the public API at
//! `theo_agent_runtime::agent_loop::AgentResult` remains stable via
//! the re-export in `agent_loop/mod.rs`.

pub use crate::result::AgentResult;
