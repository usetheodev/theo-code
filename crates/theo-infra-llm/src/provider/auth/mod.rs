//! Authentication strategies for LLM providers.
//!
//! Strategy pattern: each auth type implements AuthStrategy.
//! Factory function creates the right strategy from AuthKind.

pub mod bearer;
pub mod header;

use super::spec::AuthKind;
use crate::error::LlmError;
use async_trait::async_trait;

/// Strategy for authenticating requests to an LLM provider.
#[async_trait]
pub trait AuthStrategy: Send + Sync {
    /// Apply authentication to a request builder.
    async fn apply(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<reqwest::RequestBuilder, LlmError>;
}

/// No authentication (local models like Ollama).
pub struct NoAuth;

#[async_trait]
impl AuthStrategy for NoAuth {
    async fn apply(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<reqwest::RequestBuilder, LlmError> {
        Ok(builder)
    }
}

/// Create the appropriate AuthStrategy from an AuthKind + optional API key override.
pub fn create_auth(kind: &AuthKind, api_key_override: Option<String>) -> Box<dyn AuthStrategy> {
    match kind {
        AuthKind::BearerFromEnv(env_var) => {
            let key = api_key_override.or_else(|| std::env::var(env_var).ok());
            Box::new(bearer::BearerToken::new(key))
        }
        AuthKind::CustomHeaderFromEnv { header, env_var } => {
            let key = api_key_override.or_else(|| std::env::var(env_var).ok());
            Box::new(header::CustomHeader::new(header, key))
        }
        AuthKind::AwsSigV4 { .. } => {
            // Stub — will be implemented in Phase 4 (feature-gated)
            Box::new(NoAuth)
        }
        AuthKind::GcpAdc { .. } => {
            // Stub — will be implemented in Phase 4 (feature-gated)
            Box::new(NoAuth)
        }
        AuthKind::None => Box::new(NoAuth),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_auth_bearer() {
        let auth = create_auth(
            &AuthKind::BearerFromEnv("NONEXISTENT_KEY"),
            Some("sk-test".to_string()),
        );
        // Just verify it creates without panic
        let _ = auth;
    }

    #[test]
    fn create_auth_custom_header() {
        let auth = create_auth(
            &AuthKind::CustomHeaderFromEnv {
                header: "x-api-key",
                env_var: "NONEXISTENT",
            },
            Some("key-123".to_string()),
        );
        let _ = auth;
    }

    #[test]
    fn create_auth_none() {
        let auth = create_auth(&AuthKind::None, None);
        let _ = auth;
    }
}
