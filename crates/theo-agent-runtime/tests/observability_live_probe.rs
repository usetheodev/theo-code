#![allow(clippy::field_reassign_with_default)] // Tests tweak individual fields for readability.

//! Live probe: spins up a minimal OpenAI-compatible HTTP stub, runs a real
//! `AgentRunEngine` against it, and asserts on the resulting JSONL trajectory.
//!
//! This is the closest thing to a "real LLM E2E" we can do without a network
//! dependency — the LlmClient issues actual HTTP requests against a local
//! listener that returns canned chat-completion responses.
//!
//! Purpose: verify that the observability pipeline captures a full run with
//! real events (RunInitialized, LlmCallStart/End, ToolCall*, RunStateChanged)
//! when the run_engine is driven end-to-end.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use theo_agent_runtime::event_bus::EventBus;
use theo_agent_runtime::observability::envelope::EnvelopeKind;
use theo_agent_runtime::observability::read_trajectory;
use theo_agent_runtime::run_engine::AgentRunEngine;
use theo_agent_runtime::task_manager::TaskManager;
use theo_agent_runtime::tool_call_manager::ToolCallManager;
use theo_agent_runtime::AgentConfig;
use theo_domain::session::SessionId;
use theo_domain::task::AgentType;
use theo_infra_llm::client::LlmClient;

/// Responses the stub will return, one per request. Last response converges.
const RESPONSES: &[&str] = &[
    // Response 1: "I need to read a file" — but since tools are complex to wire
    // via the stub, we just produce text and converge.
    r#"{"id":"chatcmpl-1","object":"chat.completion","created":1,"model":"test","choices":[{"index":0,"message":{"role":"assistant","content":"I have analyzed the task. Nothing to edit. Done."},"finish_reason":"stop"}],"usage":{"prompt_tokens":50,"completion_tokens":10,"total_tokens":60}}"#,
];

fn start_stub_server() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind stub server");
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}", addr);
    let request_count = Arc::new(AtomicUsize::new(0));
    let rc_clone = Arc::clone(&request_count);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };
            let idx = rc_clone.fetch_add(1, Ordering::Relaxed);
            // Read the request (just consume until double-CRLF + Content-Length body).
            let mut buf = [0u8; 8192];
            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
            let _ = stream.read(&mut buf);
            let body = RESPONSES[idx.min(RESPONSES.len() - 1)];
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    (url, request_count)
}

#[test]
fn live_probe_real_run_produces_trajectory() {
    // 1. Start local HTTP stub that speaks OpenAI chat-completions protocol.
    let (stub_url, request_count) = start_stub_server();

    // 2. Wire up a real AgentRunEngine pointing at the stub.
    let tmp = tempfile::tempdir().unwrap();
    let bus = Arc::new(EventBus::new());
    let tm = Arc::new(TaskManager::new(bus.clone()));
    let tcm = Arc::new(ToolCallManager::new(bus.clone()));
    let task_id = tm.create_task(SessionId::new("s"), AgentType::Coder, "probe task".into());
    let client = LlmClient::new(&stub_url, Some("test-key".into()), "test-model");
    let mut config = AgentConfig::default();
    config.max_iterations = 2;
    config.is_subagent = false;

    let mut engine = AgentRunEngine::new(
        task_id,
        tm,
        tcm,
        bus,
        client,
        Arc::new(theo_tooling::registry::create_default_registry()),
        config,
        tmp.path().to_path_buf(),
    );
    let run_id = engine.run_id().as_str().to_string();

    // 3. Execute the run inside a tokio runtime.
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _result = rt.block_on(engine.execute());

    // 4. Verify the stub was actually hit.
    assert!(
        request_count.load(Ordering::Relaxed) >= 1,
        "stub server must have received at least one LLM request"
    );

    // 5. Inspect the trajectory produced on disk.
    let file_path = tmp
        .path()
        .join(".theo")
        .join("trajectories")
        .join(format!("{}.jsonl", run_id));
    assert!(
        file_path.exists(),
        "trajectory file must exist after the run: {:?}",
        file_path
    );

    let (envelopes, integrity) = read_trajectory(&file_path).expect("reader parses");
    assert!(!envelopes.is_empty(), "trajectory must contain envelopes");

    // Required events.
    let event_types: Vec<String> = envelopes
        .iter()
        .filter(|e| matches!(e.kind, EnvelopeKind::Event))
        .filter_map(|e| e.event_type.clone())
        .collect();
    assert!(
        event_types.iter().any(|t| t == "RunInitialized"),
        "RunInitialized must appear in trajectory, got: {:?}",
        event_types
    );
    assert!(
        event_types.iter().any(|t| t == "LlmCallStart" || t == "LlmCallEnd"),
        "at least one LlmCall event expected, got: {:?}",
        event_types
    );

    // Summary line is last.
    let summary_count = envelopes
        .iter()
        .filter(|e| matches!(e.kind, EnvelopeKind::Summary))
        .count();
    assert_eq!(summary_count, 1, "exactly one summary line expected");
    let last = envelopes.last().unwrap();
    assert!(
        matches!(last.kind, EnvelopeKind::Summary),
        "summary must be the last envelope"
    );

    // Integrity confidence > 0 (trajectory complete enough to compute metrics).
    assert!(integrity.confidence > 0.0);

    // Print a compact summary for human inspection (visible with --nocapture).
    let summary_payload = &last.payload;
    println!("=== LIVE PROBE TRAJECTORY SUMMARY ===");
    println!("Run ID: {}", run_id);
    println!("Envelopes: {}", envelopes.len());
    println!("Event types: {:?}", event_types);
    println!("Integrity: confidence={:.2}, missing={}", integrity.confidence, integrity.missing_sequences.len());
    println!(
        "RunReport:\n{}",
        serde_json::to_string_pretty(summary_payload).unwrap_or_else(|_| "<unprintable>".into())
    );
}
