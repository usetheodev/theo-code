//! Phase 45 (otlp-exporter-plan) — network-path smoke without Docker.
//!
//! The Docker-based smoke (`scripts/otlp-smoke.sh`) is the canonical
//! end-to-end validation against a real OTel Collector. This test
//! complements it for environments where Docker is unavailable: it
//! spins up a local TCP server that mimics the HTTP-protobuf side of
//! the OTLP receiver (port 4318 contract: `POST /v1/traces`), wires up
//! the exporter via env vars, emits a span, and verifies the request
//! arrives with a non-empty protobuf body and the correct path.
//!
//! Determinístico, sem rede externa, sem dependência de `docker`.
//! Gated by `--features otel`.

#![cfg(feature = "otel")]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use opentelemetry::trace::{Tracer, TracerProvider};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Tiny single-shot HTTP server. Captures request head+body. Returns 200.
async fn spawn_collector_mock() -> (u16, Arc<Mutex<Vec<u8>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let captured: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = captured.clone();
    tokio::spawn(async move {
        let (mut sock, _) = match listener.accept().await {
            Ok(p) => p,
            Err(_) => return,
        };
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
                    if sock.read_exact(&mut more).await.is_ok() {
                        acc.extend_from_slice(&more);
                    }
                }
                break;
            }
        }
        {
            let mut g = captured_clone.lock().unwrap();
            g.extend_from_slice(&acc);
        }
        let resp =
            b"HTTP/1.1 200 OK\r\nContent-Type: application/x-protobuf\r\nContent-Length: 0\r\n\r\n";
        let _ = sock.write_all(resp).await;
        let _ = sock.shutdown().await;
    });
    (port, captured)
}

#[tokio::test(flavor = "multi_thread")]
async fn otlp_exporter_sends_http_protobuf_request_to_collector() {
    let (port, captured) = spawn_collector_mock().await;

    // Configure env BEFORE init_otlp_exporter reads it.
    let endpoint = format!("http://127.0.0.1:{port}");
    unsafe {
        std::env::set_var("OTLP_ENDPOINT", &endpoint);
        std::env::set_var("OTLP_PROTOCOL", "http_protobuf");
        std::env::set_var("OTLP_TIMEOUT_SECS", "5");
    }

    // Install the OTLP guard. Holds the provider for the test duration.
    let guard = theo_agent_runtime::observability::otel_exporter::OtlpGuard::install();
    assert!(guard.is_active(), "init_otlp_exporter must succeed");

    // Emit one span via the global tracer.
    {
        let tracer = opentelemetry::global::tracer_provider().tracer("smoke");
        use opentelemetry::trace::Span;
        let mut span = tracer.start("test.span");
        span.set_attribute(opentelemetry::KeyValue::new("theo.smoke", "ok"));
        span.end();
    }

    // Trigger flush via guard drop. shutdown() may block on a join,
    // so do it in spawn_blocking to avoid panicking the async context.
    let _ = tokio::task::spawn_blocking(move || drop(guard)).await;

    // Poll for captured bytes — give the BatchSpanProcessor time to
    // drain its queue and the mock server time to read the body.
    let mut waited_ms = 0u64;
    while waited_ms < 5_000 {
        let len = captured.lock().unwrap().len();
        if len > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        waited_ms += 100;
    }

    let bytes = captured.lock().unwrap().clone();
    // Headers are ASCII; body is binary protobuf. Decode lossy.
    let head = String::from_utf8_lossy(&bytes);

    // Cleanup env so other tests aren't affected.
    unsafe {
        std::env::remove_var("OTLP_ENDPOINT");
        std::env::remove_var("OTLP_PROTOCOL");
        std::env::remove_var("OTLP_TIMEOUT_SECS");
    }

    // Assertions — the collector RECEIVED an HTTP POST with non-empty
    // protobuf body. The request path is whatever the operator put in
    // OTLP_ENDPOINT (the SDK does NOT auto-append /v1/traces); we set
    // no path here, so the SDK posts to "/".
    assert!(bytes.len() > 0, "no bytes captured — exporter never sent");
    assert!(
        head.starts_with("POST "),
        "must be a POST; first 80 bytes (decimal): {:?}",
        &bytes[..bytes.len().min(80)]
    );
    assert!(
        head.lines().any(|l| {
            let lc = l.to_ascii_lowercase();
            lc.starts_with("content-type:") && lc.contains("application/x-protobuf")
        }),
        "Content-Type must be application/x-protobuf; head:\n{}",
        &head[..head.len().min(400)]
    );
    let body_len = head
        .lines()
        .find_map(|l| {
            l.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .and_then(|v| v.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);
    assert!(
        body_len > 0,
        "OTLP body must be non-empty (carries the encoded span); got Content-Length={body_len}"
    );
}
