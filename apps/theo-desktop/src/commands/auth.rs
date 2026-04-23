use tauri::State;
// T1.3: facade re-export.
use theo_application::facade::auth::OpenAIAuth;

use crate::state::AppState;

const CODEX_ENDPOINT: &str = "https://chatgpt.com/backend-api/codex/responses";
const CODEX_MODEL: &str = "gpt-5.3-codex";

#[tauri::command]
pub async fn auth_login_browser() -> Result<serde_json::Value, String> {
    let auth = OpenAIAuth::with_default_store();
    let tokens = auth.login_browser().await.map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "success": true,
        "account_id": tokens.account_id,
    }))
}

#[tauri::command]
pub async fn auth_start_device_flow() -> Result<serde_json::Value, String> {
    let auth = OpenAIAuth::with_default_store();
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
pub async fn auth_poll_device_flow(
    device_code: String,
    interval: u64,
    expires_in: u64,
) -> Result<serde_json::Value, String> {
    let auth = OpenAIAuth::with_default_store();
    let dc = theo_application::facade::auth::openai::DeviceCode {
        user_code: String::new(),
        verification_uri: String::new(),
        device_code,
        interval,
        expires_in,
    };
    let tokens = auth
        .poll_device_flow(&dc)
        .await
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "success": true,
        "account_id": tokens.account_id,
    }))
}

#[tauri::command]
pub async fn auth_status() -> Result<serde_json::Value, String> {
    let auth = OpenAIAuth::with_default_store();
    match auth.get_tokens() {
        Ok(Some(tokens)) => {
            let expired = tokens.is_expired();
            Ok(serde_json::json!({
                "authenticated": !expired,
                "expired": expired,
                "account_id": tokens.account_id,
                "has_refresh_token": tokens.refresh_token.is_some(),
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
pub async fn auth_logout() -> Result<(), String> {
    let auth = OpenAIAuth::with_default_store();
    auth.logout().map_err(|e| e.to_string())
}

/// Apply stored OAuth token to agent config.
///
/// OAuth tokens from auth.openai.com use the Codex endpoint
/// (https://chatgpt.com/backend-api/codex/responses), NOT /v1/chat/completions.
#[tauri::command]
pub async fn auth_apply_to_config(state: State<'_, AppState>) -> Result<bool, String> {
    let auth = OpenAIAuth::with_default_store();

    let tokens = match auth.get_tokens() {
        Ok(Some(t)) if !t.is_expired() => t,
        Ok(Some(_)) => {
            // Expired — try refresh
            auth.refresh().await.map_err(|e| e.to_string())?
        }
        _ => return Ok(false),
    };

    let mut config = state.config.lock().await;

    // OAuth tokens use the Codex Responses API endpoint
    config.api_key = Some(tokens.access_token);
    config.endpoint_override = Some(CODEX_ENDPOINT.to_string());
    config.base_url = "https://chatgpt.com".to_string();

    // Set ChatGPT-Account-Id header if available
    if let Some(ref account_id) = tokens.account_id {
        config
            .extra_headers
            .insert("ChatGPT-Account-Id".to_string(), account_id.clone());
    }

    // Set a sensible default model
    if config.model == "default" || config.model.is_empty() {
        config.model = CODEX_MODEL.to_string();
    }

    eprintln!(
        "[theo] OAuth applied: endpoint={} model={} account={:?}",
        CODEX_ENDPOINT, config.model, tokens.account_id
    );

    Ok(true)
}
