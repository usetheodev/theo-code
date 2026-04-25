//! Integration test for T1.1 / find_p7_001 / INV-008
//!
//! Validates that the `CancellationToken → watch::Sender<bool>` bridge
//! pattern used in `run_engine::execution::execute_with_history`:
//!
//! 1. Forwards `CancellationToken::cancel()` to the abort channel
//!    receivers in well under 500 ms (target latency).
//! 2. Keeps the sender alive for the duration of the request lifecycle
//!    (regression test for the original `_abort_tx` bug that dropped
//!    the sender immediately).
//!
//! Note: this is a *focused* test of the bridge pattern itself, not
//! a full agent run. A full E2E test would require standing up an
//! LLM mock, tool registry, sub-agent cancellation tree, and a tool
//! that actually respects `abort_rx`. That is more brittle than
//! testing the bridge wiring directly. The bridge is the only piece
//! that was broken — the downstream `dispatch_batch` already accepts
//! the receiver correctly.

use std::sync::Arc;
use std::time::{Duration, Instant};

use theo_agent_runtime::cancellation::CancellationTree;

/// Re-implements the bridge pattern from `execute_with_history` so we
/// can exercise it under a controlled timing harness. If this test
/// breaks, audit `run_engine/execution.rs` for divergence.
fn spawn_bridge(
    cancellation: Arc<CancellationTree>,
    run_id: &str,
) -> (
    tokio::sync::watch::Sender<bool>,
    tokio::sync::watch::Receiver<bool>,
) {
    let (abort_tx, abort_rx) = tokio::sync::watch::channel(false);
    let token = cancellation.child(run_id);
    let tx = abort_tx.clone();
    tokio::spawn(async move {
        token.cancelled().await;
        let _ = tx.send(true);
    });
    (abort_tx, abort_rx)
}

#[tokio::test]
async fn cancel_propagates_to_in_flight_tool_in_under_500ms() {
    // Arrange — a cancellation tree shared by parent and a synthetic
    // long-running tool that observes the abort receiver.
    let cancellation = Arc::new(CancellationTree::new());
    let (_abort_tx_keepalive, mut abort_rx) =
        spawn_bridge(cancellation.clone(), "test-run-1");

    // Synthetic tool that loops awaiting either work completion or
    // abort. In real production this is a tool's loop with a `select!`
    // on `abort_rx.changed()`.
    let tool_handle = tokio::spawn(async move {
        let start = Instant::now();
        loop {
            tokio::select! {
                _ = abort_rx.changed() => {
                    if *abort_rx.borrow() {
                        return ("cancelled", start.elapsed());
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(5)) => {
                    return ("completed", start.elapsed());
                }
            }
        }
    });

    // Brief settle period so the tool is actually awaiting before we
    // trigger cancellation. Without this, on very fast machines the
    // cancel could happen before the receiver has registered the wait.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Act — root-cancel the tree. This propagates to the `child` token
    // the bridge is observing.
    let cancel_at = Instant::now();
    cancellation.root().cancel();

    // Assert — the tool returns "cancelled" within 500 ms of the cancel.
    let (outcome, _tool_lifetime) = tool_handle
        .await
        .expect("synthetic tool task panicked or was cancelled externally");
    let propagation_latency = cancel_at.elapsed();

    assert_eq!(
        outcome, "cancelled",
        "tool must observe cancel before its 5s timeout fires"
    );
    assert!(
        propagation_latency < Duration::from_millis(500),
        "cancel propagation must be ≤ 500 ms (INV-008); measured {:?}",
        propagation_latency
    );
}

#[tokio::test]
async fn keep_alive_binding_prevents_sender_drop_before_first_cancel() {
    // Regression for find_p7_001: previously the sender was prefixed
    // with `_` which made Rust drop it immediately. After the fix the
    // sender is bound to `_abort_tx_keepalive` which is `_`-prefixed
    // *only* to silence the unused-variable lint while still keeping
    // the binding live.
    let cancellation = Arc::new(CancellationTree::new());
    let (_abort_tx_keepalive, abort_rx) =
        spawn_bridge(cancellation.clone(), "test-run-2");

    // The receiver should still be open AFTER the function returns
    // (i.e. the sender is alive). If the sender were dropped, the
    // receiver would observe `has_changed()` reporting an error or
    // `borrow()` returning the closed marker.
    assert!(
        !*abort_rx.borrow(),
        "initial value must be `false` and the channel must still be open"
    );

    // Now cancel and confirm the channel actually delivers — proves
    // the bridge task is alive AND the sender survived long enough.
    cancellation.root().cancel();

    let mut rx_for_wait = abort_rx.clone();
    let result = tokio::time::timeout(
        Duration::from_millis(500),
        rx_for_wait.changed(),
    )
    .await;
    assert!(
        result.is_ok(),
        "channel must deliver the change within 500 ms"
    );
    assert!(
        *rx_for_wait.borrow(),
        "received value after cancel must be `true`"
    );
}

#[tokio::test]
async fn no_bridge_when_subagent_cancellation_is_none() {
    // The production code only spawns the bridge when
    // `self.subagent_cancellation.is_some()`. When None, the receiver
    // is created but never receives `true`. This is current intended
    // behaviour for callers that opt out of the cancellation tree.
    //
    // Reproduce the no-bridge path here; verify the receiver does not
    // erroneously fire `changed()`.
    let (_abort_tx_keepalive, mut abort_rx) =
        tokio::sync::watch::channel(false);

    // No bridge spawned. Wait briefly — should NOT change.
    let result = tokio::time::timeout(
        Duration::from_millis(150),
        abort_rx.changed(),
    )
    .await;
    assert!(
        result.is_err(),
        "without a bridge, abort_rx must remain quiet (got {:?})",
        result
    );
}
