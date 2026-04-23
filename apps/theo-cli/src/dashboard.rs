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

use std::collections::HashSet;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Json, Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use futures::Stream;
use serde::Deserialize;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

use theo_application::use_cases::{agents_dashboard, observability_ui};

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

/// GET /api/agents — aggregated stats per sub-agent name.
async fn list_agents_handler(State(state): State<AppState>) -> Response {
    let agents = agents_dashboard::list_agents(&state.project_dir);
    Json(agents).into_response()
}

/// GET /api/agents/:name — detail for one agent (stats + recent runs).
async fn get_agent_handler(
    State(state): State<AppState>,
    AxumPath(agent_name): AxumPath<String>,
) -> Response {
    match agents_dashboard::get_agent(&state.project_dir, &agent_name, 20) {
        Some(d) => Json(d).into_response(),
        None => (StatusCode::NOT_FOUND, format!("agent '{}' not found", agent_name))
            .into_response(),
    }
}

/// GET /api/agents/:name/runs — every persisted run for that agent.
async fn list_agent_runs_handler(
    State(state): State<AppState>,
    AxumPath(agent_name): AxumPath<String>,
) -> Response {
    let runs = agents_dashboard::list_agent_runs(&state.project_dir, &agent_name);
    Json(runs).into_response()
}

/// GET /api/agents/events — SSE stream of new sub-agent runs.
///
/// Poll-based for now: the dashboard server is a separate process from the
/// agent runtime, so we can't share an in-memory `EventBus`. Every 2s we
/// re-list `.theo/subagent/runs/`; previously unseen `run_id`s are emitted
/// as `subagent_run_added` events. Status changes on existing runs are
/// emitted as `subagent_run_updated` events. Keep-alive comments every 15s.
async fn agents_events_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    use async_stream::stream;
    let project_dir = state.project_dir.clone();
    let stream = stream! {
        let mut seen: HashSet<String> = HashSet::new();
        let mut statuses: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            interval.tick().await;
            let agents = agents_dashboard::list_agents(&project_dir);
            for stats in agents {
                let detail = match agents_dashboard::get_agent(
                    &project_dir,
                    &stats.agent_name,
                    50,
                ) {
                    Some(d) => d,
                    None => continue,
                };
                for run in detail.recent_runs {
                    if seen.insert(run.run_id.clone()) {
                        let payload = serde_json::json!({
                            "type": "subagent_run_added",
                            "agent_name": stats.agent_name,
                            "run_id": run.run_id,
                            "status": run.status,
                            "tokens_used": run.tokens_used,
                        });
                        statuses.insert(run.run_id.clone(), run.status.clone());
                        if let Ok(ev) = Event::default()
                            .event("subagent_run_added")
                            .json_data(&payload)
                        {
                            yield Ok::<_, Infallible>(ev);
                        }
                    } else if let Some(prior) = statuses.get(&run.run_id)
                        && prior != &run.status
                    {
                        let payload = serde_json::json!({
                            "type": "subagent_run_updated",
                            "agent_name": stats.agent_name,
                            "run_id": run.run_id,
                            "status": run.status,
                            "tokens_used": run.tokens_used,
                        });
                        statuses.insert(run.run_id.clone(), run.status.clone());
                        if let Ok(ev) = Event::default()
                            .event("subagent_run_updated")
                            .json_data(&payload)
                        {
                            yield Ok::<_, Infallible>(ev);
                        }
                    }
                }
            }
        }
    };
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

/// Build the router.
fn build_router(project_dir: PathBuf, static_dir: Option<PathBuf>) -> Router {
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
        // Phase 15 (sota-gaps): per-agent breakdown endpoints
        .route("/agents", get(list_agents_handler))
        .route("/agents/events", get(agents_events_handler))
        .route("/agents/:name", get(get_agent_handler))
        .route("/agents/:name/runs", get(list_agent_runs_handler))
        .with_state(state);

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
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("dashboard-dist");
            if candidate.exists() {
                return Some(candidate);
            }
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
