use tauri::State;
use theo_infra_auth::{CopilotAuth, CopilotConfig};

use crate::state::AppState;

// ─── Models available via GitHub Copilot ───
// Copilot routes to multiple vendors (OpenAI, Anthropic, Google) via model field.
// Models available via GitHub Copilot — IDs must match exactly what the API accepts.
// Availability depends on the user's Copilot plan (Free, Pro, Business, Enterprise).
// gpt-4o is available on all plans; Claude/Gemini require Pro+.
const COPILOT_MODELS: &[&str] = &[
    "gpt-4o",
    "gpt-4.1",
    "gpt-4.1-mini",
    "o3-mini",
    "o4-mini",
    "claude-sonnet-4",
    "claude-haiku-4.5",
    "claude-sonnet-4.5",
    "claude-sonnet-4.6",
    "claude-opus-4.5",
    "claude-opus-4.6",
    "gemini-2.5-pro",
    "gemini-3-flash-preview",
];

const OPENAI_MODELS: &[&str] = &[
    "gpt-4o",
    "gpt-4o-mini",
    "gpt-4.1",
    "gpt-4.1-mini",
    "gpt-4.1-nano",
    "o3-mini",
    "o4-mini",
];

const ANTHROPIC_MODELS: &[&str] = &[
    "claude-sonnet-4-20250514",
    "claude-haiku-4-20250514",
    "claude-opus-4-20250514",
];

#[tauri::command]
pub async fn copilot_start_device_flow(
    enterprise_url: Option<String>,
) -> Result<serde_json::Value, String> {
    let auth = make_auth(enterprise_url);
    let dc = auth.start_device_flow().await.map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "user_code": dc.user_code,
        "verification_uri": dc.verification_uri,
        "device_code": dc.device_code,
        "interval": dc.interval,
    }))
}

#[tauri::command]
pub async fn copilot_poll_device_flow(
    device_code: String,
    interval: u64,
    enterprise_url: Option<String>,
) -> Result<serde_json::Value, String> {
    let auth = make_auth(enterprise_url);
    let dc = theo_infra_auth::copilot::CopilotDeviceCode {
        user_code: String::new(),
        verification_uri: String::new(),
        device_code,
        interval,
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
        "domain": result.domain,
    }))
}

#[tauri::command]
pub async fn copilot_status() -> Result<serde_json::Value, String> {
    let auth = CopilotAuth::with_default_store();
    match auth.get_tokens() {
        Ok(Some(tokens)) => Ok(serde_json::json!({
            "authenticated": true,
            "expired": false,
            "domain": tokens.domain,
        })),
        Ok(None) => Ok(serde_json::json!({
            "authenticated": false,
            "expired": false,
        })),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn copilot_logout() -> Result<(), String> {
    let auth = CopilotAuth::with_default_store();
    auth.logout().map_err(|e| e.to_string())
}

/// Apply Copilot token to agent config.
///
/// Uses `api.githubcopilot.com` — GitHub routes to OpenAI/Anthropic/Google
/// based on the model field in the request body.
#[tauri::command]
pub async fn copilot_apply_to_config(
    state: State<'_, AppState>,
    model: Option<String>,
) -> Result<bool, String> {
    let auth = CopilotAuth::with_default_store();

    let tokens = match auth.get_tokens() {
        Ok(Some(t)) => t,
        _ => return Ok(false),
    };

    let mut config = state.config.lock().await;

    // Copilot API: https://api.githubcopilot.com/chat/completions (NO /v1/)
    // Must use endpoint_override because LlmClient.url() appends /v1/chat/completions
    config.api_key = Some(tokens.access_token);
    config.base_url = auth.api_base_url(); // "https://api.githubcopilot.com"
    config.endpoint_override = Some(format!("{}/chat/completions", auth.api_base_url()));

    // Set model if provided
    if let Some(m) = model {
        if !m.is_empty() {
            config.model = m;
        }
    }

    // Copilot-specific headers (matching opencode behavior)
    config.extra_headers.clear();
    config.extra_headers.insert(
        "Openai-Intent".to_string(),
        "conversation-edits".to_string(),
    );

    Ok(true)
}

/// Return available models for a given provider.
///
/// Backend is the source of truth for model lists — frontend just renders.
#[tauri::command]
pub async fn provider_models(provider: String) -> Result<serde_json::Value, String> {
    let (models, default_model) = match provider.as_str() {
        "copilot" => (COPILOT_MODELS.to_vec(), "gpt-4o"),
        "openai" => (OPENAI_MODELS.to_vec(), "gpt-4o"),
        "anthropic" => (ANTHROPIC_MODELS.to_vec(), "claude-sonnet-4-20250514"),
        _ => (vec![], ""),
    };

    Ok(serde_json::json!({
        "models": models,
        "default": default_model,
    }))
}

fn make_auth(enterprise_url: Option<String>) -> CopilotAuth {
    if let Some(url) = enterprise_url {
        CopilotAuth::with_config(
            theo_infra_auth::AuthStore::open(),
            CopilotConfig::enterprise(url),
        )
    } else {
        CopilotAuth::with_default_store()
    }
}
