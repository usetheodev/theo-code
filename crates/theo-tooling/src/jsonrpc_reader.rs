//! T3.1 / T13.1 — Reader-loop driver.
//!
//! Connects a `StdioSession`'s read side to a `Correlator` (for
//! responses) and a notification channel (for unsolicited messages).
//! Runs in a `tokio::spawn`'d task — clients hold its `JoinHandle`
//! and abort it on shutdown.
//!
//! Generic over the protocol decoder: caller provides a `Decoder`
//! function that turns frame body bytes into a typed message which
//! is then classified into Response (correlator) vs Notification
//! (channel).

use std::sync::Arc;

use tokio::io::AsyncRead;
use tokio::sync::mpsc;

use crate::jsonrpc_correlator::{Correlator, CorrelatorError};
use crate::jsonrpc_session::{SessionError, StdioSession};

/// Errors the reader loop can encounter.
#[derive(Debug, thiserror::Error)]
pub enum ReaderError {
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error("decoder failed: {0}")]
    Decode(String),
    #[error("notification channel closed (consumer dropped)")]
    NotificationClosed,
}

/// Classification result from the protocol-specific decoder.
/// `Response<R>` carries the correlation key + the typed response;
/// `Notification<N>` carries the unsolicited message.
#[derive(Debug)]
pub enum DecodedMessage<K, R, N> {
    Response { key: K, response: R },
    Notification(N),
}

/// Reader-loop entry point. Reads frames forever from `session`,
/// decodes via `decode`, and either:
/// - dispatches a Response to the correlator (caller awaits the
///   matching `oneshot::Receiver`), OR
/// - sends a Notification through the mpsc channel (caller polls
///   the receiver in a separate task or `select!`).
///
/// Returns when:
/// - The peer closes stdout (graceful shutdown — Ok(())).
/// - An IO error occurs.
/// - The notification channel is closed (consumer dropped).
/// - A decode error occurs.
///
/// On `NoPendingRequest` from the correlator (server replied to a
/// key we never sent), the loop logs but does NOT exit — a
/// misbehaving server shouldn't crash the client.
pub async fn run_reader_loop<W, R, K, Resp, Notif, F>(
    session: &mut StdioSession<W, R>,
    correlator: Arc<Correlator<K, Resp>>,
    notif_tx: mpsc::Sender<Notif>,
    mut decode: F,
) -> Result<(), ReaderError>
where
    W: tokio::io::AsyncWrite + Unpin,
    R: AsyncRead + Unpin,
    K: Eq + std::hash::Hash,
    F: FnMut(&[u8]) -> Result<DecodedMessage<K, Resp, Notif>, String>,
{
    loop {
        let body = match session.read_frame_or_eof().await? {
            Some(b) => b,
            None => return Ok(()), // graceful peer close
        };
        let msg = decode(&body).map_err(ReaderError::Decode)?;
        match msg {
            DecodedMessage::Response { key, response } => {
                match correlator.dispatch(key, response).await {
                    Ok(()) => {}
                    Err(CorrelatorError::NoPendingRequest) => {
                        // Server replied to an id we never sent.
                        // Log + continue — don't kill the client.
                        log::warn!(
                            "jsonrpc_reader: response for unknown key dropped"
                        );
                    }
                    Err(CorrelatorError::WaiterDropped) => {
                        // Caller cancelled before response arrived.
                        // Same — log + continue.
                        log::debug!(
                            "jsonrpc_reader: response arrived after waiter dropped"
                        );
                    }
                    Err(other) => {
                        // DuplicateKey can't happen here; treat as decode error
                        // for surface-level visibility.
                        return Err(ReaderError::Decode(format!(
                            "unexpected correlator error: {other}"
                        )));
                    }
                }
            }
            DecodedMessage::Notification(n) => {
                notif_tx
                    .send(n)
                    .await
                    .map_err(|_| ReaderError::NotificationClosed)?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;
    use tokio::io::{AsyncWriteExt, duplex};

    use crate::jsonrpc_stdio::encode_frame;
    use crate::lsp::protocol::{
        InboundMessage, JsonRpcNotification, JsonRpcResponse, try_decode_frame,
    };

    /// LSP-shaped decoder used in the reader loop tests.
    fn lsp_decode(
        body: &[u8],
    ) -> Result<DecodedMessage<u64, JsonRpcResponse, JsonRpcNotification>, String> {
        let buf = encode_frame(body);
        let (msg, _n) = try_decode_frame(&buf)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "incomplete frame".to_string())?;
        match msg {
            InboundMessage::Response(r) => Ok(DecodedMessage::Response {
                key: r.id,
                response: r,
            }),
            InboundMessage::Notification(n) => Ok(DecodedMessage::Notification(n)),
        }
    }

    /// Build a session whose read-side is fed by a tokio duplex.
    /// Returns the session + the test-controlled writer half so the
    /// test can push frames into the SUT.
    fn duplex_session() -> (
        StdioSession<tokio::io::DuplexStream, tokio::io::DuplexStream>,
        tokio::io::DuplexStream,
    ) {
        let (sut_writer, _harness_reader) = duplex(64 * 1024);
        let (harness_writer, sut_reader) = duplex(64 * 1024);
        (StdioSession::new(sut_writer, sut_reader), harness_writer)
    }

    #[tokio::test]
    async fn t31rdr_response_dispatched_to_correlator_waiter() {
        let (mut session, mut harness) = duplex_session();
        let cor: Arc<Correlator<u64, JsonRpcResponse>> = Arc::new(Correlator::new());
        let (tx, _rx) = mpsc::channel::<JsonRpcNotification>(8);

        // Register a waiter for id=42 BEFORE the reader loop runs.
        let rx_resp = cor.register(42).await.unwrap();

        let cor_clone = cor.clone();
        let reader = tokio::spawn(async move {
            run_reader_loop(&mut session, cor_clone, tx, lsp_decode).await
        });

        // Push the response into the duplex.
        let body = json!({"jsonrpc": "2.0", "id": 42, "result": "ok"});
        harness
            .write_all(&encode_frame(serde_json::to_vec(&body).unwrap().as_slice()))
            .await
            .unwrap();
        harness.flush().await.unwrap();

        let response = rx_resp.await.unwrap();
        assert_eq!(response.id, 42);
        assert_eq!(response.result.as_ref().unwrap(), "ok");

        // Clean shutdown: drop the harness writer so reader exits Ok.
        drop(harness);
        let _ = reader.await.unwrap(); // either Ok(()) or NotificationClosed if no rx
    }

    #[tokio::test]
    async fn t31rdr_notification_routed_to_channel() {
        let (mut session, mut harness) = duplex_session();
        let cor: Arc<Correlator<u64, JsonRpcResponse>> = Arc::new(Correlator::new());
        let (tx, mut notif_rx) = mpsc::channel::<JsonRpcNotification>(8);

        let cor_clone = cor.clone();
        let reader = tokio::spawn(async move {
            run_reader_loop(&mut session, cor_clone, tx, lsp_decode).await
        });

        let body = json!({"jsonrpc":"2.0","method":"window/logMessage","params":{"type":3,"message":"hi"}});
        harness
            .write_all(&encode_frame(serde_json::to_vec(&body).unwrap().as_slice()))
            .await
            .unwrap();
        harness.flush().await.unwrap();

        let notif = notif_rx.recv().await.unwrap();
        assert_eq!(notif.method, "window/logMessage");

        drop(harness);
        let _ = reader.await.unwrap();
    }

    #[tokio::test]
    async fn t31rdr_unknown_response_id_logged_loop_continues() {
        // Server replies with id=99 we never registered. Reader
        // logs + continues. Then a notification proves the loop
        // didn't die.
        let (mut session, mut harness) = duplex_session();
        let cor: Arc<Correlator<u64, JsonRpcResponse>> = Arc::new(Correlator::new());
        let (tx, mut notif_rx) = mpsc::channel::<JsonRpcNotification>(8);

        let cor_clone = cor.clone();
        let reader = tokio::spawn(async move {
            run_reader_loop(&mut session, cor_clone, tx, lsp_decode).await
        });

        // Bad response (no waiter for id=99).
        let bad = json!({"jsonrpc":"2.0","id":99,"result":null});
        harness
            .write_all(&encode_frame(serde_json::to_vec(&bad).unwrap().as_slice()))
            .await
            .unwrap();

        // Then a valid notification.
        let good = json!({"jsonrpc":"2.0","method":"x","params":null});
        harness
            .write_all(&encode_frame(serde_json::to_vec(&good).unwrap().as_slice()))
            .await
            .unwrap();
        harness.flush().await.unwrap();

        let notif = notif_rx.recv().await.unwrap();
        assert_eq!(notif.method, "x");

        drop(harness);
        let _ = reader.await.unwrap();
    }

    #[tokio::test]
    async fn t31rdr_clean_eof_returns_ok() {
        let (mut session, harness) = duplex_session();
        let cor: Arc<Correlator<u64, JsonRpcResponse>> = Arc::new(Correlator::new());
        let (tx, _rx) = mpsc::channel::<JsonRpcNotification>(8);

        let cor_clone = cor.clone();
        let reader = tokio::spawn(async move {
            run_reader_loop(&mut session, cor_clone, tx, lsp_decode).await
        });

        // Drop harness immediately — clean EOF.
        drop(harness);
        let result = reader.await.unwrap();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn t31rdr_decode_error_terminates_loop_with_typed_error() {
        let (mut session, mut harness) = duplex_session();
        let cor: Arc<Correlator<u64, JsonRpcResponse>> = Arc::new(Correlator::new());
        let (tx, _rx) = mpsc::channel::<JsonRpcNotification>(8);

        let cor_clone = cor.clone();
        let reader = tokio::spawn(async move {
            run_reader_loop(&mut session, cor_clone, tx, lsp_decode).await
        });

        // Send non-JSON inside a valid frame envelope.
        harness
            .write_all(&encode_frame(b"not valid json"))
            .await
            .unwrap();
        harness.flush().await.unwrap();

        let result = reader.await.unwrap();
        match result {
            Err(ReaderError::Decode(_)) => {}
            other => panic!("expected Decode error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t31rdr_notification_channel_closed_terminates_loop() {
        let (mut session, mut harness) = duplex_session();
        let cor: Arc<Correlator<u64, JsonRpcResponse>> = Arc::new(Correlator::new());
        let (tx, rx) = mpsc::channel::<JsonRpcNotification>(8);

        // Drop the receiver so the channel is closed when the reader
        // tries to send.
        drop(rx);

        let cor_clone = cor.clone();
        let reader = tokio::spawn(async move {
            run_reader_loop(&mut session, cor_clone, tx, lsp_decode).await
        });

        let n = json!({"jsonrpc":"2.0","method":"x","params":null});
        harness
            .write_all(&encode_frame(serde_json::to_vec(&n).unwrap().as_slice()))
            .await
            .unwrap();
        harness.flush().await.unwrap();

        let result = reader.await.unwrap();
        assert!(matches!(result, Err(ReaderError::NotificationClosed)));
    }

    #[tokio::test]
    async fn t31rdr_two_responses_dispatched_in_order() {
        let (mut session, mut harness) = duplex_session();
        let cor: Arc<Correlator<u64, JsonRpcResponse>> = Arc::new(Correlator::new());
        let (tx, _rx) = mpsc::channel::<JsonRpcNotification>(8);

        let rx1 = cor.register(1).await.unwrap();
        let rx2 = cor.register(2).await.unwrap();

        let cor_clone = cor.clone();
        let reader = tokio::spawn(async move {
            run_reader_loop(&mut session, cor_clone, tx, lsp_decode).await
        });

        for id in [1u64, 2u64] {
            let body = json!({"jsonrpc":"2.0","id":id,"result":id*10});
            harness
                .write_all(&encode_frame(serde_json::to_vec(&body).unwrap().as_slice()))
                .await
                .unwrap();
        }
        harness.flush().await.unwrap();

        let r1 = rx1.await.unwrap();
        let r2 = rx2.await.unwrap();
        assert_eq!(r1.id, 1);
        assert_eq!(r1.result.as_ref().unwrap(), 10);
        assert_eq!(r2.id, 2);
        assert_eq!(r2.result.as_ref().unwrap(), 20);

        drop(harness);
        let _ = reader.await.unwrap();
    }

    #[tokio::test]
    async fn t31rdr_out_of_order_responses_pair_correctly() {
        let (mut session, mut harness) = duplex_session();
        let cor: Arc<Correlator<u64, JsonRpcResponse>> = Arc::new(Correlator::new());
        let (tx, _rx) = mpsc::channel::<JsonRpcNotification>(8);

        let rx1 = cor.register(1).await.unwrap();
        let rx2 = cor.register(2).await.unwrap();

        let cor_clone = cor.clone();
        let reader = tokio::spawn(async move {
            run_reader_loop(&mut session, cor_clone, tx, lsp_decode).await
        });

        // Server replies in REVERSE order — id 2 then id 1.
        for body in [
            json!({"jsonrpc":"2.0","id":2,"result":"second"}),
            json!({"jsonrpc":"2.0","id":1,"result":"first"}),
        ] {
            harness
                .write_all(&encode_frame(serde_json::to_vec(&body).unwrap().as_slice()))
                .await
                .unwrap();
        }
        harness.flush().await.unwrap();

        let r2 = rx2.await.unwrap();
        let r1 = rx1.await.unwrap();
        assert_eq!(r1.result.as_ref().unwrap(), "first");
        assert_eq!(r2.result.as_ref().unwrap(), "second");

        drop(harness);
        let _ = reader.await.unwrap();
    }
}
