//! Per-agent dashboard endpoints — Phase 15 (sota-gaps-plan).
//!
//! All `/api/agents*` handlers + their axum-level integration tests.
//! Lives in its own module so `cargo test -p theo --bin theo dashboard_agents`
//! (the verify command from the plan) targets exactly this surface.

use std::collections::HashSet;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use axum::Router;
use futures::Stream;

use theo_application::use_cases::agents_dashboard;

#[derive(Clone)]
pub struct AgentsState {
    pub project_dir: Arc<PathBuf>,
}

/// GET /api/agents — aggregated stats per sub-agent name.
async fn list_agents_handler(State(state): State<AgentsState>) -> Response {
    let agents = agents_dashboard::list_agents(&state.project_dir);
    Json(agents).into_response()
}

/// GET /api/agents/:name — detail for one agent (stats + recent runs).
async fn get_agent_handler(
    State(state): State<AgentsState>,
    AxumPath(agent_name): AxumPath<String>,
) -> Response {
    match agents_dashboard::get_agent(&state.project_dir, &agent_name, 20) {
        Some(d) => Json(d).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            format!("agent '{}' not found", agent_name),
        )
            .into_response(),
    }
}

/// GET /api/agents/:name/runs — every persisted run for that agent.
async fn list_agent_runs_handler(
    State(state): State<AgentsState>,
    AxumPath(agent_name): AxumPath<String>,
) -> Response {
    let runs = agents_dashboard::list_agent_runs(&state.project_dir, &agent_name);
    Json(runs).into_response()
}

/// GET /api/agents/events — SSE stream of new sub-agent runs.
///
/// Poll-based for now: the dashboard server is a separate process from
/// the agent runtime, so we can't share an in-memory `EventBus`. Every
/// 2s we re-list `.theo/subagent/runs/`; previously unseen `run_id`s
/// are emitted as `subagent_run_added` events. Status changes on
/// existing runs are emitted as `subagent_run_updated` events.
/// Keep-alive comments every 15s.
async fn agents_events_handler(
    State(state): State<AgentsState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    use async_stream::stream;
    let project_dir = state.project_dir.clone();
    let stream = stream! {
        let mut seen: HashSet<String> = HashSet::new();
        let mut statuses: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        // Phase 28 (sota-gaps-followup): two parallel sources of events.
        //  1. Run-store poll (200 ms) → subagent_run_added / _updated
        //  2. Trajectory file-tail (200 ms) → HandoffEvaluated /
        //     SubagentStarted / SubagentCompleted observed in real-time
        let mut tailers = TrajectoryTailers::new(project_dir.clone());
        let mut interval = tokio::time::interval(Duration::from_millis(200));
        loop {
            interval.tick().await;

            // — 1. Run-store poll (existing behavior, kept for persistence-only path).
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

            // — 2. Trajectory file-tail: emit HandoffEvaluated /
            //   SubagentStarted / SubagentCompleted in real-time.
            for raw_event in tailers.poll_new_events() {
                let event_type = raw_event
                    .get("event_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !is_subagent_event(event_type) {
                    continue;
                }
                let kind_label = match event_type {
                    "HandoffEvaluated" => "handoff_evaluated",
                    "SubagentStarted" => "subagent_started",
                    "SubagentCompleted" => "subagent_completed",
                    _ => continue,
                };
                if let Ok(ev) = Event::default()
                    .event(kind_label)
                    .json_data(&raw_event)
                {
                    yield Ok::<_, Infallible>(ev);
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

/// Phase 28 (sota-gaps-followup): true when `event_type` is one of the
/// sub-agent lifecycle events the dashboard surfaces.
fn is_subagent_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "HandoffEvaluated" | "SubagentStarted" | "SubagentCompleted"
    )
}

/// Phase 28 (sota-gaps-followup): tracks file offsets per trajectory JSONL
/// so each `poll_new_events` call returns only events appended since the
/// last call. Picks up new trajectory files (new run_ids) on every poll.
struct TrajectoryTailers {
    base_dir: PathBuf,
    offsets: std::collections::HashMap<PathBuf, u64>,
}

impl TrajectoryTailers {
    fn new(project_dir: Arc<PathBuf>) -> Self {
        Self {
            base_dir: project_dir.join(".theo").join("trajectories"),
            offsets: std::collections::HashMap::new(),
        }
    }

    /// Read all new lines from every JSONL file in `<project>/.theo/trajectories/`
    /// and return parsed events in append order. Files are tracked by their
    /// absolute path; new files added between polls are picked up.
    fn poll_new_events(&mut self) -> Vec<serde_json::Value> {
        use std::io::{BufRead, BufReader, Seek, SeekFrom};
        let mut out = Vec::new();
        let entries = match std::fs::read_dir(&self.base_dir) {
            Ok(e) => e,
            Err(_) => return out, // dir doesn't exist yet → nothing to tail
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let prev_offset = *self.offsets.get(&path).unwrap_or(&0);
            let mut file = match std::fs::File::open(&path) {
                Ok(f) => f,
                Err(_) => continue,
            };
            if file.seek(SeekFrom::Start(prev_offset)).is_err() {
                continue;
            }
            let mut reader = BufReader::new(&file);
            let mut bytes_read: u64 = 0;
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(n) => {
                        bytes_read += n as u64;
                        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
                        if trimmed.is_empty() {
                            continue;
                        }
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
                            out.push(v);
                        }
                    }
                    Err(_) => break,
                }
            }
            self.offsets.insert(path, prev_offset + bytes_read);
        }
        out
    }
}

/// Build the per-agent sub-router. Mounted under `/api/agents` by the
/// parent dashboard router.
pub fn build_router(project_dir: PathBuf) -> Router {
    let state = AgentsState {
        project_dir: Arc::new(project_dir),
    };
    Router::new()
        .route("/", get(list_agents_handler))
        .route("/events", get(agents_events_handler))
        .route("/:name", get(get_agent_handler))
        .route("/:name/runs", get(list_agent_runs_handler))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Tests — plan §15 RED list (sota-gaps-plan.md lines 422-431)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Method, Request};
    use http_body_util::BodyExt;
    use tempfile::TempDir;
    // T3.3 / find_p3_009 / ADR-023 — runtime types now via theo-application.
    use theo_application::cli_runtime::builtins;
    use theo_application::cli_runtime::{FileSubagentRunStore, RunStatus, SubagentRun};
    use tower::ServiceExt;

    fn fixture_project() -> (TempDir, FileSubagentRunStore) {
        let dir = TempDir::new().unwrap();
        let store =
            FileSubagentRunStore::new(dir.path().join(".theo").join("subagent"));
        (dir, store)
    }

    fn save(
        store: &FileSubagentRunStore,
        agent_name: &str,
        status: RunStatus,
        started_at: i64,
    ) -> String {
        let spec = if agent_name == "explorer" {
            builtins::explorer()
        } else {
            theo_domain::agent_spec::AgentSpec::on_demand(agent_name, "obj")
        };
        let id = format!("r-{}-{}", agent_name, started_at);
        let mut run = SubagentRun::new_running(&id, None, &spec, "obj", "/tmp", None);
        run.status = status;
        run.started_at = started_at;
        run.finished_at = Some(started_at + 5);
        store.save(&run).unwrap();
        id
    }

    async fn body_to_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    }

    fn router_for(dir: &std::path::Path) -> Router {
        Router::new().nest("/api/agents", build_router(dir.to_path_buf()))
    }

    #[tokio::test]
    async fn endpoint_agents_returns_summary_list() {
        let (dir, store) = fixture_project();
        save(&store, "explorer", RunStatus::Completed, 1);
        save(&store, "explorer", RunStatus::Completed, 2);
        save(&store, "implementer", RunStatus::Failed, 3);

        let resp = router_for(dir.path())
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/agents")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_to_json(resp).await;
        let arr = json.as_array().expect("array");
        assert_eq!(arr.len(), 2);
        let names: Vec<&str> = arr
            .iter()
            .map(|a| a["agent_name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"explorer"));
        assert!(names.contains(&"implementer"));
    }

    #[tokio::test]
    async fn endpoint_agents_empty_when_no_runs_persisted() {
        let dir = TempDir::new().unwrap();
        let resp = router_for(dir.path())
            .oneshot(
                Request::builder()
                    .uri("/api/agents")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_to_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn endpoint_agents_name_returns_404_unknown() {
        let dir = TempDir::new().unwrap();
        let resp = router_for(dir.path())
            .oneshot(
                Request::builder()
                    .uri("/api/agents/ghost")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn endpoint_agents_name_returns_detail_for_existing() {
        let (dir, store) = fixture_project();
        save(&store, "explorer", RunStatus::Completed, 1);

        let resp = router_for(dir.path())
            .oneshot(
                Request::builder()
                    .uri("/api/agents/explorer")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_to_json(resp).await;
        assert_eq!(json["stats"]["agent_name"], "explorer");
        assert_eq!(json["stats"]["run_count"], 1);
        assert_eq!(json["recent_runs"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn endpoint_agents_runs_filtered_by_agent_name() {
        let (dir, store) = fixture_project();
        save(&store, "explorer", RunStatus::Completed, 1);
        save(&store, "explorer", RunStatus::Failed, 2);
        save(&store, "implementer", RunStatus::Completed, 3);

        let resp = router_for(dir.path())
            .oneshot(
                Request::builder()
                    .uri("/api/agents/explorer/runs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_to_json(resp).await;
        let runs = json.as_array().unwrap();
        assert_eq!(runs.len(), 2, "only explorer runs returned");
        for r in runs {
            assert!(r["run_id"].as_str().unwrap().contains("explorer"));
        }
    }

    #[tokio::test]
    async fn endpoint_agents_runs_sorted_desc_by_started_at() {
        let (dir, store) = fixture_project();
        save(&store, "x", RunStatus::Completed, 5);
        save(&store, "x", RunStatus::Completed, 1);
        save(&store, "x", RunStatus::Completed, 100);

        let resp = router_for(dir.path())
            .oneshot(
                Request::builder()
                    .uri("/api/agents/x/runs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_to_json(resp).await;
        let timestamps: Vec<i64> = json
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["started_at"].as_i64().unwrap())
            .collect();
        assert_eq!(timestamps, vec![100, 5, 1]);
    }

    #[tokio::test]
    async fn endpoint_agents_events_emits_subagent_started() {
        // Plan §15: the SSE stream surfaces every new persisted run.
        // The dashboard runs in a separate process from the agent, so
        // "SubagentStarted" is observed indirectly: when a SubagentRun
        // record appears (which happens at spawn time inside the agent
        // process), the dashboard tails the runs/ directory and emits a
        // `subagent_run_added` SSE frame.
        use std::time::Duration;
        let (dir, store) = fixture_project();
        let app = router_for(dir.path());
        save(&store, "explorer", RunStatus::Running, 100);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/agents/events")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let mut body = resp.into_body();
        let deadline = std::time::Instant::now() + Duration::from_secs(4);
        let mut buf = Vec::new();
        while std::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(500), body.frame())
                .await
            {
                Ok(Some(Ok(frame))) => {
                    if let Some(data) = frame.data_ref() {
                        buf.extend_from_slice(data);
                        if buf
                            .windows(b"subagent_run_added".len())
                            .any(|w| w == b"subagent_run_added")
                        {
                            return;
                        }
                    }
                }
                _ => continue,
            }
        }
        panic!(
            "SSE stream did not emit subagent_run_added within 4s; buf:\n{}",
            String::from_utf8_lossy(&buf)
        );
    }

    #[tokio::test]
    async fn endpoint_agents_events_emits_subagent_completed() {
        // Status transition Running -> Completed surfaces as a
        // `subagent_run_updated` SSE frame whose payload carries
        // status="completed".
        use std::time::Duration;
        let (dir, store) = fixture_project();
        save(&store, "x", RunStatus::Running, 1);
        let app = router_for(dir.path());

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/agents/events")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let mut body = resp.into_body();
        // First tick (~2s) emits added; transition the status afterwards.
        let _ = tokio::time::timeout(Duration::from_millis(2_500), body.frame()).await;

        // Now transition to Completed.
        let mut run = store.load("r-x-1").unwrap();
        run.status = RunStatus::Completed;
        store.save(&run).unwrap();

        // Read frames until either we see the updated event or 5s elapse.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut buf = Vec::new();
        while std::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(500), body.frame())
                .await
            {
                Ok(Some(Ok(frame))) => {
                    if let Some(data) = frame.data_ref() {
                        buf.extend_from_slice(data);
                        let s = String::from_utf8_lossy(&buf);
                        if s.contains("subagent_run_updated")
                            && s.contains("completed")
                        {
                            return;
                        }
                    }
                }
                _ => continue,
            }
        }
        panic!(
            "SSE stream did not emit subagent_run_updated/completed within 5s; buf:\n{}",
            String::from_utf8_lossy(&buf)
        );
    }

    #[tokio::test]
    async fn endpoint_agents_events_returns_200_with_text_event_stream() {
        let dir = TempDir::new().unwrap();
        let resp = router_for(dir.path())
            .oneshot(
                Request::builder()
                    .uri("/api/agents/events")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(ct.contains("text/event-stream"), "got: {}", ct);
    }

    // ── Phase 28 (sota-gaps-followup): file-tail bridge ──

    pub mod sse_handler {
        use super::*;
        use std::io::Write;

        fn write_trajectory_event(
            project_dir: &std::path::Path,
            run_id: &str,
            event_type: &str,
            payload: serde_json::Value,
        ) {
            let dir = project_dir.join(".theo").join("trajectories");
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join(format!("{}.jsonl", run_id));
            let envelope = serde_json::json!({
                "v": 1,
                "ts": 0,
                "run_id": run_id,
                "kind": "event",
                "event_type": event_type,
                "entity_id": run_id,
                "payload": payload,
            });
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .unwrap();
            writeln!(f, "{}", serde_json::to_string(&envelope).unwrap()).unwrap();
        }

        #[test]
        fn is_subagent_event_filters_correctly() {
            assert!(is_subagent_event("HandoffEvaluated"));
            assert!(is_subagent_event("SubagentStarted"));
            assert!(is_subagent_event("SubagentCompleted"));
            assert!(!is_subagent_event("ToolCallCompleted"));
            assert!(!is_subagent_event("LlmCallStart"));
            assert!(!is_subagent_event(""));
        }

        #[test]
        fn trajectory_tailers_returns_empty_when_no_dir() {
            let dir = TempDir::new().unwrap();
            let mut t = TrajectoryTailers::new(Arc::new(dir.path().to_path_buf()));
            assert!(t.poll_new_events().is_empty());
        }

        #[test]
        fn trajectory_tailers_returns_event_when_jsonl_appended() {
            let dir = TempDir::new().unwrap();
            write_trajectory_event(
                dir.path(),
                "r1",
                "SubagentStarted",
                serde_json::json!({"agent_name": "explorer"}),
            );
            let mut t = TrajectoryTailers::new(Arc::new(dir.path().to_path_buf()));
            let events = t.poll_new_events();
            assert_eq!(events.len(), 1);
            assert_eq!(
                events[0].get("event_type").and_then(|v| v.as_str()),
                Some("SubagentStarted")
            );
        }

        #[test]
        fn trajectory_tailers_only_returns_appended_lines_on_subsequent_poll() {
            let dir = TempDir::new().unwrap();
            write_trajectory_event(
                dir.path(),
                "r1",
                "SubagentStarted",
                serde_json::json!({}),
            );
            let mut t = TrajectoryTailers::new(Arc::new(dir.path().to_path_buf()));
            let _ = t.poll_new_events(); // consume first batch

            // Second poll with no new events → empty
            assert!(t.poll_new_events().is_empty());

            // Append → only the new line is returned
            write_trajectory_event(
                dir.path(),
                "r1",
                "SubagentCompleted",
                serde_json::json!({}),
            );
            let new_events = t.poll_new_events();
            assert_eq!(new_events.len(), 1);
            assert_eq!(
                new_events[0].get("event_type").and_then(|v| v.as_str()),
                Some("SubagentCompleted")
            );
        }

        #[test]
        fn trajectory_tailers_picks_up_new_files_between_polls() {
            let dir = TempDir::new().unwrap();
            write_trajectory_event(dir.path(), "r1", "SubagentStarted", serde_json::json!({}));
            let mut t = TrajectoryTailers::new(Arc::new(dir.path().to_path_buf()));
            let _ = t.poll_new_events();

            // Brand-new run_id appears
            write_trajectory_event(dir.path(), "r2", "HandoffEvaluated", serde_json::json!({}));
            let new_events = t.poll_new_events();
            assert_eq!(new_events.len(), 1);
            assert_eq!(
                new_events[0].get("event_type").and_then(|v| v.as_str()),
                Some("HandoffEvaluated")
            );
        }

        #[tokio::test]
        async fn sse_handler_emits_handoff_evaluated_within_500ms_of_event_append() {
            use axum::body::Body;
            use axum::http::Request;
            use http_body_util::BodyExt;
            use std::time::Duration;
            use tower::ServiceExt;

            let dir = TempDir::new().unwrap();
            let app = router_for(dir.path());

            // Append BEFORE the SSE handler starts so the first poll picks it up.
            write_trajectory_event(
                dir.path(),
                "r1",
                "HandoffEvaluated",
                serde_json::json!({"decision": "block", "reason": "test"}),
            );

            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/agents/events")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);

            let mut body = resp.into_body();
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            let mut buf = Vec::new();
            while std::time::Instant::now() < deadline {
                match tokio::time::timeout(
                    Duration::from_millis(300),
                    body.frame(),
                )
                .await
                {
                    Ok(Some(Ok(frame))) => {
                        if let Some(data) = frame.data_ref() {
                            buf.extend_from_slice(data);
                            let s = String::from_utf8_lossy(&buf);
                            if s.contains("handoff_evaluated") {
                                return;
                            }
                        }
                    }
                    _ => continue,
                }
            }
            panic!(
                "SSE did not surface handoff_evaluated within 2s; buf:\n{}",
                String::from_utf8_lossy(&buf)
            );
        }

        #[tokio::test]
        async fn sse_handler_filters_out_non_subagent_events() {
            // Append a ToolCallCompleted event → must NOT appear in the stream.
            use axum::body::Body;
            use axum::http::Request;
            use http_body_util::BodyExt;
            use std::time::Duration;
            use tower::ServiceExt;

            let dir = TempDir::new().unwrap();
            let app = router_for(dir.path());
            write_trajectory_event(
                dir.path(),
                "r1",
                "ToolCallCompleted",
                serde_json::json!({"tool_name": "bash"}),
            );

            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/agents/events")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            let mut body = resp.into_body();
            // Tail for ~1s; nothing relevant should be emitted.
            let deadline = std::time::Instant::now() + Duration::from_secs(1);
            let mut buf = Vec::new();
            while std::time::Instant::now() < deadline {
                match tokio::time::timeout(
                    Duration::from_millis(300),
                    body.frame(),
                )
                .await
                {
                    Ok(Some(Ok(frame))) => {
                        if let Some(data) = frame.data_ref() {
                            buf.extend_from_slice(data);
                        }
                    }
                    _ => break,
                }
            }
            let s = String::from_utf8_lossy(&buf);
            assert!(
                !s.contains("ToolCallCompleted"),
                "ToolCallCompleted must be filtered out; got:\n{}",
                s
            );
            assert!(
                !s.contains("tool_call_completed"),
                "ToolCallCompleted must be filtered out; got:\n{}",
                s
            );
        }
    }
}
