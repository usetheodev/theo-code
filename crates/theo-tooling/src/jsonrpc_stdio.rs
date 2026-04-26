//! T3.1 / T13.1 — Shared `Content-Length: N\r\n\r\n<body>` frame
//! accumulator.
//!
//! LSP and DAP both wrap their JSON payloads in the same header
//! framing. The protocol-specific decoders (`lsp::protocol::
//! try_decode_frame`, `dap::protocol::try_decode_frame`) handle the
//! typed body. This module provides the byte-level layer that sits
//! between an async byte stream (subprocess stdout) and those
//! decoders: feed it bytes, get back complete frames.
//!
//! Pure code — no IO, no async, no subprocess. The future
//! `subprocess` glue calls `feed(bytes)` from a `tokio::spawn` reader
//! task and dispatches each yielded frame to the protocol's
//! `try_decode_frame`.

use std::collections::VecDeque;

/// Errors from the byte-level frame accumulator.
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("invalid framing header: {0}")]
    BadHeader(String),
    #[error("missing Content-Length header in headers `{0}`")]
    MissingContentLength(String),
    #[error("frame body exceeds max size {max} bytes")]
    BodyTooLarge { max: usize },
}

/// Default max single-frame body size: 16 MiB. Generous enough for
/// any sane LSP/DAP message; small enough that a malformed
/// `Content-Length: 9999999999` doesn't immediately OOM the runtime.
pub const DEFAULT_MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

/// Streaming frame accumulator. Hold one per subprocess stdout
/// reader.
///
/// Usage:
/// ```rust,ignore
/// let mut acc = FrameAccumulator::new();
/// loop {
///     let n = stdout.read(&mut buf).await?;
///     if n == 0 { break; }
///     acc.feed(&buf[..n]);
///     while let Some(body) = acc.next_frame()? {
///         // body is the complete N-byte JSON payload.
///         // Hand it to lsp::protocol::try_decode_frame or
///         // dap::protocol::try_decode_frame for typed parsing.
///     }
/// }
/// ```
#[derive(Debug)]
pub struct FrameAccumulator {
    buf: VecDeque<u8>,
    max_body: usize,
}

impl Default for FrameAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameAccumulator {
    /// Default cap of [`DEFAULT_MAX_BODY_BYTES`].
    pub fn new() -> Self {
        Self::with_max_body(DEFAULT_MAX_BODY_BYTES)
    }

    /// Custom body cap — useful for tests that want to exercise the
    /// "body too large" branch.
    pub fn with_max_body(max_body: usize) -> Self {
        Self {
            buf: VecDeque::new(),
            max_body,
        }
    }

    /// Append more bytes to the internal buffer. Cheap; copies once.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.buf.extend(bytes);
    }

    /// Number of bytes currently buffered.
    pub fn buffered_len(&self) -> usize {
        self.buf.len()
    }

    /// Try to extract the next complete frame's body bytes.
    ///
    /// - `Ok(None)` — header or body still incomplete; caller should
    ///   feed more bytes and retry.
    /// - `Ok(Some(body))` — full frame extracted; the caller now has
    ///   the JSON payload as a `Vec<u8>`. Internal buffer advanced
    ///   past the consumed bytes.
    /// - `Err(_)` — malformed header or body too large. Caller should
    ///   typically log + drop the connection.
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>, FrameError> {
        let snapshot: Vec<u8> = self.buf.iter().copied().collect();

        let Some(header_end) = find_header_end(&snapshot) else {
            return Ok(None);
        };
        let headers = std::str::from_utf8(&snapshot[..header_end - 4])
            .map_err(|e| FrameError::BadHeader(format!("non-utf8 headers: {e}")))?;
        let content_length = parse_content_length(headers)?;

        if content_length > self.max_body {
            return Err(FrameError::BodyTooLarge {
                max: self.max_body,
            });
        }

        let body_start = header_end;
        let body_end = body_start + content_length;
        if snapshot.len() < body_end {
            return Ok(None);
        }
        let body = snapshot[body_start..body_end].to_vec();

        // Drain consumed bytes from the front of the deque.
        for _ in 0..body_end {
            self.buf.pop_front();
        }
        Ok(Some(body))
    }

    /// Reset the buffer. Use after a malformed frame causes an Err
    /// so we don't keep hitting the same bad bytes forever. Caller
    /// should typically also drop the underlying connection.
    pub fn clear(&mut self) {
        self.buf.clear();
    }
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn parse_content_length(headers: &str) -> Result<usize, FrameError> {
    for line in headers.split("\r\n").filter(|l| !l.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            return Err(FrameError::BadHeader(format!(
                "header without colon: `{line}`"
            )));
        };
        if name.trim().eq_ignore_ascii_case("content-length") {
            return value
                .trim()
                .parse::<usize>()
                .map_err(|e| FrameError::BadHeader(format!(
                    "invalid Content-Length value `{value}`: {e}"
                )));
        }
    }
    Err(FrameError::MissingContentLength(headers.into()))
}

/// Encode a body into a complete frame: prepend `Content-Length: N\r\n\r\n`.
/// Mirrors `lsp::protocol::encode_frame` and `dap::protocol::encode_frame`
/// — kept here so the writer side of the future subprocess wrapper
/// has a single place to source the framing.
pub fn encode_frame(body: &[u8]) -> Vec<u8> {
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let mut out = Vec::with_capacity(header.len() + body.len());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(body: &[u8]) -> Vec<u8> {
        encode_frame(body)
    }

    #[test]
    fn t31stdio_new_accumulator_is_empty() {
        let acc = FrameAccumulator::new();
        assert_eq!(acc.buffered_len(), 0);
    }

    #[test]
    fn t31stdio_next_frame_returns_none_when_empty() {
        let mut acc = FrameAccumulator::new();
        assert!(acc.next_frame().unwrap().is_none());
    }

    #[test]
    fn t31stdio_extracts_one_complete_frame() {
        let mut acc = FrameAccumulator::new();
        acc.feed(&frame(b"{\"hello\":1}"));
        let body = acc.next_frame().unwrap().expect("complete frame");
        assert_eq!(body, b"{\"hello\":1}");
        assert_eq!(acc.buffered_len(), 0);
    }

    #[test]
    fn t31stdio_extracts_two_consecutive_frames() {
        let mut acc = FrameAccumulator::new();
        let mut bytes = frame(b"{\"a\":1}");
        bytes.extend(frame(b"{\"b\":2}"));
        acc.feed(&bytes);
        let body1 = acc.next_frame().unwrap().unwrap();
        assert_eq!(body1, b"{\"a\":1}");
        let body2 = acc.next_frame().unwrap().unwrap();
        assert_eq!(body2, b"{\"b\":2}");
        // No leftover bytes after both consumed.
        assert_eq!(acc.buffered_len(), 0);
        // Third call returns None — nothing left.
        assert!(acc.next_frame().unwrap().is_none());
    }

    #[test]
    fn t31stdio_partial_header_returns_none_until_complete() {
        let mut acc = FrameAccumulator::new();
        // Only the start of the header — no \r\n\r\n yet.
        acc.feed(b"Content-Length: 7\r\n");
        assert!(acc.next_frame().unwrap().is_none());
        // Complete the header but body not yet present.
        acc.feed(b"\r\n");
        assert!(acc.next_frame().unwrap().is_none());
        // Body partial.
        acc.feed(b"{\"a\":");
        assert!(acc.next_frame().unwrap().is_none());
        // Body complete.
        acc.feed(b"1}");
        assert_eq!(acc.next_frame().unwrap().unwrap(), b"{\"a\":1}");
    }

    #[test]
    fn t31stdio_byte_at_a_time_eventually_yields_frame() {
        // Worst-case: subprocess writes one byte at a time. The
        // accumulator must still surface the complete frame.
        let mut acc = FrameAccumulator::new();
        let bytes = frame(b"{\"x\":42}");
        for byte in &bytes {
            acc.feed(std::slice::from_ref(byte));
        }
        let body = acc.next_frame().unwrap().expect("complete");
        assert_eq!(body, b"{\"x\":42}");
    }

    #[test]
    fn t31stdio_extra_headers_skipped() {
        // Real servers send Content-Type alongside Content-Length.
        let mut acc = FrameAccumulator::new();
        let body = b"{\"k\":\"v\"}";
        let header = format!(
            "Content-Length: {}\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n",
            body.len()
        );
        acc.feed(header.as_bytes());
        acc.feed(body);
        assert_eq!(acc.next_frame().unwrap().unwrap(), body);
    }

    #[test]
    fn t31stdio_invalid_content_length_returns_bad_header() {
        let mut acc = FrameAccumulator::new();
        acc.feed(b"Content-Length: not-a-number\r\n\r\n{}");
        let err = acc.next_frame().unwrap_err();
        assert!(matches!(err, FrameError::BadHeader(_)));
    }

    #[test]
    fn t31stdio_missing_content_length_returns_error() {
        let mut acc = FrameAccumulator::new();
        acc.feed(b"Content-Type: x\r\n\r\n{}");
        let err = acc.next_frame().unwrap_err();
        assert!(matches!(err, FrameError::MissingContentLength(_)));
    }

    #[test]
    fn t31stdio_oversized_body_rejected() {
        let mut acc = FrameAccumulator::with_max_body(10);
        acc.feed(b"Content-Length: 1000\r\n\r\n");
        let err = acc.next_frame().unwrap_err();
        match err {
            FrameError::BodyTooLarge { max } => assert_eq!(max, 10),
            other => panic!("expected BodyTooLarge, got {other:?}"),
        }
    }

    #[test]
    fn t31stdio_clear_resets_buffer() {
        let mut acc = FrameAccumulator::new();
        acc.feed(b"some bytes");
        assert!(acc.buffered_len() > 0);
        acc.clear();
        assert_eq!(acc.buffered_len(), 0);
    }

    #[test]
    fn t31stdio_buffered_len_tracks_input() {
        let mut acc = FrameAccumulator::new();
        acc.feed(b"abc");
        assert_eq!(acc.buffered_len(), 3);
        acc.feed(b"def");
        assert_eq!(acc.buffered_len(), 6);
    }

    #[test]
    fn t31stdio_default_max_body_is_16_mib() {
        assert_eq!(DEFAULT_MAX_BODY_BYTES, 16 * 1024 * 1024);
    }

    #[test]
    fn t31stdio_encode_frame_round_trip_through_accumulator() {
        // Encode a body, feed it back through the accumulator,
        // verify we get the same bytes out. Closes the loop on the
        // wire-format symmetry between writer and reader.
        let body = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"test\"}";
        let bytes = encode_frame(body);
        let mut acc = FrameAccumulator::new();
        acc.feed(&bytes);
        let extracted = acc.next_frame().unwrap().unwrap();
        assert_eq!(extracted, body);
    }
}
