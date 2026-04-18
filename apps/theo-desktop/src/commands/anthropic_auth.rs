use tauri::State;
use theo_infra_auth::{AnthropicAuth, AnthropicConfig};

use crate::state::AppState;

const ANTHROPIC_MODELS: &[&str] = &[
    "claude-sonnet-4-20250514",
    "claude-haiku-4-20250514",
    "claude-opus-4-20250514",
];

#[tauri::command]
pub async fn anthropic_start_device_flow(
    server: Option<String>,
) -> Result<serde_json::Value, String> {
    let auth = make_auth(server);
    let dc = auth.start_device_flow().await.map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "user_code": dc.user_code,
        "verification_uri": dc.verification_uri,
        "device_code": dc.device_code,
        "interval": dc.interval,
        "expires_in": dc.expires_in,
    }))
}

#[tauri::command]
pub async fn anthropic_poll_device_flow(
    device_code: String,
    interval: u64,
    expires_in: u64,
    server: Option<String>,
) -> Result<serde_json::Value, String> {
    let auth = make_auth(server);
    let dc = theo_infra_auth::anthropic::AnthropicDeviceCode {
        user_code: String::new(),
        verification_uri: String::new(),
        device_code,
        interval,
        expires_in,
    };

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(900),
        auth.poll_device_flow(&dc),
    )
    .await
    .map_err(|_| "Authorization timed out (15 minutes). Please try again.".to_string())?
    .map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "success": true,
        "email": result.email,
    }))
}

#[tauri::command]
pub async fn anthropic_status() -> Result<serde_json::Value, String> {
    let auth = AnthropicAuth::with_default_store();
    match auth.get_tokens() {
        Ok(Some(tokens)) => {
            let expired = tokens.is_expired();
            Ok(serde_json::json!({
                "authenticated": !expired,
                "expired": expired,
                "email": tokens.email,
            }))
        }
        Ok(None) => Ok(serde_json::json!({
            "authenticated": false,
            "expired": false,
        })),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn anthropic_logout() -> Result<(), String> {
    let auth = AnthropicAuth::with_default_store();
    auth.logout().map_err(|e| e.to_string())
}

/// Apply Anthropic Console token to agent config.
#[tauri::command]
pub async fn anthropic_apply_to_config(
    state: State<'_, AppState>,
    model: Option<String>,
) -> Result<bool, String> {
    let auth = AnthropicAuth::with_default_store();

    let tokens = match auth.get_or_refresh_tokens().await {
        Ok(t) => t,
        Err(_) => return Ok(false),
    };

    let mut config = state.config.lock().await;

    config.api_key = Some(tokens.access_token);
    config.base_url = "https://api.anthropic.com".to_string();
    config.endpoint_override = None;

    if let Some(m) = model {
        if !m.is_empty() {
            config.model = m;
        }
    }

    // Anthropic-specific headers
    config.extra_headers.clear();
    config
        .extra_headers
        .insert("anthropic-version".to_string(), "2023-06-01".to_string());

    Ok(true)
}

/// Return available Anthropic models.
#[tauri::command]
pub async fn anthropic_models() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "models": ANTHROPIC_MODELS,
        "default": "claude-sonnet-4-20250514",
    }))
}

fn make_auth(server: Option<String>) -> AnthropicAuth {
    if let Some(s) = server {
        AnthropicAuth::with_config(
            theo_infra_auth::AuthStore::open(),
            AnthropicConfig::with_server(s),
        )
    } else {
        AnthropicAuth::with_default_store()
    }
}
