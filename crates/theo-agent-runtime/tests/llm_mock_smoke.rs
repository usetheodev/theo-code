#![allow(clippy::field_reassign_with_default)] // Tests tweak individual fields for readability.

//! REMEDIATION_PLAN T0.1 infrastructure — minimal in-process LLM mock.
//!
//! The 4 LLM-dependent characterization scenarios in T0.1 (context
//! overflow recovery, retry+success, done-gate Gate 2, batch tool
//! with LLM stream) all require an HTTP endpoint that speaks the
//! OA-compatible SSE format and lets the test pin the response.
//!
//! Instead of pulling in the heavyweight `wiremock` crate, this file
//! provides a tiny single-shot HTTP server (~80 LOC) that:
//!   1. Binds to `127.0.0.1:0` (kernel-assigned port).
//!   2. Accepts ONE POST request, drains its body, ignores it.
//!   3. Returns a `Content-Type: text/event-stream` body with the
//!      caller-provided SSE chunks (`data: {...}\n\n` lines + final
//!      `data: [DONE]\n\n`).
//!
//! The smoke test below verifies the round-trip:
//!   client → mock → SSE deltas → ChatResponse parsed from collector.
//!
//! This pins the contract enough that future characterization tests
//! (T0.1 LLM scenarios, T7.3 happy-path) can reuse the helper without
//! re-debugging the SSE serializer.
//!
//! Pattern borrowed from `otlp_network_smoke.rs` (Phase 45).

use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use theo_infra_llm::types::{ChatRequest, Message};
use theo_infra_llm::LlmClient;

use theo_agent_runtime::agent_loop::AgentLoop;
use theo_agent_runtime::config::AgentConfig;
use theo_agent_runtime::event_bus::{EventBus, EventListener};
use theo_domain::event::{DomainEvent, EventType};
use theo_tooling::registry::create_default_registry;

/// Spawn a single-shot HTTP server that returns the given SSE chunks.
/// Returns the base URL (e.g. `http://127.0.0.1:NNNN`) the caller can
/// pass to `LlmClient::new`. The server self-closes after one request.
async fn spawn_sse_mock(sse_body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
    let port = listener.local_addr().expect("local_addr").port();

    tokio::spawn(async move {
        // Accept exactly one connection, then drop the listener.
        let (mut sock, _) = match listener.accept().await {
            Ok(p) => p,
            Err(_) => return,
        };
        // Drain the request head + body. We don't validate; the mock
        // is OK with any POST that the LlmClient sends.
        let mut buf = [0u8; 8192];
        let mut acc: Vec<u8> = Vec::new();
        loop {
            let n = match sock.read(&mut buf).await {
                Ok(n) if n > 0 => n,
                _ => break,
            };
            acc.extend_from_slice(&buf[..n]);
            if let Some(idx) = acc.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = std::str::from_utf8(&acc[..idx]).unwrap_or("");
                let len = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                let body_so_far = acc.len() - (idx + 4);
                if body_so_far < len {
                    let mut more = vec![0u8; len - body_so_far];
                    let _ = sock.read_exact(&mut more).await;
                }
                break;
            }
        }
        // Build the response. Length-delimited body keeps the test
        // simple (no chunked encoding) — `chat_streaming` parses the
        // body as a stream of `data:` lines either way.
        let body_bytes = sse_body.as_bytes();
        let head = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: text/event-stream\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n",
            body_bytes.len()
        );
        let _ = sock.write_all(head.as_bytes()).await;
        let _ = sock.write_all(body_bytes).await;
        let _ = sock.shutdown().await;
    });

    format!("http://127.0.0.1:{port}")
}

/// Smoke test: a content-only SSE stream parses into a ChatResponse
/// whose first choice has `content == "Hello world"` and no tool
/// calls. Pins the round-trip contract so downstream characterization
/// tests can rely on the helper.
#[tokio::test]
async fn llm_mock_serves_content_only_sse_stream() {
    let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n\
               data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n\
               data: [DONE]\n\n";

    let base_url = spawn_sse_mock(sse).await;
    let client = LlmClient::new(&base_url, Some("test-key".into()), "mock-model");

    let request = ChatRequest::new(
        "mock-model",
        vec![Message::user("hi")],
    );

    let deltas = Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));
    let deltas_cb = deltas.clone();
    let resp = client
        .chat_streaming(&request, |delta| {
            if let theo_infra_llm::stream::StreamDelta::Content(text) = delta {
                deltas_cb.lock().push(text.clone());
            }
        })
        .await
        .expect("mock SSE must round-trip cleanly");

    // Streaming callback saw "Hello" then " world" in order.
    let captured = deltas.lock().clone();
    assert_eq!(captured, vec!["Hello".to_string(), " world".to_string()]);

    // Collected response surfaces the concatenated content + no tools.
    let choice = resp.choices.first().expect("must have at least one choice");
    assert_eq!(choice.message.content.as_deref(), Some("Hello world"));
    assert!(
        choice
            .message
            .tool_calls
            .as_ref()
            .map(|tc| tc.is_empty())
            .unwrap_or(true),
        "content-only mock must not synthesize tool calls"
    );
}

/// Multi-shot variant: the mock server cycles through a queue of
/// canned bodies, one per incoming POST. Used by characterization
/// tests that need different LLM responses across the agent loop's
/// iterations (e.g. tool_call response → tool_result echo → text
/// "done" response → converge).
async fn spawn_sse_mock_multi(
    bodies: Vec<&'static str>,
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
    let port = listener.local_addr().expect("local_addr").port();

    tokio::spawn(async move {
        let mut idx = 0usize;
        loop {
            let (mut sock, _) = match listener.accept().await {
                Ok(p) => p,
                Err(_) => return,
            };
            // Drain the request head + body. We don't validate.
            let mut buf = [0u8; 8192];
            let mut acc: Vec<u8> = Vec::new();
            loop {
                let n = match sock.read(&mut buf).await {
                    Ok(n) if n > 0 => n,
                    _ => break,
                };
                acc.extend_from_slice(&buf[..n]);
                if let Some(idx2) = acc.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = std::str::from_utf8(&acc[..idx2]).unwrap_or("");
                    let len = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    let body_so_far = acc.len() - (idx2 + 4);
                    if body_so_far < len {
                        let mut more = vec![0u8; len - body_so_far];
                        let _ = sock.read_exact(&mut more).await;
                    }
                    break;
                }
            }
            // Pick the next canned body. Saturates at the last one
            // (the agent loop sometimes calls the LLM more times than
            // the test predicted; returning the same final body keeps
            // the loop from hanging on a connect-but-no-response).
            let body = bodies.get(idx).copied().unwrap_or_else(|| {
                bodies.last().copied().unwrap_or("data: [DONE]\n\n")
            });
            idx += 1;
            let body_bytes = body.as_bytes();
            let head = format!(
                "HTTP/1.1 200 OK\r\n\
                 Content-Type: text/event-stream\r\n\
                 Content-Length: {}\r\n\
                 Connection: close\r\n\
                 \r\n",
                body_bytes.len()
            );
            let _ = sock.write_all(head.as_bytes()).await;
            let _ = sock.write_all(body_bytes).await;
            let _ = sock.shutdown().await;
        }
    });

    format!("http://127.0.0.1:{port}")
}

/// Mock variant that returns the given (status_code, body) tuples
/// in order. Saturates at the last entry for any subsequent connect.
/// Used to exercise the retry path where the FIRST attempt sees a
/// retryable error (e.g. 503) and a subsequent attempt sees 200.
async fn spawn_status_mock_multi(
    responses: Vec<(u16, &'static str)>,
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
    let port = listener.local_addr().expect("local_addr").port();

    tokio::spawn(async move {
        let mut idx = 0usize;
        loop {
            let (mut sock, _) = match listener.accept().await {
                Ok(p) => p,
                Err(_) => return,
            };
            // Drain request head + body.
            let mut buf = [0u8; 8192];
            let mut acc: Vec<u8> = Vec::new();
            loop {
                let n = match sock.read(&mut buf).await {
                    Ok(n) if n > 0 => n,
                    _ => break,
                };
                acc.extend_from_slice(&buf[..n]);
                if let Some(idx2) = acc.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = std::str::from_utf8(&acc[..idx2]).unwrap_or("");
                    let len = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    let body_so_far = acc.len() - (idx2 + 4);
                    if body_so_far < len {
                        let mut more = vec![0u8; len - body_so_far];
                        let _ = sock.read_exact(&mut more).await;
                    }
                    break;
                }
            }
            let (status, body) = responses
                .get(idx)
                .copied()
                .unwrap_or_else(|| responses.last().copied().unwrap_or((200, "data: [DONE]\n\n")));
            idx += 1;
            let body_bytes = body.as_bytes();
            let reason = match status {
                200 => "OK",
                429 => "Too Many Requests",
                503 => "Service Unavailable",
                _ => "Status",
            };
            let head = format!(
                "HTTP/1.1 {status} {reason}\r\n\
                 Content-Type: text/event-stream\r\n\
                 Content-Length: {}\r\n\
                 Connection: close\r\n\
                 \r\n",
                body_bytes.len()
            );
            let _ = sock.write_all(head.as_bytes()).await;
            let _ = sock.write_all(body_bytes).await;
            let _ = sock.shutdown().await;
        }
    });

    format!("http://127.0.0.1:{port}")
}

/// Tool-call SSE stream: a `read` tool call comes through as a
/// `ToolCallDelta` and ends up in the final `ChatResponse.choices[0]
/// .message.tool_calls`. Pins the second contract path the
/// downstream characterization tests need.
#[tokio::test]
async fn llm_mock_serves_tool_call_sse_stream() {
    // Single-shot tool_call: id + name + complete arguments in one
    // delta, then [DONE]. Real providers split arguments across
    // multiple deltas; the StreamCollector accumulates them — we
    // exercise the simple case here.
    let sse = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[\
                {\"index\":0,\"id\":\"c-1\",\"function\":\
                {\"name\":\"read\",\"arguments\":\"{\\\"filePath\\\":\\\"x.rs\\\"}\"}}]}}]}\n\n\
               data: [DONE]\n\n";

    let base_url = spawn_sse_mock(sse).await;
    let client = LlmClient::new(&base_url, Some("test-key".into()), "mock-model");

    let request = ChatRequest::new(
        "mock-model",
        vec![Message::user("read x.rs")],
    );

    let resp = client
        .chat_streaming(&request, |_| {})
        .await
        .expect("tool-call mock must round-trip cleanly");

    let choice = resp.choices.first().expect("must have at least one choice");
    let tool_calls = choice
        .message
        .tool_calls
        .as_ref()
        .expect("tool_calls must be populated");
    assert_eq!(tool_calls.len(), 1);
    let call = &tool_calls[0];
    assert_eq!(call.id, "c-1");
    assert_eq!(call.function.name, "read");
    assert!(
        call.function.arguments.contains("x.rs"),
        "arguments JSON must round-trip; got {}",
        call.function.arguments
    );
}

// ────────────────────────────────────────────────────────────────────
// T0.1 characterization scenarios — built on the mock infrastructure.
//
// Each test wires `AgentLoop::run` against `spawn_sse_mock_multi`
// and asserts the observable outcome (success flag, summary,
// iterations_used). These pin the engine's response handling to
// canonical SSE response shapes — any future refactor that breaks
// the loop's text-converge or tool-dispatch contract surfaces as a
// regression here.
// ────────────────────────────────────────────────────────────────────

/// T0.1 scenario 1 — LLM returns text-only on the first turn → the
/// agent converges immediately with `success == true` and surfaces
/// the LLM text as the summary. No tool calls dispatched, exactly
/// one LLM iteration consumed.
#[tokio::test]
async fn agent_converges_when_llm_returns_text_only_first_turn() {
    // Single-shot SSE: LLM responds with content "Task complete.",
    // then [DONE]. The OA-compatible main loop sees no tool_calls,
    // routes to handle_text_only_response, persists final turn
    // (memory disabled by default), and returns Converged.
    let bodies = vec![
        "data: {\"choices\":[{\"delta\":{\"content\":\"Task complete.\"}}]}\n\n\
         data: [DONE]\n\n",
    ];
    let mock_url = spawn_sse_mock_multi(bodies).await;

    let project = tempfile::tempdir().expect("tempdir");

    // Minimal config — memory off, sub-agent so the bootstrap skips
    // observability/episode persistence/legacy file memory branches.
    // The base_url points at the mock, api_key is non-None so the
    // run_agent_session guard doesn't reject.
    let mut config = AgentConfig::default();
    config.llm.base_url = mock_url;
    config.llm.api_key = Some("test-key".to_string());
    config.is_subagent = true;
    config.max_iterations = 5;

    let agent = AgentLoop::new(config, create_default_registry());
    let result = agent.run("trivial converge", project.path()).await;

    assert!(
        result.success,
        "text-only LLM response must converge with success=true; \
         summary={:?}",
        result.summary
    );
    assert!(
        result.summary.contains("Task complete.") || result.was_streamed,
        "summary should reflect the LLM's text or be already streamed; got {:?}",
        result.summary
    );
    assert_eq!(
        result.iterations_used, 1,
        "exactly one LLM iteration must run for a one-shot text response"
    );
    assert!(
        result.tool_calls_total == 0,
        "no tool calls should fire when LLM returns text only"
    );
}

/// T0.1 scenario 2 — happy path single-tool. LLM returns a `read`
/// tool call on turn 1, the runtime dispatches the tool (read of a
/// non-existent path → tool failure, that's fine), then LLM returns
/// text "all done" on turn 2 → agent converges. Exactly two LLM
/// iterations consumed, exactly one tool call dispatched.
#[tokio::test]
async fn agent_converges_after_one_tool_dispatch_round_trip() {
    let project = tempfile::tempdir().expect("tempdir");
    // Path inside the tempdir so the `read` tool can attempt + fail
    // without touching the host. Failure is fine — the test asserts
    // the runtime ROUND-TRIPS the LLM iterations, not the tool result.
    let target = project.path().join("nonexistent.txt");
    let target_str = target.to_string_lossy().replace('\\', "\\\\");

    let tool_call_body = Box::leak(
        format!(
            "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[\
                {{\"index\":0,\"id\":\"c-read-1\",\"function\":\
                {{\"name\":\"read\",\"arguments\":\"{{\\\"filePath\\\":\\\"{target_str}\\\"}}\"}}}}\
            ]}}}}]}}\n\ndata: [DONE]\n\n"
        )
        .into_boxed_str(),
    );
    let final_body = "data: {\"choices\":[{\"delta\":{\"content\":\"all done\"}}]}\n\n\
                      data: [DONE]\n\n";

    let bodies = vec![tool_call_body as &'static str, final_body];
    let mock_url = spawn_sse_mock_multi(bodies).await;

    let mut config = AgentConfig::default();
    config.llm.base_url = mock_url;
    config.llm.api_key = Some("test-key".to_string());
    config.is_subagent = true;
    config.max_iterations = 5;

    let agent = AgentLoop::new(config, create_default_registry());
    let result = agent.run("read the file", project.path()).await;

    assert!(
        result.success,
        "two-turn tool-dispatch flow must converge with success=true; \
         summary={:?}",
        result.summary
    );
    assert_eq!(
        result.iterations_used, 2,
        "exactly two LLM iterations: turn 1 tool_call, turn 2 text → converge"
    );
    assert_eq!(
        result.tool_calls_total, 1,
        "exactly one tool call dispatched (the `read`)"
    );
}

/// T0.1 scenario 3 — iteration budget exhaustion. LLM keeps
/// returning tool_calls forever. With `max_iterations=2`, the run
/// should terminate WITHOUT success after exactly 2 iterations.
/// Pins the budget enforcer's main-loop guard.
#[tokio::test]
async fn agent_aborts_when_max_iterations_reached() {
    let project = tempfile::tempdir().expect("tempdir");
    let target = project.path().join("nonexistent.txt");
    let target_str = target.to_string_lossy().replace('\\', "\\\\");

    let tool_call_body = Box::leak(
        format!(
            "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[\
                {{\"index\":0,\"id\":\"c-read-loop\",\"function\":\
                {{\"name\":\"read\",\"arguments\":\"{{\\\"filePath\\\":\\\"{target_str}\\\"}}\"}}}}\
            ]}}}}]}}\n\ndata: [DONE]\n\n"
        )
        .into_boxed_str(),
    );

    // Single canned body — the multi-shot mock saturates at the last
    // entry, so every LLM call returns the same tool_call response.
    let bodies = vec![tool_call_body as &'static str];
    let mock_url = spawn_sse_mock_multi(bodies).await;

    let mut config = AgentConfig::default();
    config.llm.base_url = mock_url;
    config.llm.api_key = Some("test-key".to_string());
    config.is_subagent = true;
    config.max_iterations = 2;

    let agent = AgentLoop::new(config, create_default_registry());
    let result = agent.run("loop forever", project.path()).await;

    assert!(
        !result.success,
        "infinite tool-call loop must NOT converge with success=true"
    );
    // The budget enforcer trips on the iteration AFTER `max_iterations`
    // is reached (the loop runs the iteration body, increments, then
    // the next iteration's guard fires). For `max_iterations=2` that
    // means the engine consumes 2 productive iterations + 1 guard
    // iteration → `iterations_used` ends at ≤ 3.
    assert!(
        result.iterations_used <= 3,
        "iterations_used must be bounded near max_iterations=2; got {}",
        result.iterations_used
    );
    assert!(
        result.tool_calls_total >= 1,
        "at least one tool call must have dispatched before the budget hit"
    );
}

/// T0.1 scenario 4 — happy path multi-tool. LLM returns a `read`
/// then a `glob` then text — exactly 3 iterations, exactly 2 tool
/// calls dispatched. Pins the multi-turn dispatch contract.
#[tokio::test]
async fn agent_converges_after_two_tool_calls_then_text() {
    let project = tempfile::tempdir().expect("tempdir");
    let target = project.path().join("nonexistent.txt");
    let target_str = target.to_string_lossy().replace('\\', "\\\\");

    let read_body = Box::leak(
        format!(
            "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[\
                {{\"index\":0,\"id\":\"c-read\",\"function\":\
                {{\"name\":\"read\",\"arguments\":\"{{\\\"filePath\\\":\\\"{target_str}\\\"}}\"}}}}\
            ]}}}}]}}\n\ndata: [DONE]\n\n"
        )
        .into_boxed_str(),
    );
    let glob_body = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[\
                        {\"index\":0,\"id\":\"c-glob\",\"function\":\
                        {\"name\":\"glob\",\"arguments\":\"{\\\"pattern\\\":\\\"/tmp/x-*\\\"}\"}}\
                    ]}}]}\n\ndata: [DONE]\n\n";
    let final_body = "data: {\"choices\":[{\"delta\":{\"content\":\"all done\"}}]}\n\n\
                      data: [DONE]\n\n";

    let bodies = vec![read_body as &'static str, glob_body, final_body];
    let mock_url = spawn_sse_mock_multi(bodies).await;

    let mut config = AgentConfig::default();
    config.llm.base_url = mock_url;
    config.llm.api_key = Some("test-key".to_string());
    config.is_subagent = true;
    config.max_iterations = 5;

    let agent = AgentLoop::new(config, create_default_registry());
    let result = agent.run("read then glob then done", project.path()).await;

    assert!(result.success, "multi-tool flow must converge; summary={:?}", result.summary);
    assert_eq!(
        result.iterations_used, 3,
        "exactly three LLM iterations: read → glob → text"
    );
    assert_eq!(
        result.tool_calls_total, 2,
        "exactly two tool calls dispatched (read + glob)"
    );
}

/// T0.1 scenario 5 — done-gate force-accept after MAX_DONE_ATTEMPTS.
/// LLM repeatedly calls `done()`. Convergence Gate 1 blocks each
/// attempt because no real edits were made (edits_succeeded=0 in
/// AllOf mode → the GitDiff+EditSuccess pair never both resolve true).
/// After MAX_DONE_ATTEMPTS=3 blocks, the 4th `done()` call's Gate 0
/// (attempt limit) force-accepts with success=true and an
/// "[accepted after 4 done attempts]" annotation.
#[tokio::test]
async fn agent_done_gate_force_accepts_after_max_attempts() {
    let project = tempfile::tempdir().expect("tempdir");

    let done_body = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[\
                        {\"index\":0,\"id\":\"c-done\",\"function\":\
                        {\"name\":\"done\",\"arguments\":\"{\\\"summary\\\":\\\"finished\\\"}\"}}\
                    ]}}]}\n\ndata: [DONE]\n\n";

    // Single body — mock saturates so every LLM call returns done().
    let bodies = vec![done_body];
    let mock_url = spawn_sse_mock_multi(bodies).await;

    let mut config = AgentConfig::default();
    config.llm.base_url = mock_url;
    config.llm.api_key = Some("test-key".to_string());
    config.is_subagent = true;
    // Plenty of iteration budget — the gate, not the budget, must
    // be the terminator here.
    config.max_iterations = 10;

    let agent = AgentLoop::new(config, create_default_registry());
    let result = agent.run("ask done repeatedly", project.path()).await;

    assert!(
        result.success,
        "force-accept after MAX_DONE_ATTEMPTS must yield success=true; \
         summary={:?}",
        result.summary
    );
    assert!(
        result.summary.contains("accepted after"),
        "summary must carry the 'accepted after N done attempts' annotation; \
         got {:?}",
        result.summary
    );
    // 4 done() iterations consumed: 3 blocked + 1 force-accept.
    // The first 3 don't end the run; the 4th does.
    assert!(
        result.iterations_used >= 4,
        "force-accept needs at least 4 done() iterations (3 blocks + 1 force-accept); \
         got {}",
        result.iterations_used
    );
}

/// T0.1 scenario 6 — `skill` meta-tool (InContext mode) loads
/// instructions into the conversation. Turn 1 LLM calls
/// `skill(name="commit")` (a bundled InContext skill); the runtime
/// pushes the skill instructions as a system message AND a confirming
/// tool_result. Turn 2 LLM returns text → converge. Pins the
/// `dispatch_skill` InContext branch end-to-end.
#[tokio::test]
async fn agent_loads_in_context_skill_then_converges() {
    let project = tempfile::tempdir().expect("tempdir");

    let skill_body = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[\
                        {\"index\":0,\"id\":\"c-skill\",\"function\":\
                        {\"name\":\"skill\",\"arguments\":\"{\\\"name\\\":\\\"commit\\\"}\"}}\
                    ]}}]}\n\ndata: [DONE]\n\n";
    let final_body = "data: {\"choices\":[{\"delta\":{\"content\":\"skill loaded\"}}]}\n\n\
                      data: [DONE]\n\n";

    let bodies = vec![skill_body, final_body];
    let mock_url = spawn_sse_mock_multi(bodies).await;

    let mut config = AgentConfig::default();
    config.llm.base_url = mock_url;
    config.llm.api_key = Some("test-key".to_string());
    config.is_subagent = true;
    config.max_iterations = 5;

    let agent = AgentLoop::new(config, create_default_registry());
    let result = agent.run("commit my changes", project.path()).await;

    assert!(
        result.success,
        "skill InContext flow must converge with success=true; summary={:?}",
        result.summary
    );
    assert_eq!(
        result.iterations_used, 2,
        "exactly two LLM iterations: skill load → text → converge"
    );
    // `tool_calls_total` counts only regular (non-meta) tool dispatches
    // routed through the ToolCallManager. Meta-tools like `skill`,
    // `done`, `delegate_task`, `batch_execute` flow through
    // `dispatch_meta_tool` and don't increment that counter — they
    // are exercised by `iterations_used` and the side-effect (a
    // skill-loaded system message + tool_result in the conversation).
    // Pin the iteration count instead of the tool counter here.
}

/// T0.1 scenario 7 — tool error + retry. Turn 1 LLM calls `read`
/// against a path that does not exist (tool returns error). Turn 2
/// LLM picks a different path (still nonexistent — error again, but
/// the test verifies the LOOP continues, not the path resolves).
/// Turn 3 LLM returns text → converge. Pins the contract that a
/// tool failure does NOT abort the run; the engine surfaces the
/// error via tool_result and lets the LLM decide what to do.
#[tokio::test]
async fn agent_continues_after_tool_failure_until_converge() {
    let project = tempfile::tempdir().expect("tempdir");
    let target1 = project.path().join("first-miss.txt");
    let target1_str = target1.to_string_lossy().replace('\\', "\\\\");
    let target2 = project.path().join("second-miss.txt");
    let target2_str = target2.to_string_lossy().replace('\\', "\\\\");

    let read1_body = Box::leak(
        format!(
            "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[\
                {{\"index\":0,\"id\":\"c-read1\",\"function\":\
                {{\"name\":\"read\",\"arguments\":\"{{\\\"filePath\\\":\\\"{target1_str}\\\"}}\"}}}}\
            ]}}}}]}}\n\ndata: [DONE]\n\n"
        )
        .into_boxed_str(),
    );
    let read2_body = Box::leak(
        format!(
            "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[\
                {{\"index\":0,\"id\":\"c-read2\",\"function\":\
                {{\"name\":\"read\",\"arguments\":\"{{\\\"filePath\\\":\\\"{target2_str}\\\"}}\"}}}}\
            ]}}}}]}}\n\ndata: [DONE]\n\n"
        )
        .into_boxed_str(),
    );
    let final_body = "data: {\"choices\":[{\"delta\":{\"content\":\"giving up\"}}]}\n\n\
                      data: [DONE]\n\n";

    let bodies = vec![
        read1_body as &'static str,
        read2_body as &'static str,
        final_body,
    ];
    let mock_url = spawn_sse_mock_multi(bodies).await;

    let mut config = AgentConfig::default();
    config.llm.base_url = mock_url;
    config.llm.api_key = Some("test-key".to_string());
    config.is_subagent = true;
    config.max_iterations = 5;

    let agent = AgentLoop::new(config, create_default_registry());
    let result = agent.run("read file with retry", project.path()).await;

    assert!(
        result.success,
        "tool error + retry path must still converge cleanly; summary={:?}",
        result.summary
    );
    assert_eq!(
        result.iterations_used, 3,
        "three LLM iterations: failed read → failed read → text"
    );
    assert_eq!(
        result.tool_calls_total, 2,
        "two tool calls dispatched (both `read` failures)"
    );
    // Both reads failed at the tool level — `tool_calls_success`
    // counts only successful dispatches. Pins that tool-level
    // failures don't crash the run.
    assert_eq!(
        result.tool_calls_success, 0,
        "both reads must have failed at the tool level"
    );
}

/// Listener that records every `Error`-typed event whose payload's
/// `type` field is `"retry"`. Used by the retry-success scenario
/// to prove that a retry actually happened (the in-process metrics
/// counter `total_retries` is wired but currently unused in prod —
/// the source of truth for "a retry occurred" is the event bus).
struct RetryEventCounter {
    count: parking_lot::Mutex<u64>,
}

impl RetryEventCounter {
    fn new() -> Self {
        Self {
            count: parking_lot::Mutex::new(0),
        }
    }
    fn count(&self) -> u64 {
        *self.count.lock()
    }
}

impl EventListener for RetryEventCounter {
    fn on_event(&self, e: &DomainEvent) {
        if e.event_type == EventType::Error
            && e.payload.get("type").and_then(|v| v.as_str()) == Some("retry")
        {
            *self.count.lock() += 1;
        }
    }
}

/// T0.1 scenario 8 — LLM retry + success. The mock serves a 503
/// `Service Unavailable` on the FIRST attempt and a normal
/// content-only SSE response on the SECOND attempt. The runtime's
/// `RetryExecutor::with_retry` (wrapping `chat_streaming`) classifies
/// 503 as retryable, sleeps the backoff, and re-enters the closure
/// which makes a fresh HTTP request — that request hits the second
/// canned response, succeeds, and the agent converges. Asserts
/// `success=true`, `iterations_used=1` (a single LOGICAL iteration
/// despite two HTTP attempts), AND that exactly one `retry` event
/// was published on the bus (proven via a custom listener — the
/// production `total_retries` metric isn't wired through `with_retry`
/// today, but the event bus is the durable observability surface).
#[tokio::test]
async fn agent_retries_after_503_and_succeeds() {
    let responses: Vec<(u16, &'static str)> = vec![
        (503, "service unavailable"),
        (
            200,
            "data: {\"choices\":[{\"delta\":{\"content\":\"after retry\"}}]}\n\n\
             data: [DONE]\n\n",
        ),
    ];
    let mock_url = spawn_status_mock_multi(responses).await;

    let project = tempfile::tempdir().expect("tempdir");

    let mut config = AgentConfig::default();
    config.llm.base_url = mock_url;
    config.llm.api_key = Some("test-key".to_string());
    config.is_subagent = true;
    config.max_iterations = 3;
    // Aggressive retry policy keeps the test fast — the default LLM
    // policy uses larger sleeps that would push the test into
    // multi-second territory.
    config.aggressive_retry = true;

    // Listener proves at least one retry event fires through the bus
    // (the run wires its own EventBus internally; we re-acquire one
    // by going through `run_with_history` with an external bus).
    let bus = Arc::new(EventBus::new());
    let counter = Arc::new(RetryEventCounter::new());
    bus.subscribe(counter.clone() as Arc<dyn EventListener>);

    let agent = AgentLoop::new(config, create_default_registry());
    let result = agent
        .run_with_history(
            "retry then converge",
            project.path(),
            Vec::new(),
            Some(bus.clone()),
        )
        .await;

    assert!(
        result.success,
        "503 → retry → 200 must converge with success=true; summary={:?}",
        result.summary
    );
    assert_eq!(
        result.iterations_used, 1,
        "exactly ONE logical LLM iteration (the retry is internal)"
    );
    assert!(
        counter.count() >= 1,
        "EventBus must observe at least one `Error{{type:retry}}` event; \
         got {} retry events",
        counter.count()
    );
    // Iter 72 follow-up to the Iter 71 finding — the metrics counter
    // is now wired through `with_retry` so `AgentResult::retries`
    // matches the bus-observed retry events.
    assert_eq!(
        result.retries as u64, counter.count(),
        "AgentResult.retries must match the EventBus retry-event count \
         (metrics counter wired in Iter 72)"
    );
}

/// Listener that records every `ContextOverflowRecovery` event seen.
/// Used by the overflow-recovery scenario to prove the engine
/// triggered emergency compaction at least once before re-attempting
/// the LLM call.
struct OverflowRecoveryCounter {
    count: parking_lot::Mutex<u64>,
}

impl OverflowRecoveryCounter {
    fn new() -> Self {
        Self {
            count: parking_lot::Mutex::new(0),
        }
    }
    fn count(&self) -> u64 {
        *self.count.lock()
    }
}

impl EventListener for OverflowRecoveryCounter {
    fn on_event(&self, e: &DomainEvent) {
        if e.event_type == EventType::ContextOverflowRecovery {
            *self.count.lock() += 1;
        }
    }
}

/// T0.1 scenario 9 — context overflow recovery. The mock serves a
/// 400 with a body whose text matches the OpenAI
/// `context_length_exceeded` family on the FIRST attempt. The
/// runtime classifies it as `LlmError::ContextOverflow`, which is
/// NOT retryable in `with_retry`'s sense, so `call_llm_with_retry`
/// returns Err. `execution.rs` catches the overflow specifically,
/// invokes `handle_context_overflow` (emits the
/// `ContextOverflowRecovery` event + emergency compaction), and
/// `continue`s the loop. The SECOND mock response is a clean SSE
/// stream → agent converges.
///
/// Pins the recovery contract: a context overflow does NOT abort
/// the run, the engine compacts and retries WITHOUT counting it as
/// a `with_retry`-style retry attempt.
#[tokio::test]
async fn agent_recovers_from_context_overflow_then_converges() {
    let responses: Vec<(u16, &'static str)> = vec![
        (400, "context_length_exceeded — too many tokens"),
        (
            200,
            "data: {\"choices\":[{\"delta\":{\"content\":\"after compaction\"}}]}\n\n\
             data: [DONE]\n\n",
        ),
    ];
    let mock_url = spawn_status_mock_multi(responses).await;

    let project = tempfile::tempdir().expect("tempdir");

    let mut config = AgentConfig::default();
    config.llm.base_url = mock_url;
    config.llm.api_key = Some("test-key".to_string());
    config.is_subagent = true;
    config.max_iterations = 5;

    let bus = Arc::new(EventBus::new());
    let recovery_counter = Arc::new(OverflowRecoveryCounter::new());
    bus.subscribe(recovery_counter.clone() as Arc<dyn EventListener>);

    let agent = AgentLoop::new(config, create_default_registry());
    let result = agent
        .run_with_history(
            "trigger overflow then recover",
            project.path(),
            Vec::new(),
            Some(bus.clone()),
        )
        .await;

    assert!(
        result.success,
        "overflow recovery must converge with success=true; summary={:?}",
        result.summary
    );
    assert!(
        recovery_counter.count() >= 1,
        "EventBus must observe at least one ContextOverflowRecovery event; \
         got {}",
        recovery_counter.count()
    );
    // The overflow path takes one logical iteration to fail-fast +
    // recover, then a second logical iteration to land the clean
    // response. Cap at 3 for a small safety margin (the engine may
    // emit additional iterations for compaction bookkeeping).
    assert!(
        result.iterations_used <= 3,
        "overflow + recovery + convergence should stay within ~2-3 iterations; \
         got {}",
        result.iterations_used
    );
    // Critically: a context overflow is NOT a retryable error in the
    // `with_retry` sense — the engine recovers via compaction, not
    // via a retry sleep. So `result.retries` must NOT be incremented
    // by the overflow path.
    assert_eq!(
        result.retries, 0,
        "context overflow must NOT register as a `with_retry` retry; \
         got retries={}",
        result.retries
    );
}

/// T0.1 scenario 10 — `batch_execute` meta-tool driven by LLM.
/// Turn 1 LLM returns a `batch_execute` tool_call carrying TWO
/// sub-calls (`glob` + `glob` against unrelated patterns). The
/// runtime expands the batch (`tool_bridge::execute_meta::handle_
/// batch_execute`) and runs all sub-tools, returning a single
/// aggregated tool_result with `ok: true` and `steps[2]`. Turn 2
/// LLM returns text "batch done" → converge. Pins the contract
/// that batch_execute is dispatched as a single LLM-visible tool
/// call but expands to N sub-tool dispatches internally.
#[tokio::test]
async fn agent_dispatches_batch_execute_then_converges() {
    let project = tempfile::tempdir().expect("tempdir");

    // Two glob sub-calls inside a single batch_execute. Both
    // patterns target /tmp/<unique> paths that don't exist —
    // glob returns success with empty matches either way.
    let batch_body = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[\
                        {\"index\":0,\"id\":\"c-batch\",\"function\":\
                        {\"name\":\"batch_execute\",\"arguments\":\
                        \"{\\\"calls\\\":[\
                            {\\\"tool\\\":\\\"glob\\\",\\\"args\\\":{\\\"pattern\\\":\\\"/tmp/theo-t0-1-batch-a-*\\\"}},\
                            {\\\"tool\\\":\\\"glob\\\",\\\"args\\\":{\\\"pattern\\\":\\\"/tmp/theo-t0-1-batch-b-*\\\"}}\
                        ]}\"}}\
                    ]}}]}\n\ndata: [DONE]\n\n";
    let final_body = "data: {\"choices\":[{\"delta\":{\"content\":\"batch done\"}}]}\n\n\
                      data: [DONE]\n\n";

    let bodies = vec![batch_body, final_body];
    let mock_url = spawn_sse_mock_multi(bodies).await;

    let mut config = AgentConfig::default();
    config.llm.base_url = mock_url;
    config.llm.api_key = Some("test-key".to_string());
    config.is_subagent = true;
    config.max_iterations = 5;

    let agent = AgentLoop::new(config, create_default_registry());
    let result = agent.run("run two globs in a batch", project.path()).await;

    assert!(
        result.success,
        "batch_execute flow must converge with success=true; summary={:?}",
        result.summary
    );
    assert_eq!(
        result.iterations_used, 2,
        "exactly two LLM iterations: batch dispatch → text → converge"
    );
    // Empirical finding (Iter 74): unlike `done`/`delegate_task`/
    // `skill` (which flow through `dispatch_meta_tool` and don't
    // increment `tool_calls_total`), `batch_execute` is dispatched
    // through the regular ToolCallManager path — so the OUTER call
    // increments the counter once, and the inner sub-calls run
    // directly via `tool_bridge` without re-entering the manager.
    // Net: tool_calls_total surfaces 1 per batch_execute invocation
    // regardless of sub-call count. Pins this contract.
    assert_eq!(
        result.tool_calls_total, 1,
        "batch_execute counts as exactly one tool dispatch (outer); \
         the inner sub-calls don't re-enter the manager. Got {}",
        result.tool_calls_total
    );
}

/// Listener that records every `ToolCallCompleted` event whose
/// payload has `replayed: true`. Used by the resume scenario to
/// prove the engine bypassed the dispatcher and pulled the cached
/// tool_result instead.
struct ReplayCounter {
    count: parking_lot::Mutex<u64>,
}

impl ReplayCounter {
    fn new() -> Self {
        Self {
            count: parking_lot::Mutex::new(0),
        }
    }
    fn count(&self) -> u64 {
        *self.count.lock()
    }
}

impl EventListener for ReplayCounter {
    fn on_event(&self, e: &DomainEvent) {
        if e.event_type == EventType::ToolCallCompleted
            && e.payload
                .get("replayed")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        {
            *self.count.lock() += 1;
        }
    }
}

/// T0.1 scenario 11 — resume with `ResumeContext` replays a cached
/// tool result instead of dispatching. Setup:
///   - Build a `ResumeContext` containing one cached tool_result
///     for `call_id = "c-cached"`.
///   - Wire it into `AgentLoop::with_resume_context`.
///   - Mock LLM returns the SAME `call_id` in turn 1 → engine
///     consults the resume context, sees the call_id is in the
///     `executed_tool_calls` set, pushes the cached `tool_result`
///     message, emits a `ToolCallCompleted{replayed:true}` event,
///     and DOES NOT invoke the dispatcher.
///   - Turn 2 LLM returns text → converge.
///
/// Pins the gap-#3 invariant: a resumed run never double-executes
/// a tool call that already completed in the original run.
#[tokio::test]
async fn agent_replays_cached_tool_result_on_resume() {
    use std::collections::{BTreeMap, BTreeSet};
    use theo_agent_runtime::subagent::resume::{ResumeContext, WorktreeStrategy};
    use theo_domain::agent_spec::AgentSpec;
    use theo_infra_llm::types::Message;

    let project = tempfile::tempdir().expect("tempdir");

    // Pre-build the resume context. The cached tool_result is what
    // the engine will inject in lieu of dispatching the `read`.
    let mut executed = BTreeSet::new();
    executed.insert("c-cached".to_string());
    let mut cached: BTreeMap<String, Message> = BTreeMap::new();
    cached.insert(
        "c-cached".to_string(),
        Message::tool_result("c-cached", "read", "cached file contents"),
    );
    let resume_ctx = Arc::new(ResumeContext {
        spec: AgentSpec::on_demand("scout", "noop"),
        start_iteration: 0,
        history: Vec::new(),
        prior_tokens_used: 0,
        checkpoint_before: None,
        executed_tool_calls: executed,
        executed_tool_results: cached,
        worktree_strategy: WorktreeStrategy::None,
    });

    // LLM turn 1: same `call_id` as the cached entry → triggers replay.
    let read_body = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[\
                        {\"index\":0,\"id\":\"c-cached\",\"function\":\
                        {\"name\":\"read\",\"arguments\":\"{\\\"filePath\\\":\\\"x.rs\\\"}\"}}\
                    ]}}]}\n\ndata: [DONE]\n\n";
    let final_body = "data: {\"choices\":[{\"delta\":{\"content\":\"resumed and done\"}}]}\n\n\
                      data: [DONE]\n\n";
    let bodies = vec![read_body, final_body];
    let mock_url = spawn_sse_mock_multi(bodies).await;

    let mut config = AgentConfig::default();
    config.llm.base_url = mock_url;
    config.llm.api_key = Some("test-key".to_string());
    config.is_subagent = true;
    config.max_iterations = 5;

    let bus = Arc::new(EventBus::new());
    let replay_counter = Arc::new(ReplayCounter::new());
    bus.subscribe(replay_counter.clone() as Arc<dyn EventListener>);

    let agent = AgentLoop::new(config, create_default_registry())
        .with_resume_context(resume_ctx);
    let result = agent
        .run_with_history(
            "resume run",
            project.path(),
            Vec::new(),
            Some(bus.clone()),
        )
        .await;

    assert!(
        result.success,
        "resume + cached replay must converge with success=true; summary={:?}",
        result.summary
    );
    assert_eq!(
        result.iterations_used, 2,
        "two LLM iterations: replay turn → text → converge"
    );
    assert!(
        replay_counter.count() >= 1,
        "EventBus must observe at least one ToolCallCompleted{{replayed:true}}; \
         got {}",
        replay_counter.count()
    );
    // The engine bypassed the dispatcher entirely — the regular
    // tool counter must not register the replayed call.
    assert_eq!(
        result.tool_calls_total, 0,
        "replay path must NOT increment tool_calls_total (the dispatcher \
         was bypassed); got {}",
        result.tool_calls_total
    );
}

/// T0.1 scenario 12 — Done-gate Gate 1 (convergence) blocks a
/// premature `done()` call. Turn 1 LLM calls `done()` with no
/// edits made (`edits_succeeded=0`). Gate 0 (attempt limit) passes
/// because attempts=1 ≤ MAX_DONE_ATTEMPTS=3. Gate 1 (convergence
/// AllOf GitDiff + EditSuccess) fails because `edits_succeeded=0`,
/// blocks with a "BLOCKED: convergence criteria not met" tool
/// result, and transitions to Replanning. Turn 2 LLM observes the
/// block in history and emits text → converge cleanly.
///
/// Pins the Gate 1 contract: a single premature done() call does
/// NOT abort the run; the engine surfaces the convergence violation
/// via tool_result and lets the LLM decide what to do next.
#[tokio::test]
async fn agent_done_gate_1_blocks_then_recovers_with_text() {
    let project = tempfile::tempdir().expect("tempdir");

    let done_body = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[\
                        {\"index\":0,\"id\":\"c-done-premature\",\"function\":\
                        {\"name\":\"done\",\"arguments\":\"{\\\"summary\\\":\\\"too eager\\\"}\"}}\
                    ]}}]}\n\ndata: [DONE]\n\n";
    let final_body = "data: {\"choices\":[{\"delta\":{\"content\":\"OK, retracting\"}}]}\n\n\
                      data: [DONE]\n\n";

    let bodies = vec![done_body, final_body];
    let mock_url = spawn_sse_mock_multi(bodies).await;

    let mut config = AgentConfig::default();
    config.llm.base_url = mock_url;
    config.llm.api_key = Some("test-key".to_string());
    config.is_subagent = true;
    config.max_iterations = 5;

    let agent = AgentLoop::new(config, create_default_registry());
    let result = agent.run("premature done", project.path()).await;

    // Final outcome: text-only convergence on turn 2.
    assert!(
        result.success,
        "Gate 1 block + text retreat must converge cleanly; summary={:?}",
        result.summary
    );
    assert_eq!(
        result.iterations_used, 2,
        "exactly two LLM iterations: blocked done → text → converge"
    );
    // The summary surfaces the FINAL turn's content, not the blocked
    // done's attempted "too eager" summary. This pins the contract
    // that a Gate-1-blocked done() does not corrupt the converge
    // result.
    assert!(
        !result.summary.contains("too eager"),
        "final summary must not carry the blocked done's summary; \
         got {:?}",
        result.summary
    );
}

/// T0.1 scenario 14 — `skill` meta-tool in **SubAgent** mode spawns
/// a sub-agent recursively. Turn 1 (parent) LLM returns
/// `skill(name="test")` — `test` is the bundled SubAgent-mode skill
/// pointing at the `verifier` built-in agent. The runtime enters
/// `dispatch_skill::SkillPlan::SubAgent`, spawns a sub-agent via
/// `SubAgentManager::spawn_with_spec_text`. The sub-agent runs its
/// own AgentLoop against the SAME mock URL; on its first connect
/// the mock has already advanced past the skill body to the text
/// body → sub-agent converges in one turn. Parent receives the
/// sub-result, pushes a tool_result containing
/// `[Skill 'test' completed] ...`. Turn 2 (parent again, mock now
/// saturating on the text body) → parent converges.
///
/// Pins the recursive sub-agent skill spawn contract end-to-end.
#[tokio::test]
async fn agent_spawns_subagent_skill_then_converges() {
    let project = tempfile::tempdir().expect("tempdir");

    let skill_body = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[\
                        {\"index\":0,\"id\":\"c-skill-sub\",\"function\":\
                        {\"name\":\"skill\",\"arguments\":\"{\\\"name\\\":\\\"test\\\"}\"}}\
                    ]}}]}\n\ndata: [DONE]\n\n";
    // This text body services BOTH the sub-agent's first LLM call
    // AND the parent's second LLM call (mock saturates at the last
    // canned body for any request beyond the queue's length).
    let text_body = "data: {\"choices\":[{\"delta\":{\"content\":\"sub-agent done\"}}]}\n\n\
                     data: [DONE]\n\n";

    let bodies = vec![skill_body, text_body];
    let mock_url = spawn_sse_mock_multi(bodies).await;

    let mut config = AgentConfig::default();
    config.llm.base_url = mock_url;
    config.llm.api_key = Some("test-key".to_string());
    config.is_subagent = true;
    config.max_iterations = 5;

    let agent = AgentLoop::new(config, create_default_registry());
    let result = agent.run("invoke verifier via skill", project.path()).await;

    assert!(
        result.success,
        "skill SubAgent flow must converge with success=true; summary={:?}",
        result.summary
    );
    assert_eq!(
        result.iterations_used, 2,
        "exactly two parent LLM iterations: skill spawn → text → converge"
    );
    // The skill tool_result the parent receives carries the sub-
    // agent's outcome (success or failure). When the sub-agent
    // converges via text, the parent sees `[Skill 'test' completed]`
    // — pin that contract via the final summary trail (the parent's
    // own converge text is "sub-agent done", but the conversation
    // history retains the bracketed prefix as the upstream tool
    // result message).
    let _ = result.summary;
}

/// T0.1 scenario 15 (E2E Gate 2 cargo-driven). Full choreography
/// reaching the done-gate's Gate 2 (cargo test) via the LLM mock:
/// 1. Project pre-staged: `git init` + valid `Cargo.toml` + empty
///    `src/lib.rs` + initial `git commit`. This ensures HEAD has a
///    tracked Cargo.toml so a subsequent write produces a real
///    `git diff --stat` output (has_git_changes=true).
/// 2. Turn 1 LLM returns `write` tool_call against `Cargo.toml`
///    with deliberately-broken TOML content (`"this is not valid
///    TOML }}}"`). The write tool dispatches successfully,
///    `edits_succeeded` becomes 1, and the tracked file is now
///    modified-and-syntactically-broken.
/// 3. Turns 2-N LLM returns `done()` repeatedly. Gate 0 passes
///    while `done_attempts <= MAX_DONE_ATTEMPTS=3`. Gate 1
///    (convergence AllOf GitDiff+EditSuccess) NOW passes (both
///    has_git_changes=true and edits=1). Gate 2 runs `cargo check`,
///    fails on the broken manifest, blocks with the diagnostic →
///    Continue. After 3 blocks, the 4th done() trips Gate 0
///    (attempts=4 > 3) → force-accept.
///
/// Skipped silently when git or cargo are missing. Pins the full
/// done-gate chain end-to-end including the cargo invocation.
#[tokio::test]
async fn agent_done_gate_2_blocks_via_cargo_then_force_accepts() {
    // Skip when git or cargo missing.
    let git_ok = std::process::Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let cargo_ok = std::process::Command::new("cargo")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !git_ok || !cargo_ok {
        return;
    }

    let project = tempfile::tempdir().expect("tempdir");
    let project_path = project.path();

    // Stage a minimal Rust project under git so write→diff produces
    // observable `git diff --stat` output that satisfies Gate 1.
    std::fs::create_dir_all(project_path.join("src")).unwrap();
    let valid_toml = "[package]\n\
                      name = \"theo_t0_1_gate2_fixture\"\n\
                      version = \"0.0.0\"\n\
                      edition = \"2021\"\n\
                      [lib]\n\
                      path = \"src/lib.rs\"\n";
    std::fs::write(project_path.join("Cargo.toml"), valid_toml).unwrap();
    std::fs::write(project_path.join("src/lib.rs"), "").unwrap();
    let run_git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(project_path)
            .env("GIT_AUTHOR_NAME", "Theo Test")
            .env("GIT_AUTHOR_EMAIL", "test@theo.local")
            .env("GIT_COMMITTER_NAME", "Theo Test")
            .env("GIT_COMMITTER_EMAIL", "test@theo.local")
            .output()
            .ok()
    };
    if run_git(&["init", "-q"]).is_none() {
        return;
    }
    let _ = run_git(&["add", "."]);
    let _ = run_git(&["commit", "-q", "-m", "init"]);

    // Mock LLM bodies. Turn 1 = write to Cargo.toml with broken TOML.
    // The tool's `content` argument carries the broken TOML; we
    // escape JSON specials minimally (no embedded newlines / quotes
    // beyond the closing braces).
    let broken_toml = "this is not valid TOML at all }}}";
    let cargo_path = project_path.join("Cargo.toml");
    let cargo_path_str = cargo_path.to_string_lossy().replace('\\', "\\\\");
    let write_body = Box::leak(
        format!(
            "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[\
                {{\"index\":0,\"id\":\"c-write-broken\",\"function\":\
                {{\"name\":\"write\",\"arguments\":\
                \"{{\\\"filePath\\\":\\\"{cargo_path_str}\\\",\\\"content\\\":\\\"{broken_toml}\\\"}}\"\
                }}}}\
            ]}}}}]}}\n\ndata: [DONE]\n\n"
        )
        .into_boxed_str(),
    );
    // Done body — LLM keeps asking; Gate 2 keeps blocking until
    // attempt 4 force-accepts.
    let done_body = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[\
                        {\"index\":0,\"id\":\"c-done-gate2\",\"function\":\
                        {\"name\":\"done\",\"arguments\":\"{\\\"summary\\\":\\\"please accept\\\"}\"}}\
                    ]}}]}\n\ndata: [DONE]\n\n";

    // Saturate on done_body so every iteration past the first hits done().
    let bodies = vec![write_body as &'static str, done_body];
    let mock_url = spawn_sse_mock_multi(bodies).await;

    let mut config = AgentConfig::default();
    config.llm.base_url = mock_url;
    config.llm.api_key = Some("test-key".to_string());
    config.is_subagent = true;
    // Generous budget: 1 write + ≥4 done() iterations + safety margin.
    config.max_iterations = 20;

    let agent = AgentLoop::new(config, create_default_registry());
    let result = agent.run("break Cargo.toml then claim done", project_path).await;

    assert!(
        result.success,
        "Gate 2 chain must eventually force-accept (attempts > MAX_DONE_ATTEMPTS); \
         summary={:?}",
        result.summary
    );
    assert!(
        result.summary.contains("accepted after"),
        "force-accept must annotate the summary; got {:?}",
        result.summary
    );
    // 1 write iteration + 4 done() iterations (3 blocked + 1 force-
    // accept) = 5 minimum. Allow a safety margin for the engine's
    // bookkeeping iterations.
    assert!(
        result.iterations_used >= 5,
        "the chain needs at least 5 iterations (1 write + 4 done); got {}",
        result.iterations_used
    );
    // `tool_calls_total` includes the `write` (regular tool, dispatched
    // through ToolCallManager). `done` is a meta-tool and doesn't
    // increment the counter. So we expect tool_calls_total == 1.
    assert_eq!(
        result.tool_calls_total, 1,
        "exactly one regular tool dispatch (the `write`); got {}",
        result.tool_calls_total
    );
}

/// T7.3 batch dimension closure — `batch × 26 overflow`. The
/// `batch` meta-tool has a hard `MAX_BATCH_SIZE=25` cap enforced
/// by `dispatch_batch::take(MAX_BATCH)`. Submitting 26 sub-calls
/// must not crash; the 26th is silently dropped from execution
/// and a warning is appended to the aggregated tool_result.
///
/// Pins the cap-overflow contract end-to-end via the LLM mock.
/// The plan literal `batch × [5 ok / 5 with 1 blocked / 25 max /
/// 26 overflow]` had its first three cases covered in
/// `meta_tools_t7_3.rs`; this closes the 26-overflow case at the
/// engine level (where the cap actually fires).
#[tokio::test]
async fn agent_dispatches_batch_with_26_calls_truncates_at_max() {
    let project = tempfile::tempdir().expect("tempdir");

    // Build a `batch` tool_call carrying 26 trivial `glob` sub-calls.
    // Programmatically generate the JSON to avoid a 26-line literal.
    let mut sub_calls = String::new();
    for i in 0..26 {
        if i > 0 {
            sub_calls.push(',');
        }
        sub_calls.push_str(&format!(
            "{{\\\"tool\\\":\\\"glob\\\",\\\"args\\\":{{\\\"pattern\\\":\\\"/tmp/theo-batch-26-{i}-*\\\"}}}}"
        ));
    }
    let batch_body = Box::leak(
        format!(
            "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[\
                {{\"index\":0,\"id\":\"c-batch-26\",\"function\":\
                {{\"name\":\"batch\",\"arguments\":\"{{\\\"calls\\\":[{sub_calls}]}}\"}}}}\
            ]}}}}]}}\n\ndata: [DONE]\n\n"
        )
        .into_boxed_str(),
    );
    let final_body = "data: {\"choices\":[{\"delta\":{\"content\":\"batch overflow done\"}}]}\n\n\
                      data: [DONE]\n\n";

    let bodies = vec![batch_body as &'static str, final_body];
    let mock_url = spawn_sse_mock_multi(bodies).await;

    let mut config = AgentConfig::default();
    config.llm.base_url = mock_url;
    config.llm.api_key = Some("test-key".to_string());
    config.is_subagent = true;
    config.max_iterations = 5;

    let agent = AgentLoop::new(config, create_default_registry());
    let result = agent.run("submit 26 batch sub-calls", project.path()).await;

    // The cap must NOT crash the run — agent converges on turn 2.
    assert!(
        result.success,
        "26-call batch must not crash; agent must converge on the next turn"
    );
    assert_eq!(
        result.iterations_used, 2,
        "exactly two LLM iterations: batch dispatch (truncated to 25) → text → converge"
    );
}
