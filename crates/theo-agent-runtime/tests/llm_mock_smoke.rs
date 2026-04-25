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
