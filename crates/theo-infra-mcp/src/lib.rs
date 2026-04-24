//! `theo-infra-mcp` — Model Context Protocol client for Theo Code.
//!
//! Track C — Phase 8.
//!
//! Implements the MCP spec (modelcontextprotocol.io 2025-03-26): JSON-RPC 2.0
//! over stdio (default) or HTTP (Streamable HTTP). Sub-agents (and the parent)
//! consume external tool servers (databases, IDEs, custom integrations).
//!
//! References:
//! - Anthropic MCP Spec: https://modelcontextprotocol.io/
//! - OpenDev `crates/opendev-mcp/` — Rust reference implementation
//! - Hermes `tools/mcp_tool.py` — feature-rich Python implementation
//!
//! Scope (this iteration):
//! - JSON-RPC 2.0 message types (request/response/error)
//! - `McpClient` trait (transport-agnostic)
//! - `StdioTransport` (subprocess-based, kill on drop)
//! - `McpServerConfig` (name, command, env, args)
//! - `tools/list` + `tools/call` discovery and invocation
//!
//! Out of scope (future iterations):
//! - HTTP transport (requires reqwest streaming)
//! - OAuth 2.1 manager
//! - Resources protocol (`resources/list`, `resources/read`)

pub mod client;
pub mod config;
pub mod discovery;
pub mod dispatch;
pub mod error;
pub mod protocol;
pub mod registry;
pub mod transport_http;
pub mod transport_stdio;

pub use client::{McpAnyClient, McpClient, McpHttpClient, McpStdioClient};
pub use config::McpServerConfig;
pub use discovery::{
    effective_default_timeout, shared_cache, DiscoveryCache, DiscoveryReport,
    DEFAULT_PER_SERVER_TIMEOUT,
};
pub use dispatch::{DispatchOutcome, McpDispatcher};
pub use error::McpError;
pub use protocol::{McpRequest, McpResponse, McpTool};
pub use registry::McpRegistry;
