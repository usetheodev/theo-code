//! T3.1 / T13.1 — Generic request/response correlator.
//!
//! When a client sends a request to an LSP/DAP server, the server's
//! response arrives later — possibly out of order with respect to
//! other in-flight requests, possibly interleaved with notifications.
//! The correlator registers a `oneshot::Sender` keyed by the
//! request id (LSP) or request_seq (DAP), and the reader task
//! delivers the matched response to the awaiting caller.
//!
//! Generic over the key type (u64 for both LSP+DAP today; could be
//! string in some custom JSON-RPC dialects) and the response type
//! (`JsonRpcResponse` for LSP, `DapResponse` for DAP).

use std::collections::HashMap;
use std::hash::Hash;

use tokio::sync::{Mutex, oneshot};

/// Errors specific to correlation.
#[derive(Debug, thiserror::Error)]
pub enum CorrelatorError {
    #[error("duplicate request key — already registered")]
    DuplicateKey,
    #[error("no pending request for key (response arrived without a sender)")]
    NoPendingRequest,
    #[error("waiter dropped before response arrived")]
    WaiterDropped,
}

/// Maps in-flight request keys to their `oneshot::Sender`s.
///
/// Mutex-protected because the writer side (caller registering a
/// new request) and the reader side (dispatching a response) run on
/// different tasks. The lock is held only for the table mutation,
/// never across an `await` of the response future.
pub struct Correlator<K: Eq + Hash, R> {
    pending: Mutex<HashMap<K, oneshot::Sender<R>>>,
}

impl<K: Eq + Hash, R> Default for Correlator<K, R> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Eq + Hash, R> Correlator<K, R> {
    /// Empty correlator.
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// Register a new pending request. Returns the receiver the
    /// caller awaits to get the matching response.
    ///
    /// Errors:
    /// - `DuplicateKey` when `key` is already pending — caller bug
    ///   (should mint fresh ids per request).
    pub async fn register(&self, key: K) -> Result<oneshot::Receiver<R>, CorrelatorError> {
        let (tx, rx) = oneshot::channel::<R>();
        let mut guard = self.pending.lock().await;
        if guard.contains_key(&key) {
            return Err(CorrelatorError::DuplicateKey);
        }
        guard.insert(key, tx);
        Ok(rx)
    }

    /// Deliver a response to the registered waiter. The reader task
    /// (driving `read_frame`) calls this when a response arrives.
    ///
    /// Errors:
    /// - `NoPendingRequest` when no sender exists for `key` — server
    ///   replied to an id we never sent (server bug).
    /// - `WaiterDropped` when the receiver was dropped before the
    ///   response arrived (caller cancelled / dropped the future).
    pub async fn dispatch(&self, key: K, response: R) -> Result<(), CorrelatorError> {
        let mut guard = self.pending.lock().await;
        let tx = guard.remove(&key).ok_or(CorrelatorError::NoPendingRequest)?;
        drop(guard); // release lock BEFORE the send (oneshot::send isn't await but cheap discipline)
        tx.send(response).map_err(|_| CorrelatorError::WaiterDropped)
    }

    /// Number of in-flight requests.
    pub async fn pending_count(&self) -> usize {
        self.pending.lock().await.len()
    }

    /// Cancel all pending requests by dropping their senders. Each
    /// awaiting receiver will see `oneshot::error::RecvError`.
    /// Useful on subprocess shutdown.
    pub async fn cancel_all(&self) {
        self.pending.lock().await.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResp = String;

    #[tokio::test]
    async fn t31cor_new_correlator_is_empty() {
        let c: Correlator<u64, TestResp> = Correlator::new();
        assert_eq!(c.pending_count().await, 0);
    }

    #[tokio::test]
    async fn t31cor_register_then_dispatch_delivers_response() {
        let c: Correlator<u64, TestResp> = Correlator::new();
        let rx = c.register(1).await.unwrap();
        assert_eq!(c.pending_count().await, 1);
        c.dispatch(1, "pong".to_string()).await.unwrap();
        assert_eq!(c.pending_count().await, 0);
        let got = rx.await.unwrap();
        assert_eq!(got, "pong");
    }

    #[tokio::test]
    async fn t31cor_dispatch_unknown_key_returns_no_pending_request() {
        let c: Correlator<u64, TestResp> = Correlator::new();
        let err = c
            .dispatch(99, "ghost".to_string())
            .await
            .unwrap_err();
        assert!(matches!(err, CorrelatorError::NoPendingRequest));
    }

    #[tokio::test]
    async fn t31cor_register_duplicate_key_returns_error() {
        let c: Correlator<u64, TestResp> = Correlator::new();
        let _rx1 = c.register(1).await.unwrap();
        let err = c.register(1).await.unwrap_err();
        assert!(matches!(err, CorrelatorError::DuplicateKey));
    }

    #[tokio::test]
    async fn t31cor_waiter_dropped_returns_typed_error_on_dispatch() {
        let c: Correlator<u64, TestResp> = Correlator::new();
        let rx = c.register(1).await.unwrap();
        drop(rx); // caller cancelled before response
        let err = c.dispatch(1, "lost".to_string()).await.unwrap_err();
        assert!(matches!(err, CorrelatorError::WaiterDropped));
    }

    #[tokio::test]
    async fn t31cor_out_of_order_responses_are_routed_correctly() {
        let c: std::sync::Arc<Correlator<u64, TestResp>> =
            std::sync::Arc::new(Correlator::new());
        let rx_a = c.register(1).await.unwrap();
        let rx_b = c.register(2).await.unwrap();
        let rx_c = c.register(3).await.unwrap();
        // Dispatch in REVERSE order — each receiver still gets its
        // own response.
        let c1 = c.clone();
        let c2 = c.clone();
        let c3 = c.clone();
        let h1 = tokio::spawn(async move { c1.dispatch(3, "third".into()).await });
        let h2 = tokio::spawn(async move { c2.dispatch(1, "first".into()).await });
        let h3 = tokio::spawn(async move { c3.dispatch(2, "second".into()).await });
        h1.await.unwrap().unwrap();
        h2.await.unwrap().unwrap();
        h3.await.unwrap().unwrap();
        assert_eq!(rx_a.await.unwrap(), "first");
        assert_eq!(rx_b.await.unwrap(), "second");
        assert_eq!(rx_c.await.unwrap(), "third");
        assert_eq!(c.pending_count().await, 0);
    }

    #[tokio::test]
    async fn t31cor_pending_count_reflects_register_dispatch_lifecycle() {
        let c: Correlator<u64, TestResp> = Correlator::new();
        assert_eq!(c.pending_count().await, 0);
        let _rx1 = c.register(1).await.unwrap();
        let _rx2 = c.register(2).await.unwrap();
        assert_eq!(c.pending_count().await, 2);
        c.dispatch(1, "a".into()).await.unwrap();
        assert_eq!(c.pending_count().await, 1);
    }

    #[tokio::test]
    async fn t31cor_cancel_all_drops_every_sender() {
        let c: Correlator<u64, TestResp> = Correlator::new();
        let rx1 = c.register(1).await.unwrap();
        let rx2 = c.register(2).await.unwrap();
        assert_eq!(c.pending_count().await, 2);
        c.cancel_all().await;
        assert_eq!(c.pending_count().await, 0);
        // Receivers see the cancellation as RecvError on await.
        assert!(rx1.await.is_err());
        assert!(rx2.await.is_err());
    }

    #[tokio::test]
    async fn t31cor_works_with_string_key_for_custom_dialects() {
        // The correlator is generic — exercise with String keys to
        // prove the bound on K is genuinely Eq + Hash.
        let c: Correlator<String, TestResp> = Correlator::new();
        let rx = c.register("req-abc".into()).await.unwrap();
        c.dispatch("req-abc".into(), "ok".into()).await.unwrap();
        assert_eq!(rx.await.unwrap(), "ok");
    }

    #[tokio::test]
    async fn t31cor_concurrent_register_and_dispatch_safe_under_arc() {
        // Spawn N tasks each registering, then N tasks dispatching.
        // Proves the Mutex works correctly under concurrency.
        use std::sync::Arc;
        let c: Arc<Correlator<u64, u64>> = Arc::new(Correlator::new());
        let mut rxs = Vec::new();
        for id in 0..50 {
            let cc = c.clone();
            let h = tokio::spawn(async move { cc.register(id).await });
            rxs.push((id, h));
        }
        let mut receivers = Vec::new();
        for (id, h) in rxs {
            receivers.push((id, h.await.unwrap().unwrap()));
        }
        // Dispatch in reverse.
        for id in (0..50u64).rev() {
            let cc = c.clone();
            tokio::spawn(async move { cc.dispatch(id, id * 10).await })
                .await
                .unwrap()
                .unwrap();
        }
        for (id, rx) in receivers {
            let value = rx.await.unwrap();
            assert_eq!(value, id * 10);
        }
        assert_eq!(c.pending_count().await, 0);
    }
}
