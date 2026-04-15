//! LLM Provider abstraction layer.
//!
//! Provides the `LlmProvider` trait that agent-runtime depends on (DIP),
//! `ProviderSpec` for declarative provider config, and `ProviderRegistry`
//! for discovery.

pub mod auth;
pub mod catalog;
pub mod client;
pub mod format;
pub mod registry;
pub mod spec;

use crate::error::LlmError;
use crate::stream::SseStream;
use crate::types::*;
use async_trait::async_trait;

/// Core trait for LLM providers — the abstraction agent-runtime depends on.
///
/// Implementors: `LlmClient` (backward compat), `SpecBasedProvider` (new).
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Send a chat completion request (non-streaming).
    async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError>;

    /// Send a streaming chat completion request.
    async fn chat_stream(&self, request: &ChatRequest) -> Result<SseStream, LlmError>;

    /// The model this provider is configured to use.
    fn model(&self) -> &str;

    /// The provider identifier (e.g., "openai", "anthropic", "groq").
    fn provider_id(&self) -> &str;
}
