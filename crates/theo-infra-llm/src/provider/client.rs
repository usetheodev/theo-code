//! SpecBasedProvider — creates an LlmProvider from a ProviderSpec.
//!
//! Composes: ProviderSpec (config) + auth (strategy) + format (converter) + HTTP transport.
//! This is the primary implementation for the new provider system.
//!
//! For now, this delegates to LlmClient internally (Phase 1 — backward compat).
//! In Phase 2, it will use AuthStrategy + FormatConverter directly.

use super::LlmProvider;
use super::spec::ProviderSpec;
use crate::client::LlmClient;
use crate::error::LlmError;
use crate::stream::SseStream;
use crate::types::*;
use async_trait::async_trait;

/// Provider implementation that derives behavior from a ProviderSpec.
///
/// Phase 1: delegates to LlmClient (proven, backward-compatible).
/// Phase 2: will compose AuthStrategy + FormatConverter directly.
pub struct SpecBasedProvider {
    spec: ProviderSpec,
    inner: LlmClient,
}

impl SpecBasedProvider {
    /// Create a new SpecBasedProvider from a spec and optional API key override.
    pub fn new(spec: ProviderSpec, model: &str, api_key_override: Option<String>) -> Self {
        // Resolve API key: override > env var
        let api_key = api_key_override.or_else(|| {
            spec.api_key_env_var()
                .and_then(|var| std::env::var(var).ok())
        });

        // Build LlmClient from spec (Phase 1 — delegates to proven code)
        let mut client = LlmClient::new(spec.base_url, api_key, model);

        // Apply default headers from spec
        for &(key, value) in spec.default_headers {
            client = client.with_header(key, value);
        }

        // If chat_path differs from default, set endpoint override
        let default_path = "/v1/chat/completions";
        if spec.chat_path != default_path {
            let full_url = spec.endpoint_url();
            client = client.with_endpoint(full_url);
        }

        Self {
            spec,
            inner: client,
        }
    }

    /// Get the underlying ProviderSpec.
    pub fn spec(&self) -> &ProviderSpec {
        &self.spec
    }
}

#[async_trait]
impl LlmProvider for SpecBasedProvider {
    async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        self.inner.chat(request).await
    }

    async fn chat_stream(&self, request: &ChatRequest) -> Result<SseStream, LlmError> {
        self.inner.chat_stream(request).await
    }

    fn model(&self) -> &str {
        self.inner.model()
    }

    fn provider_id(&self) -> &str {
        self.spec.id
    }
}

#[cfg(test)]
mod tests {
    use super::super::spec::*;
    use super::*;

    const TEST_SPEC: ProviderSpec = ProviderSpec {
        id: "test_provider",
        display_name: "Test Provider",
        base_url: "https://api.test.com",
        chat_path: "/v1/chat/completions",
        format: FormatKind::OaCompatible,
        auth: AuthKind::BearerFromEnv("TEST_API_KEY"),
        default_headers: &[],
        supports_streaming: true,
        hermes_fallback: false,
    };

    #[test]
    fn spec_based_provider_creates_from_spec() {
        let provider = SpecBasedProvider::new(TEST_SPEC, "gpt-4", None);
        assert_eq!(provider.provider_id(), "test_provider");
        assert_eq!(provider.model(), "gpt-4");
    }

    #[test]
    fn spec_based_provider_with_api_key_override() {
        let provider = SpecBasedProvider::new(TEST_SPEC, "gpt-4", Some("sk-override".to_string()));
        assert_eq!(provider.provider_id(), "test_provider");
    }

    #[test]
    fn spec_based_provider_preserves_spec() {
        let provider = SpecBasedProvider::new(TEST_SPEC, "gpt-4", None);
        assert_eq!(provider.spec().id, "test_provider");
        assert_eq!(provider.spec().base_url, "https://api.test.com");
    }

    #[test]
    fn spec_based_provider_with_custom_headers() {
        let spec = ProviderSpec {
            default_headers: &[("anthropic-version", "2023-06-01")],
            ..TEST_SPEC
        };
        let provider = SpecBasedProvider::new(spec, "claude-3", None);
        assert_eq!(provider.provider_id(), "test_provider");
    }

    #[test]
    fn spec_based_provider_with_custom_path() {
        let spec = ProviderSpec {
            chat_path: "/v1/messages",
            ..TEST_SPEC
        };
        let provider = SpecBasedProvider::new(spec, "claude-3", None);
        assert_eq!(provider.provider_id(), "test_provider");
    }
}
