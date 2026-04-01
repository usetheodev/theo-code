use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};
use theo_api_contracts::events::FrontendEvent;
use theo_application::use_cases::run_agent_session;

use crate::events::TauriEventSink;
use crate::state::AppState;

#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    message: String,
) -> Result<(), String> {
    let project_dir = state
        .project_dir
        .lock()
        .await
        .clone()
        .ok_or("No project directory selected. Open Settings first.")?;

    let config = state.config.lock().await.clone();

    eprintln!("[theo] send_message: url={} model={} project={}", config.base_url, config.model, project_dir.display());

    // Create cancel channel
    let (cancel_tx, _cancel_rx) = tokio::sync::watch::channel(false);
    *state.cancel_tx.lock().await = Some(cancel_tx);

    let event_sink = Arc::new(TauriEventSink::new(app.clone()));

    // Run agent via application layer
    let app_handle = app.clone();
    tokio::spawn(async move {
        match run_agent_session::run_agent_session(config, &message, &project_dir, event_sink).await {
            Ok(result) => {
                eprintln!("[theo] agent finished: success={} summary={}", result.success, result.summary);
                if !result.success && result.iterations_used == 0 {
                    let _ = app_handle.emit("agent-event", &FrontendEvent::Error {
                        message: format!("Agent failed to start: {}", result.summary),
                    });
                    let _ = app_handle.emit("agent-event", &FrontendEvent::Done {
                        success: false,
                        summary: result.summary,
                    });
                }
            }
            Err(e) => {
                let _ = app_handle.emit("agent-event", &FrontendEvent::Error {
                    message: e.to_string(),
                });
                let _ = app_handle.emit("agent-event", &FrontendEvent::Done {
                    success: false,
                    summary: e.to_string(),
                });
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn cancel_agent(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(tx) = state.cancel_tx.lock().await.take() {
        let _ = tx.send(true);
    }
    Ok(())
}

#[tauri::command]
pub async fn set_project_dir(
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    let dir = PathBuf::from(&path);
    if !dir.exists() {
        return Err(format!("Directory does not exist: {path}"));
    }
    *state.project_dir.lock().await = Some(dir);
    Ok(())
}

#[tauri::command]
pub async fn get_project_dir(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let dir = state.project_dir.lock().await;
    Ok(dir.as_ref().map(|d| d.display().to_string()))
}

#[tauri::command]
pub async fn update_config(
    state: State<'_, AppState>,
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
    max_iterations: Option<usize>,
    temperature: Option<f32>,
) -> Result<(), String> {
    let mut config = state.config.lock().await;
    if let Some(url) = base_url {
        if !url.is_empty() {
            config.base_url = url;
        }
    }
    if let Some(m) = model {
        if !m.is_empty() {
            config.model = m;
        }
    }
    if let Some(k) = api_key {
        if !k.is_empty() {
            config.api_key = Some(k);
        }
    }
    if let Some(n) = max_iterations {
        config.max_iterations = n;
    }
    if let Some(t) = temperature {
        config.temperature = t;
    }
    eprintln!("[theo] config updated: url={} model={} has_key={}", config.base_url, config.model, config.api_key.is_some());
    Ok(())
}

#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let config = state.config.lock().await;
    Ok(serde_json::json!({
        "base_url": config.base_url,
        "model": config.model,
        "has_api_key": config.api_key.is_some(),
        "max_iterations": config.max_iterations,
        "temperature": config.temperature,
    }))
}
