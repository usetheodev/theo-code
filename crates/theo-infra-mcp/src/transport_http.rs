//! HTTP/Streamable transport per MCP spec 2025-03-26 §Transports.
//!
//! Phase 35 (mcp-http-and-discover-flake).
//!
//! Single endpoint:
//! - `POST {url}` with `Content-Type: application/json` + JSON-RPC body
//! - Response is either:
//!     - `application/json` → single `McpResponse`
//!     - `text/event-stream` → SSE stream; first `data:` event whose JSON
//!       payload carries a matching `id` is returned
//! - Optional `Mcp-Session-Id` header captured from the response and
//!   echoed on subsequent requests (transport-scoped)
//! - Auth via static headers in `McpServerConfig::Http { headers }`
//!
//! Out of scope (per plan D3):
//! - GET endpoint for server-to-client notifications
//! - WebSocket fallback (deprecated)
//! - OAuth 2.1 manager (headers only)

use std::collections::BTreeMap;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE};

use crate::error::McpError;
use crate::protocol::{McpRequest, McpResponse};

#[derive(Debug)]
pub struct HttpTransport {
    url: String,
    client: reqwest::Client,
    extra_headers: HeaderMap,
    session_id: std::sync::Mutex<Option<String>>,
}

impl HttpTransport {
    /// Construct a new HTTP transport for the given endpoint.
    ///
    /// Errors:
    /// - `McpError::InvalidConfig` when a header name or value contains
    ///   bytes that are illegal per RFC 7230 (e.g. control chars).
    /// - `McpError::InvalidConfig` when `reqwest::Client::builder()`
    ///   fails (typically a TLS feature mismatch).
    pub fn new(
        url: impl Into<String>,
        headers: BTreeMap<String, String>,
        request_timeout: Duration,
    ) -> Result<Self, McpError> {
        let client = reqwest::Client::builder()
            .timeout(request_timeout)
            .build()
            .map_err(|e| McpError::InvalidConfig(format!("reqwest build: {e}")))?;
        let mut hm = HeaderMap::new();
        for (k, v) in headers {
            let name = HeaderName::from_bytes(k.as_bytes())
                .map_err(|e| McpError::InvalidConfig(format!("bad header name '{k}': {e}")))?;
            let val = HeaderValue::from_str(&v)
                .map_err(|e| McpError::InvalidConfig(format!("bad value for '{k}': {e}")))?;
            hm.insert(name, val);
        }
        Ok(Self {
            url: url.into(),
            client,
            extra_headers: hm,
            session_id: std::sync::Mutex::new(None),
        })
    }

    /// Send a JSON-RPC request and parse the response (single JSON or SSE).
    pub async fn request(&self, req: McpRequest) -> Result<McpResponse, McpError> {
        let body = serde_json::to_vec(&req)?;
        let mut builder = self
            .client
            .post(&self.url)
            .header(CONTENT_TYPE, "application/json")
            .header("Accept", "application/json, text/event-stream");
        for (k, v) in self.extra_headers.iter() {
            builder = builder.header(k, v);
        }
        // Echo Mcp-Session-Id when we have one captured from a prior call.
        let staged_session = self
            .session_id
            .lock()
            .ok()
            .and_then(|g| g.as_ref().cloned());
        if let Some(s) = staged_session {
            builder = builder.header("Mcp-Session-Id", s);
        }
        let resp = builder
            .body(body)
            .send()
            .await
            .map_err(|e| McpError::Io(std::io::Error::other(format!("http: {e}"))))?;

        // Capture Mcp-Session-Id from the response (initialize / first call).
        if let Some(s) = resp
            .headers()
            .get("Mcp-Session-Id")
            .and_then(|v| v.to_str().ok())
            .map(String::from)
            && let Ok(mut g) = self.session_id.lock()
        {
            *g = Some(s);
        }

        let ct = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();

        if ct.contains("text/event-stream") {
            parse_sse_until_id_match(resp, &req.id).await
        } else {
            let bytes = resp
                .bytes()
                .await
                .map_err(|e| McpError::Io(std::io::Error::other(format!("body: {e}"))))?;
            let parsed: McpResponse = serde_json::from_slice(&bytes)?;
            Ok(parsed)
        }
    }
}

/// Parse a stream of SSE events, returning the first response whose
/// `id` matches `req_id`. Notifications (no `id` or `id: null`) and
/// mismatched ids are skipped per the MCP spec.
///
/// Events terminate on `\n\n` (also tolerating `\r\n\r\n`). Multiple
/// `data:` lines within a single event are joined with `\n` per RFC.
async fn parse_sse_until_id_match(
    resp: reqwest::Response,
    req_id: &serde_json::Value,
) -> Result<McpResponse, McpError> {
    use futures::StreamExt;
    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    while let Some(chunk) = stream.next().await {
        let bytes =
            chunk.map_err(|e| McpError::Io(std::io::Error::other(format!("sse chunk: {e}"))))?;
        let s = std::str::from_utf8(&bytes)
            .map_err(|e| McpError::Io(std::io::Error::other(format!("utf8: {e}"))))?;
        buffer.push_str(s);
        // Normalize CRLF to LF so the same scan handles both servers.
        buffer = buffer.replace("\r\n", "\n");
        while let Some(end) = buffer.find("\n\n") {
            let event = buffer[..end].to_string();
            buffer.drain(..end + 2);
            if let Some(parsed) = decode_sse_event(&event, req_id)? {
                return Ok(parsed);
            }
        }
    }
    Err(McpError::TransportClosed)
}

/// Decode a single SSE event (everything up to the `\n\n`) into an
/// `McpResponse` if the `id` matches. Returns `Ok(None)` for events
/// to skip (notifications, mismatched ids, empty data).
pub(crate) fn decode_sse_event(
    event: &str,
    req_id: &serde_json::Value,
) -> Result<Option<McpResponse>, McpError> {
    let payload: String = event
        .lines()
        .filter_map(|l| l.strip_prefix("data:").map(|s| s.trim_start()))
        .collect::<Vec<_>>()
        .join("\n");
    if payload.is_empty() {
        return Ok(None);
    }
    let v: serde_json::Value = serde_json::from_str(&payload)?;
    // Skip notifications (no id) and mismatched ids.
    if v.get("id").is_none() || v.get("id") == Some(&serde_json::Value::Null) {
        return Ok(None);
    }
    let resp: McpResponse = serde_json::from_value(v)?;
    if &resp.id != req_id {
        return Ok(None);
    }
    Ok(Some(resp))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req_id_one() -> serde_json::Value {
        serde_json::json!(1)
    }

    // ── HttpTransport::new validation ──

    #[test]
    fn http_transport_new_accepts_minimal_args() {
        let t = HttpTransport::new(
            "http://localhost:1",
            BTreeMap::new(),
            Duration::from_secs(5),
        );
        assert!(t.is_ok());
    }

    #[test]
    fn http_transport_new_accepts_well_formed_authorization_header() {
        let mut h = BTreeMap::new();
        h.insert("Authorization".into(), "Bearer xyz".into());
        let t = HttpTransport::new("http://x", h, Duration::from_secs(5));
        assert!(t.is_ok());
    }

    #[test]
    fn http_transport_new_rejects_invalid_header_name() {
        let mut h = BTreeMap::new();
        h.insert("Bad Header With Spaces".into(), "v".into());
        let err = HttpTransport::new("http://x", h, Duration::from_secs(5))
            .expect_err("invalid header name must be rejected");
        match err {
            McpError::InvalidConfig(msg) => assert!(msg.contains("bad header name")),
            _ => panic!("expected InvalidConfig, got {err:?}"),
        }
    }

    #[test]
    fn http_transport_new_rejects_invalid_header_value() {
        let mut h = BTreeMap::new();
        // Newline / control char in header value is illegal per RFC 7230.
        h.insert("X-Trail".into(), "bad\nvalue".into());
        let err = HttpTransport::new("http://x", h, Duration::from_secs(5))
            .expect_err("invalid header value must be rejected");
        assert!(matches!(err, McpError::InvalidConfig(_)));
    }

    // ── SSE parser (decode_sse_event) ──

    #[test]
    fn decode_sse_event_returns_response_with_matching_id() {
        let event = r#"data: {"jsonrpc":"2.0","id":1,"result":{"x":1}}"#;
        let parsed = decode_sse_event(event, &req_id_one()).unwrap();
        let r = parsed.expect("matched id must produce Some");
        assert_eq!(r.id, serde_json::json!(1));
        assert_eq!(r.result.unwrap()["x"], serde_json::json!(1));
    }

    #[test]
    fn decode_sse_event_skips_notifications_with_no_id_field() {
        let event = r#"data: {"jsonrpc":"2.0","method":"notify"}"#;
        assert!(decode_sse_event(event, &req_id_one()).unwrap().is_none());
    }

    #[test]
    fn decode_sse_event_skips_notifications_with_null_id() {
        let event = r#"data: {"jsonrpc":"2.0","id":null,"method":"x"}"#;
        assert!(decode_sse_event(event, &req_id_one()).unwrap().is_none());
    }

    #[test]
    fn decode_sse_event_skips_mismatched_id() {
        let event = r#"data: {"jsonrpc":"2.0","id":42,"result":{}}"#;
        assert!(decode_sse_event(event, &req_id_one()).unwrap().is_none());
    }

    #[test]
    fn decode_sse_event_handles_multi_line_data_field() {
        // Per SSE spec, multiple `data:` lines join with `\n`. JSON
        // tolerates internal whitespace so this still parses.
        let event = "data: {\"jsonrpc\":\"2.0\",\n\
                     data: \"id\":1,\n\
                     data: \"result\":{}}";
        let parsed = decode_sse_event(event, &req_id_one()).unwrap();
        assert!(parsed.is_some(), "multi-line payload should join + parse");
    }

    #[test]
    fn decode_sse_event_skips_event_with_no_data_lines() {
        // E.g. SSE comment-only event ":" + heartbeat.
        let event = ": this is a comment";
        assert!(decode_sse_event(event, &req_id_one()).unwrap().is_none());
    }

    #[test]
    fn decode_sse_event_returns_serde_error_for_invalid_json() {
        let event = "data: {not json";
        let err = decode_sse_event(event, &req_id_one())
            .expect_err("invalid json must surface as Err");
        assert!(matches!(err, McpError::Serde(_)));
    }

    #[test]
    fn decode_sse_event_treats_id_string_match_consistently() {
        // MCP allows string ids too — match is via Value equality.
        let event = r#"data: {"jsonrpc":"2.0","id":"abc","result":{}}"#;
        let parsed = decode_sse_event(event, &serde_json::json!("abc"))
            .unwrap()
            .expect("string id must match");
        assert_eq!(parsed.id, serde_json::json!("abc"));
    }

    // ── Mock-server end-to-end (HttpTransport::request) ──

    pub mod mock_server {
        use super::*;
        use std::sync::Arc;
        use std::sync::Mutex;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        /// Captures the bytes of one HTTP request and replies with the
        /// provided raw HTTP response. Returns (server_url, captured_request).
        ///
        /// The server is single-shot: it accepts ONE connection, reads
        /// until end-of-headers + Content-Length bytes, then writes the
        /// response and closes. Sufficient for all transport tests.
        async fn spawn_one_shot(
            response: &'static [u8],
        ) -> (String, Arc<Mutex<Vec<u8>>>) {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let captured: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
            let captured_clone = captured.clone();
            tokio::spawn(async move {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 4096];
                let mut acc = Vec::new();
                // Read headers (until we see \r\n\r\n).
                loop {
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    acc.extend_from_slice(&buf[..n]);
                    if let Some(idx) = find_double_crlf(&acc) {
                        // Parse Content-Length to know how much body to expect.
                        let head = std::str::from_utf8(&acc[..idx]).unwrap_or("");
                        let len = head
                            .lines()
                            .find_map(|l| {
                                let lc = l.to_ascii_lowercase();
                                lc.strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        let body_so_far = acc.len() - (idx + 4);
                        if body_so_far < len {
                            let mut remaining = vec![0u8; len - body_so_far];
                            sock.read_exact(&mut remaining).await.unwrap();
                            acc.extend_from_slice(&remaining);
                        }
                        break;
                    }
                }
                {
                    let mut g = captured_clone.lock().unwrap();
                    g.extend_from_slice(&acc);
                }
                let _ = sock.write_all(response).await;
                let _ = sock.shutdown().await;
            });
            (format!("http://{addr}"), captured)
        }

        fn find_double_crlf(buf: &[u8]) -> Option<usize> {
            buf.windows(4).position(|w| w == b"\r\n\r\n")
        }

        const SINGLE_JSON_RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\n\
            Content-Type: application/json\r\n\
            Content-Length: 45\r\n\
            \r\n\
            {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"ok\":true}}";

        #[tokio::test]
        async fn request_against_mock_returns_json_response() {
            let (url, _captured) = spawn_one_shot(SINGLE_JSON_RESPONSE).await;
            let t = HttpTransport::new(
                url,
                BTreeMap::new(),
                Duration::from_secs(5),
            )
            .unwrap();
            let req = McpRequest::new(1, "tools/list");
            let resp = t.request(req).await.expect("mock returns JSON");
            assert_eq!(resp.id, serde_json::json!(1));
            assert_eq!(resp.result.unwrap()["ok"], serde_json::json!(true));
        }

        const SSE_RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\n\
            Content-Type: text/event-stream\r\n\
            Transfer-Encoding: chunked\r\n\
            \r\n\
            5d\r\n\
            data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"tools\":[{\"name\":\"x\"}]}}\n\n\r\n\
            0\r\n\
            \r\n";

        #[tokio::test]
        async fn request_against_mock_returns_sse_response() {
            let (url, _captured) = spawn_one_shot(SSE_RESPONSE).await;
            let t = HttpTransport::new(
                url,
                BTreeMap::new(),
                Duration::from_secs(5),
            )
            .unwrap();
            let req = McpRequest::new(1, "tools/list");
            let resp = t.request(req).await.expect("SSE event must parse");
            assert_eq!(resp.id, serde_json::json!(1));
            let tools = resp.result.unwrap();
            assert_eq!(tools["tools"][0]["name"], serde_json::json!("x"));
        }

        #[tokio::test]
        async fn request_includes_extra_headers_in_outgoing_request() {
            let (url, captured) = spawn_one_shot(SINGLE_JSON_RESPONSE).await;
            let mut h = BTreeMap::new();
            h.insert("X-Theo-Test".into(), "marker-42".into());
            let t = HttpTransport::new(url, h, Duration::from_secs(5)).unwrap();
            let req = McpRequest::new(1, "tools/list");
            let _ = t.request(req).await;
            let bytes = captured.lock().unwrap().clone();
            let head = std::str::from_utf8(&bytes).unwrap_or("");
            assert!(
                head.lines().any(|l| l
                    .to_ascii_lowercase()
                    .starts_with("x-theo-test:")
                    && l.contains("marker-42")),
                "extra header must be present in request; got:\n{head}"
            );
        }

        #[tokio::test]
        async fn request_sends_post_with_application_json_content_type() {
            let (url, captured) = spawn_one_shot(SINGLE_JSON_RESPONSE).await;
            let t = HttpTransport::new(
                url,
                BTreeMap::new(),
                Duration::from_secs(5),
            )
            .unwrap();
            let req = McpRequest::new(1, "tools/list");
            let _ = t.request(req).await;
            let bytes = captured.lock().unwrap().clone();
            let head = std::str::from_utf8(&bytes).unwrap_or("");
            assert!(head.starts_with("POST "), "must be POST; got:\n{head}");
            assert!(
                head.lines().any(|l| l
                    .to_ascii_lowercase()
                    .starts_with("content-type: application/json")),
                "Content-Type must be application/json; got:\n{head}"
            );
        }

        const SINGLE_JSON_WITH_SESSION: &[u8] = b"HTTP/1.1 200 OK\r\n\
            Content-Type: application/json\r\n\
            Mcp-Session-Id: sess-abc-123\r\n\
            Content-Length: 45\r\n\
            \r\n\
            {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"ok\":true}}";

        #[tokio::test]
        async fn request_captures_mcp_session_id_from_response_header() {
            let (url, _captured) = spawn_one_shot(SINGLE_JSON_WITH_SESSION).await;
            let t = HttpTransport::new(
                url,
                BTreeMap::new(),
                Duration::from_secs(5),
            )
            .unwrap();
            let req = McpRequest::new(1, "tools/list");
            let _ = t.request(req).await;
            let staged = t.session_id.lock().unwrap().clone();
            assert_eq!(staged, Some("sess-abc-123".to_string()));
        }
    }
}
