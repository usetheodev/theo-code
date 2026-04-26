//! Format converters for LLM providers.
//!
//! Template method pattern: each format kind has a converter that transforms
//! between internal OA-compatible types and the provider's API format.

pub mod anthropic;
pub mod codex;
pub mod passthrough;
pub mod serialize_oa;

use super::spec::FormatKind;
use crate::error::LlmError;
use crate::stream::StreamDelta;
use crate::types::{ChatRequest, ChatResponse};

/// Trait for converting between internal format and provider-specific format.
///
/// Default implementation: OA-compatible passthrough (identity).
pub trait FormatConverter: Send + Sync {
    /// Convert internal ChatRequest to provider-specific JSON body.
    fn convert_request(&self, request: &ChatRequest) -> serde_json::Value;

    /// Convert provider-specific JSON response to internal ChatResponse.
    fn convert_response(&self, body: serde_json::Value) -> Result<ChatResponse, LlmError>;

    /// Parse an SSE chunk line into a StreamDelta.
    /// Returns None for non-data lines.
    fn parse_chunk(&self, line: &str) -> Result<Option<StreamDelta>, LlmError> {
        // Default: standard OA-compatible SSE parsing
        Ok(crate::stream::parse_sse_line(line))
    }
}

/// Create the appropriate FormatConverter from a FormatKind.
pub fn create_converter(kind: FormatKind) -> Box<dyn FormatConverter> {
    match kind {
        FormatKind::OaCompatible => Box::new(passthrough::OaPassthrough),
        FormatKind::Anthropic => Box::new(anthropic::AnthropicConverter),
        FormatKind::OpenAiResponses => Box::new(codex::CodexConverter),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_converter_oa_compatible() {
        let converter = create_converter(FormatKind::OaCompatible);
        let _ = converter; // Just verify it creates
    }

    #[test]
    fn create_converter_anthropic() {
        let converter = create_converter(FormatKind::Anthropic);
        let _ = converter;
    }

    #[test]
    fn create_converter_codex() {
        let converter = create_converter(FormatKind::OpenAiResponses);
        let _ = converter;
    }
}
