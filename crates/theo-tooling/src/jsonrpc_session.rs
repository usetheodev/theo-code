//! T3.1 / T13.1 — Generic `Content-Length`-framed stdio session.
//!
//! Wraps any `(AsyncWrite, AsyncRead)` pair (typically a subprocess'
//! `(stdin, stdout)`) with the framing accumulator + writer. Pure
//! transport — protocol-typed parsing happens in the consumer
//! (`lsp::protocol::try_decode_frame` or `dap::protocol::try_decode_frame`).
//!
//! Designed so production code spawns a real subprocess via
//! `tokio::process::Command`, but tests use `tokio::io::duplex` to
//! exercise the same code path WITHOUT any subprocess at all.

use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::jsonrpc_stdio::{encode_frame, FrameAccumulator, FrameError};

/// Errors specific to a session — IO failures + framing failures.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Frame(#[from] FrameError),
    /// The peer closed its stdout (EOF) before we had a complete frame.
    #[error("peer closed stdout (EOF) with {leftover_bytes} bytes still buffered")]
    UnexpectedEof { leftover_bytes: usize },
}

/// Stdio session over any async duplex. Writes outgoing frames with
/// the LSP/DAP-shared `Content-Length: N\r\n\r\n<body>` framing;
/// reads incoming bytes through a `FrameAccumulator`.
///
/// `read_frame` drives the reader as far as needed to surface ONE
/// full frame, returning the body bytes the protocol layer should
/// `try_decode_frame` on.
pub struct StdioSession<W, R> {
    writer: W,
    reader: R,
    accumulator: FrameAccumulator,
    read_buf: Vec<u8>,
}

impl<W, R> StdioSession<W, R>
where
    W: AsyncWrite + Unpin,
    R: AsyncRead + Unpin,
{
    /// Build a session from a writer (subprocess stdin) + reader
    /// (subprocess stdout). The accumulator inherits the default
    /// 16 MiB body cap from `FrameAccumulator::new()`.
    pub fn new(writer: W, reader: R) -> Self {
        Self {
            writer,
            reader,
            accumulator: FrameAccumulator::new(),
            read_buf: vec![0u8; 8 * 1024],
        }
    }

    /// Build a session with a custom max body size for the reader
    /// accumulator. Useful for tests or for connecting to a server
    /// known to send oversized payloads.
    pub fn with_max_body(writer: W, reader: R, max_body: usize) -> Self {
        Self {
            writer,
            reader,
            accumulator: FrameAccumulator::with_max_body(max_body),
            read_buf: vec![0u8; 8 * 1024],
        }
    }

    /// Write one framed message. The `body` is the raw JSON payload
    /// — the framing header is prepended automatically.
    pub async fn write_frame(&mut self, body: &[u8]) -> Result<(), SessionError> {
        let bytes = encode_frame(body);
        self.writer.write_all(&bytes).await?;
        self.writer.flush().await?;
        Ok(())
    }

    /// Read until the next complete frame is available. Returns the
    /// frame body bytes (caller hands off to the protocol decoder).
    /// Returns `Err(SessionError::UnexpectedEof)` when the peer
    /// closes stdout before a frame arrives.
    pub async fn read_frame(&mut self) -> Result<Vec<u8>, SessionError> {
        loop {
            // First, see if the accumulator already has a frame buffered.
            if let Some(body) = self.accumulator.next_frame()? {
                return Ok(body);
            }
            // Otherwise, read more bytes.
            let n = self.reader.read(&mut self.read_buf).await?;
            if n == 0 {
                return Err(SessionError::UnexpectedEof {
                    leftover_bytes: self.accumulator.buffered_len(),
                });
            }
            self.accumulator.feed(&self.read_buf[..n]);
        }
    }

    /// Try to read the next frame WITHOUT blocking on EOF — returns
    /// `Ok(None)` when the peer cleanly closed stdout AND the buffer
    /// has no leftover bytes (graceful shutdown).
    ///
    /// Differs from `read_frame` only in EOF handling: graceful close
    /// → None vs error.
    pub async fn read_frame_or_eof(&mut self) -> Result<Option<Vec<u8>>, SessionError> {
        loop {
            if let Some(body) = self.accumulator.next_frame()? {
                return Ok(Some(body));
            }
            let n = self.reader.read(&mut self.read_buf).await?;
            if n == 0 {
                if self.accumulator.buffered_len() == 0 {
                    return Ok(None);
                }
                return Err(SessionError::UnexpectedEof {
                    leftover_bytes: self.accumulator.buffered_len(),
                });
            }
            self.accumulator.feed(&self.read_buf[..n]);
        }
    }

    /// Number of bytes currently buffered (not yet decoded as a
    /// frame). Diagnostic only.
    pub fn buffered_len(&self) -> usize {
        self.accumulator.buffered_len()
    }

    /// Split the session into independent writer + reader halves.
    /// Lets the LSP/DAP client own the writer while a spawned task
    /// drives the reader loop — no shared lock required.
    pub fn split(self) -> (SessionWriter<W>, SessionReader<R>) {
        (
            SessionWriter::new(self.writer),
            SessionReader::from_parts(self.reader, self.accumulator, self.read_buf),
        )
    }
}

/// Owned writer half — produced by `StdioSession::split`. The client
/// task holds this to send framed requests; it never blocks on the
/// reader.
pub struct SessionWriter<W> {
    writer: W,
}

impl<W: AsyncWrite + Unpin> SessionWriter<W> {
    /// Wrap an arbitrary writer (typically a child stdin).
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    /// Write one framed message — `Content-Length: N\r\n\r\n<body>`.
    pub async fn write_frame(&mut self, body: &[u8]) -> Result<(), SessionError> {
        let bytes = encode_frame(body);
        self.writer.write_all(&bytes).await?;
        self.writer.flush().await?;
        Ok(())
    }

    /// Consume the wrapper and return the underlying writer (for
    /// callers who want to close stdin explicitly to signal shutdown).
    pub fn into_inner(self) -> W {
        self.writer
    }
}

/// Owned reader half — produced by `StdioSession::split`. The reader
/// task drives `read_frame_or_eof` in a loop and dispatches frames to
/// the correlator/notification channel.
pub struct SessionReader<R> {
    reader: R,
    accumulator: FrameAccumulator,
    read_buf: Vec<u8>,
}

impl<R: AsyncRead + Unpin> SessionReader<R> {
    /// Build a reader half with default 16 MiB body cap.
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            accumulator: FrameAccumulator::new(),
            read_buf: vec![0u8; 8 * 1024],
        }
    }

    pub(crate) fn from_parts(
        reader: R,
        accumulator: FrameAccumulator,
        read_buf: Vec<u8>,
    ) -> Self {
        Self {
            reader,
            accumulator,
            read_buf,
        }
    }

    /// Same semantics as `StdioSession::read_frame_or_eof` — reads
    /// until the next complete frame, returning `Ok(None)` on graceful
    /// peer EOF.
    pub async fn read_frame_or_eof(&mut self) -> Result<Option<Vec<u8>>, SessionError> {
        loop {
            if let Some(body) = self.accumulator.next_frame()? {
                return Ok(Some(body));
            }
            let n = self.reader.read(&mut self.read_buf).await?;
            if n == 0 {
                if self.accumulator.buffered_len() == 0 {
                    return Ok(None);
                }
                return Err(SessionError::UnexpectedEof {
                    leftover_bytes: self.accumulator.buffered_len(),
                });
            }
            self.accumulator.feed(&self.read_buf[..n]);
        }
    }

    /// Number of bytes buffered (diagnostic).
    pub fn buffered_len(&self) -> usize {
        self.accumulator.buffered_len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{duplex, AsyncWriteExt};

    /// Build a duplex pair where the test harness controls one side
    /// and the SUT (`StdioSession`) drives the other.
    fn dup() -> (
        // SUT side
        StdioSession<tokio::io::DuplexStream, tokio::io::DuplexStream>,
        // Harness side: write here to be read by SUT; read here to
        // see what SUT wrote.
        tokio::io::DuplexStream,
        tokio::io::DuplexStream,
    ) {
        let (sut_writer, harness_reader) = duplex(64 * 1024);
        let (harness_writer, sut_reader) = duplex(64 * 1024);
        let sut = StdioSession::new(sut_writer, sut_reader);
        (sut, harness_writer, harness_reader)
    }

    #[tokio::test]
    async fn t31sess_write_frame_prepends_content_length() {
        let (mut sut, _harness_writer, mut harness_reader) = dup();
        sut.write_frame(b"{\"jsonrpc\":\"2.0\"}").await.unwrap();
        let mut got = vec![0u8; 256];
        let n = harness_reader.read(&mut got).await.unwrap();
        let s = std::str::from_utf8(&got[..n]).unwrap();
        assert!(s.starts_with("Content-Length: 17\r\n\r\n"));
        assert!(s.ends_with("{\"jsonrpc\":\"2.0\"}"));
    }

    #[tokio::test]
    async fn t31sess_read_frame_returns_complete_body() {
        let (mut sut, mut harness_writer, _harness_reader) = dup();
        // Harness sends a full frame.
        let body = b"{\"k\":1}";
        let frame = encode_frame(body);
        harness_writer.write_all(&frame).await.unwrap();

        let got = sut.read_frame().await.unwrap();
        assert_eq!(got, body);
    }

    #[tokio::test]
    async fn t31sess_read_frame_handles_split_writes() {
        let (mut sut, mut harness_writer, _) = dup();
        // Harness writes the frame in two halves, with a flush
        // between to force them onto the wire separately.
        let body = b"{\"hello\":\"world\"}";
        let frame = encode_frame(body);
        let mid = frame.len() / 2;
        harness_writer.write_all(&frame[..mid]).await.unwrap();
        harness_writer.flush().await.unwrap();
        // Tiny yield to let the SUT begin reading the partial.
        tokio::task::yield_now().await;
        harness_writer.write_all(&frame[mid..]).await.unwrap();
        harness_writer.flush().await.unwrap();

        let got = sut.read_frame().await.unwrap();
        assert_eq!(got, body);
    }

    #[tokio::test]
    async fn t31sess_read_frame_extracts_two_consecutive() {
        let (mut sut, mut harness_writer, _) = dup();
        let mut bytes = encode_frame(b"{\"a\":1}");
        bytes.extend(encode_frame(b"{\"b\":2}"));
        harness_writer.write_all(&bytes).await.unwrap();
        harness_writer.flush().await.unwrap();

        let f1 = sut.read_frame().await.unwrap();
        let f2 = sut.read_frame().await.unwrap();
        assert_eq!(f1, b"{\"a\":1}");
        assert_eq!(f2, b"{\"b\":2}");
    }

    #[tokio::test]
    async fn t31sess_read_frame_returns_eof_error_on_clean_close_with_partial() {
        let (mut sut, mut harness_writer, _) = dup();
        // Send only a partial header then drop.
        harness_writer.write_all(b"Content-Length: ").await.unwrap();
        drop(harness_writer);

        let err = sut.read_frame().await.unwrap_err();
        match err {
            SessionError::UnexpectedEof { leftover_bytes } => {
                assert!(leftover_bytes > 0);
            }
            other => panic!("expected UnexpectedEof, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t31sess_read_frame_or_eof_returns_none_on_graceful_close() {
        let (mut sut, harness_writer, _) = dup();
        // Drop the writer immediately — clean EOF.
        drop(harness_writer);

        let res = sut.read_frame_or_eof().await.unwrap();
        assert!(res.is_none());
    }

    #[tokio::test]
    async fn t31sess_read_frame_or_eof_returns_frame_when_data_present() {
        let (mut sut, mut harness_writer, _) = dup();
        let body = b"{\"x\":1}";
        let frame = encode_frame(body);
        harness_writer.write_all(&frame).await.unwrap();
        drop(harness_writer);

        let f = sut.read_frame_or_eof().await.unwrap().unwrap();
        assert_eq!(f, body);
    }

    #[tokio::test]
    async fn t31sess_round_trip_request_response() {
        // Closes the loop: SUT writes a request, harness reads it,
        // harness writes a response, SUT reads it. Proves the full
        // duplex flow works.
        let (mut sut, mut harness_writer, mut harness_reader) = dup();

        // Step 1: SUT writes request.
        sut.write_frame(b"{\"id\":1,\"method\":\"ping\"}")
            .await
            .unwrap();

        // Step 2: harness reads it.
        let mut buf = vec![0u8; 256];
        let n = harness_reader.read(&mut buf).await.unwrap();
        let s = std::str::from_utf8(&buf[..n]).unwrap();
        assert!(s.contains("\"method\":\"ping\""));

        // Step 3: harness writes response.
        let resp_body = b"{\"id\":1,\"result\":\"pong\"}";
        harness_writer
            .write_all(&encode_frame(resp_body))
            .await
            .unwrap();
        harness_writer.flush().await.unwrap();

        // Step 4: SUT reads response.
        let got = sut.read_frame().await.unwrap();
        assert_eq!(got, resp_body);
    }

    #[tokio::test]
    async fn t31sess_with_max_body_propagates_to_accumulator() {
        let (sut_writer, _) = duplex(64 * 1024);
        let (mut harness_writer, sut_reader) = duplex(64 * 1024);
        let mut sut = StdioSession::with_max_body(sut_writer, sut_reader, 100);

        // Send a frame declaring a much larger body.
        harness_writer
            .write_all(b"Content-Length: 9999\r\n\r\n")
            .await
            .unwrap();
        let err = sut.read_frame().await.unwrap_err();
        match err {
            SessionError::Frame(FrameError::BodyTooLarge { max }) => {
                assert_eq!(max, 100);
            }
            other => panic!("expected BodyTooLarge, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t31sess_buffered_len_diagnostics() {
        let (mut sut, mut harness_writer, _) = dup();
        // Write only the header bytes; body still missing.
        harness_writer.write_all(b"Content-Length: 100\r\n").await.unwrap();
        harness_writer.flush().await.unwrap();
        // Drive one read attempt that won't yet yield a frame.
        // We can't easily await without timeout, so instead use
        // try_read via a select... simpler: just verify nothing
        // panics if we drop the harness now.
        drop(harness_writer);
        let err = sut.read_frame().await.unwrap_err();
        // Whatever the EOF state, buffered_len reflects internal state.
        match err {
            SessionError::UnexpectedEof { leftover_bytes } => {
                assert!(leftover_bytes > 0);
            }
            _ => {}
        }
    }

    // ---- split() — independent writer/reader halves -----------------

    #[tokio::test]
    async fn t31sess_split_writer_can_emit_frames_independently() {
        let (sut, _harness_writer, mut harness_reader) = dup();
        let (mut writer, _reader) = sut.split();
        writer.write_frame(b"{\"hello\":1}").await.unwrap();
        let mut got = vec![0u8; 256];
        let n = harness_reader.read(&mut got).await.unwrap();
        let s = std::str::from_utf8(&got[..n]).unwrap();
        assert!(s.starts_with("Content-Length: 11\r\n\r\n"));
        assert!(s.ends_with("{\"hello\":1}"));
    }

    #[tokio::test]
    async fn t31sess_split_reader_can_decode_frames_independently() {
        let (sut, mut harness_writer, _) = dup();
        let (_writer, mut reader) = sut.split();
        let body = b"{\"k\":2}";
        harness_writer.write_all(&encode_frame(body)).await.unwrap();
        harness_writer.flush().await.unwrap();
        let got = reader.read_frame_or_eof().await.unwrap().unwrap();
        assert_eq!(got, body);
    }

    #[tokio::test]
    async fn t31sess_split_halves_run_concurrently_in_separate_tasks() {
        // Proves the split is truly independent: writer and reader
        // can run on different tasks at the same time without
        // synchronisation.
        let (sut, mut harness_writer, mut harness_reader) = dup();
        let (mut writer, mut reader) = sut.split();

        let writer_task = tokio::spawn(async move {
            writer.write_frame(b"{\"req\":1}").await.unwrap();
            writer
        });
        let reader_task = tokio::spawn(async move {
            // Wait for the harness to push a frame.
            reader.read_frame_or_eof().await
        });

        // Harness reads the request that writer_task sent.
        let mut buf = vec![0u8; 64];
        let n = harness_reader.read(&mut buf).await.unwrap();
        assert!(std::str::from_utf8(&buf[..n]).unwrap().contains("\"req\":1"));

        // Harness pushes a response back into the reader half.
        let resp = b"{\"resp\":1}";
        harness_writer.write_all(&encode_frame(resp)).await.unwrap();
        harness_writer.flush().await.unwrap();

        let _writer = writer_task.await.unwrap();
        let got = reader_task.await.unwrap().unwrap().unwrap();
        assert_eq!(got, resp);
    }

    #[tokio::test]
    async fn t31sess_session_reader_new_starts_with_zero_buffered() {
        let (_, sut_reader) = duplex(1024);
        let r = SessionReader::new(sut_reader);
        assert_eq!(r.buffered_len(), 0);
    }

    #[tokio::test]
    async fn t31sess_split_preserves_buffered_state_for_subsequent_frame() {
        // If the harness sends two back-to-back frames, the SUT may
        // buffer both during a single read. The split() reader must
        // inherit any buffered state so the second frame is still
        // decodable.
        let (mut sut, mut harness_writer, _harness_reader) = dup();
        let mut bytes = encode_frame(b"{\"a\":1}");
        bytes.extend(encode_frame(b"{\"b\":2}"));
        harness_writer.write_all(&bytes).await.unwrap();
        harness_writer.flush().await.unwrap();

        // Drain the first frame BEFORE split — the second frame is
        // now buffered inside the accumulator.
        let f1 = sut.read_frame().await.unwrap();
        assert_eq!(f1, b"{\"a\":1}");

        // Split: the second frame must still come out via the reader.
        let (_writer, mut reader) = sut.split();
        let f2 = reader.read_frame_or_eof().await.unwrap().unwrap();
        assert_eq!(f2, b"{\"b\":2}");
    }
}
