//! Tauri commands for the observability dashboard.
//!
//! Thin shim over `theo_application::use_cases::observability_ui`. All
//! heavy lifting (trajectory parsing, projection, metrics) lives in the
//! application layer.

use std::path::PathBuf;

use theo_agent_runtime::observability::{DerivedMetrics, TrajectoryProjection};
use theo_application::use_cases::observability_ui::{self, RunSummary};

use crate::state::AppState;

/// Resolve the current project directory. Returns an explicit error when the
/// user has not selected a project yet — silently falling back to CWD would
/// read trajectories from the Tauri bundle directory, which is always wrong.
async fn project_dir(state: &tauri::State<'_, AppState>) -> Result<PathBuf, String> {
    state
        .project_dir
        .lock()
        .await
        .clone()
        .ok_or_else(|| "No project directory selected. Use set_project_dir first.".to_string())
}

#[tauri::command]
pub async fn list_runs(state: tauri::State<'_, AppState>) -> Result<Vec<RunSummary>, String> {
    let pd = project_dir(&state).await?;
    Ok(observability_ui::list_runs(&pd))
}

#[tauri::command]
pub async fn get_run_trajectory(
    state: tauri::State<'_, AppState>,
    run_id: String,
) -> Result<TrajectoryProjection, String> {
    let pd = project_dir(&state).await?;
    observability_ui::get_run_trajectory(&pd, &run_id)
}

#[tauri::command]
pub async fn get_run_metrics(
    state: tauri::State<'_, AppState>,
    run_id: String,
) -> Result<DerivedMetrics, String> {
    let pd = project_dir(&state).await?;
    observability_ui::get_run_metrics(&pd, &run_id)
}

#[tauri::command]
pub async fn compare_runs(
    state: tauri::State<'_, AppState>,
    run_ids: Vec<String>,
) -> Result<Vec<DerivedMetrics>, String> {
    let pd = project_dir(&state).await?;
    Ok(observability_ui::compare_runs(&pd, &run_ids))
}
