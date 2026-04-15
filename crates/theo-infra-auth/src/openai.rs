use crate::callback;
use crate::error::AuthError;
use crate::pkce::{self, PkceChallenge};
use crate::store::{AuthEntry, AuthStore};
use serde::Deserialize;

// ─── OpenAI OAuth constants ───

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const ISSUER: &str = "https://auth.openai.com";
const AUTH_URL: &str = "https://auth.openai.com/oauth/authorize";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const DEVICE_CODE_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/usercode";
const DEVICE_TOKEN_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/token";
const SCOPES: &str = "openid profile email offline_access";
const CALLBACK_PORT: u16 = 1455;
const CALLBACK_PATH: &str = "/auth/callback";
const CALLBACK_TIMEOUT_SECS: u64 = 300; // 5 minutes
const PROVIDER_ID: &str = "openai";

/// Authentication method.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AuthMethod {
    /// Browser-based OAuth with PKCE (interactive).
    Browser,
    /// Device authorization flow (headless/CLI).
    Device,
}

/// Stored OpenAI tokens.
#[derive(Debug, Clone)]
pub struct OpenAITokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<u64>,
    pub account_id: Option<String>,
}

impl OpenAITokens {
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

/// Device code response from OpenAI.
#[derive(Debug, Clone)]
pub struct DeviceCode {
    pub user_code: String,
    pub verification_uri: String,
    pub device_code: String,
    pub interval: u64,
    pub expires_in: u64,
}

/// OpenAI OAuth client.
pub struct OpenAIAuth {
    http: reqwest::Client,
    store: AuthStore,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    id_token: Option<String>,
    #[allow(dead_code)]
    token_type: Option<String>,
}

#[derive(Deserialize)]
struct DeviceCodeResponse {
    user_code: String,
    #[serde(default)]
    verification_uri: Option<String>,
    /// OpenAI returns `device_auth_id` instead of RFC 8628 `device_code`
    #[serde(alias = "device_code")]
    device_auth_id: Option<String>,
    #[serde(default)]
    device_code: Option<String>,
    /// Can be string or number depending on API version
    #[serde(deserialize_with = "deserialize_interval")]
    interval: Option<u64>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    expires_at: Option<String>,
}

fn deserialize_interval<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let v = Option::<serde_json::Value>::deserialize(deserializer)?;
    match v {
        Some(serde_json::Value::Number(n)) => Ok(n.as_u64()),
        Some(serde_json::Value::String(s)) => Ok(s.parse().ok()),
        _ => Ok(None),
    }
}

#[derive(Deserialize)]
struct DeviceTokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    id_token: Option<String>,
    error: Option<String>,
}

impl OpenAIAuth {
    pub fn new(store: AuthStore) -> Self {
        Self {
            http: reqwest::Client::new(),
            store,
        }
    }

    pub fn with_default_store() -> Self {
        Self::new(AuthStore::open())
    }

    /// Get stored tokens, if any.
    pub fn get_tokens(&self) -> Result<Option<OpenAITokens>, AuthError> {
        let entry = self.store.get(PROVIDER_ID)?;
        match entry {
            Some(AuthEntry::OAuth {
                access_token,
                refresh_token,
                expires_at,
                account_id,
                ..
            }) => Ok(Some(OpenAITokens {
                access_token,
                refresh_token,
                expires_at,
                account_id,
            })),
            _ => Ok(None),
        }
    }

    /// Check if we have valid (non-expired) tokens.
    pub fn has_valid_tokens(&self) -> bool {
        self.get_tokens()
            .ok()
            .flatten()
            .is_some_and(|t| !t.is_expired())
    }

    /// Remove stored tokens.
    pub fn logout(&self) -> Result<(), AuthError> {
        self.store.remove(PROVIDER_ID)
    }

    // ─── Browser flow (OAuth 2.0 PKCE) ───

    /// Build the authorization URL for the browser flow.
    /// Returns (url, state, pkce) — caller must open the URL in a browser.
    pub fn build_auth_url(&self) -> (String, String, PkceChallenge) {
        let state = pkce::generate_state();
        let pkce = PkceChallenge::generate();
        let redirect_uri = format!("http://localhost:{CALLBACK_PORT}{CALLBACK_PATH}");

        let url = format!(
            "{AUTH_URL}?\
             response_type=code\
             &client_id={CLIENT_ID}\
             &redirect_uri={redirect_uri}\
             &scope={SCOPES}\
             &code_challenge={challenge}\
             &code_challenge_method=S256\
             &state={state}\
             &originator=theo-code",
            challenge = pkce.challenge,
        );

        (url, state, pkce)
    }

    /// Run the full browser OAuth flow:
    /// 1. Open browser to authorization URL
    /// 2. Wait for callback on localhost
    /// 3. Exchange code for tokens
    /// 4. Store tokens
    pub async fn login_browser(&self) -> Result<OpenAITokens, AuthError> {
        let (url, state, pkce) = self.build_auth_url();

        // Try to open browser
        open_browser(&url)?;

        // Wait for callback
        let result = callback::wait_for_callback(CALLBACK_PORT, &state, CALLBACK_TIMEOUT_SECS).await?;

        // Exchange code for tokens
        let tokens = self.exchange_code(&result.code, &pkce.verifier).await?;
        Ok(tokens)
    }

    /// Exchange authorization code for tokens.
    async fn exchange_code(
        &self,
        code: &str,
        code_verifier: &str,
    ) -> Result<OpenAITokens, AuthError> {
        let redirect_uri = format!("http://localhost:{CALLBACK_PORT}{CALLBACK_PATH}");

        let resp = self
            .http
            .post(TOKEN_URL)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", &redirect_uri),
                ("client_id", CLIENT_ID),
                ("code_verifier", code_verifier),
            ])
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AuthError::OAuth(format!("token exchange failed ({status}): {body}")));
        }

        let token_resp: TokenResponse = resp.json().await
            .map_err(|e| AuthError::OAuth(format!("parse token response: {e}")))?;

        let account_id = token_resp
            .id_token
            .as_deref()
            .and_then(extract_account_id_from_jwt);

        let expires_at = token_resp.expires_in.map(|secs| now_secs() + secs);

        let tokens = OpenAITokens {
            access_token: token_resp.access_token.clone(),
            refresh_token: token_resp.refresh_token.clone(),
            expires_at,
            account_id: account_id.clone(),
        };

        // Store
        self.store.set(
            PROVIDER_ID,
            AuthEntry::OAuth {
                access_token: token_resp.access_token,
                refresh_token: token_resp.refresh_token,
                expires_at,
                account_id,
                scopes: Some(SCOPES.to_string()),
            },
        )?;

        Ok(tokens)
    }

    // ─── Refresh ───

    /// Refresh the access token using the stored refresh token.
    pub async fn refresh(&self) -> Result<OpenAITokens, AuthError> {
        let current = self
            .get_tokens()?
            .ok_or_else(|| AuthError::OAuth("no stored tokens to refresh".to_string()))?;

        let refresh_token = current
            .refresh_token
            .ok_or_else(|| AuthError::OAuth("no refresh token available".to_string()))?;

        let resp = self
            .http
            .post(TOKEN_URL)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", &refresh_token),
                ("client_id", CLIENT_ID),
            ])
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AuthError::OAuth(format!("refresh failed ({status}): {body}")));
        }

        let token_resp: TokenResponse = resp.json().await
            .map_err(|e| AuthError::OAuth(format!("parse refresh response: {e}")))?;

        let expires_at = token_resp.expires_in.map(|secs| now_secs() + secs);

        self.store.update_tokens(
            PROVIDER_ID,
            token_resp.access_token.clone(),
            token_resp.refresh_token.clone(),
            expires_at,
        )?;

        Ok(OpenAITokens {
            access_token: token_resp.access_token,
            refresh_token: token_resp.refresh_token,
            expires_at,
            account_id: current.account_id,
        })
    }

    /// Get valid tokens, refreshing if expired.
    pub async fn get_or_refresh_tokens(&self) -> Result<OpenAITokens, AuthError> {
        let tokens = self.get_tokens()?.ok_or(AuthError::OAuth("not logged in".to_string()))?;

        if tokens.is_expired() {
            self.refresh().await
        } else {
            Ok(tokens)
        }
    }

    // ─── Device flow (RFC 8628) ───

    /// Start the device authorization flow.
    /// Returns a DeviceCode with user_code and verification_uri to show to the user.
    pub async fn start_device_flow(&self) -> Result<DeviceCode, AuthError> {
        let resp = self
            .http
            .post(DEVICE_CODE_URL)
            .json(&serde_json::json!({
                "client_id": CLIENT_ID,
                "scope": SCOPES,
            }))
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AuthError::OAuth(format!("device code request failed ({status}): {body}")));
        }

        let dc: DeviceCodeResponse = resp.json().await
            .map_err(|e| AuthError::OAuth(format!("parse device code: {e}")))?;

        // OpenAI returns device_auth_id instead of device_code
        let device_code = dc.device_code
            .or(dc.device_auth_id)
            .unwrap_or_default();

        // Verification URI may not be in response — use hardcoded OpenAI URL
        let verification_uri = dc.verification_uri
            .unwrap_or_else(|| "https://auth.openai.com/activate".to_string());

        // expires_in from expires_at if needed
        let expires_in = dc.expires_in.unwrap_or_else(|| {
            // Parse expires_at if available, default to 600s
            600
        });

        Ok(DeviceCode {
            user_code: dc.user_code,
            verification_uri,
            device_code,
            interval: dc.interval.unwrap_or(5),
            expires_in,
        })
    }

    /// Poll for device flow completion.
    /// Blocks until the user authorizes, token expires, or an error occurs.
    pub async fn poll_device_flow(&self, device_code: &DeviceCode) -> Result<OpenAITokens, AuthError> {
        let interval = std::time::Duration::from_secs(device_code.interval);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(device_code.expires_in);

        loop {
            if std::time::Instant::now() >= deadline {
                return Err(AuthError::DeviceExpired);
            }

            tokio::time::sleep(interval).await;

            let resp = self
                .http
                .post(DEVICE_TOKEN_URL)
                .json(&serde_json::json!({
                    "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
                    "device_code": &device_code.device_code,
                    "client_id": CLIENT_ID,
                }))
                .send()
                .await?;

            let dt: DeviceTokenResponse = resp.json().await
                .map_err(|e| AuthError::OAuth(format!("parse device token: {e}")))?;

            if let Some(error) = &dt.error {
                match error.as_str() {
                    "authorization_pending" => continue,
                    "slow_down" => {
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        continue;
                    }
                    "expired_token" => return Err(AuthError::DeviceExpired),
                    _ => return Err(AuthError::OAuth(format!("device flow error: {error}"))),
                }
            }

            let access_token = dt.access_token
                .ok_or_else(|| AuthError::OAuth("device flow: missing access_token".to_string()))?;

            let account_id = dt.id_token.as_deref().and_then(extract_account_id_from_jwt);
            let expires_at = dt.expires_in.map(|secs| now_secs() + secs);

            let tokens = OpenAITokens {
                access_token: access_token.clone(),
                refresh_token: dt.refresh_token.clone(),
                expires_at,
                account_id: account_id.clone(),
            };

            self.store.set(
                PROVIDER_ID,
                AuthEntry::OAuth {
                    access_token,
                    refresh_token: dt.refresh_token,
                    expires_at,
                    account_id,
                    scopes: Some(SCOPES.to_string()),
                },
            )?;

            return Ok(tokens);
        }
    }

    // ─── Constants accessors ───

    pub fn client_id() -> &'static str { CLIENT_ID }
    pub fn issuer() -> &'static str { ISSUER }
    pub fn provider_id() -> &'static str { PROVIDER_ID }
}

// ─── Helpers ───

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Extract account ID from a JWT id_token (without full JWT validation).
///
/// We decode the payload (second segment) and look for known claim names.
fn extract_account_id_from_jwt(token: &str) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }

    // Decode payload (second part)
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    let payload = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&payload).ok()?;

    // Priority 1: direct claim
    if let Some(id) = claims.get("chatgpt_account_id").and_then(|v| v.as_str()) {
        return Some(id.to_string());
    }

    // Priority 2: namespaced claim
    if let Some(auth) = claims.get("https://api.openai.com/auth") {
        if let Some(id) = auth.get("chatgpt_account_id").and_then(|v| v.as_str()) {
            return Some(id.to_string());
        }
    }

    // Priority 3: org ID
    if let Some(orgs) = claims.get("organizations").and_then(|v| v.as_array()) {
        if let Some(first) = orgs.first() {
            if let Some(id) = first.get("id").and_then(|v| v.as_str()) {
                return Some(id.to_string());
            }
        }
    }

    None
}

/// Attempt to open a URL in the system's default browser.
fn open_browser(url: &str) -> Result<(), AuthError> {
    #[cfg(target_os = "linux")]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();

    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();

    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("cmd").args(["/c", "start", "", url]).spawn();

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    let result: Result<std::process::Child, std::io::Error> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "unsupported platform",
    ));

    result.map_err(|e| AuthError::BrowserOpen(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_auth_url() {
        let store = AuthStore::new(std::path::PathBuf::from("/tmp/test_auth.json"));
        let auth = OpenAIAuth::new(store);
        let (url, state, pkce) = auth.build_auth_url();

        assert!(url.starts_with(AUTH_URL));
        assert!(url.contains(&format!("client_id={CLIENT_ID}")));
        assert!(url.contains("code_challenge="));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains(&format!("state={state}")));
        assert!(url.contains("scope=openid"));
        assert!(url.contains("originator=theo-code"));
        assert_eq!(pkce.method, "S256");
    }

    #[test]
    fn test_extract_account_id_direct() {
        // Build a fake JWT with chatgpt_account_id claim
        let payload = serde_json::json!({ "chatgpt_account_id": "acc_123" });
        let token = build_fake_jwt(&payload);
        assert_eq!(extract_account_id_from_jwt(&token), Some("acc_123".to_string()));
    }

    #[test]
    fn test_extract_account_id_namespaced() {
        let payload = serde_json::json!({
            "https://api.openai.com/auth": { "chatgpt_account_id": "acc_456" }
        });
        let token = build_fake_jwt(&payload);
        assert_eq!(extract_account_id_from_jwt(&token), Some("acc_456".to_string()));
    }

    #[test]
    fn test_extract_account_id_org_fallback() {
        let payload = serde_json::json!({
            "organizations": [{"id": "org_789"}]
        });
        let token = build_fake_jwt(&payload);
        assert_eq!(extract_account_id_from_jwt(&token), Some("org_789".to_string()));
    }

    #[test]
    fn test_extract_account_id_none() {
        let payload = serde_json::json!({ "sub": "user" });
        let token = build_fake_jwt(&payload);
        assert_eq!(extract_account_id_from_jwt(&token), None);
    }

    #[test]
    fn test_tokens_expired() {
        let tokens = OpenAITokens {
            access_token: "at".to_string(),
            refresh_token: None,
            expires_at: Some(1), // definitely expired
            account_id: None,
        };
        assert!(tokens.is_expired());
    }

    #[test]
    fn test_tokens_not_expired() {
        let tokens = OpenAITokens {
            access_token: "at".to_string(),
            refresh_token: None,
            expires_at: Some(9999999999),
            account_id: None,
        };
        assert!(!tokens.is_expired());
    }

    #[test]
    fn test_constants() {
        assert_eq!(OpenAIAuth::client_id(), "app_EMoamEEZ73f0CkXaXp7hrann");
        assert_eq!(OpenAIAuth::issuer(), "https://auth.openai.com");
        assert_eq!(OpenAIAuth::provider_id(), "openai");
    }

    fn build_fake_jwt(payload: &serde_json::Value) -> String {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;

        let header = URL_SAFE_NO_PAD.encode(b"{}");
        let body = URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
        let sig = URL_SAFE_NO_PAD.encode(b"sig");
        format!("{header}.{body}.{sig}")
    }
}
