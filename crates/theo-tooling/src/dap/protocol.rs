//! T13.1 — Debug Adapter Protocol wire format.
//!
//! DAP uses the same `Content-Length: <N>\r\n\r\n<body>` framing as
//! LSP but a different message model:
//!
//! - **Request** (client → server):
//!   `{seq, type: "request", command, arguments?}`
//! - **Response** (server → client, paired by `request_seq`):
//!   `{seq, type: "response", request_seq, command, success, body?,
//!    message?}` — `message` is a human-readable error string when
//!    `success == false`.
//! - **Event** (server → client, unsolicited):
//!   `{seq, type: "event", event, body?}`
//!
//! Spec: <https://microsoft.github.io/debug-adapter-protocol/>.
//!
//! This module is PURE: no IO, no async. Real client wiring (spawning
//! `lldb-vscode`, reading stdout) is the next iteration.

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Sequence number generator
// ---------------------------------------------------------------------------

/// Monotonic DAP `seq` generator. The first message must use seq=1
/// per the spec.
#[derive(Debug, Default)]
pub struct DapSeqGen {
    next: AtomicU64,
}

impl DapSeqGen {
    /// Build a generator starting at 1.
    pub fn new() -> Self {
        Self {
            next: AtomicU64::new(1),
        }
    }

    /// Allocate the next seq.
    pub fn next(&self) -> u64 {
        self.next.fetch_add(1, Ordering::SeqCst)
    }
}

// ---------------------------------------------------------------------------
// Message shapes
// ---------------------------------------------------------------------------

/// Outbound request. `command` is e.g. `"setBreakpoints"`, `"next"`,
/// `"evaluate"`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DapRequest {
    pub seq: u64,
    #[serde(rename = "type")]
    pub message_type: String,
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
}

impl DapRequest {
    /// Build a request with `type: "request"` pre-filled.
    pub fn new(seq: u64, command: impl Into<String>, arguments: Option<Value>) -> Self {
        Self {
            seq,
            message_type: "request".into(),
            command: command.into(),
            arguments,
        }
    }
}

/// Inbound response, paired with its triggering request via
/// `request_seq`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DapResponse {
    pub seq: u64,
    #[serde(rename = "type")]
    pub message_type: String,
    pub request_seq: u64,
    pub command: String,
    pub success: bool,
    /// Error message — present only when `success == false`. The DAP
    /// spec uses `message` rather than nesting an error object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Result body — shape depends on the command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
}

/// Inbound unsolicited event (e.g. `stopped`, `output`, `terminated`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DapEvent {
    pub seq: u64,
    #[serde(rename = "type")]
    pub message_type: String,
    pub event: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
}

/// Inbound message classification — what the server can send to us.
#[derive(Debug, Clone, PartialEq)]
pub enum DapMessage {
    Response(DapResponse),
    Event(DapEvent),
}

/// Errors from the framing / parsing layer. Mirrors `LspProtocolError`
/// but kept distinct so `Result<_, DapProtocolError>` stays
/// type-safe at the call site.
#[derive(Debug, thiserror::Error)]
pub enum DapProtocolError {
    #[error("invalid framing header: {0}")]
    BadHeader(String),
    #[error("missing Content-Length header")]
    MissingContentLength,
    #[error("invalid JSON body: {0}")]
    BadJson(String),
    #[error("missing or invalid `type` field: {0}")]
    BadMessageType(String),
    #[error("unknown DAP message type: {0}")]
    UnknownType(String),
}

// ---------------------------------------------------------------------------
// Framing — re-uses the LSP wire format (intentionally identical).
// ---------------------------------------------------------------------------

/// Encode a JSON payload with the LSP-compatible framing header.
pub fn encode_frame(body: &[u8]) -> Vec<u8> {
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let mut out = Vec::with_capacity(header.len() + body.len());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(body);
    out
}

/// Encode any serializable DAP message as a complete frame.
pub fn encode_message<T: Serialize>(value: &T) -> Result<Vec<u8>, DapProtocolError> {
    let body = serde_json::to_vec(value).map_err(|e| DapProtocolError::BadJson(e.to_string()))?;
    Ok(encode_frame(&body))
}

/// Try to decode a single frame. On success returns `(message,
/// bytes_consumed)`. Buffer not yet complete → `Ok(None)`.
pub fn try_decode_frame(buf: &[u8]) -> Result<Option<(DapMessage, usize)>, DapProtocolError> {
    let Some(header_end) = find_header_end(buf) else {
        return Ok(None);
    };
    let headers = std::str::from_utf8(&buf[..header_end - 4])
        .map_err(|e| DapProtocolError::BadHeader(format!("non-utf8 headers: {e}")))?;
    let content_length = parse_content_length(headers)?;
    let body_start = header_end;
    let body_end = body_start + content_length;
    if buf.len() < body_end {
        return Ok(None);
    }
    let body = &buf[body_start..body_end];
    let v: Value =
        serde_json::from_slice(body).map_err(|e| DapProtocolError::BadJson(e.to_string()))?;
    let msg = classify_inbound(&v)?;
    Ok(Some((msg, body_end)))
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn parse_content_length(headers: &str) -> Result<usize, DapProtocolError> {
    for line in headers.split("\r\n").filter(|l| !l.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            return Err(DapProtocolError::BadHeader(format!(
                "header without colon: `{line}`"
            )));
        };
        if name.trim().eq_ignore_ascii_case("content-length") {
            return value
                .trim()
                .parse::<usize>()
                .map_err(|e| DapProtocolError::BadHeader(format!(
                    "invalid Content-Length value `{value}`: {e}"
                )));
        }
    }
    Err(DapProtocolError::MissingContentLength)
}

fn classify_inbound(v: &Value) -> Result<DapMessage, DapProtocolError> {
    let ty = v
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| DapProtocolError::BadMessageType("missing `type`".into()))?;
    match ty {
        "response" => {
            let r: DapResponse = serde_json::from_value(v.clone())
                .map_err(|e| DapProtocolError::BadJson(e.to_string()))?;
            Ok(DapMessage::Response(r))
        }
        "event" => {
            let e: DapEvent = serde_json::from_value(v.clone())
                .map_err(|e| DapProtocolError::BadJson(e.to_string()))?;
            Ok(DapMessage::Event(e))
        }
        // Server requests (rare — reverse direction) aren't part of
        // this client model. Surface them as an error so the caller
        // can route or ignore explicitly.
        other => Err(DapProtocolError::UnknownType(other.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ---- DapSeqGen ----

    #[test]
    fn t131_seq_gen_starts_at_one() {
        let g = DapSeqGen::new();
        assert_eq!(g.next(), 1);
    }

    #[test]
    fn t131_seq_gen_is_monotonic() {
        let g = DapSeqGen::new();
        let a = g.next();
        let b = g.next();
        assert!(b > a);
    }

    // ---- Encode ----

    #[test]
    fn t131_encode_request_includes_content_length() {
        let req = DapRequest::new(1, "setBreakpoints", Some(json!({"source":{"path":"x"}})));
        let frame = encode_message(&req).unwrap();
        let prefix = std::str::from_utf8(&frame[..30]).unwrap();
        assert!(prefix.starts_with("Content-Length: "));
    }

    #[test]
    fn t131_encode_request_pre_fills_type_field() {
        let req = DapRequest::new(7, "next", None);
        assert_eq!(req.message_type, "request");
        let frame = encode_message(&req).unwrap();
        let s = std::str::from_utf8(&frame).unwrap();
        assert!(s.contains("\"type\":\"request\""));
        assert!(s.contains("\"command\":\"next\""));
    }

    #[test]
    fn t131_encode_request_omits_arguments_when_none() {
        let req = DapRequest::new(1, "continue", None);
        let frame = encode_message(&req).unwrap();
        let s = std::str::from_utf8(&frame).unwrap();
        assert!(!s.contains("\"arguments\""));
    }

    // ---- Decode ----

    #[test]
    fn t131_decode_response_success_pair_with_request() {
        let body = json!({
            "seq": 2,
            "type": "response",
            "request_seq": 1,
            "command": "setBreakpoints",
            "success": true,
            "body": {"breakpoints": [{"verified": true}]}
        });
        let frame = encode_message(&body).unwrap();
        let (msg, n) = try_decode_frame(&frame).unwrap().unwrap();
        assert_eq!(n, frame.len());
        match msg {
            DapMessage::Response(r) => {
                assert_eq!(r.request_seq, 1);
                assert!(r.success);
                assert!(r.message.is_none());
                assert!(r.body.is_some());
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn t131_decode_response_failure_carries_message() {
        let body = json!({
            "seq": 3,
            "type": "response",
            "request_seq": 2,
            "command": "evaluate",
            "success": false,
            "message": "expression evaluation failed"
        });
        let frame = encode_message(&body).unwrap();
        let (msg, _) = try_decode_frame(&frame).unwrap().unwrap();
        match msg {
            DapMessage::Response(r) => {
                assert!(!r.success);
                assert_eq!(r.message.as_deref(), Some("expression evaluation failed"));
                assert!(r.body.is_none());
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn t131_decode_event_unsolicited() {
        let body = json!({
            "seq": 5,
            "type": "event",
            "event": "stopped",
            "body": {"reason": "breakpoint", "threadId": 1}
        });
        let frame = encode_message(&body).unwrap();
        let (msg, _) = try_decode_frame(&frame).unwrap().unwrap();
        match msg {
            DapMessage::Event(e) => {
                assert_eq!(e.event, "stopped");
                assert_eq!(e.body.as_ref().unwrap()["reason"], "breakpoint");
            }
            other => panic!("expected Event, got {other:?}"),
        }
    }

    #[test]
    fn t131_decode_unknown_type_returns_error() {
        // A reverse-direction server-to-client request would have
        // type="request" — out of scope for our client.
        let body = json!({"seq": 1, "type": "request", "command": "runInTerminal"});
        let frame = encode_message(&body).unwrap();
        let err = try_decode_frame(&frame).unwrap_err();
        assert!(matches!(err, DapProtocolError::UnknownType(_)));
    }

    #[test]
    fn t131_decode_returns_none_for_incomplete_buffer() {
        let buf = b"Content-Length: 200\r\n\r\n{}";
        assert!(try_decode_frame(buf).unwrap().is_none());
    }

    #[test]
    fn t131_decode_consumes_exact_byte_count_for_streaming() {
        let m1 = json!({"seq": 1, "type": "event", "event": "initialized"});
        let m2 = json!({"seq": 2, "type": "event", "event": "stopped"});
        let mut buf = encode_message(&m1).unwrap();
        buf.extend(encode_message(&m2).unwrap());
        let (e1, n1) = try_decode_frame(&buf).unwrap().unwrap();
        match e1 {
            DapMessage::Event(e) => assert_eq!(e.event, "initialized"),
            _ => panic!(),
        }
        let (e2, _) = try_decode_frame(&buf[n1..]).unwrap().unwrap();
        match e2 {
            DapMessage::Event(e) => assert_eq!(e.event, "stopped"),
            _ => panic!(),
        }
    }

    #[test]
    fn t131_decode_invalid_json_body_returns_bad_json() {
        let buf = b"Content-Length: 4\r\n\r\nzzzz";
        let err = try_decode_frame(buf).unwrap_err();
        assert!(matches!(err, DapProtocolError::BadJson(_)));
    }

    #[test]
    fn t131_decode_missing_type_returns_error() {
        let body = json!({"seq": 1, "command": "init"});
        let frame = encode_message(&body).unwrap();
        let err = try_decode_frame(&frame).unwrap_err();
        assert!(matches!(err, DapProtocolError::BadMessageType(_)));
    }

    #[test]
    fn t131_decode_missing_content_length_returns_error() {
        let buf = b"Content-Type: application/json\r\n\r\n{\"seq\":1,\"type\":\"event\",\"event\":\"x\"}";
        let err = try_decode_frame(buf).unwrap_err();
        assert!(matches!(err, DapProtocolError::MissingContentLength));
    }

    // ---- Serde shape ----

    #[test]
    fn t131_request_serde_roundtrip() {
        let r = DapRequest::new(10, "evaluate", Some(json!({"expression":"1+1"})));
        let json = serde_json::to_string(&r).unwrap();
        let back: DapRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn t131_response_success_omits_message_field() {
        let r = DapResponse {
            seq: 1,
            message_type: "response".into(),
            request_seq: 0,
            command: "init".into(),
            success: true,
            message: None,
            body: Some(json!(true)),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("\"message\""));
    }

    #[test]
    fn t131_event_omits_body_when_none() {
        let e = DapEvent {
            seq: 1,
            message_type: "event".into(),
            event: "terminated".into(),
            body: None,
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(!json.contains("\"body\""));
    }
}
