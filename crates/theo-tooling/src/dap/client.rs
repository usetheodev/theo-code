//! T13.1 — `DapClient` facade.
//!
//! Mirror of `LspClient` for the Debug Adapter Protocol. Composes the
//! same 7 IO-stack layers but with DAP's distinct message model:
//!
//! - **Request** carries a `seq` — the server's response carries
//!   `request_seq` matching that value (correlation key).
//! - **Response** has `success: bool` instead of error/result split.
//! - **Event** is the unsolicited message type (`stopped`, `output`,
//!   `terminated`) — routed to the events `mpsc::Receiver`.
//!
//! Public surface mirrors LspClient:
//! - `DapClient::spawn(program, args)` → `(client, mpsc::Receiver<DapEvent>)`
//! - `client.request(command, arguments)` → `DapResponse` (await)
//! - `client.shutdown()` → graceful disconnect + reader drain
//!
//! Tests use `tokio::io::duplex` via `from_split_for_test()`.

use std::sync::Arc;

use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;

use crate::dap::protocol::{DapEvent, DapRequest, DapResponse, DapSeqGen};
use crate::jsonrpc_correlator::{Correlator, CorrelatorError};
use crate::jsonrpc_reader::{DecodedMessage, ReaderError, run_reader_loop};
use crate::jsonrpc_session::{SessionError, SessionReader, SessionWriter, StdioSession};

/// Errors the DAP client surfaces.
#[derive(Debug, thiserror::Error)]
pub enum DapClientError {
    #[error("failed to spawn DAP server `{program}`: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("DAP server stdio capture failed (stdin/stdout pipe missing)")]
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

/// DAP client over an arbitrary writer. `DapClient<ChildStdin>` is
/// the production type produced by `spawn()`; tests use other writer
/// types via `from_split_for_test()`.
pub struct DapClient<W: AsyncWrite + Unpin + Send + 'static = ChildStdin> {
    writer: Mutex<SessionWriter<W>>,
    correlator: Arc<Correlator<u64, DapResponse>>,
    reader_handle: Mutex<Option<JoinHandle<Result<(), ReaderError>>>>,
    seq_gen: DapSeqGen,
    /// Held to keep `kill_on_drop(true)` alive for spawned servers.
    /// None for test-mode (duplex) clients.
    _child: Mutex<Option<Child>>,
}

impl DapClient<ChildStdin> {
    /// Spawn a DAP server and wire up the full IO stack. Returns the
    /// client + a receiver for server-side events.
    pub async fn spawn(
        program: &str,
        args: &[&str],
    ) -> Result<(Self, mpsc::Receiver<DapEvent>), DapClientError> {
        let mut cmd = Command::new(program);
        cmd.args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|source| DapClientError::Spawn {
            program: program.into(),
            source,
        })?;
        let stdin: ChildStdin = child
            .stdin
            .take()
            .ok_or(DapClientError::StdioCaptureFailed)?;
        let stdout: ChildStdout = child
            .stdout
            .take()
            .ok_or(DapClientError::StdioCaptureFailed)?;

        let session = StdioSession::new(stdin, stdout);
        let (writer, reader) = session.split();
        let (client, evt_rx) = Self::from_split_with_child(writer, reader, Some(child), 64);
        Ok((client, evt_rx))
    }
}

impl<W: AsyncWrite + Unpin + Send + 'static> DapClient<W> {
    /// Test helper: build a client from arbitrary split halves.
    pub fn from_split_for_test<R>(
        writer: SessionWriter<W>,
        reader: SessionReader<R>,
        evt_buffer_size: usize,
    ) -> (Self, mpsc::Receiver<DapEvent>)
    where
        R: AsyncRead + Unpin + Send + 'static,
    {
        Self::from_split_with_child(writer, reader, None, evt_buffer_size)
    }

    fn from_split_with_child<R>(
        writer: SessionWriter<W>,
        mut reader: SessionReader<R>,
        child: Option<Child>,
        evt_buffer_size: usize,
    ) -> (Self, mpsc::Receiver<DapEvent>)
    where
        R: AsyncRead + Unpin + Send + 'static,
    {
        let correlator = Arc::new(Correlator::<u64, DapResponse>::new());
        let (evt_tx, evt_rx) = mpsc::channel::<DapEvent>(evt_buffer_size);
        let cor_clone = correlator.clone();
        let reader_handle = tokio::spawn(async move {
            run_reader_loop(&mut reader, cor_clone, evt_tx, decode_dap).await
        });
        let client = DapClient {
            writer: Mutex::new(writer),
            correlator,
            reader_handle: Mutex::new(Some(reader_handle)),
            seq_gen: DapSeqGen::new(),
            _child: Mutex::new(child),
        };
        (client, evt_rx)
    }

    /// Send a DAP request and await the matched response. Safe to
    /// call concurrently — the correlator pairs each `request_seq`
    /// to its waiter.
    pub async fn request(
        &self,
        command: impl Into<String>,
        arguments: Option<Value>,
    ) -> Result<DapResponse, DapClientError> {
        let seq = self.seq_gen.next();
        let req = DapRequest::new(seq, command, arguments);
        let body = serde_json::to_vec(&req)?;
        // Register BEFORE writing — race-safe.
        let rx = self.correlator.register(seq).await?;
        {
            let mut w = self.writer.lock().await;
            w.write_frame(&body).await?;
        }
        rx.await.map_err(|_| DapClientError::NoResponse)
    }

    /// Number of in-flight requests still awaiting a response.
    pub async fn pending_requests(&self) -> usize {
        self.correlator.pending_count().await
    }

    /// Best-effort graceful shutdown: send DAP `disconnect` request,
    /// then await the reader task. Idempotent + timeout-bounded.
    pub async fn shutdown(&self) -> Result<(), DapClientError> {
        let reader_alive = self.reader_handle.lock().await.is_some();
        if reader_alive {
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                self.request("disconnect", None),
            )
            .await;
        }
        if let Some(handle) = self.reader_handle.lock().await.take() {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
        }
        Ok(())
    }
}

/// DAP-shaped decoder used by the reader loop. Distinguishes
/// responses (`type: "response"`, key = `request_seq`) from events
/// (`type: "event"`).
fn decode_dap(body: &[u8]) -> Result<DecodedMessage<u64, DapResponse, DapEvent>, String> {
    let v: serde_json::Value = serde_json::from_slice(body).map_err(|e| e.to_string())?;
    let ty = v
        .get("type")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "missing `type` field".to_string())?;
    match ty {
        "response" => {
            let r: DapResponse =
                serde_json::from_value(v).map_err(|e| e.to_string())?;
            Ok(DecodedMessage::Response {
                key: r.request_seq,
                response: r,
            })
        }
        "event" => {
            let e: DapEvent = serde_json::from_value(v).map_err(|e| e.to_string())?;
            Ok(DecodedMessage::Notification(e))
        }
        // DAP requests from server → client (reverse-request) are rare;
        // route as event so they don't crash the loop.
        "request" => {
            let pseudo_event = DapEvent {
                seq: v.get("seq").and_then(|s| s.as_u64()).unwrap_or(0),
                message_type: "event".into(),
                event: format!(
                    "reverse_request:{}",
                    v.get("command").and_then(|c| c.as_str()).unwrap_or("?")
                ),
                body: v.get("arguments").cloned(),
            };
            Ok(DecodedMessage::Notification(pseudo_event))
        }
        other => Err(format!("unknown DAP message type: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;
    use tokio::io::{AsyncWriteExt, duplex};

    use crate::jsonrpc_stdio::encode_frame;

    /// Build a DapClient over a tokio duplex pair.
    fn duplex_client() -> (
        DapClient<tokio::io::DuplexStream>,
        mpsc::Receiver<DapEvent>,
        SessionReader<tokio::io::DuplexStream>, // harness reads client writes
        tokio::io::DuplexStream,                // harness writes server replies
    ) {
        let (sut_writer, harness_outbound) = duplex(64 * 1024);
        let (harness_inbound, sut_reader) = duplex(64 * 1024);
        let writer = SessionWriter::new(sut_writer);
        let reader = SessionReader::new(sut_reader);
        let (client, evt_rx) = DapClient::from_split_for_test(writer, reader, 32);
        (
            client,
            evt_rx,
            SessionReader::new(harness_outbound),
            harness_inbound,
        )
    }

    async fn read_outbound_frame(
        outbound: &mut SessionReader<tokio::io::DuplexStream>,
    ) -> serde_json::Value {
        let body = outbound.read_frame_or_eof().await.unwrap().unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn t131cli_request_writes_request_frame_and_awaits_response() {
        let (client, _evt_rx, mut outbound, mut inbound) = duplex_client();

        let req_task = tokio::spawn(async move {
            client
                .request("setBreakpoints", Some(json!({"source": {"path": "/a.rs"}})))
                .await
        });

        let req_value = read_outbound_frame(&mut outbound).await;
        assert_eq!(req_value["type"], "request");
        assert_eq!(req_value["command"], "setBreakpoints");
        let req_seq = req_value["seq"].as_u64().unwrap();

        // Reply: type=response, request_seq matches, success=true.
        let resp = json!({
            "seq": 100,
            "type": "response",
            "request_seq": req_seq,
            "command": "setBreakpoints",
            "success": true,
            "body": {"breakpoints": [{"verified": true}]}
        });
        inbound
            .write_all(&encode_frame(serde_json::to_vec(&resp).unwrap().as_slice()))
            .await
            .unwrap();
        inbound.flush().await.unwrap();

        let got = req_task.await.unwrap().unwrap();
        assert_eq!(got.request_seq, req_seq);
        assert!(got.success);
        assert_eq!(got.body.unwrap()["breakpoints"][0]["verified"], true);
    }

    #[tokio::test]
    async fn t131cli_request_failure_response_is_returned_with_message() {
        let (client, _evt_rx, mut outbound, mut inbound) = duplex_client();

        let task = tokio::spawn(async move { client.request("evaluate", None).await });
        let req = read_outbound_frame(&mut outbound).await;
        let req_seq = req["seq"].as_u64().unwrap();

        // Server replies with success=false and an error message.
        let resp = json!({
            "seq": 200,
            "type": "response",
            "request_seq": req_seq,
            "command": "evaluate",
            "success": false,
            "message": "expression not evaluable in current context"
        });
        inbound
            .write_all(&encode_frame(serde_json::to_vec(&resp).unwrap().as_slice()))
            .await
            .unwrap();
        inbound.flush().await.unwrap();

        let got = task.await.unwrap().unwrap();
        assert!(!got.success);
        assert_eq!(
            got.message.as_deref().unwrap(),
            "expression not evaluable in current context"
        );
    }

    #[tokio::test]
    async fn t131cli_event_arrives_on_event_channel() {
        let (_client, mut evt_rx, _outbound, mut inbound) = duplex_client();

        let body = json!({
            "seq": 5,
            "type": "event",
            "event": "stopped",
            "body": {"reason": "breakpoint", "threadId": 1}
        });
        inbound
            .write_all(&encode_frame(serde_json::to_vec(&body).unwrap().as_slice()))
            .await
            .unwrap();
        inbound.flush().await.unwrap();

        let evt = evt_rx.recv().await.unwrap();
        assert_eq!(evt.event, "stopped");
        assert_eq!(evt.body.unwrap()["reason"], "breakpoint");
    }

    #[tokio::test]
    async fn t131cli_concurrent_requests_pair_correctly_via_request_seq() {
        let (client, _evt_rx, mut outbound, mut inbound) = duplex_client();
        let client = Arc::new(client);

        let c1 = client.clone();
        let r1 = tokio::spawn(async move { c1.request("a", None).await });
        let c2 = client.clone();
        let r2 = tokio::spawn(async move { c2.request("b", None).await });

        let v1 = read_outbound_frame(&mut outbound).await;
        let v2 = read_outbound_frame(&mut outbound).await;
        let seq_a = if v1["command"] == "a" {
            v1["seq"].as_u64().unwrap()
        } else {
            v2["seq"].as_u64().unwrap()
        };
        let seq_b = if v1["command"] == "b" {
            v1["seq"].as_u64().unwrap()
        } else {
            v2["seq"].as_u64().unwrap()
        };
        assert_ne!(seq_a, seq_b);

        // Reply in REVERSE order — by request_seq.
        for (rseq, marker) in [(seq_b, "second"), (seq_a, "first")] {
            let resp = json!({
                "seq": 1000 + rseq,
                "type": "response",
                "request_seq": rseq,
                "command": "x",
                "success": true,
                "body": marker,
            });
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
        assert_eq!(resp_a.body.unwrap(), "first");
        assert_eq!(resp_b.body.unwrap(), "second");
    }

    #[tokio::test]
    async fn t131cli_pending_requests_count_lifecycle() {
        let (client, _evt_rx, mut outbound, mut inbound) = duplex_client();
        let client = Arc::new(client);

        assert_eq!(client.pending_requests().await, 0);

        let c = client.clone();
        let task = tokio::spawn(async move { c.request("threads", None).await });
        let v = read_outbound_frame(&mut outbound).await;
        let seq = v["seq"].as_u64().unwrap();

        let resp = json!({
            "seq": 50,
            "type": "response",
            "request_seq": seq,
            "command": "threads",
            "success": true,
            "body": {"threads": []}
        });
        inbound
            .write_all(&encode_frame(serde_json::to_vec(&resp).unwrap().as_slice()))
            .await
            .unwrap();
        inbound.flush().await.unwrap();

        let _ = task.await.unwrap().unwrap();
        assert_eq!(client.pending_requests().await, 0);
    }

    #[tokio::test]
    async fn t131cli_shutdown_drains_reader_handle() {
        let (client, _evt_rx, mut outbound, mut inbound) = duplex_client();

        // Fake server: respond to disconnect, then drop inbound.
        let task = tokio::spawn(async move {
            let body = outbound.read_frame_or_eof().await.unwrap().unwrap();
            let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let req_seq = v["seq"].as_u64().unwrap();
            let resp = json!({
                "seq": 999,
                "type": "response",
                "request_seq": req_seq,
                "command": "disconnect",
                "success": true,
            });
            inbound
                .write_all(&encode_frame(serde_json::to_vec(&resp).unwrap().as_slice()))
                .await
                .unwrap();
            inbound.flush().await.unwrap();
            drop(inbound);
            outbound
        });

        client.shutdown().await.unwrap();
        let _outbound = task.await.unwrap();
        // Idempotent.
        client.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn t131cli_spawn_unknown_binary_returns_typed_error() {
        let res = DapClient::spawn("definitely-not-a-real-dap-server-xyz", &[]).await;
        match res {
            Err(DapClientError::Spawn { program, .. }) => {
                assert!(program.contains("definitely-not-a-real-dap-server-xyz"));
            }
            Ok(_) => panic!("expected spawn failure for missing binary"),
            Err(other) => panic!("expected Spawn error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t131cli_decode_dap_classifies_response_via_request_seq() {
        let body = br#"{"seq":1,"type":"response","request_seq":42,"command":"x","success":true}"#;
        match decode_dap(body).unwrap() {
            DecodedMessage::Response { key, response } => {
                // Key is request_seq, NOT seq — this distinguishes
                // DAP from LSP and is the source of many bugs in
                // hand-rolled clients.
                assert_eq!(key, 42);
                assert_eq!(response.seq, 1);
                assert_eq!(response.command, "x");
            }
            other => panic!("expected response, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t131cli_decode_dap_classifies_event() {
        let body = br#"{"seq":2,"type":"event","event":"output","body":{"output":"hi"}}"#;
        match decode_dap(body).unwrap() {
            DecodedMessage::Notification(e) => {
                assert_eq!(e.event, "output");
                assert_eq!(e.body.unwrap()["output"], "hi");
            }
            other => panic!("expected event, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t131cli_decode_dap_handles_reverse_request_as_pseudo_event() {
        // Some DAP servers send `runInTerminal` as a server→client
        // request. We surface it as an event with prefixed name so the
        // reader loop doesn't die.
        let body = br#"{"seq":3,"type":"request","command":"runInTerminal","arguments":{"args":["sh"]}}"#;
        match decode_dap(body).unwrap() {
            DecodedMessage::Notification(e) => {
                assert_eq!(e.event, "reverse_request:runInTerminal");
                assert_eq!(e.body.unwrap()["args"][0], "sh");
            }
            other => panic!("expected pseudo-event for reverse request, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t131cli_decode_dap_rejects_unknown_type() {
        let body = br#"{"seq":4,"type":"weirdo","data":1}"#;
        let err = decode_dap(body).unwrap_err();
        assert!(err.contains("unknown DAP message type"));
    }
}
