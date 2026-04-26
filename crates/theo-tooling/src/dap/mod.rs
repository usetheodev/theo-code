//! T13.1 — Debug Adapter Protocol primitives.
//!
//! Talks to external DAP servers (`lldb-vscode`, `debugpy`,
//! `vscode-js-debug`) for `set_breakpoint`/`step`/`eval`/`watch`. Like
//! `lsp::`, DAP uses `Content-Length: N\r\n\r\n<body>` framing — but
//! the message shapes are different (sequential `seq` numbers,
//! `type: request|response|event` discriminator).
//!
//! This module ships the protocol layer only. Server discovery + tool
//! execution (`debug_set_breakpoint`, etc.) is the next iteration.

pub mod client;
pub mod discovery;
pub mod operations;
pub mod protocol;
pub mod session_manager;

pub use client::{DapClient, DapClientError};
pub use discovery::{discover as discover_adapters, DiscoveredAdapter};
pub use protocol::{
    encode_frame, encode_message, try_decode_frame, DapEvent, DapMessage, DapProtocolError,
    DapRequest, DapResponse, DapSeqGen,
};
pub use session_manager::{DapSessionError, DapSessionManager};
