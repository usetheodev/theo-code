//! Bearer token authentication — the most common LLM auth strategy.
//! Applies `Authorization: Bearer <token>` header.

use super::AuthStrategy;
use crate::error::LlmError;
use async_trait::async_trait;

/// Bearer token auth: `Authorization: Bearer <key>`.
pub struct BearerToken {
    key: Option<String>,
}

impl BearerToken {
    pub fn new(key: Option<String>) -> Self {
        Self { key }
    }
}

#[async_trait]
impl AuthStrategy for BearerToken {
    async fn apply(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<reqwest::RequestBuilder, LlmError> {
        match &self.key {
            Some(key) => Ok(builder.header("Authorization", format!("Bearer {key}"))),
            None => Err(LlmError::Parse("API key not configured".to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_with_key_creates() {
        let auth = BearerToken::new(Some("sk-test".to_string()));
        assert!(auth.key.is_some());
    }

    #[test]
    fn bearer_without_key_creates() {
        let auth = BearerToken::new(None);
        assert!(auth.key.is_none());
    }

    #[tokio::test]
    async fn bearer_with_key_applies_header() {
        let auth = BearerToken::new(Some("sk-test123".to_string()));
        let client = reqwest::Client::new();
        let builder = client.post("http://localhost/test");
        let result = auth.apply(builder).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn bearer_without_key_returns_error() {
        let auth = BearerToken::new(None);
        let client = reqwest::Client::new();
        let builder = client.post("http://localhost/test");
        let result = auth.apply(builder).await;
        assert!(result.is_err());
    }
}
