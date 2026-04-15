//! Custom header authentication — for providers like Anthropic (x-api-key).

use super::AuthStrategy;
use crate::error::LlmError;
use async_trait::async_trait;

/// Custom header auth: `<header_name>: <key>`.
/// Used by Anthropic (x-api-key), GitLab (PRIVATE-TOKEN), etc.
pub struct CustomHeader {
    header_name: &'static str,
    key: Option<String>,
}

impl CustomHeader {
    pub fn new(header_name: &'static str, key: Option<String>) -> Self {
        Self { header_name, key }
    }
}

#[async_trait]
impl AuthStrategy for CustomHeader {
    async fn apply(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<reqwest::RequestBuilder, LlmError> {
        match &self.key {
            Some(key) => Ok(builder.header(self.header_name, key.as_str())),
            None => Err(LlmError::Parse(format!(
                "API key not configured for header '{}'",
                self.header_name
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn custom_header_with_key_applies() {
        let auth = CustomHeader::new("x-api-key", Some("sk-ant-test".to_string()));
        let client = reqwest::Client::new();
        let builder = client.post("http://localhost/test");
        let result = auth.apply(builder).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn custom_header_without_key_errors() {
        let auth = CustomHeader::new("x-api-key", None);
        let client = reqwest::Client::new();
        let builder = client.post("http://localhost/test");
        let result = auth.apply(builder).await;
        assert!(result.is_err());
    }

    #[test]
    fn custom_header_stores_name() {
        let auth = CustomHeader::new("PRIVATE-TOKEN", Some("glpat-test".to_string()));
        assert_eq!(auth.header_name, "PRIVATE-TOKEN");
    }
}
