//! Observability dashboard HTTP server.
//!
//! Serves the built Theo UI bundle at `/` and exposes the same four
//! operations as the Tauri commands under `/api/*`:
//!
//! - `GET  /api/list_runs`
//! - `GET  /api/run/:run_id/trajectory`
//! - `GET  /api/run/:run_id/metrics`
//! - `POST /api/runs/compare`  (body: `{"run_ids": [...]}`)
//!
//! Intended for remote access via port-forward — `theo dashboard --repo .`
//! and then `ssh -L 5173:localhost:5173 <machine>` from the client.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::{Json, Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

use theo_application::use_cases::observability_ui;

use crate::dashboard_agents;

#[derive(Clone)]
struct AppState {
    project_dir: Arc<PathBuf>,
}

#[derive(Deserialize)]
struct CompareRequest {
    run_ids: Vec<String>,
}

fn error_response(err: String) -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, err).into_response()
}

async fn list_runs_handler(State(state): State<AppState>) -> impl IntoResponse {
    let runs = observability_ui::list_runs(&state.project_dir);
    Json(runs)
}

async fn get_trajectory_handler(
    State(state): State<AppState>,
    AxumPath(run_id): AxumPath<String>,
) -> Response {
    match observability_ui::get_run_trajectory(&state.project_dir, &run_id) {
        Ok(t) => Json(t).into_response(),
        Err(e) => (StatusCode::NOT_FOUND, e).into_response(),
    }
}

async fn get_metrics_handler(
    State(state): State<AppState>,
    AxumPath(run_id): AxumPath<String>,
) -> Response {
    match observability_ui::get_run_metrics(&state.project_dir, &run_id) {
        Ok(m) => Json(m).into_response(),
        Err(e) => (StatusCode::NOT_FOUND, e).into_response(),
    }
}

async fn get_report_handler(
    State(state): State<AppState>,
    AxumPath(run_id): AxumPath<String>,
) -> Response {
    match observability_ui::get_run_report(&state.project_dir, &run_id) {
        Ok(r) => Json(r).into_response(),
        Err(e) => (StatusCode::NOT_FOUND, e).into_response(),
    }
}

async fn system_stats_handler(State(state): State<AppState>) -> Response {
    let stats = observability_ui::get_system_stats(&state.project_dir);
    Json(stats).into_response()
}

async fn compare_runs_handler(
    State(state): State<AppState>,
    Json(req): Json<CompareRequest>,
) -> Response {
    let metrics = observability_ui::compare_runs(&state.project_dir, &req.run_ids);
    Json(metrics).into_response()
}

// ---------------------------------------------------------------------------
// Phase 15: per-agent dashboard endpoints
// ---------------------------------------------------------------------------
// Handlers + tests live in `dashboard_agents.rs`; `build_router` below
// nests the sub-router under `/api/agents`.

/// Build the router.
fn build_router(project_dir: PathBuf, static_dir: Option<PathBuf>) -> Router {
    let agents_router = dashboard_agents::build_router(project_dir.clone());
    let state = AppState {
        project_dir: Arc::new(project_dir),
    };

    let api = Router::new()
        .route("/list_runs", get(list_runs_handler))
        .route("/run/:run_id/trajectory", get(get_trajectory_handler))
        .route("/run/:run_id/metrics", get(get_metrics_handler))
        .route("/run/:run_id/report", get(get_report_handler))
        .route("/system/stats", get(system_stats_handler))
        .route("/runs/compare", post(compare_runs_handler))
        .with_state(state)
        // Phase 15 (sota-gaps): per-agent endpoints in dashboard_agents.rs
        .nest("/agents", agents_router);

    let mut app = Router::new().nest("/api", api);

    // Static file serving for the built UI bundle. We serve `dashboard.html`
    // (a dedicated browser-only entry that does NOT import Tauri-coupled
    // pages) at `/`, and fall back to it for any unknown route so SPA-style
    // deep links continue to work.
    if let Some(dir) = static_dir {
        if dir.exists() {
            let dashboard_path = dir.join("dashboard.html");
            let fallback_path = if dashboard_path.exists() {
                dashboard_path
            } else {
                dir.join("index.html")
            };
            app = app
                .route(
                    "/",
                    get({
                        let p = fallback_path.clone();
                        move || {
                            let p = p.clone();
                            async move {
                                match std::fs::read_to_string(&p) {
                                    Ok(s) => (StatusCode::OK, [("Content-Type", "text/html")], s).into_response(),
                                    Err(e) => error_response(e.to_string()),
                                }
                            }
                        }
                    }),
                )
                .fallback_service(
                    ServeDir::new(&dir).not_found_service(axum::routing::get(move || {
                        let p = fallback_path.clone();
                        async move {
                            match std::fs::read_to_string(&p) {
                                Ok(s) => (StatusCode::OK, [("Content-Type", "text/html")], s).into_response(),
                                Err(e) => error_response(e.to_string()),
                            }
                        }
                    })),
                );
        } else {
            eprintln!(
                "[dashboard] WARNING: static dir {:?} not found — API-only mode",
                dir
            );
        }
    }

    app.layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
}

/// Start the dashboard server. Blocks until Ctrl+C.
pub async fn serve(project_dir: PathBuf, port: u16, static_dir: Option<PathBuf>) -> std::io::Result<()> {
    let project_dir = project_dir
        .canonicalize()
        .unwrap_or(project_dir);
    let trajectories = project_dir.join(".theo").join("trajectories");
    if !trajectories.exists() {
        eprintln!(
            "[dashboard] WARNING: {:?} does not exist — no runs will be shown yet.",
            trajectories
        );
    }

    let app = build_router(project_dir.clone(), static_dir.clone());

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("[dashboard] Theo Observability Dashboard");
    eprintln!("[dashboard] Project: {}", project_dir.display());
    if let Some(sd) = &static_dir {
        eprintln!("[dashboard] Static:  {}", sd.display());
    }
    eprintln!("[dashboard] Listening on http://{}", addr);
    eprintln!("[dashboard] Remote access: ssh -L {p}:localhost:{p} <host>", p = port);
    eprintln!("[dashboard] API: /api/list_runs");

    axum::serve(listener, app).await
}


/// Heuristic: locate the UI bundle shipped next to the binary (or dev path).
pub fn find_default_static_dir() -> Option<PathBuf> {
    // 1) Binary-relative dist/ (e.g., ./dashboard-dist)
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join("dashboard-dist");
        if candidate.exists() {
            return Some(candidate);
        }
    }
    // 2) Workspace-relative for dev runs: apps/theo-ui/dist
    let candidates = [
        "apps/theo-ui/dist",
        "../apps/theo-ui/dist",
        "../../apps/theo-ui/dist",
    ];
    for c in candidates {
        let p: &Path = Path::new(c);
        if p.exists() {
            return Some(p.to_path_buf());
        }
    }
    None
}
