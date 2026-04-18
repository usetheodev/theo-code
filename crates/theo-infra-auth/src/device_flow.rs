//! Generic RFC 8628 device flow — works with any compliant server.
//!
//! Used by: OpenCode servers, self-hosted auth servers, etc.
//! Protocol:
//!   POST ${server}/auth/device/code   → {device_code, user_code, verification_uri_complete}
//!   POST ${server}/auth/device/token  → {access_token} or {error: "authorization_pending"}

use serde::Deserialize;

use crate::error::AuthError;

const DEFAULT_CLIENT_ID: &str = "theo-cli";

#[derive(Debug, Clone)]
pub struct DeviceFlowConfig {
    pub server_url: String,
    pub client_id: String,
}

impl DeviceFlowConfig {
    pub fn new(server_url: &str) -> Self {
        Self {
            server_url: server_url.trim_end_matches('/').to_string(),
            client_id: DEFAULT_CLIENT_ID.to_string(),
        }
    }

    pub fn with_client_id(mut self, client_id: &str) -> Self {
        self.client_id = client_id.to_string();
        self
    }
}

#[derive(Debug, Clone)]
pub struct DeviceFlowCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_url: String,
    pub interval_secs: u64,
    pub expires_in_secs: u64,
}

#[derive(Debug, Clone)]
pub struct DeviceFlowTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
}

#[derive(Deserialize)]
struct CodeResponse {
    device_code: String,
    user_code: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    #[serde(default)]
    verification_uri: Option<String>,
    #[serde(default, deserialize_with = "flexible_u64")]
    interval: Option<u64>,
    #[serde(default, deserialize_with = "flexible_u64")]
    expires_in: Option<u64>,
}

#[derive(Deserialize)]
struct TokenResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default, deserialize_with = "flexible_u64")]
    expires_in: Option<u64>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

fn flexible_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
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

/// Start the device authorization flow against any RFC 8628 server.
pub async fn start_device_flow(
    http: &reqwest::Client,
    config: &DeviceFlowConfig,
) -> Result<DeviceFlowCode, AuthError> {
    let url = format!("{}/auth/device/code", config.server_url);

    let resp = http
        .post(&url)
        .json(&serde_json::json!({
            "client_id": config.client_id,
        }))
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(AuthError::OAuth(format!(
            "device code request to {} failed ({status}): {body}",
            url
        )));
    }

    let cr: CodeResponse = resp
        .json()
        .await
        .map_err(|e| AuthError::OAuth(format!("parse device code response: {e}")))?;

    let verification_url = cr
        .verification_uri_complete
        .or(cr.verification_uri)
        .unwrap_or_else(|| format!("{}/auth/device/activate", config.server_url));

    Ok(DeviceFlowCode {
        device_code: cr.device_code,
        user_code: cr.user_code,
        verification_url,
        interval_secs: cr.interval.unwrap_or(5),
        expires_in_secs: cr.expires_in.unwrap_or(600),
    })
}

/// Poll for device flow token until authorized, expired, or error.
pub async fn poll_device_flow(
    http: &reqwest::Client,
    config: &DeviceFlowConfig,
    code: &DeviceFlowCode,
) -> Result<DeviceFlowTokens, AuthError> {
    let url = format!("{}/auth/device/token", config.server_url);
    let interval = std::time::Duration::from_secs(code.interval_secs);
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_secs(code.expires_in_secs);

    loop {
        if std::time::Instant::now() >= deadline {
            return Err(AuthError::DeviceExpired);
        }

        tokio::time::sleep(interval).await;

        let resp = http
            .post(&url)
            .json(&serde_json::json!({
                "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
                "device_code": &code.device_code,
                "client_id": &config.client_id,
            }))
            .send()
            .await?;

        let body = resp
            .text()
            .await
            .map_err(|e| AuthError::OAuth(format!("read token response: {e}")))?;

        let tr: TokenResponse = match serde_json::from_str(&body) {
            Ok(tr) => tr,
            Err(_) => {
                // Try to extract error from non-standard response
                if let Ok(obj) = serde_json::from_str::<serde_json::Value>(&body) {
                    let err = obj
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    if err.contains("pending") {
                        continue;
                    }
                    if err.contains("expired") {
                        return Err(AuthError::DeviceExpired);
                    }
                }
                return Err(AuthError::OAuth(format!("unexpected token response: {body}")));
            }
        };

        if let Some(error) = &tr.error {
            match error.as_str() {
                "authorization_pending" | "slow_down" => continue,
                "expired_token" | "access_denied" => return Err(AuthError::DeviceExpired),
                _ => {
                    let desc = tr.error_description.as_deref().unwrap_or(error);
                    return Err(AuthError::OAuth(format!("device flow error: {desc}")));
                }
            }
        }

        if let Some(access_token) = tr.access_token {
            return Ok(DeviceFlowTokens {
                access_token,
                refresh_token: tr.refresh_token,
                expires_in: tr.expires_in,
            });
        }

        // No error and no token — keep polling
        continue;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_new() {
        let c = DeviceFlowConfig::new("https://api.example.com/");
        assert_eq!(c.server_url, "https://api.example.com");
        assert_eq!(c.client_id, "theo-cli");
    }

    #[test]
    fn config_with_client_id() {
        let c = DeviceFlowConfig::new("https://api.example.com")
            .with_client_id("my-app");
        assert_eq!(c.client_id, "my-app");
    }
}
