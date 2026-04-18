//! Anthropic Console OAuth — Device Authorization Flow.
//!
//! Login with Anthropic account via device code flow.
//! Supports token refresh, user info, and organization switching.
//!
//! Flow:
//! 1. POST {server}/auth/device/code → { device_code, user_code, verification_uri_complete }
//! 2. User opens URL and authorizes
//! 3. Poll POST {server}/auth/device/token until authorized
//! 4. Store access_token + refresh_token

use crate::error::AuthError;
use crate::store::{AuthEntry, AuthStore};
use serde::Deserialize;

const DEFAULT_SERVER: &str = "https://console.anthropic.com";
const CLIENT_ID: &str = "theo-code";
const PROVIDER_ID: &str = "anthropic-console";
const POLLING_SAFETY_MARGIN_MS: u64 = 3000;

/// Anthropic device code.
#[derive(Debug, Clone)]
pub struct AnthropicDeviceCode {
    pub user_code: String,
    pub verification_uri: String,
    pub device_code: String,
    pub interval: u64,
    pub expires_in: u64,
}

/// Stored Anthropic tokens.
#[derive(Debug, Clone)]
pub struct AnthropicTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<u64>,
    pub email: Option<String>,
    pub org_id: Option<String>,
}

impl AnthropicTokens {
    pub fn is_expired(&self) -> bool {
        if let Some(exp) = self.expires_at {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            exp <= now
        } else {
            false
        }
    }
}

/// Configuration for Anthropic auth.
#[derive(Debug, Clone)]
pub struct AnthropicConfig {
    pub server: String,
    pub client_id: String,
}

impl Default for AnthropicConfig {
    fn default() -> Self {
        Self {
            server: DEFAULT_SERVER.to_string(),
            client_id: CLIENT_ID.to_string(),
        }
    }
}

impl AnthropicConfig {
    pub fn with_server(server: impl Into<String>) -> Self {
        let server = server.into().trim_end_matches('/').to_string();
        Self {
            server,
            client_id: CLIENT_ID.to_string(),
        }
    }

    fn device_code_url(&self) -> String {
        format!("{}/auth/device/code", self.server)
    }

    fn device_token_url(&self) -> String {
        format!("{}/auth/device/token", self.server)
    }

    fn user_url(&self) -> String {
        format!("{}/api/user", self.server)
    }
}

/// Anthropic Console OAuth client.
pub struct AnthropicAuth {
    http: reqwest::Client,
    store: AuthStore,
    config: AnthropicConfig,
}

#[derive(Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri_complete: Option<String>,
    #[serde(default = "default_expires")]
    expires_in: u64,
    #[serde(default = "default_interval")]
    interval: u64,
}

fn default_expires() -> u64 {
    600
}
fn default_interval() -> u64 {
    5
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct UserResponse {
    #[allow(dead_code)]
    id: Option<String>,
    email: Option<String>,
}

impl AnthropicAuth {
    pub fn new(store: AuthStore) -> Self {
        Self {
            http: reqwest::Client::new(),
            store,
            config: AnthropicConfig::default(),
        }
    }

    pub fn with_config(store: AuthStore, config: AnthropicConfig) -> Self {
        Self {
            http: reqwest::Client::new(),
            store,
            config,
        }
    }

    pub fn with_default_store() -> Self {
        Self::new(AuthStore::open())
    }

    /// Get stored tokens.
    pub fn get_tokens(&self) -> Result<Option<AnthropicTokens>, AuthError> {
        let entry = self.store.get(PROVIDER_ID)?;
        match entry {
            Some(AuthEntry::OAuth {
                access_token,
                refresh_token,
                expires_at,
                account_id,
                ..
            }) => Ok(Some(AnthropicTokens {
                access_token,
                refresh_token,
                expires_at,
                email: account_id.clone(),
                org_id: None,
            })),
            _ => Ok(None),
        }
    }

    pub fn has_valid_tokens(&self) -> bool {
        self.get_tokens()
            .ok()
            .flatten()
            .is_some_and(|t| !t.is_expired())
    }

    pub fn logout(&self) -> Result<(), AuthError> {
        self.store.remove(PROVIDER_ID)
    }

    /// Start device authorization flow.
    pub async fn start_device_flow(&self) -> Result<AnthropicDeviceCode, AuthError> {
        let resp = self
            .http
            .post(&self.config.device_code_url())
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "client_id": self.config.client_id,
            }))
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AuthError::OAuth(format!(
                "Anthropic device code failed ({status}): {body}"
            )));
        }

        let dc: DeviceCodeResponse = resp
            .json()
            .await
            .map_err(|e| AuthError::OAuth(format!("parse Anthropic device code: {e}")))?;

        let verification_uri = dc
            .verification_uri_complete
            .unwrap_or_else(|| format!("{}/auth/device", self.config.server));

        Ok(AnthropicDeviceCode {
            user_code: dc.user_code,
            verification_uri,
            device_code: dc.device_code,
            interval: dc.interval,
            expires_in: dc.expires_in,
        })
    }

    /// Poll for device flow completion.
    pub async fn poll_device_flow(
        &self,
        device_code: &AnthropicDeviceCode,
    ) -> Result<AnthropicTokens, AuthError> {
        let mut interval_ms = device_code.interval * 1000 + POLLING_SAFETY_MARGIN_MS;
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(device_code.expires_in);

        loop {
            if std::time::Instant::now() >= deadline {
                return Err(AuthError::DeviceExpired);
            }

            tokio::time::sleep(std::time::Duration::from_millis(interval_ms)).await;

            let resp = self
                .http
                .post(&self.config.device_token_url())
                .header("Accept", "application/json")
                .header("Content-Type", "application/json")
                .json(&serde_json::json!({
                    "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
                    "device_code": device_code.device_code,
                    "client_id": self.config.client_id,
                }))
                .send()
                .await?;

            let data: TokenResponse = resp
                .json()
                .await
                .map_err(|e| AuthError::OAuth(format!("parse Anthropic token: {e}")))?;

            if let Some(access_token) = data.access_token {
                let expires_at = data.expires_in.map(|secs| now_secs() + secs);

                // Try to get user email
                let email = self.fetch_user_email(&access_token).await.ok();

                let tokens = AnthropicTokens {
                    access_token: access_token.clone(),
                    refresh_token: data.refresh_token.clone(),
                    expires_at,
                    email: email.clone(),
                    org_id: None,
                };

                self.store.set(
                    PROVIDER_ID,
                    AuthEntry::OAuth {
                        access_token,
                        refresh_token: data.refresh_token,
                        expires_at,
                        account_id: email,
                        scopes: None,
                    },
                )?;

                return Ok(tokens);
            }

            if let Some(error) = &data.error {
                match error.as_str() {
                    "authorization_pending" => continue,
                    "slow_down" => {
                        interval_ms += 5000;
                        continue;
                    }
                    "expired_token" => return Err(AuthError::DeviceExpired),
                    "access_denied" => {
                        return Err(AuthError::OAuth("access denied by user".to_string()));
                    }
                    _ => {
                        return Err(AuthError::OAuth(format!(
                            "Anthropic device flow error: {error}"
                        )));
                    }
                }
            }

            continue;
        }
    }

    /// Refresh the access token.
    pub async fn refresh(&self) -> Result<AnthropicTokens, AuthError> {
        let current = self
            .get_tokens()?
            .ok_or_else(|| AuthError::OAuth("no stored tokens to refresh".to_string()))?;

        let refresh_token = current
            .refresh_token
            .ok_or_else(|| AuthError::OAuth("no refresh token available".to_string()))?;

        let resp = self
            .http
            .post(&self.config.device_token_url())
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "grant_type": "refresh_token",
                "refresh_token": refresh_token,
                "client_id": self.config.client_id,
            }))
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AuthError::OAuth(format!(
                "Anthropic refresh failed ({status}): {body}"
            )));
        }

        let data: TokenResponse = resp
            .json()
            .await
            .map_err(|e| AuthError::OAuth(format!("parse refresh response: {e}")))?;

        let access_token = data
            .access_token
            .ok_or_else(|| AuthError::OAuth("refresh: missing access_token".to_string()))?;

        let expires_at = data.expires_in.map(|secs| now_secs() + secs);

        self.store.update_tokens(
            PROVIDER_ID,
            access_token.clone(),
            data.refresh_token.clone(),
            expires_at,
        )?;

        Ok(AnthropicTokens {
            access_token,
            refresh_token: data.refresh_token,
            expires_at,
            email: current.email,
            org_id: current.org_id,
        })
    }

    /// Get valid tokens, refreshing if expired.
    pub async fn get_or_refresh_tokens(&self) -> Result<AnthropicTokens, AuthError> {
        let tokens = self
            .get_tokens()?
            .ok_or(AuthError::OAuth("not logged in to Anthropic".to_string()))?;

        if tokens.is_expired() {
            self.refresh().await
        } else {
            Ok(tokens)
        }
    }

    /// Fetch user email from Anthropic API.
    async fn fetch_user_email(&self, access_token: &str) -> Result<String, AuthError> {
        let resp = self
            .http
            .get(&self.config.user_url())
            .header("Authorization", format!("Bearer {access_token}"))
            .header("Accept", "application/json")
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(AuthError::OAuth("failed to fetch user info".to_string()));
        }

        let user: UserResponse = resp
            .json()
            .await
            .map_err(|e| AuthError::OAuth(format!("parse user info: {e}")))?;

        user.email
            .ok_or_else(|| AuthError::OAuth("user email not found".to_string()))
    }

    pub fn provider_id() -> &'static str {
        PROVIDER_ID
    }

    pub fn config(&self) -> &AnthropicConfig {
        &self.config
    }

    pub fn server(&self) -> &str {
        &self.config.server
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
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
    fn anthropic_config_default() {
        let config = AnthropicConfig::default();
        assert_eq!(config.server, "https://console.anthropic.com");
        assert_eq!(config.client_id, "theo-code");
    }

    #[test]
    fn anthropic_config_custom_server() {
        let config = AnthropicConfig::with_server("https://custom.anthropic.com/");
        assert_eq!(config.server, "https://custom.anthropic.com");
    }

    #[test]
    fn device_code_url() {
        let config = AnthropicConfig::default();
        assert_eq!(
            config.device_code_url(),
            "https://console.anthropic.com/auth/device/code"
        );
    }

    #[test]
    fn device_token_url() {
        let config = AnthropicConfig::default();
        assert_eq!(
            config.device_token_url(),
            "https://console.anthropic.com/auth/device/token"
        );
    }

    #[test]
    fn anthropic_auth_creates() {
        let (store, _dir) = temp_store();
        let auth = AnthropicAuth::new(store);
        assert!(!auth.has_valid_tokens());
        assert_eq!(AnthropicAuth::provider_id(), "anthropic-console");
    }

    #[test]
    fn anthropic_store_and_retrieve_tokens() {
        let (store, _dir) = temp_store();
        store
            .set(
                "anthropic-console",
                AuthEntry::OAuth {
                    access_token: "sk-ant-test".to_string(),
                    refresh_token: Some("rt-test".to_string()),
                    expires_at: Some(9999999999),
                    account_id: Some("user@example.com".to_string()),
                    scopes: None,
                },
            )
            .unwrap();

        let auth = AnthropicAuth::new(store);
        assert!(auth.has_valid_tokens());
        let tokens = auth.get_tokens().unwrap().unwrap();
        assert_eq!(tokens.access_token, "sk-ant-test");
        assert_eq!(tokens.email, Some("user@example.com".to_string()));
        assert!(!tokens.is_expired());
    }

    #[test]
    fn anthropic_expired_tokens() {
        let tokens = AnthropicTokens {
            access_token: "expired".to_string(),
            refresh_token: None,
            expires_at: Some(1),
            email: None,
            org_id: None,
        };
        assert!(tokens.is_expired());
    }

    #[test]
    fn anthropic_no_expiry_not_expired() {
        let tokens = AnthropicTokens {
            access_token: "valid".to_string(),
            refresh_token: None,
            expires_at: None,
            email: None,
            org_id: None,
        };
        assert!(!tokens.is_expired());
    }

    #[test]
    fn anthropic_logout() {
        let (store, _dir) = temp_store();
        store
            .set(
                "anthropic-console",
                AuthEntry::OAuth {
                    access_token: "test".to_string(),
                    refresh_token: None,
                    expires_at: None,
                    account_id: None,
                    scopes: None,
                },
            )
            .unwrap();

        let auth = AnthropicAuth::new(store);
        assert!(auth.has_valid_tokens());
        auth.logout().unwrap();
        assert!(!auth.has_valid_tokens());
    }

    #[test]
    fn anthropic_coexists_with_copilot_and_openai() {
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
        store
            .set(
                "anthropic-console",
                AuthEntry::OAuth {
                    access_token: "sk-ant-test".to_string(),
                    refresh_token: None,
                    expires_at: None,
                    account_id: None,
                    scopes: None,
                },
            )
            .unwrap();

        // All three coexist
        assert_eq!(
            store.get("openai").unwrap().unwrap().bearer_token(),
            "sk-openai"
        );
        assert_eq!(
            store.get("github-copilot").unwrap().unwrap().bearer_token(),
            "gho_copilot"
        );
        assert_eq!(
            store
                .get("anthropic-console")
                .unwrap()
                .unwrap()
                .bearer_token(),
            "sk-ant-test"
        );
    }

    #[test]
    fn anthropic_provider_id_stable() {
        assert_eq!(AnthropicAuth::provider_id(), "anthropic-console");
    }

    #[test]
    fn anthropic_server_accessor() {
        let (store, _dir) = temp_store();
        let auth = AnthropicAuth::new(store);
        assert_eq!(auth.server(), "https://console.anthropic.com");
    }
}
