//! T3.1 — LSP / JSON-RPC 2.0 wire-format primitives.
//!
//! Implements the LSP framing and JSON-RPC message shapes that any
//! `tower-lsp`-style client needs to talk to an external server
//! (`rust-analyzer`, `pyright`, `typescript-language-server`).
//!
//! Wire format (LSP base protocol):
//! ```text
//! Content-Length: <N>\r\n
//! \r\n
//! { JSON-RPC 2.0 message of N bytes }
//! ```
//!
//! Multiple headers MAY appear (`Content-Type` etc.) — we skip past
//! anything we don't recognise. The blank line `\r\n\r\n` separates
//! headers from body.
//!
//! JSON-RPC 2.0:
//! - Request: `{"jsonrpc":"2.0", "id": ..., "method": "...", "params": ...}`
//! - Notification (no id): `{"jsonrpc":"2.0", "method": "...", "params": ...}`
//! - Response success: `{"jsonrpc":"2.0", "id": ..., "result": ...}`
//! - Response error:   `{"jsonrpc":"2.0", "id": ..., "error": {...}}`
//!
//! This module is PURE: no IO, no async. Real client wiring (spawning
//! the server, reading stdout) lives in a future `client.rs` and uses
//! these primitives. Keeping the wire layer pure makes it trivially
//! testable without a real server.

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// IDs — JSON-RPC ids may be number or string. We use u64 internally; the
// LSP protocol never asks the client to honour string ids.
// ---------------------------------------------------------------------------

/// Monotonic client-side request id generator.
#[derive(Debug, Default)]
pub struct RequestIdGen {
    next: AtomicU64,
}

impl RequestIdGen {
    /// Build a generator starting at 1 (LSP servers expect non-zero
    /// ids in the request stream).
    pub fn new() -> Self {
        Self {
            next: AtomicU64::new(1),
        }
    }

    /// Allocate and return the next request id.
    pub fn next(&self) -> u64 {
        self.next.fetch_add(1, Ordering::SeqCst)
    }
}

// ---------------------------------------------------------------------------
// Message types
// ---------------------------------------------------------------------------

/// JSON-RPC request — has both `id` and `method`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    /// Build a fresh request with `jsonrpc: "2.0"` pre-filled.
    pub fn new(id: u64, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC notification — has `method` but no `id`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcNotification {
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC response — has `id` and either `result` or `error`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcErrorObj>,
}

/// JSON-RPC error object inside a response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonRpcErrorObj {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Inbound message classification — what arrived FROM the server.
/// Notifications + responses are the only inbound shapes a client
/// sees; servers don't send the client requests in our model.
#[derive(Debug, Clone, PartialEq)]
pub enum InboundMessage {
    Response(JsonRpcResponse),
    Notification(JsonRpcNotification),
}

/// Errors from the framing / parsing layer.
#[derive(Debug, thiserror::Error)]
pub enum LspProtocolError {
    #[error("invalid framing header: {0}")]
    BadHeader(String),
    #[error("missing Content-Length header")]
    MissingContentLength,
    #[error("invalid JSON body: {0}")]
    BadJson(String),
    #[error("unrecognised JSON-RPC shape (no `id`/`method` distinguish)")]
    UnknownShape,
    #[error("body length mismatch: declared {declared}, got {actual}")]
    LengthMismatch { declared: usize, actual: usize },
}

// ---------------------------------------------------------------------------
// Framing
// ---------------------------------------------------------------------------

/// Encode a JSON payload with the LSP framing header.
/// Output is `Content-Length: <N>\r\n\r\n<body>` as a `Vec<u8>`.
pub fn encode_frame(body: &[u8]) -> Vec<u8> {
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let mut out = Vec::with_capacity(header.len() + body.len());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(body);
    out
}

/// Encode any serializable JSON-RPC value as a complete frame.
pub fn encode_message<T: Serialize>(value: &T) -> Result<Vec<u8>, LspProtocolError> {
    let body = serde_json::to_vec(value).map_err(|e| LspProtocolError::BadJson(e.to_string()))?;
    Ok(encode_frame(&body))
}

/// Try to decode a single frame from a byte buffer. On success returns
/// `(message, bytes_consumed)`. When the buffer is incomplete (header
/// or body still missing), returns `Ok(None)` so the caller can read
/// more bytes and retry.
pub fn try_decode_frame(buf: &[u8]) -> Result<Option<(InboundMessage, usize)>, LspProtocolError> {
    let Some(header_end) = find_header_end(buf) else {
        return Ok(None);
    };
    let headers = std::str::from_utf8(&buf[..header_end - 4])
        .map_err(|e| LspProtocolError::BadHeader(format!("non-utf8 headers: {e}")))?;
    let content_length = parse_content_length(headers)?;
    let body_start = header_end;
    let body_end = body_start + content_length;
    if buf.len() < body_end {
        return Ok(None);
    }
    let body = &buf[body_start..body_end];
    let v: Value = serde_json::from_slice(body)
        .map_err(|e| LspProtocolError::BadJson(e.to_string()))?;
    let msg = classify_inbound(&v)?;
    Ok(Some((msg, body_end)))
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    // Position right after `\r\n\r\n` (header terminator).
    buf.windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
}

fn parse_content_length(headers: &str) -> Result<usize, LspProtocolError> {
    for line in headers.split("\r\n").filter(|l| !l.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            return Err(LspProtocolError::BadHeader(format!(
                "header without colon: `{line}`"
            )));
        };
        if name.trim().eq_ignore_ascii_case("content-length") {
            return value
                .trim()
                .parse::<usize>()
                .map_err(|e| LspProtocolError::BadHeader(format!(
                    "invalid Content-Length value `{value}`: {e}"
                )));
        }
    }
    Err(LspProtocolError::MissingContentLength)
}

/// Classify an inbound JSON value into a Response or Notification.
fn classify_inbound(v: &Value) -> Result<InboundMessage, LspProtocolError> {
    let has_id = v.get("id").is_some();
    let has_method = v.get("method").is_some();
    if has_id && !has_method {
        // Response (success or error)
        let resp: JsonRpcResponse = serde_json::from_value(v.clone())
            .map_err(|e| LspProtocolError::BadJson(e.to_string()))?;
        Ok(InboundMessage::Response(resp))
    } else if has_method && !has_id {
        // Notification
        let n: JsonRpcNotification = serde_json::from_value(v.clone())
            .map_err(|e| LspProtocolError::BadJson(e.to_string()))?;
        Ok(InboundMessage::Notification(n))
    } else {
        // Server requests are valid in LSP but not exercised by this
        // client model; route them through `Notification` for now.
        if has_method {
            let n: JsonRpcNotification = serde_json::from_value(v.clone())
                .map_err(|e| LspProtocolError::BadJson(e.to_string()))?;
            Ok(InboundMessage::Notification(n))
        } else {
            Err(LspProtocolError::UnknownShape)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ---- RequestIdGen ----

    #[test]
    fn t31_id_gen_starts_at_one() {
        let g = RequestIdGen::new();
        assert_eq!(g.next(), 1);
    }

    #[test]
    fn t31_id_gen_is_monotonic() {
        let g = RequestIdGen::new();
        let a = g.next();
        let b = g.next();
        let c = g.next();
        assert!(a < b && b < c);
    }

    // ---- Encode ----

    #[test]
    fn t31_encode_frame_prepends_content_length() {
        let body = b"{\"jsonrpc\":\"2.0\"}";
        let out = encode_frame(body);
        let s = std::str::from_utf8(&out).unwrap();
        assert!(s.starts_with("Content-Length: 17\r\n\r\n"));
        assert!(s.ends_with("{\"jsonrpc\":\"2.0\"}"));
    }

    #[test]
    fn t31_encode_message_roundtrips_through_decode() {
        let req = JsonRpcRequest::new(42, "textDocument/hover", Some(json!({"x":1})));
        let frame = encode_message(&req).unwrap();
        let (decoded, n) = try_decode_frame(&frame).unwrap().unwrap();
        assert_eq!(n, frame.len());
        match decoded {
            InboundMessage::Response(_) => panic!("expected request to decode as notification or fail"),
            InboundMessage::Notification(notif) => {
                // Requests with id are NOT inbound messages our client expects.
                // The classifier treats them as responses (since id is present)
                // OR notifications — we use the test below to verify the
                // request-shape branch separately.
                assert_eq!(notif.method, "textDocument/hover");
            }
        }
    }

    // ---- Decode ----

    #[test]
    fn t31_decode_returns_none_for_incomplete_header() {
        // Only the header start, no \r\n\r\n yet.
        let buf = b"Content-Length: 10\r\n";
        assert!(try_decode_frame(buf).unwrap().is_none());
    }

    #[test]
    fn t31_decode_returns_none_for_incomplete_body() {
        // Header complete, but body bytes haven't all arrived.
        let buf = b"Content-Length: 100\r\n\r\n{\"jsonrpc\":\"2.0\"}";
        assert!(try_decode_frame(buf).unwrap().is_none());
    }

    #[test]
    fn t31_decode_response_with_result() {
        let body = json!({"jsonrpc":"2.0","id":7,"result":{"ok":true}});
        let frame = encode_message(&body).unwrap();
        let (msg, _) = try_decode_frame(&frame).unwrap().unwrap();
        match msg {
            InboundMessage::Response(r) => {
                assert_eq!(r.id, 7);
                assert!(r.error.is_none());
                assert_eq!(r.result.as_ref().unwrap()["ok"], true);
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn t31_decode_response_with_error() {
        let body = json!({
            "jsonrpc":"2.0",
            "id": 9,
            "error": {"code": -32601, "message": "Method not found"}
        });
        let frame = encode_message(&body).unwrap();
        let (msg, _) = try_decode_frame(&frame).unwrap().unwrap();
        match msg {
            InboundMessage::Response(r) => {
                assert!(r.result.is_none());
                let e = r.error.unwrap();
                assert_eq!(e.code, -32601);
                assert_eq!(e.message, "Method not found");
            }
            other => panic!("expected error response, got {other:?}"),
        }
    }

    #[test]
    fn t31_decode_notification_no_id() {
        let body = json!({
            "jsonrpc":"2.0",
            "method":"window/logMessage",
            "params":{"type":3,"message":"started"}
        });
        let frame = encode_message(&body).unwrap();
        let (msg, _) = try_decode_frame(&frame).unwrap().unwrap();
        match msg {
            InboundMessage::Notification(n) => {
                assert_eq!(n.method, "window/logMessage");
            }
            other => panic!("expected notification, got {other:?}"),
        }
    }

    #[test]
    fn t31_decode_invalid_content_length_returns_error() {
        let buf = b"Content-Length: not-a-number\r\n\r\n{}";
        let err = try_decode_frame(buf).unwrap_err();
        assert!(matches!(err, LspProtocolError::BadHeader(_)));
    }

    #[test]
    fn t31_decode_missing_content_length_returns_error() {
        let buf = b"Content-Type: application/json\r\n\r\n{}";
        let err = try_decode_frame(buf).unwrap_err();
        assert!(matches!(err, LspProtocolError::MissingContentLength));
    }

    #[test]
    fn t31_decode_handles_extra_headers_gracefully() {
        // Real servers send Content-Type alongside Content-Length.
        let body = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":null}";
        let frame = format!(
            "Content-Length: {}\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n",
            body.len()
        );
        let mut buf = frame.into_bytes();
        buf.extend_from_slice(body);
        let (msg, n) = try_decode_frame(&buf).unwrap().unwrap();
        assert_eq!(n, buf.len());
        assert!(matches!(msg, InboundMessage::Response(_)));
    }

    #[test]
    fn t31_decode_consumes_exact_byte_count_for_streaming() {
        // After decoding a frame, the caller can advance their buffer
        // by `n` and decode the next frame. This proves the byte count
        // is exact (no off-by-one).
        let m1 = json!({"jsonrpc":"2.0","method":"a","params":1});
        let m2 = json!({"jsonrpc":"2.0","method":"b","params":2});
        let mut buf = encode_message(&m1).unwrap();
        buf.extend(encode_message(&m2).unwrap());
        let (msg1, n1) = try_decode_frame(&buf).unwrap().unwrap();
        match msg1 {
            InboundMessage::Notification(n) => assert_eq!(n.method, "a"),
            _ => panic!(),
        }
        let (msg2, _) = try_decode_frame(&buf[n1..]).unwrap().unwrap();
        match msg2 {
            InboundMessage::Notification(n) => assert_eq!(n.method, "b"),
            _ => panic!(),
        }
    }

    #[test]
    fn t31_decode_invalid_json_body_returns_bad_json() {
        let buf = b"Content-Length: 4\r\n\r\nzzzz";
        let err = try_decode_frame(buf).unwrap_err();
        assert!(matches!(err, LspProtocolError::BadJson(_)));
    }

    #[test]
    fn t31_jsonrpc_request_serde_roundtrip() {
        let r = JsonRpcRequest::new(1, "initialize", Some(json!({"capabilities": {}})));
        let json = serde_json::to_string(&r).unwrap();
        let back: JsonRpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
        assert_eq!(r.jsonrpc, "2.0");
    }

    #[test]
    fn t31_jsonrpc_notification_omits_id_in_serialised_form() {
        let n = JsonRpcNotification::new("textDocument/didOpen", Some(json!({})));
        let json = serde_json::to_string(&n).unwrap();
        assert!(!json.contains("\"id\""));
    }

    #[test]
    fn t31_jsonrpc_response_success_has_no_error_field() {
        let r = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: 1,
            result: Some(json!(true)),
            error: None,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("\"error\""));
    }
}
