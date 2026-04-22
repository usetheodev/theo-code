//! GitHub Copilot OAuth — Device Authorization Flow (RFC 8628).
//!
//! Copilot uses a simple device flow against GitHub's OAuth server.
//! Token never expires (expires=0). Supports GitHub Enterprise.
//!
//! Flow:
//! 1. POST github.com/login/device/code → { user_code, verification_uri, device_code }
//! 2. User opens verification_uri and enters user_code
//! 3. Poll github.com/login/oauth/access_token until authorized
//! 4. Store access_token as both access and refresh (Copilot convention)

use crate::error::AuthError;
use crate::store::{AuthEntry, AuthStore};
use serde::Deserialize;

/// Default GitHub OAuth client ID for Copilot (same as VS Code extension).
pub const DEFAULT_CLIENT_ID: &str = "Ov23li8tweQw6odWQebz";
const DEFAULT_DOMAIN: &str = "github.com";
const SCOPE: &str = "read:user";
const PROVIDER_ID: &str = "github-copilot";
const POLLING_SAFETY_MARGIN_MS: u64 = 3000;

/// GitHub Copilot OAuth device code.
#[derive(Debug, Clone)]
pub struct CopilotDeviceCode {
    pub user_code: String,
    pub verification_uri: String,
    pub device_code: String,
    pub interval: u64,
}

/// Stored Copilot tokens.
#[derive(Debug, Clone)]
pub struct CopilotTokens {
    pub access_token: String,
    /// Domain used (for enterprise support).
    pub domain: String,
}

/// Configuration for Copilot auth.
#[derive(Debug, Clone)]
pub struct CopilotConfig {
    /// GitHub domain (default: "github.com", override for enterprise).
    pub domain: String,
    /// OAuth client ID (default: VS Code Copilot ID).
    pub client_id: String,
}

impl Default for CopilotConfig {
    fn default() -> Self {
        Self {
            domain: DEFAULT_DOMAIN.to_string(),
            client_id: DEFAULT_CLIENT_ID.to_string(),
        }
    }
}

impl CopilotConfig {
    /// Create config for GitHub Enterprise.
    pub fn enterprise(domain: impl Into<String>) -> Self {
        let domain = domain.into();
        let domain = domain
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_string();
        Self {
            domain,
            client_id: DEFAULT_CLIENT_ID.to_string(),
        }
    }

    fn device_code_url(&self) -> String {
        format!("https://{}/login/device/code", self.domain)
    }

    fn access_token_url(&self) -> String {
        format!("https://{}/login/oauth/access_token", self.domain)
    }

    /// The Copilot API base URL.
    pub fn api_base_url(&self) -> String {
        if self.domain == DEFAULT_DOMAIN {
            "https://api.githubcopilot.com".to_string()
        } else {
            format!("https://copilot-api.{}", self.domain)
        }
    }
}

/// GitHub Copilot OAuth client.
pub struct CopilotAuth {
    http: reqwest::Client,
    store: AuthStore,
    config: CopilotConfig,
}

#[derive(Deserialize)]
struct DeviceCodeResponse {
    user_code: String,
    verification_uri: String,
    device_code: String,
    interval: Option<u64>,
}

#[derive(Deserialize)]
struct AccessTokenResponse {
    access_token: Option<String>,
    error: Option<String>,
    interval: Option<u64>,
}

impl CopilotAuth {
    /// Create a new CopilotAuth with default config (github.com).
    pub fn new(store: AuthStore) -> Self {
        Self {
            http: reqwest::Client::new(),
            store,
            config: CopilotConfig::default(),
        }
    }

    /// Create with custom config (for GitHub Enterprise).
    pub fn with_config(store: AuthStore, config: CopilotConfig) -> Self {
        Self {
            http: reqwest::Client::new(),
            store,
            config,
        }
    }

    /// Create with default store.
    pub fn with_default_store() -> Self {
        Self::new(AuthStore::open())
    }

    /// Get stored Copilot tokens.
    pub fn get_tokens(&self) -> Result<Option<CopilotTokens>, AuthError> {
        let entry = self.store.get(PROVIDER_ID)?;
        match entry {
            Some(AuthEntry::OAuth { access_token, .. }) => Ok(Some(CopilotTokens {
                access_token,
                domain: self.config.domain.clone(),
            })),
            _ => Ok(None),
        }
    }

    /// Check if we have stored tokens.
    pub fn has_tokens(&self) -> bool {
        self.get_tokens().ok().flatten().is_some()
    }

    /// Remove stored tokens.
    pub fn logout(&self) -> Result<(), AuthError> {
        self.store.remove(PROVIDER_ID)
    }

    /// Start the device authorization flow.
    ///
    /// Returns a CopilotDeviceCode — show user_code and verification_uri to the user.
    pub async fn start_device_flow(&self) -> Result<CopilotDeviceCode, AuthError> {
        let resp = self
            .http
            .post(self.config.device_code_url())
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "client_id": self.config.client_id,
                "scope": SCOPE,
            }))
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AuthError::OAuth(format!(
                "Copilot device code request failed ({status}): {body}"
            )));
        }

        let dc: DeviceCodeResponse = resp
            .json()
            .await
            .map_err(|e| AuthError::OAuth(format!("parse Copilot device code: {e}")))?;

        Ok(CopilotDeviceCode {
            user_code: dc.user_code,
            verification_uri: dc.verification_uri,
            device_code: dc.device_code,
            interval: dc.interval.unwrap_or(5),
        })
    }

    /// Poll for device flow completion.
    ///
    /// Blocks until the user authorizes or an error occurs.
    /// Copilot tokens don't expire — they're stored with expires=0.
    pub async fn poll_device_flow(
        &self,
        device_code: &CopilotDeviceCode,
    ) -> Result<CopilotTokens, AuthError> {
        let mut interval_ms = device_code.interval * 1000 + POLLING_SAFETY_MARGIN_MS;

        loop {
            tokio::time::sleep(std::time::Duration::from_millis(interval_ms)).await;

            let resp = self
                .http
                .post(self.config.access_token_url())
                .header("Accept", "application/json")
                .header("Content-Type", "application/json")
                .json(&serde_json::json!({
                    "client_id": self.config.client_id,
                    "device_code": device_code.device_code,
                    "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
                }))
                .send()
                .await?;

            if !resp.status().is_success() {
                return Err(AuthError::OAuth("Copilot token request failed".to_string()));
            }

            let data: AccessTokenResponse = resp
                .json()
                .await
                .map_err(|e| AuthError::OAuth(format!("parse Copilot token: {e}")))?;

            if let Some(access_token) = data.access_token {
                let tokens = CopilotTokens {
                    access_token: access_token.clone(),
                    domain: self.config.domain.clone(),
                };

                // Store with expires=0 (Copilot tokens don't expire)
                self.store.set(
                    PROVIDER_ID,
                    AuthEntry::OAuth {
                        access_token,
                        refresh_token: None,
                        expires_at: None, // never expires
                        account_id: None,
                        scopes: Some(SCOPE.to_string()),
                    },
                )?;

                return Ok(tokens);
            }

            if let Some(error) = &data.error {
                match error.as_str() {
                    "authorization_pending" => continue,
                    "slow_down" => {
                        // RFC 8628: add 5 seconds to interval
                        let server_interval = data.interval.unwrap_or(0);
                        if server_interval > 0 {
                            interval_ms = server_interval * 1000 + POLLING_SAFETY_MARGIN_MS;
                        } else {
                            interval_ms += 5000;
                        }
                        continue;
                    }
                    "expired_token" => return Err(AuthError::DeviceExpired),
                    _ => {
                        return Err(AuthError::OAuth(format!(
                            "Copilot device flow error: {error}"
                        )));
                    }
                }
            }

            // No token and no error — continue polling
            continue;
        }
    }

    /// Get the Copilot API base URL for this configuration.
    pub fn api_base_url(&self) -> String {
        self.config.api_base_url()
    }

    pub fn provider_id() -> &'static str {
        PROVIDER_ID
    }

    pub fn config(&self) -> &CopilotConfig {
        &self.config
    }
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
    fn copilot_config_default() {
        let config = CopilotConfig::default();
        assert_eq!(config.domain, "github.com");
        assert_eq!(config.client_id, DEFAULT_CLIENT_ID);
    }

    #[test]
    fn copilot_config_enterprise() {
        let config = CopilotConfig::enterprise("https://company.ghe.com/");
        assert_eq!(config.domain, "company.ghe.com");
    }

    #[test]
    fn copilot_config_enterprise_bare_domain() {
        let config = CopilotConfig::enterprise("company.ghe.com");
        assert_eq!(config.domain, "company.ghe.com");
    }

    #[test]
    fn device_code_url_default() {
        let config = CopilotConfig::default();
        assert_eq!(
            config.device_code_url(),
            "https://github.com/login/device/code"
        );
    }

    #[test]
    fn device_code_url_enterprise() {
        let config = CopilotConfig::enterprise("corp.ghe.com");
        assert_eq!(
            config.device_code_url(),
            "https://corp.ghe.com/login/device/code"
        );
    }

    #[test]
    fn access_token_url_default() {
        let config = CopilotConfig::default();
        assert_eq!(
            config.access_token_url(),
            "https://github.com/login/oauth/access_token"
        );
    }

    #[test]
    fn api_base_url_default() {
        let config = CopilotConfig::default();
        assert_eq!(config.api_base_url(), "https://api.githubcopilot.com");
    }

    #[test]
    fn api_base_url_enterprise() {
        let config = CopilotConfig::enterprise("corp.ghe.com");
        assert_eq!(config.api_base_url(), "https://copilot-api.corp.ghe.com");
    }

    #[test]
    fn copilot_auth_creates() {
        let (store, _dir) = temp_store();
        let auth = CopilotAuth::new(store);
        assert!(!auth.has_tokens());
        assert_eq!(CopilotAuth::provider_id(), "github-copilot");
    }

    #[test]
    fn copilot_auth_store_and_retrieve_tokens() {
        let (store, _dir) = temp_store();
        store
            .set(
                "github-copilot",
                AuthEntry::OAuth {
                    access_token: "gho_test123".to_string(),
                    refresh_token: None,
                    expires_at: None,
                    account_id: None,
                    scopes: Some("read:user".to_string()),
                },
            )
            .unwrap();

        let auth = CopilotAuth::new(store);
        assert!(auth.has_tokens());
        let tokens = auth.get_tokens().unwrap().unwrap();
        assert_eq!(tokens.access_token, "gho_test123");
        assert_eq!(tokens.domain, "github.com");
    }

    #[test]
    fn copilot_auth_logout() {
        let (store, _dir) = temp_store();
        store
            .set(
                "github-copilot",
                AuthEntry::OAuth {
                    access_token: "gho_test".to_string(),
                    refresh_token: None,
                    expires_at: None,
                    account_id: None,
                    scopes: None,
                },
            )
            .unwrap();
        let auth = CopilotAuth::new(store);
        assert!(auth.has_tokens());
        auth.logout().unwrap();
        assert!(!auth.has_tokens());
    }

    #[test]
    fn copilot_enterprise_config() {
        let (store, _dir) = temp_store();
        let config = CopilotConfig::enterprise("corp.github.com");
        let auth = CopilotAuth::with_config(store, config);
        assert_eq!(auth.api_base_url(), "https://copilot-api.corp.github.com");
        assert_eq!(auth.config().domain, "corp.github.com");
    }

    #[test]
    fn copilot_coexists_with_openai_in_store() {
        let (store, _dir) = temp_store();
        store
            .set(
                "openai",
                AuthEntry::ApiKey {
                    key: "sk-openai".to_string(),
                },
            )
            .unwrap();
        store
            .set(
                "github-copilot",
                AuthEntry::OAuth {
                    access_token: "gho_copilot".to_string(),
                    refresh_token: None,
                    expires_at: None,
                    account_id: None,
                    scopes: None,
                },
            )
            .unwrap();

        let auth = CopilotAuth::new(store.clone());
        assert!(auth.has_tokens());

        // OpenAI entry still there
        let openai = store.get("openai").unwrap().unwrap();
        assert_eq!(openai.bearer_token(), "sk-openai");

        // Copilot entry
        let copilot = store.get("github-copilot").unwrap().unwrap();
        assert_eq!(copilot.bearer_token(), "gho_copilot");
    }
}
