//! T14.1 — Partial-progress emission helpers for long-running tools.
//!
//! `ToolContext.stdout_tx: Option<mpsc::Sender<String>>` is the
//! plumbing the runtime supplies; this module is the ergonomic
//! wrapper tools call when they want to surface progress to the UI
//! mid-execution (page load, large screenshot, multi-step LLM
//! advisor, etc.).
//!
//! Wire format: every emission is a single line of JSON with shape
//!
//!   {"type": "partial", "tool": "<id>", "content": "<text>",
//!    "progress": <0.0..=1.0 | null>}
//!
//! Consumers (the CLI's streaming renderer in `apps/theo-cli/src/render/
//! streaming.rs`, future TUI) parse these lines and update the
//! display with the documented 50 ms debounce.
//!
//! Pure helper — no IO of its own (the `Sender<String>` is owned by
//! the consumer side). All emissions are best-effort: when the
//! channel is closed (no consumer listening), the emit is silently
//! dropped instead of failing the tool.

use serde_json::json;
use theo_domain::tool::ToolContext;

/// Emit a free-form progress message ("step 2 of 5: rendering ...").
/// Best-effort — silently dropped when no consumer is wired up
/// (which is the common case for headless / non-streaming runs).
pub fn emit_progress(ctx: &ToolContext, tool_id: &str, content: impl Into<String>) {
    emit_inner(ctx, tool_id, content.into(), None);
}

/// Emit progress with a percentage indicator (0.0..=1.0). Out-of-
/// range values are clamped — callers don't need to validate.
pub fn emit_progress_with_pct(
    ctx: &ToolContext,
    tool_id: &str,
    content: impl Into<String>,
    progress: f32,
) {
    let clamped = progress.clamp(0.0, 1.0);
    emit_inner(ctx, tool_id, content.into(), Some(clamped));
}

/// Internal emission. Builds the JSON line + best-effort send.
/// Public-ish for the test module; not exposed at crate root.
fn emit_inner(ctx: &ToolContext, tool_id: &str, content: String, progress: Option<f32>) {
    let Some(tx) = ctx.stdout_tx.as_ref() else {
        return; // No consumer wired — silent no-op.
    };
    let envelope = json!({
        "type": "partial",
        "tool": tool_id,
        "content": content,
        "progress": progress,
    });
    let line = envelope.to_string();
    // try_send (non-blocking) so a slow consumer can't pin a tool's
    // hot path. If the channel is full or closed, the partial is
    // dropped — a rendering glitch is preferable to a stuck tool.
    let _ = tx.try_send(line);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use theo_domain::session::{MessageId, SessionId};
    use tokio::sync::mpsc;

    fn make_ctx_with_tx() -> (ToolContext, mpsc::Receiver<String>) {
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let (tx, rx) = mpsc::channel::<String>(32);
        let ctx = ToolContext {
            session_id: SessionId::new("ses_test"),
            message_id: MessageId::new(""),
            call_id: "call_test".into(),
            agent: "build".into(),
            abort: abort_rx,
            project_dir: PathBuf::from("/tmp"),
            graph_context: None,
            stdout_tx: Some(tx),
        };
        (ctx, rx)
    }

    fn make_ctx_no_tx() -> ToolContext {
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        ToolContext {
            session_id: SessionId::new("ses_test"),
            message_id: MessageId::new(""),
            call_id: "call_test".into(),
            agent: "build".into(),
            abort: abort_rx,
            project_dir: PathBuf::from("/tmp"),
            graph_context: None,
            stdout_tx: None,
        }
    }

    #[tokio::test]
    async fn t141p_emit_progress_writes_jsonl_envelope() {
        let (ctx, mut rx) = make_ctx_with_tx();
        emit_progress(&ctx, "browser_open", "loading page");
        let line = rx.recv().await.expect("expected one envelope");
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["type"], "partial");
        assert_eq!(v["tool"], "browser_open");
        assert_eq!(v["content"], "loading page");
        assert!(v["progress"].is_null());
    }

    #[tokio::test]
    async fn t141p_emit_progress_with_pct_records_clamped_value() {
        let (ctx, mut rx) = make_ctx_with_tx();
        emit_progress_with_pct(&ctx, "browser_screenshot", "encoding", 0.5);
        let line = rx.recv().await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        // 0.5 round-trips exactly through f32→json→f64; arbitrary
        // decimals like 0.42 do not (f32 precision). Use a value
        // that's exactly representable in binary floating point.
        assert_eq!(v["progress"], 0.5);
    }

    #[tokio::test]
    async fn t141p_emit_progress_clamps_out_of_range_values() {
        let (ctx, mut rx) = make_ctx_with_tx();
        // Below 0 → clamped to 0.
        emit_progress_with_pct(&ctx, "x", "below", -0.5);
        let line = rx.recv().await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["progress"], 0.0);
        // Above 1 → clamped to 1.
        emit_progress_with_pct(&ctx, "x", "above", 99.0);
        let line = rx.recv().await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["progress"], 1.0);
    }

    #[tokio::test]
    async fn t141p_emit_progress_without_consumer_is_silent_noop() {
        // No stdout_tx → nothing happens. Critical so headless runs
        // don't pay for partial emission they can't observe.
        let ctx = make_ctx_no_tx();
        emit_progress(&ctx, "x", "no consumer");
        emit_progress_with_pct(&ctx, "x", "still nothing", 0.5);
        // No assertion needed — the absence of panic IS the guarantee.
    }

    #[tokio::test]
    async fn t141p_emit_progress_full_channel_is_dropped_not_blocked() {
        // Build a channel of size 1 and fill it — the second emit
        // should be silently dropped (try_send → Err) instead of
        // pinning the tool's hot path.
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let (tx, _rx_kept) = mpsc::channel::<String>(1);
        let ctx = ToolContext {
            session_id: SessionId::new("ses_test"),
            message_id: MessageId::new(""),
            call_id: "call_test".into(),
            agent: "build".into(),
            abort: abort_rx,
            project_dir: PathBuf::from("/tmp"),
            graph_context: None,
            stdout_tx: Some(tx),
        };
        emit_progress(&ctx, "x", "first — fills the buffer");
        // Channel buffer is full. Second emit must NOT block.
        emit_progress(&ctx, "x", "second — must be dropped");
        // Reaching this assertion proves the helper didn't park.
        // (If it did, the test would hang and fail via tokio::test
        // timeout machinery.)
    }

    #[tokio::test]
    async fn t141p_emit_progress_closed_channel_is_dropped() {
        // Receiver dropped → channel closed → try_send returns
        // Closed. Helper must not panic.
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let (tx, rx) = mpsc::channel::<String>(8);
        drop(rx); // closes channel
        let ctx = ToolContext {
            session_id: SessionId::new("ses_test"),
            message_id: MessageId::new(""),
            call_id: "call_test".into(),
            agent: "build".into(),
            abort: abort_rx,
            project_dir: PathBuf::from("/tmp"),
            graph_context: None,
            stdout_tx: Some(tx),
        };
        emit_progress(&ctx, "x", "consumer is gone");
        emit_progress_with_pct(&ctx, "x", "still gone", 0.7);
        // No panic = pass.
    }

    #[tokio::test]
    async fn t141p_two_progresses_arrive_in_order() {
        let (ctx, mut rx) = make_ctx_with_tx();
        emit_progress(&ctx, "x", "first");
        emit_progress(&ctx, "x", "second");
        emit_progress(&ctx, "x", "third");
        let l1 = rx.recv().await.unwrap();
        let l2 = rx.recv().await.unwrap();
        let l3 = rx.recv().await.unwrap();
        let v1: serde_json::Value = serde_json::from_str(&l1).unwrap();
        let v2: serde_json::Value = serde_json::from_str(&l2).unwrap();
        let v3: serde_json::Value = serde_json::from_str(&l3).unwrap();
        assert_eq!(v1["content"], "first");
        assert_eq!(v2["content"], "second");
        assert_eq!(v3["content"], "third");
    }
}
