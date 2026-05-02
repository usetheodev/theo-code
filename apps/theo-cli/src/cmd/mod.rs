//! Command handlers for `theo` CLI subcommands.
//!
//! Decomposed from monolithic cmd.rs during T5.3.b of
//! god-files-2026-07-23-plan.md (ADR D6 — one file per subcommand).
//! Each `cmd_*` function handles one CLI subcommand. Cross-cutting
//! helpers (resolve_agent_config, build_fresh, resolve_dir) live in
//! helpers.rs.

#![allow(unused_imports, dead_code)]

mod auth;
mod context;
mod dashboard;
mod headless;
mod helpers;
mod impact;
mod init;
mod agent;
mod pilot;
mod stats;
mod trajectory;

pub use auth::*;
pub use context::*;
pub use dashboard::*;
pub use headless::*;
pub use helpers::*;
pub use impact::*;
pub use init::*;
pub use agent::*;
pub use pilot::*;
pub use stats::*;
pub use trajectory::*;
