//! T13.1 — Debug Adapter Protocol (DAP) primitives + tool family.
//!
//! Talks to external DAP servers (`lldb-vscode`, `debugpy`,
//! `vscode-js-debug`) for `set_breakpoint`/`step`/`eval`/`watch`. Like
//! `lsp::`, DAP uses `Content-Length: N\r\n\r\n<body>` framing — but
//! the message shapes are different (sequential `seq` numbers,
//! `type: request|response|event` discriminator).
//!
//! 11 agent-callable tools live in this module, one per file
//! (`status`, `launch`, `breakpoint`, `continue_`, `step`, `eval`,
//! `stack_trace`, `variables`, `scopes`, `threads`, `terminate`).
//! Shared helpers live in `tool_common.rs`. Pre-2026-04-28 the family
//! lived in a single 1783-LOC `tool.rs`; the per-file split was
//! T1.1 of `docs/plans/god-files-2026-07-23-plan.md`.

pub mod client;
pub mod discovery;
pub mod operations;
pub mod protocol;
pub mod session_manager;

mod tool_common;

mod breakpoint;
mod continue_;
mod eval;
mod launch;
mod scopes;
mod stack_trace;
mod status;
mod step;
mod terminate;
mod threads;
mod variables;

pub use breakpoint::DebugSetBreakpointTool;
pub use continue_::DebugContinueTool;
pub use eval::DebugEvalTool;
pub use launch::DebugLaunchTool;
pub use scopes::DebugScopesTool;
pub use stack_trace::DebugStackTraceTool;
pub use status::DebugStatusTool;
pub use step::DebugStepTool;
pub use terminate::DebugTerminateTool;
pub use threads::DebugThreadsTool;
pub use variables::DebugVariablesTool;

pub use client::{DapClient, DapClientError};
pub use discovery::{DiscoveredAdapter, discover as discover_adapters};
pub use protocol::{
    DapEvent, DapMessage, DapProtocolError, DapRequest, DapResponse, DapSeqGen, encode_frame,
    encode_message, try_decode_frame,
};
pub use session_manager::{DapSessionError, DapSessionManager};

#[cfg(test)]
#[path = "tool_tests.rs"]
mod tests;
