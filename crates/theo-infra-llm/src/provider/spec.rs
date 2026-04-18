//! Provider specification — declarative config for LLM providers.
//!
//! A ProviderSpec is a const-constructible struct that describes everything
//! needed to communicate with an LLM provider: URL, auth, format, headers.
//!
//! Most OA-compatible providers are just a ProviderSpec const — zero code.

use serde::{Deserialize, Serialize};

/// What API format this provider speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatKind {
    /// Standard /v1/chat/completions — direct passthrough, no conversion.
    OaCompatible,
    /// Anthropic Messages API — requires format conversion.
    Anthropic,
    /// OpenAI Responses API (Codex) — requires format conversion.
    OpenAiResponses,
}

/// How this provider authenticates requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthKind {
    /// Bearer token from env var: `Authorization: Bearer <value>`.
    BearerFromEnv(&'static str),
    /// Custom header from env var: `<header>: <value>`.
    CustomHeaderFromEnv {
        header: &'static str,
        env_var: &'static str,
    },
    /// AWS SigV4 signing (for Bedrock).
    AwsSigV4 {
        region_env: &'static str,
        service: &'static str,
    },
    /// GCP Application Default Credentials (for Vertex AI).
    GcpAdc {
        project_env: &'static str,
        location_env: &'static str,
    },
    /// No authentication (local models like Ollama).
    None,
}

/// Declarative specification for an LLM provider.
///
/// Designed to be const-constructible so providers can be declared as consts
/// in a catalog module.
#[derive(Debug, Clone)]
pub struct ProviderSpec {
    /// Unique identifier: "openai", "anthropic", "groq", etc.
    pub id: &'static str,
    /// Human-readable name.
    pub display_name: &'static str,
    /// Base URL for the API.
    pub base_url: &'static str,
    /// Path appended to base_url for chat completions.
    pub chat_path: &'static str,
    /// API format this provider uses.
    pub format: FormatKind,
    /// Authentication mechanism.
    pub auth: AuthKind,
    /// Default headers always sent with requests.
    pub default_headers: &'static [(&'static str, &'static str)],
    /// Whether this provider supports streaming responses.
    pub supports_streaming: bool,
    /// Whether Hermes XML fallback should be applied for tool calls.
    pub hermes_fallback: bool,
}

impl ProviderSpec {
    /// Build the full endpoint URL for chat completions.
    pub fn endpoint_url(&self) -> String {
        format!("{}{}", self.base_url, self.chat_path)
    }

    /// Get the environment variable name for the API key (if applicable).
    pub fn api_key_env_var(&self) -> Option<&'static str> {
        match &self.auth {
            AuthKind::BearerFromEnv(env_var) => Some(env_var),
            AuthKind::CustomHeaderFromEnv { env_var, .. } => Some(env_var),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PROVIDER: ProviderSpec = ProviderSpec {
        id: "test",
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
    fn provider_spec_is_const_constructible() {
        assert_eq!(TEST_PROVIDER.id, "test");
        assert_eq!(TEST_PROVIDER.display_name, "Test Provider");
    }

    #[test]
    fn endpoint_url_concatenates_correctly() {
        assert_eq!(
            TEST_PROVIDER.endpoint_url(),
            "https://api.test.com/v1/chat/completions"
        );
    }

    #[test]
    fn api_key_env_var_returns_bearer_env() {
        assert_eq!(TEST_PROVIDER.api_key_env_var(), Some("TEST_API_KEY"));
    }

    #[test]
    fn api_key_env_var_returns_custom_header_env() {
        let spec = ProviderSpec {
            auth: AuthKind::CustomHeaderFromEnv {
                header: "x-api-key",
                env_var: "ANTHROPIC_API_KEY",
            },
            ..TEST_PROVIDER
        };
        assert_eq!(spec.api_key_env_var(), Some("ANTHROPIC_API_KEY"));
    }

    #[test]
    fn api_key_env_var_returns_none_for_no_auth() {
        let spec = ProviderSpec {
            auth: AuthKind::None,
            ..TEST_PROVIDER
        };
        assert_eq!(spec.api_key_env_var(), None);
    }

    #[test]
    fn format_kind_variants() {
        assert_ne!(FormatKind::OaCompatible, FormatKind::Anthropic);
        assert_ne!(FormatKind::Anthropic, FormatKind::OpenAiResponses);
    }

    #[test]
    fn auth_kind_bearer_equality() {
        assert_eq!(
            AuthKind::BearerFromEnv("KEY"),
            AuthKind::BearerFromEnv("KEY")
        );
        assert_ne!(
            AuthKind::BearerFromEnv("KEY1"),
            AuthKind::BearerFromEnv("KEY2")
        );
    }
}
