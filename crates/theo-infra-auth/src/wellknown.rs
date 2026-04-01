//! WellKnown Federation — discovery protocol for custom AI servers.
//!
//! Flow:
//! 1. GET {url}/.well-known/opencode → { auth: { command: [...], env: "VAR_NAME" } }
//! 2. Execute the command, capture stdout as token
//! 3. Store as { type: "wellknown", key: env_var_name, token: value }

use crate::error::AuthError;
use crate::store::{AuthEntry, AuthStore};
use serde::Deserialize;

const PROVIDER_PREFIX: &str = "wellknown";

#[derive(Debug, Clone, Deserialize)]
pub struct WellKnownConfig {
    pub auth: Option<WellKnownAuthConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WellKnownAuthConfig {
    pub command: Vec<String>,
    pub env: String,
}

#[derive(Debug, Clone)]
pub struct WellKnownTokens {
    pub token: String,
    pub env_var: String,
}

pub struct WellKnownAuth {
    http: reqwest::Client,
    store: AuthStore,
}

impl WellKnownAuth {
    pub fn new(store: AuthStore) -> Self {
        Self {
            http: reqwest::Client::new(),
            store,
        }
    }

    pub fn with_default_store() -> Self {
        Self::new(AuthStore::open())
    }

    /// Discover auth config from a server's .well-known/opencode endpoint.
    pub async fn discover(&self, url: &str) -> Result<WellKnownConfig, AuthError> {
        let discovery_url = format!("{}/.well-known/opencode", url.trim_end_matches('/'));
        let resp = self
            .http
            .get(&discovery_url)
            .header("Accept", "application/json")
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(AuthError::OAuth(format!(
                "WellKnown discovery failed: {} returned {}",
                discovery_url,
                resp.status()
            )));
        }

        resp.json::<WellKnownConfig>()
            .await
            .map_err(|e| AuthError::OAuth(format!("parse WellKnown config: {e}")))
    }

    /// Execute the auth command and store the token.
    pub async fn authenticate(&self, url: &str) -> Result<WellKnownTokens, AuthError> {
        let config = self.discover(url).await?;
        let auth_config = config
            .auth
            .ok_or_else(|| AuthError::OAuth("server has no auth configuration".to_string()))?;

        if auth_config.command.is_empty() {
            return Err(AuthError::OAuth("auth command is empty".to_string()));
        }

        // Execute the command
        let output = std::process::Command::new(&auth_config.command[0])
            .args(&auth_config.command[1..])
            .output()
            .map_err(|e| AuthError::OAuth(format!("exec auth command: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AuthError::OAuth(format!("auth command failed: {stderr}")));
        }

        let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if token.is_empty() {
            return Err(AuthError::OAuth("auth command returned empty token".to_string()));
        }

        // Store with provider ID based on URL
        let provider_id = provider_id_for_url(url);
        self.store.set(
            &provider_id,
            AuthEntry::ApiKey { key: token.clone() },
        )?;

        Ok(WellKnownTokens {
            token,
            env_var: auth_config.env,
        })
    }

    /// Get stored tokens for a WellKnown server.
    pub fn get_tokens(&self, url: &str) -> Result<Option<WellKnownTokens>, AuthError> {
        let provider_id = provider_id_for_url(url);
        let entry = self.store.get(&provider_id)?;
        match entry {
            Some(AuthEntry::ApiKey { key }) => Ok(Some(WellKnownTokens {
                token: key,
                env_var: String::new(),
            })),
            _ => Ok(None),
        }
    }

    pub fn logout(&self, url: &str) -> Result<(), AuthError> {
        let provider_id = provider_id_for_url(url);
        self.store.remove(&provider_id)
    }
}

fn provider_id_for_url(url: &str) -> String {
    let host = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or("unknown");
    format!("{PROVIDER_PREFIX}:{host}")
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
    fn provider_id_from_url() {
        assert_eq!(provider_id_for_url("https://ai.company.com"), "wellknown:ai.company.com");
        assert_eq!(provider_id_for_url("http://localhost:8080"), "wellknown:localhost:8080");
    }

    #[test]
    fn wellknown_store_and_retrieve() {
        let (store, _dir) = temp_store();
        let provider_id = provider_id_for_url("https://ai.test.com");
        store.set(&provider_id, AuthEntry::ApiKey { key: "wk-token".to_string() }).unwrap();

        let auth = WellKnownAuth::new(store);
        let tokens = auth.get_tokens("https://ai.test.com").unwrap().unwrap();
        assert_eq!(tokens.token, "wk-token");
    }

    #[test]
    fn wellknown_logout() {
        let (store, _dir) = temp_store();
        let provider_id = provider_id_for_url("https://ai.test.com");
        store.set(&provider_id, AuthEntry::ApiKey { key: "token".to_string() }).unwrap();

        let auth = WellKnownAuth::new(store);
        auth.logout("https://ai.test.com").unwrap();
        assert!(auth.get_tokens("https://ai.test.com").unwrap().is_none());
    }

    #[test]
    fn wellknown_config_deserializes() {
        let json = r#"{"auth":{"command":["gcloud","auth","print-access-token"],"env":"GOOGLE_TOKEN"}}"#;
        let config: WellKnownConfig = serde_json::from_str(json).unwrap();
        let auth = config.auth.unwrap();
        assert_eq!(auth.command, vec!["gcloud", "auth", "print-access-token"]);
        assert_eq!(auth.env, "GOOGLE_TOKEN");
    }

    #[test]
    fn wellknown_config_without_auth() {
        let json = r#"{}"#;
        let config: WellKnownConfig = serde_json::from_str(json).unwrap();
        assert!(config.auth.is_none());
    }
}
