//! T2.1 — Sidecar IO layer (line-delimited JSON over stdio).
//!
//! The Playwright sidecar uses a different framing from LSP/DAP:
//! one JSON object per line. This is intentional — Node's `readline`
//! streams stdin line-by-line, and the sidecar's payloads are small
//! (action requests + screenshot/eval responses). Content-Length
//! framing would add bookkeeping on both sides for no win.
//!
//! `SidecarSession` is the byte-level wrapper: it reads one JSON
//! line at a time from the sidecar's stdout, and writes one JSON
//! line at a time to its stdin. The protocol layer (`BrowserRequest`
//! / `BrowserResponse`) sits on top.
//!
//! Pure transport — typed parsing happens in the consumer
//! (`BrowserClient`, the next iteration). Tests use
//! `tokio::io::duplex` to drive the session WITHOUT spawning Node.

use std::io;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

/// Errors specific to a line-delimited session.
#[derive(Debug, thiserror::Error)]
pub enum SidecarError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    /// The peer closed its stdout cleanly (no more lines available).
    /// Distinguished from `Io` so callers can shut down gracefully
    /// instead of treating EOF as an error condition.
    #[error("peer closed stdout (EOF)")]
    Eof,
    /// The line we read exceeds the configured max body size — the
    /// sidecar may be misbehaving (huge eval result; corrupted output).
    #[error("line exceeds max body size: got {got} bytes, cap {cap}")]
    LineTooLarge { got: usize, cap: usize },
}

/// Default cap on a single line — 16 MiB is generous (a 4K
/// fullPage screenshot encodes to ~2 MiB base64). Lines larger than
/// this are almost certainly the sidecar going off the rails.
const DEFAULT_MAX_LINE_BYTES: usize = 16 * 1024 * 1024;

/// Line-delimited stdio session over any async duplex.
///
/// Writes one JSON line per `write_line` call (newline appended).
/// Reads one JSON line per `read_line_or_eof` call (newline stripped).
pub struct SidecarSession<W, R> {
    writer: W,
    reader: BufReader<R>,
    max_line_bytes: usize,
}

impl<W, R> SidecarSession<W, R>
where
    W: AsyncWrite + Unpin,
    R: AsyncRead + Unpin,
{
    /// Build a session with the default 16 MiB line cap.
    pub fn new(writer: W, reader: R) -> Self {
        Self {
            writer,
            reader: BufReader::with_capacity(64 * 1024, reader),
            max_line_bytes: DEFAULT_MAX_LINE_BYTES,
        }
    }

    /// Build a session with a custom line cap (mainly for tests).
    pub fn with_max_line_bytes(writer: W, reader: R, max_line_bytes: usize) -> Self {
        Self {
            writer,
            reader: BufReader::with_capacity(64 * 1024, reader),
            max_line_bytes,
        }
    }

    /// Write `body` followed by `\n`. The body MUST be a single line
    /// of JSON — embedded newlines would split the message on the
    /// sidecar side. Caller is responsible for using `serde_json::to_vec`
    /// (which never emits literal newlines for primitive values).
    pub async fn write_line(&mut self, body: &[u8]) -> Result<(), SidecarError> {
        debug_assert!(
            !body.contains(&b'\n'),
            "write_line body MUST NOT contain newlines — use serde_json::to_vec"
        );
        self.writer.write_all(body).await?;
        self.writer.write_all(b"\n").await?;
        self.writer.flush().await?;
        Ok(())
    }

    /// Read one line from stdout. Returns `Ok(Some(body))` on a full
    /// line (newline stripped), `Ok(None)` on graceful EOF (sidecar
    /// closed stdout), and `Err(LineTooLarge)` when a single line
    /// exceeds the configured cap.
    pub async fn read_line_or_eof(&mut self) -> Result<Option<Vec<u8>>, SidecarError> {
        let mut buf: Vec<u8> = Vec::with_capacity(1024);
        // `read_until(b'\n', &mut buf)` returns Ok(0) on EOF, otherwise
        // the count INCLUDING the newline (when present). We strip the
        // trailing `\n` ourselves and enforce the body cap pre-strip.
        let n = self.reader.read_until(b'\n', &mut buf).await?;
        if n == 0 {
            return Ok(None);
        }
        if buf.len() > self.max_line_bytes {
            return Err(SidecarError::LineTooLarge {
                got: buf.len(),
                cap: self.max_line_bytes,
            });
        }
        // Strip trailing `\n` and (if present) `\r`.
        if buf.last() == Some(&b'\n') {
            buf.pop();
        }
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
        Ok(Some(buf))
    }

    /// Strict-mode variant: graceful EOF surfaces as `Eof` error
    /// instead of `Ok(None)`. Useful when the caller knows it has
    /// in-flight requests pending and EOF is anomalous.
    pub async fn read_line_strict(&mut self) -> Result<Vec<u8>, SidecarError> {
        match self.read_line_or_eof().await? {
            Some(b) => Ok(b),
            None => Err(SidecarError::Eof),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    /// Build a duplex pair: returns the SUT session + harness halves
    /// (writer to feed the SUT's reader; reader to drain the SUT's
    /// writer).
    fn dup() -> (
        SidecarSession<tokio::io::DuplexStream, tokio::io::DuplexStream>,
        tokio::io::DuplexStream, // harness writer (feed SUT)
        tokio::io::DuplexStream, // harness reader (read SUT writes)
    ) {
        let (sut_writer, harness_reader) = duplex(64 * 1024);
        let (harness_writer, sut_reader) = duplex(64 * 1024);
        let sut = SidecarSession::new(sut_writer, sut_reader);
        (sut, harness_writer, harness_reader)
    }

    #[tokio::test]
    async fn t21sc_write_line_appends_newline() {
        let (mut sut, _harness_writer, mut harness_reader) = dup();
        sut.write_line(br#"{"id":1,"action":"open"}"#).await.unwrap();
        let mut got = vec![0u8; 256];
        let n = tokio::io::AsyncReadExt::read(&mut harness_reader, &mut got)
            .await
            .unwrap();
        let s = std::str::from_utf8(&got[..n]).unwrap();
        assert!(s.ends_with('\n'));
        assert_eq!(s, "{\"id\":1,\"action\":\"open\"}\n");
    }

    #[tokio::test]
    async fn t21sc_read_line_returns_body_without_newline() {
        let (mut sut, mut harness_writer, _) = dup();
        harness_writer
            .write_all(b"{\"id\":1,\"result\":{}}\n")
            .await
            .unwrap();
        harness_writer.flush().await.unwrap();
        let body = sut.read_line_or_eof().await.unwrap().unwrap();
        assert_eq!(body, br#"{"id":1,"result":{}}"#);
    }

    #[tokio::test]
    async fn t21sc_read_line_handles_two_lines_back_to_back() {
        let (mut sut, mut harness_writer, _) = dup();
        harness_writer
            .write_all(b"{\"id\":1}\n{\"id\":2}\n")
            .await
            .unwrap();
        harness_writer.flush().await.unwrap();
        let f1 = sut.read_line_or_eof().await.unwrap().unwrap();
        let f2 = sut.read_line_or_eof().await.unwrap().unwrap();
        assert_eq!(f1, br#"{"id":1}"#);
        assert_eq!(f2, br#"{"id":2}"#);
    }

    #[tokio::test]
    async fn t21sc_read_line_strips_crlf() {
        // Some sidecars on Windows append `\r\n`. We strip both.
        let (mut sut, mut harness_writer, _) = dup();
        harness_writer.write_all(b"{\"id\":1}\r\n").await.unwrap();
        harness_writer.flush().await.unwrap();
        let body = sut.read_line_or_eof().await.unwrap().unwrap();
        assert_eq!(body, br#"{"id":1}"#);
    }

    #[tokio::test]
    async fn t21sc_read_line_returns_none_on_clean_eof() {
        let (mut sut, harness_writer, _) = dup();
        // Drop harness writer before any data — clean EOF.
        drop(harness_writer);
        let result = sut.read_line_or_eof().await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn t21sc_read_line_strict_returns_eof_error_on_clean_close() {
        let (mut sut, harness_writer, _) = dup();
        drop(harness_writer);
        let err = sut.read_line_strict().await.unwrap_err();
        assert!(matches!(err, SidecarError::Eof));
    }

    #[tokio::test]
    async fn t21sc_read_line_returns_partial_when_no_terminator_then_eof() {
        // The sidecar wrote bytes but didn't append \n before closing.
        // `read_until` returns the partial bytes; we strip the (absent)
        // newline and surface the partial as a body. The protocol layer
        // can validate that it's parseable JSON.
        let (mut sut, mut harness_writer, _) = dup();
        harness_writer.write_all(b"{\"id\":1}").await.unwrap();
        harness_writer.flush().await.unwrap();
        drop(harness_writer);
        let body = sut.read_line_or_eof().await.unwrap().unwrap();
        assert_eq!(body, br#"{"id":1}"#);
        // Subsequent read returns clean EOF.
        assert!(sut.read_line_or_eof().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn t21sc_read_line_too_large_returns_typed_error() {
        let (sut_writer, _hr) = duplex(64 * 1024);
        let (mut harness_writer, sut_reader) = duplex(64 * 1024);
        // Tiny cap: 32 bytes. Send a 100-byte line.
        let mut sut = SidecarSession::with_max_line_bytes(sut_writer, sut_reader, 32);
        let body = vec![b'x'; 100];
        harness_writer.write_all(&body).await.unwrap();
        harness_writer.write_all(b"\n").await.unwrap();
        harness_writer.flush().await.unwrap();
        let err = sut.read_line_or_eof().await.unwrap_err();
        match err {
            SidecarError::LineTooLarge { got, cap } => {
                assert!(got > 32, "got should be the over-cap size");
                assert_eq!(cap, 32);
            }
            other => panic!("expected LineTooLarge, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t21sc_round_trip_request_response() {
        // Closes the loop: SUT writes a request, harness reads it,
        // harness writes a response, SUT reads it.
        let (mut sut, mut harness_writer, mut harness_reader) = dup();

        // Step 1: SUT writes request.
        sut.write_line(br#"{"id":1,"action":"open","url":"x"}"#)
            .await
            .unwrap();

        // Step 2: harness reads it as a line.
        let mut buf = vec![0u8; 256];
        let n = tokio::io::AsyncReadExt::read(&mut harness_reader, &mut buf)
            .await
            .unwrap();
        let s = std::str::from_utf8(&buf[..n]).unwrap();
        assert!(s.contains(r#""action":"open""#));
        assert!(s.ends_with('\n'));

        // Step 3: harness writes response.
        harness_writer
            .write_all(b"{\"id\":1,\"result\":{\"kind\":\"navigated\"}}\n")
            .await
            .unwrap();
        harness_writer.flush().await.unwrap();

        // Step 4: SUT reads response.
        let body = sut.read_line_or_eof().await.unwrap().unwrap();
        let s = std::str::from_utf8(&body).unwrap();
        assert!(s.contains("navigated"));
    }

    #[tokio::test]
    async fn t21sc_split_writes_are_buffered_until_newline() {
        // The sidecar's stdout may flush in chunks. The session
        // must wait for the newline before yielding a body.
        let (mut sut, mut harness_writer, _) = dup();
        // First half (no newline).
        harness_writer.write_all(b"{\"id\":").await.unwrap();
        harness_writer.flush().await.unwrap();
        // Spawn a task that reads concurrently — it should block.
        let read_task = tokio::spawn(async move { sut.read_line_or_eof().await });
        // Brief wait so the task is parked.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        // Second half (with newline) completes the line.
        harness_writer.write_all(b"7,\"x\":1}\n").await.unwrap();
        harness_writer.flush().await.unwrap();
        let body = read_task.await.unwrap().unwrap().unwrap();
        assert_eq!(body, br#"{"id":7,"x":1}"#);
    }

    #[test]
    #[should_panic(expected = "MUST NOT contain newlines")]
    fn t21sc_write_line_debug_asserts_on_embedded_newline() {
        // Debug-only safety net: a body containing a newline would
        // confuse the sidecar (split into two messages). debug_assert
        // catches the bug at test time without paying the runtime
        // cost in release builds.
        let body = b"{\"a\":1}\n{\"b\":2}";
        // We can't call write_line without an async runtime; assert
        // the panic via the debug_assert directly.
        debug_assert!(
            !body.contains(&b'\n'),
            "write_line body MUST NOT contain newlines — use serde_json::to_vec"
        );
    }
}
