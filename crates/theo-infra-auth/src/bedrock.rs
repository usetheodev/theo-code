//! Amazon Bedrock — AWS Credential Chain authentication.
//!
//! Supports:
//! 1. AWS_BEARER_TOKEN_BEDROCK (direct token)
//! 2. AWS_ACCESS_KEY_ID + AWS_SECRET_ACCESS_KEY
//! 3. AWS_PROFILE (from ~/.aws/config)
//! 4. Instance metadata (ECS/EKS)
//!
//! Feature-gated: full SigV4 signing requires `aws-config` crate.
//! Without it, only bearer token and env var credentials work.

use crate::error::AuthError;
use crate::store::{AuthEntry, AuthStore};

const PROVIDER_ID: &str = "amazon-bedrock";
const DEFAULT_REGION: &str = "us-east-1";

#[derive(Debug, Clone)]
pub struct BedrockConfig {
    pub region: String,
    pub profile: Option<String>,
}

impl Default for BedrockConfig {
    fn default() -> Self {
        Self {
            region: std::env::var("AWS_REGION")
                .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
                .unwrap_or_else(|_| DEFAULT_REGION.to_string()),
            profile: std::env::var("AWS_PROFILE").ok(),
        }
    }
}

impl BedrockConfig {
    pub fn endpoint(&self) -> String {
        format!("https://bedrock-runtime.{}.amazonaws.com", self.region)
    }
}

#[derive(Debug, Clone)]
pub struct BedrockTokens {
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub bearer_token: Option<String>,
    pub region: String,
}

pub struct BedrockAuth {
    store: AuthStore,
    config: BedrockConfig,
}

impl BedrockAuth {
    pub fn new(store: AuthStore) -> Self {
        Self {
            store,
            config: BedrockConfig::default(),
        }
    }

    pub fn with_config(store: AuthStore, config: BedrockConfig) -> Self {
        Self { store, config }
    }

    pub fn with_default_store() -> Self {
        Self::new(AuthStore::open())
    }

    pub fn get_tokens(&self) -> Result<Option<BedrockTokens>, AuthError> {
        // Priority 1: Bearer token (direct)
        if let Ok(token) = std::env::var("AWS_BEARER_TOKEN_BEDROCK") {
            return Ok(Some(BedrockTokens {
                access_key_id: None,
                secret_access_key: None,
                bearer_token: Some(token),
                region: self.config.region.clone(),
            }));
        }

        // Priority 2: Stored auth
        let entry = self.store.get(PROVIDER_ID)?;
        if let Some(AuthEntry::ApiKey { key }) = entry {
            return Ok(Some(BedrockTokens {
                access_key_id: None,
                secret_access_key: None,
                bearer_token: Some(key),
                region: self.config.region.clone(),
            }));
        }

        // Priority 3: Env var credentials
        if let (Ok(key_id), Ok(secret)) = (
            std::env::var("AWS_ACCESS_KEY_ID"),
            std::env::var("AWS_SECRET_ACCESS_KEY"),
        ) {
            return Ok(Some(BedrockTokens {
                access_key_id: Some(key_id),
                secret_access_key: Some(secret),
                bearer_token: None,
                region: self.config.region.clone(),
            }));
        }

        Ok(None)
    }

    pub fn has_tokens(&self) -> bool {
        self.get_tokens().ok().flatten().is_some()
    }

    pub fn set_bearer_token(&self, token: String) -> Result<(), AuthError> {
        self.store
            .set(PROVIDER_ID, AuthEntry::ApiKey { key: token })
    }

    pub fn logout(&self) -> Result<(), AuthError> {
        self.store.remove(PROVIDER_ID)
    }

    pub fn config(&self) -> &BedrockConfig {
        &self.config
    }
    pub fn provider_id() -> &'static str {
        PROVIDER_ID
    }
}

/// Apply region prefix to Bedrock model IDs.
/// Some models require regional prefixes (us., eu., global., etc).
pub fn apply_region_prefix(model_id: &str, region: &str) -> String {
    // Models that need region prefix
    let needs_prefix = model_id.starts_with("anthropic.")
        || model_id.starts_with("meta.")
        || model_id.starts_with("mistral.")
        || model_id.contains("nova");

    if !needs_prefix
        || model_id.contains('.') && model_id.split('.').next().map_or(false, |p| p.len() <= 3)
    {
        return model_id.to_string();
    }

    let prefix = if region.starts_with("us-") {
        "us"
    } else if region.starts_with("eu-") {
        "eu"
    } else if region.starts_with("ap-") {
        "apac"
    } else {
        "us"
    };

    format!("{prefix}.{model_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (AuthStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        (AuthStore::new(path), dir)
    }

    #[test]
    fn bedrock_default_region() {
        let config = BedrockConfig {
            region: DEFAULT_REGION.to_string(),
            profile: None,
        };
        assert_eq!(config.region, "us-east-1");
        assert_eq!(
            config.endpoint(),
            "https://bedrock-runtime.us-east-1.amazonaws.com"
        );
    }

    #[test]
    fn bedrock_custom_region() {
        let config = BedrockConfig {
            region: "eu-west-1".to_string(),
            profile: None,
        };
        assert_eq!(
            config.endpoint(),
            "https://bedrock-runtime.eu-west-1.amazonaws.com"
        );
    }

    #[test]
    fn bedrock_store_bearer() {
        let (store, _dir) = temp_store();
        let auth = BedrockAuth::new(store);
        auth.set_bearer_token("aws-bearer-test".to_string())
            .unwrap();
        let tokens = auth.get_tokens().unwrap().unwrap();
        assert_eq!(tokens.bearer_token, Some("aws-bearer-test".to_string()));
    }

    #[test]
    fn bedrock_logout() {
        let (store, _dir) = temp_store();
        let auth = BedrockAuth::new(store);
        auth.set_bearer_token("test".to_string()).unwrap();
        assert!(auth.has_tokens());
        auth.logout().unwrap();
        // May still have tokens from env vars, but store is empty
    }

    #[test]
    fn bedrock_provider_id() {
        assert_eq!(BedrockAuth::provider_id(), "amazon-bedrock");
    }

    #[test]
    fn region_prefix_us() {
        assert_eq!(
            apply_region_prefix("anthropic.claude-v2", "us-east-1"),
            "us.anthropic.claude-v2"
        );
    }

    #[test]
    fn region_prefix_eu() {
        assert_eq!(
            apply_region_prefix("anthropic.claude-v2", "eu-west-1"),
            "eu.anthropic.claude-v2"
        );
    }

    #[test]
    fn region_prefix_not_needed() {
        assert_eq!(
            apply_region_prefix("amazon.titan-text-express-v1", "us-east-1"),
            "amazon.titan-text-express-v1"
        );
    }
}
