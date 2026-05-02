//! T2.1 — `BrowserClient` facade.
//!
//! Composes spawn + SidecarSession + correlator + reader-loop into
//! one ergonomic API. Same shape as `LspClient` / `DapClient` but
//! over line-delimited JSON (Node sidecar uses `readline`-friendly
//! framing — see `sidecar.rs` for rationale).
//!
//! Public surface:
//!   - `BrowserClient::spawn(node, script_path)` → spawns the
//!     Playwright sidecar, returns the client
//!   - `client.request(BrowserRequest)` → `Result<BrowserResult, BrowserError>`
//!   - `client.shutdown()` → close + drain reader (idempotent)
//!
//! Concurrency: writes serialized through a `Mutex<SidecarWriter>`
//! (line-delimited frames must be atomic). Reads run in a spawned
//! task that dispatches each response by `id` to the correlator.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex, oneshot};
use tokio::task::JoinHandle;

use crate::browser::protocol::{
    BrowserAction, BrowserError, BrowserRequest, BrowserResponse, BrowserResult,
};
use crate::browser::sidecar::{SidecarError, SidecarSession};

/// Errors the browser client surfaces.
#[derive(Debug, thiserror::Error)]
pub enum BrowserClientError {
    #[error("failed to spawn sidecar `{program}`: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("sidecar stdio capture failed (stdin/stdout pipe missing)")]
    StdioCaptureFailed,
    #[error("sidecar IO error: {0}")]
    Sidecar(#[from] SidecarError),
    #[error("response receiver dropped (reader exited before response arrived)")]
    NoResponse,
    #[error("sidecar returned undecodable JSON: {0}")]
    BadJson(String),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    /// The sidecar emitted a typed BrowserError — wrapped for
    /// callers that want a single `?`-able client error.
    #[error("sidecar reported error: {0}")]
    Browser(BrowserError),
}

/// One pending request: caller awaits the response on `rx`.
struct PendingResponse {
    tx: oneshot::Sender<BrowserResponse>,
}

/// Browser client over an arbitrary writer. `BrowserClient<ChildStdin>`
/// is the production type produced by `spawn()`; tests can build the
/// client over `tokio::io::DuplexStream` via `from_split_for_test`.
pub struct BrowserClient<W: AsyncWrite + Unpin + Send + 'static = ChildStdin> {
    /// Writer half of the sidecar's stdio.
    writer: Mutex<SidecarWriter<W>>,
    /// id → oneshot sender table. The reader task locks, removes,
    /// sends.
    pending: Arc<Mutex<std::collections::HashMap<u64, PendingResponse>>>,
    /// Monotonic id generator for outgoing requests.
    next_id: AtomicU64,
    /// Reader task — held to await on shutdown.
    reader_handle: Mutex<Option<JoinHandle<Result<(), BrowserClientError>>>>,
    /// Held to keep `kill_on_drop(true)` alive for spawned sidecars.
    /// `None` for test-mode (duplex) clients.
    _child: Mutex<Option<Child>>,
}

/// Tiny wrapper that owns just the writer half of a SidecarSession
/// — the reader half is consumed by the spawned reader task.
pub struct SidecarWriter<W> {
    writer: W,
}

impl<W: AsyncWrite + Unpin> SidecarWriter<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    pub async fn write_line(&mut self, body: &[u8]) -> Result<(), SidecarError> {
        use tokio::io::AsyncWriteExt;
        self.writer.write_all(body).await?;
        self.writer.write_all(b"\n").await?;
        self.writer.flush().await?;
        Ok(())
    }
}

impl BrowserClient<ChildStdin> {
    /// Spawn the Playwright sidecar (`node <script_path>`) and wire
    /// up the IO chain. Returns the client.
    pub async fn spawn(
        node_program: &str,
        script_path: &std::path::Path,
    ) -> Result<Self, BrowserClientError> {
        let mut cmd = Command::new(node_program);
        cmd.arg(script_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|source| BrowserClientError::Spawn {
            program: node_program.into(),
            source,
        })?;
        let stdin: ChildStdin = child
            .stdin
            .take()
            .ok_or(BrowserClientError::StdioCaptureFailed)?;
        let stdout: ChildStdout = child
            .stdout
            .take()
            .ok_or(BrowserClientError::StdioCaptureFailed)?;
        // stderr stays attached to the child for diagnostics; on kill
        // it's drained by the OS.

        let writer_half = SidecarWriter::new(stdin);
        let reader_session = SidecarSession::new(NoopWriter, stdout);
        let client = Self::from_split_with_child(
            writer_half,
            reader_session,
            Some(child),
        );
        Ok(client)
    }
}

impl<W: AsyncWrite + Unpin + Send + 'static> BrowserClient<W> {
    /// Test helper: build a client over arbitrary halves. The writer
    /// is what the client uses to send requests; the reader feeds
    /// responses back. Both can be `tokio::io::DuplexStream`.
    pub fn from_split_for_test<R>(
        writer: SidecarWriter<W>,
        reader_session: SidecarSession<NoopWriter, R>,
    ) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
    {
        Self::from_split_with_child(writer, reader_session, None)
    }

    fn from_split_with_child<R>(
        writer: SidecarWriter<W>,
        mut reader_session: SidecarSession<NoopWriter, R>,
        child: Option<Child>,
    ) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
    {
        let pending: Arc<
            Mutex<std::collections::HashMap<u64, PendingResponse>>,
        > = Arc::new(Mutex::new(std::collections::HashMap::new()));
        let pending_clone = pending.clone();

        let reader_handle = tokio::spawn(async move {
            run_browser_reader_loop(&mut reader_session, pending_clone).await
        });

        Self {
            writer: Mutex::new(writer),
            pending,
            next_id: AtomicU64::new(1),
            reader_handle: Mutex::new(Some(reader_handle)),
            _child: Mutex::new(child),
        }
    }

    /// Send a request and await the matching response. Safe to call
    /// concurrently — the correlator pairs each id to its waiter.
    pub async fn request(
        &self,
        action: BrowserAction,
    ) -> Result<BrowserResult, BrowserClientError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let req = BrowserRequest { id, action };
        let body = serde_json::to_vec(&req)?;

        // Register pending BEFORE writing — race-safe.
        let (tx, rx) = oneshot::channel::<BrowserResponse>();
        {
            let mut p = self.pending.lock().await;
            p.insert(id, PendingResponse { tx });
        }
        {
            let mut w = self.writer.lock().await;
            w.write_line(&body).await?;
        }
        let resp = rx.await.map_err(|_| BrowserClientError::NoResponse)?;
        match resp.result() {
            Ok(r) => Ok(r),
            Err(e) => Err(BrowserClientError::Browser(e)),
        }
    }

    /// Number of in-flight requests waiting for a response.
    pub async fn pending_requests(&self) -> usize {
        self.pending.lock().await.len()
    }

    /// Best-effort graceful shutdown: send `Close`, then await the
    /// reader task. Idempotent — safe to call twice (second call is
    /// a no-op once the handle is drained).
    pub async fn shutdown(&self) -> Result<(), BrowserClientError> {
        let alive = self.reader_handle.lock().await.is_some();
        if alive {
            // Best-effort; the sidecar may already be gone.
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                self.request(BrowserAction::Close),
            )
            .await;
        }
        if let Some(handle) = self.reader_handle.lock().await.take() {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
        }
        Ok(())
    }
}

/// A no-op writer used when we just need a `SidecarSession` for its
/// reader (the writer half is owned separately by the client).
pub struct NoopWriter;

impl AsyncWrite for NoopWriter {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::task::Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}

/// Reader loop: read one line at a time, decode as `BrowserResponse`,
/// dispatch to the matching pending sender. Exits on:
///   - EOF (sidecar closed stdout cleanly) → Ok(())
///   - Sidecar IO error
///   - Decode error (the sidecar wrote garbage)
async fn run_browser_reader_loop<R>(
    session: &mut SidecarSession<NoopWriter, R>,
    pending: Arc<Mutex<std::collections::HashMap<u64, PendingResponse>>>,
) -> Result<(), BrowserClientError>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    loop {
        let body = match session.read_line_or_eof().await? {
            Some(b) => b,
            None => return Ok(()),
        };
        // Sidecar may emit blank lines on stderr-mixing — ignore.
        let trimmed = body
            .iter()
            .position(|&b| !(b == b' ' || b == b'\t' || b == b'\r'))
            .map(|p| &body[p..])
            .unwrap_or(&[]);
        if trimmed.is_empty() {
            continue;
        }
        let resp: BrowserResponse = serde_json::from_slice(trimmed).map_err(|e| {
            BrowserClientError::BadJson(format!(
                "could not decode `{}`: {e}",
                String::from_utf8_lossy(trimmed)
            ))
        })?;
        // Dispatch by id.
        let id = resp.id;
        let mut p = pending.lock().await;
        if let Some(slot) = p.remove(&id) {
            // ignore send error — the caller cancelled / dropped rx
            let _ = slot.tx.send(resp);
        } else {
            // Sidecar replied to an id we didn't issue. Log + continue
            // — don't crash the client over a misbehaving sidecar.
            log::warn!("browser_client: response for unknown id {id} dropped");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tokio::io::{AsyncWriteExt, duplex};

    /// Build a BrowserClient over a tokio duplex pair.
    /// Layout:
    ///   - client.writer → harness_outbound (test reads what client sent)
    ///   - harness_inbound → client.reader (test writes responses)
    fn duplex_client() -> (
        BrowserClient<tokio::io::DuplexStream>,
        SidecarSession<NoopWriter, tokio::io::DuplexStream>,
        tokio::io::DuplexStream,
    ) {
        let (sut_writer, harness_outbound) = duplex(64 * 1024);
        let (harness_inbound, sut_reader) = duplex(64 * 1024);
        let writer = SidecarWriter::new(sut_writer);
        let reader_session = SidecarSession::new(NoopWriter, sut_reader);
        let client = BrowserClient::from_split_for_test(writer, reader_session);
        (
            client,
            SidecarSession::new(NoopWriter, harness_outbound),
            harness_inbound,
        )
    }

    async fn read_outbound_line(
        outbound: &mut SidecarSession<NoopWriter, tokio::io::DuplexStream>,
    ) -> serde_json::Value {
        let body = outbound.read_line_or_eof().await.unwrap().unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn t21cli_request_writes_line_and_awaits_response() {
        let (client, mut outbound, mut inbound) = duplex_client();
        let req_task = tokio::spawn(async move {
            client
                .request(BrowserAction::Open {
                    url: "https://e.x".into(),
                })
                .await
        });

        let req_value = read_outbound_line(&mut outbound).await;
        assert_eq!(req_value["action"], "open");
        assert_eq!(req_value["url"], "https://e.x");
        let id = req_value["id"].as_u64().unwrap();

        // Push success response.
        let resp = serde_json::json!({
            "id": id,
            "result": {"kind":"navigated", "final_url":"https://e.x/landing", "title":"OK"}
        });
        inbound
            .write_all(format!("{resp}\n").as_bytes())
            .await
            .unwrap();
        inbound.flush().await.unwrap();

        let result = req_task.await.unwrap().unwrap();
        match result {
            BrowserResult::Navigated { final_url, title } => {
                assert_eq!(final_url, "https://e.x/landing");
                assert_eq!(title, "OK");
            }
            other => panic!("expected Navigated, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t21cli_error_response_surfaces_typed_browser_error() {
        let (client, mut outbound, mut inbound) = duplex_client();
        let req_task = tokio::spawn(async move {
            client
                .request(BrowserAction::Click {
                    selector: "#missing".into(),
                })
                .await
        });
        let req_value = read_outbound_line(&mut outbound).await;
        let id = req_value["id"].as_u64().unwrap();

        let resp = serde_json::json!({
            "id": id,
            "error": {"not_open": null}
        });
        inbound
            .write_all(format!("{resp}\n").as_bytes())
            .await
            .unwrap();
        inbound.flush().await.unwrap();

        let err = req_task.await.unwrap().unwrap_err();
        match err {
            BrowserClientError::Browser(BrowserError::NotOpen) => {}
            other => panic!("expected Browser(NotOpen), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t21cli_concurrent_requests_pair_correctly_by_id() {
        let (client, mut outbound, mut inbound) = duplex_client();
        let client = Arc::new(client);

        let c1 = client.clone();
        let r1 = tokio::spawn(async move {
            c1.request(BrowserAction::Click {
                selector: "#a".into(),
            })
            .await
        });
        let c2 = client.clone();
        let r2 = tokio::spawn(async move {
            c2.request(BrowserAction::Click {
                selector: "#b".into(),
            })
            .await
        });

        let v1 = read_outbound_line(&mut outbound).await;
        let v2 = read_outbound_line(&mut outbound).await;
        let id_a = if v1["selector"] == "#a" {
            v1["id"].as_u64().unwrap()
        } else {
            v2["id"].as_u64().unwrap()
        };
        let id_b = if v1["selector"] == "#b" {
            v1["id"].as_u64().unwrap()
        } else {
            v2["id"].as_u64().unwrap()
        };
        assert_ne!(id_a, id_b);

        // Reply in REVERSE order — id_b first.
        for id in [id_b, id_a] {
            let resp = serde_json::json!({"id": id, "result": {"kind":"empty"}});
            inbound
                .write_all(format!("{resp}\n").as_bytes())
                .await
                .unwrap();
        }
        inbound.flush().await.unwrap();

        let _ = r1.await.unwrap().unwrap();
        let _ = r2.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn t21cli_unknown_id_is_logged_and_loop_continues() {
        // Sidecar replies to an id we never issued. Reader logs +
        // continues. A subsequent legitimate request must still
        // resolve.
        let (client, mut outbound, mut inbound) = duplex_client();
        let bad = serde_json::json!({"id": 9999, "result": {"kind":"empty"}});
        inbound
            .write_all(format!("{bad}\n").as_bytes())
            .await
            .unwrap();

        // Now do a real request.
        let req_task = tokio::spawn(async move {
            client.request(BrowserAction::Close).await
        });
        let req_value = read_outbound_line(&mut outbound).await;
        let id = req_value["id"].as_u64().unwrap();
        let good = serde_json::json!({"id": id, "result": {"kind":"empty"}});
        inbound
            .write_all(format!("{good}\n").as_bytes())
            .await
            .unwrap();
        inbound.flush().await.unwrap();
        let result = req_task.await.unwrap().unwrap();
        assert!(matches!(result, BrowserResult::Empty));
    }

    #[tokio::test]
    async fn t21cli_blank_lines_are_skipped_silently() {
        // Some sidecars emit an empty line on warm-up. Reader must
        // skip without surfacing a decode error.
        let (client, mut outbound, mut inbound) = duplex_client();
        inbound.write_all(b"\n\n   \n").await.unwrap();
        inbound.flush().await.unwrap();

        let req_task = tokio::spawn(async move {
            client.request(BrowserAction::Close).await
        });
        let req_value = read_outbound_line(&mut outbound).await;
        let id = req_value["id"].as_u64().unwrap();
        let good = serde_json::json!({"id": id, "result": {"kind":"empty"}});
        inbound
            .write_all(format!("{good}\n").as_bytes())
            .await
            .unwrap();
        inbound.flush().await.unwrap();
        let result = req_task.await.unwrap().unwrap();
        assert!(matches!(result, BrowserResult::Empty));
    }

    #[tokio::test]
    async fn t21cli_pending_requests_count_after_response_is_zero() {
        let (client, mut outbound, mut inbound) = duplex_client();
        let client = Arc::new(client);
        assert_eq!(client.pending_requests().await, 0);

        let c = client.clone();
        let task = tokio::spawn(async move { c.request(BrowserAction::Close).await });
        let req_value = read_outbound_line(&mut outbound).await;
        let id = req_value["id"].as_u64().unwrap();
        let resp = serde_json::json!({"id": id, "result": {"kind":"empty"}});
        inbound
            .write_all(format!("{resp}\n").as_bytes())
            .await
            .unwrap();
        inbound.flush().await.unwrap();
        let _ = task.await.unwrap().unwrap();
        assert_eq!(client.pending_requests().await, 0);
    }

    #[tokio::test]
    async fn t21cli_shutdown_drains_reader_handle_idempotently() {
        let (client, mut outbound, mut inbound) = duplex_client();

        // Fake server: respond to the close request, then drop inbound.
        let task = tokio::spawn(async move {
            let body = outbound.read_line_or_eof().await.unwrap().unwrap();
            let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let id = v["id"].as_u64().unwrap();
            let resp = serde_json::json!({"id": id, "result": {"kind":"empty"}});
            inbound
                .write_all(format!("{resp}\n").as_bytes())
                .await
                .unwrap();
            inbound.flush().await.unwrap();
            drop(inbound);
            outbound
        });

        client.shutdown().await.unwrap();
        let _outbound = task.await.unwrap();
        // Idempotent: handle is now None.
        client.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn t21cli_spawn_unknown_binary_returns_typed_error() {
        let res = BrowserClient::spawn(
            "definitely-not-a-real-node-binary-xyz",
            std::path::Path::new("/tmp/nonexistent.js"),
        )
        .await;
        match res {
            Err(BrowserClientError::Spawn { program, .. }) => {
                assert!(program.contains("definitely-not-a-real-node-binary-xyz"));
            }
            Ok(_) => panic!("expected spawn failure for missing binary"),
            Err(other) => panic!("expected Spawn error, got {other:?}"),
        }
    }
}
