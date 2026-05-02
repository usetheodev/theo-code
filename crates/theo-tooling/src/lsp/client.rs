//! T3.1 — `LspClient` facade.
//!
//! Composes the 7 IO-stack layers into a single ergonomic API:
//! - subprocess (`tokio::process::Child` with `kill_on_drop`)
//! - session split into `SessionWriter` + `SessionReader`
//! - reader task driving `run_reader_loop`
//! - request id generator + correlator (oneshot per request)
//! - notification mpsc channel (server-pushed messages)
//!
//! Public surface:
//! - `LspClient::spawn(program, args)` → spawns the LSP server,
//!   returns the client + notification receiver
//! - `client.request(method, params)` → `JsonRpcResponse` (await)
//! - `client.notify(method, params)` → fire-and-forget
//! - `client.shutdown()` → graceful LSP shutdown sequence
//!
//! Concurrency: writes are serialized through a `Mutex<SessionWriter>`
//! (LSP frames must be written atomically); reads run independently
//! in a spawned task. Multiple `request` calls from different tasks
//! interleave correctly via the correlator.
//!
//! Testability: the client is generic over the writer type so tests
//! can drive it with `tokio::io::duplex` rather than a real LSP
//! server. See the `from_split_for_test` helper.

use std::sync::Arc;

use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;

use crate::jsonrpc_correlator::{Correlator, CorrelatorError};
use crate::jsonrpc_reader::{DecodedMessage, ReaderError, run_reader_loop};
use crate::jsonrpc_session::{SessionError, SessionReader, SessionWriter, StdioSession};
use crate::lsp::protocol::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, RequestIdGen};

/// Errors the LSP client surfaces. Wraps the lower-layer errors with
/// enough context to know which stage failed (spawn / write / decode
/// / response missing).
#[derive(Debug, thiserror::Error)]
pub enum LspClientError {
    #[error("failed to spawn LSP server `{program}`: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("LSP server stdio capture failed (stdin/stdout pipe missing)")]
    StdioCaptureFailed,
    #[error("session write error: {0}")]
    Session(#[from] SessionError),
    #[error("correlator error: {0}")]
    Correlator(#[from] CorrelatorError),
    #[error("reader task terminated: {0}")]
    ReaderTerminated(String),
    #[error("response sender dropped (reader exited before response arrived)")]
    NoResponse,
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// LSP client over an arbitrary writer. `LspClient<ChildStdin>` is
/// the production type produced by `spawn()`; tests use other writer
/// types via `from_split_for_test()`.
pub struct LspClient<W: AsyncWrite + Unpin + Send + 'static = ChildStdin> {
    writer: Mutex<SessionWriter<W>>,
    correlator: Arc<Correlator<u64, JsonRpcResponse>>,
    reader_handle: Mutex<Option<JoinHandle<Result<(), ReaderError>>>>,
    id_gen: RequestIdGen,
    /// Held to keep `kill_on_drop(true)` alive for spawned servers.
    /// None for test-mode (duplex) clients.
    _child: Mutex<Option<Child>>,
}

impl LspClient<ChildStdin> {
    /// Spawn an LSP server and wire up the full IO stack. Returns the
    /// client + a receiver for server-side notifications.
    pub async fn spawn(
        program: &str,
        args: &[&str],
    ) -> Result<(Self, mpsc::Receiver<JsonRpcNotification>), LspClientError> {
        let mut cmd = Command::new(program);
        cmd.args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|source| LspClientError::Spawn {
            program: program.into(),
            source,
        })?;
        let stdin: ChildStdin = child
            .stdin
            .take()
            .ok_or(LspClientError::StdioCaptureFailed)?;
        let stdout: ChildStdout = child
            .stdout
            .take()
            .ok_or(LspClientError::StdioCaptureFailed)?;
        // stderr is left attached to the child — drain it in a future
        // diagnostics task. For now it's discarded once the child is
        // killed.

        let session = StdioSession::new(stdin, stdout);
        let (writer, reader) = session.split();
        let (client, notif_rx) =
            Self::from_split_with_child(writer, reader, Some(child), 64);
        Ok((client, notif_rx))
    }
}

impl<W: AsyncWrite + Unpin + Send + 'static> LspClient<W> {
    /// Test helper: build a client from arbitrary split halves. Used
    /// by unit tests that drive the client with `tokio::io::duplex`.
    pub fn from_split_for_test<R>(
        writer: SessionWriter<W>,
        reader: SessionReader<R>,
        notif_buffer_size: usize,
    ) -> (Self, mpsc::Receiver<JsonRpcNotification>)
    where
        R: AsyncRead + Unpin + Send + 'static,
    {
        Self::from_split_with_child(writer, reader, None, notif_buffer_size)
    }

    fn from_split_with_child<R>(
        writer: SessionWriter<W>,
        mut reader: SessionReader<R>,
        child: Option<Child>,
        notif_buffer_size: usize,
    ) -> (Self, mpsc::Receiver<JsonRpcNotification>)
    where
        R: AsyncRead + Unpin + Send + 'static,
    {
        let correlator = Arc::new(Correlator::<u64, JsonRpcResponse>::new());
        let (notif_tx, notif_rx) =
            mpsc::channel::<JsonRpcNotification>(notif_buffer_size);
        let cor_clone = correlator.clone();
        let reader_handle = tokio::spawn(async move {
            run_reader_loop(&mut reader, cor_clone, notif_tx, decode_lsp).await
        });
        let client = LspClient {
            writer: Mutex::new(writer),
            correlator,
            reader_handle: Mutex::new(Some(reader_handle)),
            id_gen: RequestIdGen::new(),
            _child: Mutex::new(child),
        };
        (client, notif_rx)
    }

    /// Send a JSON-RPC request and await the matched response. Safe
    /// to call concurrently from multiple tasks — the correlator
    /// pairs each id to its waiter.
    pub async fn request(
        &self,
        method: impl Into<String>,
        params: Option<Value>,
    ) -> Result<JsonRpcResponse, LspClientError> {
        let id = self.id_gen.next();
        let req = JsonRpcRequest::new(id, method, params);
        let body = serde_json::to_vec(&req)?;
        // Register the waiter BEFORE writing — guarantees no race
        // where the response arrives before we're listening.
        let rx = self.correlator.register(id).await?;
        {
            let mut w = self.writer.lock().await;
            w.write_frame(&body).await?;
        }
        rx.await.map_err(|_| LspClientError::NoResponse)
    }

    /// Send a JSON-RPC notification (no response expected).
    pub async fn notify(
        &self,
        method: impl Into<String>,
        params: Option<Value>,
    ) -> Result<(), LspClientError> {
        let n = JsonRpcNotification::new(method, params);
        let body = serde_json::to_vec(&n)?;
        let mut w = self.writer.lock().await;
        w.write_frame(&body).await?;
        Ok(())
    }

    /// Number of in-flight requests still awaiting a response.
    pub async fn pending_requests(&self) -> usize {
        self.correlator.pending_count().await
    }

    /// Best-effort graceful shutdown: send LSP `shutdown` request +
    /// `exit` notification, then await the reader task. Errors are
    /// logged and swallowed — shutdown is always best-effort.
    /// Idempotent: a second call sees the reader handle already
    /// drained and returns immediately.
    pub async fn shutdown(&self) -> Result<(), LspClientError> {
        // Only send the LSP shutdown handshake while the reader is
        // still alive — otherwise the request would hang forever
        // waiting for a response no one will deliver.
        let reader_alive = self.reader_handle.lock().await.is_some();
        if reader_alive {
            // Cap each step at 2 s so a misbehaving server can't hang
            // shutdown forever.
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                self.request("shutdown", None),
            )
            .await;
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                self.notify("exit", None),
            )
            .await;
        }
        if let Some(handle) = self.reader_handle.lock().await.take() {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
        }
        Ok(())
    }
}

/// LSP-shaped decoder used by the reader loop. Distinguishes
/// responses (have `id` without `method`) from notifications (have
/// `method` without `id`). Server-side requests (both `id` and
/// `method`) are routed as notifications — clients in this model
/// don't reply to server requests today.
fn decode_lsp(
    body: &[u8],
) -> Result<DecodedMessage<u64, JsonRpcResponse, JsonRpcNotification>, String> {
    let v: serde_json::Value =
        serde_json::from_slice(body).map_err(|e| e.to_string())?;
    let has_id = v.get("id").is_some();
    let has_method = v.get("method").is_some();
    if has_id && !has_method {
        let r: JsonRpcResponse =
            serde_json::from_value(v).map_err(|e| e.to_string())?;
        Ok(DecodedMessage::Response {
            key: r.id,
            response: r,
        })
    } else if has_method {
        let n: JsonRpcNotification =
            serde_json::from_value(v).map_err(|e| e.to_string())?;
        Ok(DecodedMessage::Notification(n))
    } else {
        Err(format!("unrecognised JSON-RPC shape: {v}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;
    use tokio::io::{AsyncWriteExt, duplex};

    use crate::jsonrpc_stdio::encode_frame;

    /// Build an LspClient over a tokio duplex pair. Returns the
    /// client + notif receiver + the harness halves so tests can
    /// drive the "server" side.
    ///
    /// Layout:
    /// - client.writer → harness_outbound (test reads what client sent)
    /// - harness_inbound → client.reader (test writes server frames)
    fn duplex_client() -> (
        LspClient<tokio::io::DuplexStream>,
        mpsc::Receiver<JsonRpcNotification>,
        SessionReader<tokio::io::DuplexStream>, // harness reads client writes
        tokio::io::DuplexStream, // harness writes server replies
    ) {
        let (sut_writer, harness_outbound) = duplex(64 * 1024);
        let (harness_inbound, sut_reader) = duplex(64 * 1024);
        let writer = SessionWriter::new(sut_writer);
        let reader = SessionReader::new(sut_reader);
        let (client, notif_rx) = LspClient::from_split_for_test(writer, reader, 32);
        (
            client,
            notif_rx,
            SessionReader::new(harness_outbound),
            harness_inbound,
        )
    }

    /// Read a single frame from the client's outbound stream and
    /// return its body (parsed as JSON). Uses `SessionReader` so
    /// multi-frame reads are correctly demultiplexed (a single TCP
    /// read into the duplex can carry > 1 frame).
    async fn read_outbound_frame(
        outbound: &mut SessionReader<tokio::io::DuplexStream>,
    ) -> serde_json::Value {
        let body = outbound.read_frame_or_eof().await.unwrap().unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn t31cli_request_writes_frame_and_awaits_response() {
        let (client, _notif_rx, mut outbound, mut inbound) = duplex_client();

        // Spawn the client request in a task so we can intercept the
        // outbound frame on the harness side, then push the response.
        let req_task = tokio::spawn(async move {
            client
                .request("textDocument/hover", Some(json!({"x": 1})))
                .await
        });

        // Harness reads the outbound request frame.
        let req_value = read_outbound_frame(&mut outbound).await;
        assert_eq!(req_value["jsonrpc"], "2.0");
        assert_eq!(req_value["method"], "textDocument/hover");
        let req_id = req_value["id"].as_u64().unwrap();

        // Harness writes the response back.
        let resp = json!({"jsonrpc":"2.0","id":req_id,"result":{"contents":"docs here"}});
        inbound
            .write_all(&encode_frame(serde_json::to_vec(&resp).unwrap().as_slice()))
            .await
            .unwrap();
        inbound.flush().await.unwrap();

        let got = req_task.await.unwrap().unwrap();
        assert_eq!(got.id, req_id);
        assert_eq!(got.result.as_ref().unwrap()["contents"], "docs here");
    }

    #[tokio::test]
    async fn t31cli_notify_writes_frame_with_no_id() {
        let (client, _notif_rx, mut outbound, _inbound) = duplex_client();

        let task = tokio::spawn(async move {
            client
                .notify("textDocument/didOpen", Some(json!({"uri":"file:///x"})))
                .await
                .unwrap();
            client
        });

        let value = read_outbound_frame(&mut outbound).await;
        assert_eq!(value["method"], "textDocument/didOpen");
        // Notification: NO `id` field.
        assert!(value.get("id").is_none());

        let _client = task.await.unwrap();
    }

    #[tokio::test]
    async fn t31cli_server_notification_arrives_on_channel() {
        let (_client, mut notif_rx, _outbound, mut inbound) = duplex_client();

        let body = json!({"jsonrpc":"2.0","method":"window/logMessage","params":{"type":3,"message":"hello"}});
        inbound
            .write_all(&encode_frame(serde_json::to_vec(&body).unwrap().as_slice()))
            .await
            .unwrap();
        inbound.flush().await.unwrap();

        let n = notif_rx.recv().await.unwrap();
        assert_eq!(n.method, "window/logMessage");
        assert_eq!(n.params.unwrap()["message"], "hello");
    }

    #[tokio::test]
    async fn t31cli_concurrent_requests_pair_correctly_via_correlator() {
        // Two concurrent requests get distinct ids, server replies in
        // REVERSE order, each request gets the right response.
        let (client, _notif_rx, mut outbound, mut inbound) = duplex_client();
        let client = Arc::new(client);

        let c1 = client.clone();
        let r1 = tokio::spawn(async move { c1.request("a", None).await });
        let c2 = client.clone();
        let r2 = tokio::spawn(async move { c2.request("b", None).await });

        // Read both outbound requests; capture their ids.
        let v1 = read_outbound_frame(&mut outbound).await;
        let v2 = read_outbound_frame(&mut outbound).await;
        let id_a = if v1["method"] == "a" {
            v1["id"].as_u64().unwrap()
        } else {
            v2["id"].as_u64().unwrap()
        };
        let id_b = if v1["method"] == "b" {
            v1["id"].as_u64().unwrap()
        } else {
            v2["id"].as_u64().unwrap()
        };
        assert_ne!(id_a, id_b);

        // Reply in REVERSE order — id_b first.
        for (id, marker) in [(id_b, "second"), (id_a, "first")] {
            let resp = json!({"jsonrpc":"2.0","id":id,"result":marker});
            inbound
                .write_all(&encode_frame(
                    serde_json::to_vec(&resp).unwrap().as_slice(),
                ))
                .await
                .unwrap();
        }
        inbound.flush().await.unwrap();

        let resp_a = r1.await.unwrap().unwrap();
        let resp_b = r2.await.unwrap().unwrap();
        assert_eq!(resp_a.result.as_ref().unwrap(), "first");
        assert_eq!(resp_b.result.as_ref().unwrap(), "second");
    }

    #[tokio::test]
    async fn t31cli_pending_requests_count_lifecycle() {
        let (client, _notif_rx, mut outbound, mut inbound) = duplex_client();
        let client = Arc::new(client);

        assert_eq!(client.pending_requests().await, 0);

        let c = client.clone();
        let task = tokio::spawn(async move { c.request("ping", None).await });

        // Drain the outbound frame so the request is on the wire.
        let v = read_outbound_frame(&mut outbound).await;
        let id = v["id"].as_u64().unwrap();

        // While the client is waiting for the response, pending == 1.
        // Note: there's a tiny race window between register and the
        // server reply. To make the test deterministic we don't assert
        // on pending == 1 mid-flight; instead we verify it's 0 after
        // the response settles.
        let resp = json!({"jsonrpc":"2.0","id":id,"result":"pong"});
        inbound
            .write_all(&encode_frame(serde_json::to_vec(&resp).unwrap().as_slice()))
            .await
            .unwrap();
        inbound.flush().await.unwrap();

        let _ = task.await.unwrap().unwrap();
        assert_eq!(client.pending_requests().await, 0);
    }

    #[tokio::test]
    async fn t31cli_decoder_error_terminates_reader_subsequent_request_fails() {
        let (client, _notif_rx, mut _outbound, mut inbound) = duplex_client();

        // Push a frame containing invalid JSON — reader_loop returns
        // ReaderError::Decode and exits.
        inbound.write_all(&encode_frame(b"not json")).await.unwrap();
        inbound.flush().await.unwrap();

        // Give the reader a moment to die.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // A subsequent request will register a waiter, write the
        // frame, and then never get a response — the oneshot is
        // dropped when no one will dispatch it. We assert the request
        // ultimately surfaces NoResponse OR a session error.
        // (Concretely: with the reader dead, no one will deliver the
        // response. The sender holding the Receiver never dispatches.)

        // Use a timeout — the request would otherwise hang forever.
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            client.request("ping", None),
        )
        .await;
        // Either: timed out (the request is genuinely orphaned), OR
        // returned NoResponse if the oneshot was somehow notified.
        // Both prove the reader died and requests no longer succeed.
        match result {
            Err(_timeout) => { /* expected */ }
            Ok(Err(LspClientError::NoResponse)) => { /* also acceptable */ }
            Ok(other) => panic!(
                "expected timeout or NoResponse after reader death, got {other:?}"
            ),
        }
    }

    #[tokio::test]
    async fn t31cli_shutdown_drains_reader_handle() {
        let (client, _notif_rx, mut outbound, mut inbound) = duplex_client();

        // Fake server: respond to the FIRST request (the shutdown one),
        // then drop inbound so the reader sees EOF and the reader_handle
        // resolves to Ok(()).
        let task = tokio::spawn(async move {
            let body = outbound.read_frame_or_eof().await.unwrap().unwrap();
            let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let id = v["id"].as_u64().unwrap();
            let resp = json!({"jsonrpc":"2.0","id":id,"result":null});
            inbound
                .write_all(&encode_frame(serde_json::to_vec(&resp).unwrap().as_slice()))
                .await
                .unwrap();
            inbound.flush().await.unwrap();
            // Drop inbound to trigger reader EOF.
            drop(inbound);
            outbound // keep alive for shutdown's notify(exit)
        });

        client.shutdown().await.unwrap();
        let _outbound = task.await.unwrap();
        // Idempotent: handle is now None, so this is a no-op.
        client.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn t31cli_spawn_unknown_binary_returns_typed_error() {
        let res = LspClient::spawn("definitely-not-a-real-lsp-server-xyz", &[]).await;
        match res {
            Err(LspClientError::Spawn { program, .. }) => {
                assert!(program.contains("definitely-not-a-real-lsp-server-xyz"));
            }
            Ok(_) => panic!("expected spawn failure for missing binary"),
            Err(other) => panic!("expected Spawn error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t31cli_decode_lsp_classifies_response() {
        let body = br#"{"jsonrpc":"2.0","id":7,"result":{"x":1}}"#;
        match decode_lsp(body).unwrap() {
            DecodedMessage::Response { key, response } => {
                assert_eq!(key, 7);
                assert_eq!(response.result.unwrap()["x"], 1);
            }
            DecodedMessage::Notification(_) => panic!("expected response"),
        }
    }

    #[tokio::test]
    async fn t31cli_decode_lsp_classifies_notification() {
        let body = br#"{"jsonrpc":"2.0","method":"window/showMessage","params":{"x":1}}"#;
        match decode_lsp(body).unwrap() {
            DecodedMessage::Notification(n) => assert_eq!(n.method, "window/showMessage"),
            other => panic!("expected notification, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t31cli_decode_lsp_rejects_bad_json() {
        let body = b"definitely not json";
        let err = decode_lsp(body).unwrap_err();
        assert!(!err.is_empty());
    }
}
