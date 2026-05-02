//! T14.1 — Consumer side of partial-progress streaming.
//!
//! Drains the `mpsc::Receiver<String>` that `ToolContext.stdout_tx`
//! feeds, parses each line as a partial-progress envelope, and
//! emits a debounced, latest-wins rendering.
//!
//! Wire format (matches `theo_tooling::partial`):
//!
//!   {"type": "partial", "tool": "<id>", "content": "<text>",
//!    "progress": <f32 | null>}
//!
//! Debouncing: a long-running tool (browser_screenshot full-page,
//! plan_create across many phases) may emit many partials in
//! rapid succession. Re-rendering the TUI on every event would
//! flicker. We collect events for 50 ms (the documented threshold
//! the human eye perceives as "instant" — IBM 1968), keep only
//! the last per tool_id, and emit one render per window.
//!
//! Public surface:
//!   - `parse_partial(line)` — pure JSON → typed Partial
//!   - `format_partial(p)` — pure typed → display string
//!   - `run_drainer(rx, on_render)` — async loop with 50 ms debounce
//!
//! Pure logic (parser + formatter) is testable in isolation.
//! The drainer takes a callback so the TUI can plug in whatever
//! rendering it likes (status line, progress bar, log line) without
//! this module needing to know about TUI internals.

use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;
use tokio::sync::mpsc;

/// The documented debounce window. 50 ms is the standard
/// "instant-feeling" threshold for UI updates (IBM, "Time as a
/// designed experience", 1968).
pub const DEBOUNCE_WINDOW: Duration = Duration::from_millis(50);

/// One parsed partial-progress event.
#[derive(Debug, Clone, PartialEq)]
pub struct Partial {
    pub tool: String,
    pub content: String,
    pub progress: Option<f32>,
}

/// Wire-format envelope. Private — callers see `Partial`.
#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(rename = "type")]
    type_field: String,
    tool: String,
    content: String,
    #[serde(default)]
    progress: Option<f32>,
}

/// Errors the parser surfaces.
#[derive(Debug, thiserror::Error)]
pub enum PartialError {
    #[error("invalid JSON: {0}")]
    Json(String),
    #[error("unexpected envelope type `{0}` (expected `partial`)")]
    BadType(String),
}

/// Parse one line as a partial-progress envelope.
pub fn parse_partial(line: &str) -> Result<Partial, PartialError> {
    let env: Envelope = serde_json::from_str(line)
        .map_err(|e| PartialError::Json(format!("{e}")))?;
    if env.type_field != "partial" {
        return Err(PartialError::BadType(env.type_field));
    }
    Ok(Partial {
        tool: env.tool,
        content: env.content,
        progress: env.progress,
    })
}

/// Format a partial for display. Plain-text — TUI / CLI layer can
/// add styling on top. Format:
///
///   "<tool>: <content>"           (no progress)
///   "<tool>: <content> [42%]"     (with progress)
pub fn format_partial(p: &Partial) -> String {
    match p.progress {
        Some(pct) => {
            let pct_int = (pct * 100.0).round().clamp(0.0, 100.0) as u32;
            format!("{}: {} [{}%]", p.tool, p.content, pct_int)
        }
        None => format!("{}: {}", p.tool, p.content),
    }
}

/// Drain `rx`, debouncing 50 ms with latest-wins per tool_id.
/// Calls `on_render(formatted_lines)` after each window closes.
///
/// Returns when `rx` closes (no more senders). Lines that fail to
/// parse are silently skipped — a malformed envelope shouldn't
/// kill the drainer.
pub async fn run_drainer<F>(mut rx: mpsc::Receiver<String>, mut on_render: F)
where
    F: FnMut(Vec<String>),
{
    loop {
        // Wait for the FIRST event (or shutdown).
        let first = match rx.recv().await {
            Some(line) => line,
            None => return, // channel closed
        };

        // Collect this event + everything that arrives within the
        // debounce window. Latest-wins per tool_id is enforced by
        // the HashMap keyed on `tool`.
        let mut latest: HashMap<String, Partial> = HashMap::new();
        if let Ok(p) = parse_partial(&first) {
            latest.insert(p.tool.clone(), p);
        }

        let deadline = tokio::time::Instant::now() + DEBOUNCE_WINDOW;
        loop {
            let timeout = deadline.saturating_duration_since(tokio::time::Instant::now());
            if timeout.is_zero() {
                break;
            }
            match tokio::time::timeout(timeout, rx.recv()).await {
                Ok(Some(line)) => {
                    if let Ok(p) = parse_partial(&line) {
                        latest.insert(p.tool.clone(), p);
                    }
                }
                Ok(None) => {
                    // Channel closed mid-window — flush + return.
                    if !latest.is_empty() {
                        on_render(render_lines(&latest));
                    }
                    return;
                }
                Err(_elapsed) => break,
            }
        }

        if !latest.is_empty() {
            on_render(render_lines(&latest));
        }
    }
}

/// Stable-ordered render: tools sorted alphabetically so the UI
/// doesn't reorder rows between renders. Each row is one
/// `format_partial(...)` line.
fn render_lines(latest: &HashMap<String, Partial>) -> Vec<String> {
    let mut keys: Vec<&String> = latest.keys().collect();
    keys.sort();
    keys.into_iter()
        .map(|k| format_partial(&latest[k]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    // ── parse_partial ────────────────────────────────────────────

    #[test]
    fn t141c_parse_full_envelope() {
        let line = r#"{"type":"partial","tool":"browser_open","content":"loading","progress":0.5}"#;
        let p = parse_partial(line).unwrap();
        assert_eq!(p.tool, "browser_open");
        assert_eq!(p.content, "loading");
        assert_eq!(p.progress, Some(0.5));
    }

    #[test]
    fn t141c_parse_envelope_without_progress_is_none() {
        let line = r#"{"type":"partial","tool":"x","content":"step 1"}"#;
        let p = parse_partial(line).unwrap();
        assert!(p.progress.is_none());
    }

    #[test]
    fn t141c_parse_rejects_bad_type() {
        let line = r#"{"type":"final","tool":"x","content":"y"}"#;
        let err = parse_partial(line).unwrap_err();
        match err {
            PartialError::BadType(t) => assert_eq!(t, "final"),
            other => panic!("expected BadType, got {other:?}"),
        }
    }

    #[test]
    fn t141c_parse_rejects_invalid_json() {
        let err = parse_partial("not json").unwrap_err();
        assert!(matches!(err, PartialError::Json(_)));
    }

    #[test]
    fn t141c_parse_rejects_envelope_missing_required_fields() {
        let line = r#"{"type":"partial","content":"x"}"#; // missing `tool`
        let err = parse_partial(line).unwrap_err();
        assert!(matches!(err, PartialError::Json(_)));
    }

    // ── format_partial ───────────────────────────────────────────

    #[test]
    fn t141c_format_no_progress_omits_pct() {
        let p = Partial {
            tool: "browser_open".into(),
            content: "loading".into(),
            progress: None,
        };
        assert_eq!(format_partial(&p), "browser_open: loading");
    }

    #[test]
    fn t141c_format_with_progress_shows_integer_pct() {
        let p = Partial {
            tool: "browser_open".into(),
            content: "loading".into(),
            progress: Some(0.5),
        };
        assert_eq!(format_partial(&p), "browser_open: loading [50%]");
    }

    #[test]
    fn t141c_format_progress_rounds_to_nearest_pct() {
        // 0.426 → 43% (round half-up).
        let p = Partial {
            tool: "x".into(),
            content: "y".into(),
            progress: Some(0.426),
        };
        assert_eq!(format_partial(&p), "x: y [43%]");
    }

    #[test]
    fn t141c_format_clamps_progress_to_0_100() {
        let p_below = Partial {
            tool: "x".into(),
            content: "y".into(),
            progress: Some(-0.5),
        };
        let p_above = Partial {
            tool: "x".into(),
            content: "y".into(),
            progress: Some(1.5),
        };
        assert!(format_partial(&p_below).ends_with("[0%]"));
        assert!(format_partial(&p_above).ends_with("[100%]"));
    }

    // ── run_drainer (with debounce) ──────────────────────────────

    fn make_envelope(tool: &str, content: &str, progress: Option<f32>) -> String {
        let v = match progress {
            Some(p) => serde_json::json!({
                "type": "partial",
                "tool": tool,
                "content": content,
                "progress": p,
            }),
            None => serde_json::json!({
                "type": "partial",
                "tool": tool,
                "content": content,
            }),
        };
        v.to_string()
    }

    #[tokio::test]
    async fn t141c_drainer_returns_immediately_when_channel_closed() {
        let (_tx, rx) = mpsc::channel::<String>(8);
        // Drop sender so receiver sees no more events.
        drop(_tx);
        let frames: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
        let frames_clone = frames.clone();
        run_drainer(rx, move |lines| frames_clone.lock().unwrap().push(lines)).await;
        // No events were emitted → no frames rendered.
        assert!(frames.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn t141c_drainer_emits_single_frame_per_debounce_window() {
        // Three rapid-fire events for ONE tool: the drainer should
        // collapse them into a single frame containing the LAST
        // value (latest-wins).
        let (tx, rx) = mpsc::channel::<String>(16);
        let frames: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
        let frames_clone = frames.clone();
        let drainer = tokio::spawn(run_drainer(rx, move |lines| {
            frames_clone.lock().unwrap().push(lines)
        }));

        // Send three events back-to-back (within the 50 ms window).
        tx.send(make_envelope("a", "first", Some(0.1)))
            .await
            .unwrap();
        tx.send(make_envelope("a", "second", Some(0.5)))
            .await
            .unwrap();
        tx.send(make_envelope("a", "third", Some(0.9)))
            .await
            .unwrap();

        // Wait for the window to close.
        tokio::time::sleep(DEBOUNCE_WINDOW + Duration::from_millis(20)).await;
        drop(tx); // close so drainer exits
        drainer.await.unwrap();

        let frames_snap = frames.lock().unwrap();
        assert_eq!(frames_snap.len(), 1, "expected exactly one frame");
        assert_eq!(frames_snap[0], vec!["a: third [90%]".to_string()]);
    }

    #[tokio::test]
    async fn t141c_drainer_separate_tools_appear_in_same_frame() {
        // Two tools emit simultaneously — the drainer should render
        // both in ONE frame (sorted alphabetically by tool id).
        let (tx, rx) = mpsc::channel::<String>(16);
        let frames: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
        let frames_clone = frames.clone();
        let drainer = tokio::spawn(run_drainer(rx, move |lines| {
            frames_clone.lock().unwrap().push(lines)
        }));

        tx.send(make_envelope("zebra", "z", None)).await.unwrap();
        tx.send(make_envelope("alpha", "a", None)).await.unwrap();
        tokio::time::sleep(DEBOUNCE_WINDOW + Duration::from_millis(20)).await;
        drop(tx);
        drainer.await.unwrap();

        let frames_snap = frames.lock().unwrap();
        assert_eq!(frames_snap.len(), 1);
        // Sorted: alpha before zebra.
        assert_eq!(
            frames_snap[0],
            vec!["alpha: a".to_string(), "zebra: z".to_string()],
        );
    }

    #[tokio::test]
    async fn t141c_drainer_emits_multiple_frames_for_separated_bursts() {
        // First burst → frame 1. Sleep > 50 ms. Second burst → frame 2.
        let (tx, rx) = mpsc::channel::<String>(16);
        let frames: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
        let frames_clone = frames.clone();
        let drainer = tokio::spawn(run_drainer(rx, move |lines| {
            frames_clone.lock().unwrap().push(lines)
        }));

        tx.send(make_envelope("a", "first", None)).await.unwrap();
        tokio::time::sleep(DEBOUNCE_WINDOW + Duration::from_millis(60)).await;
        tx.send(make_envelope("a", "second", None)).await.unwrap();
        tokio::time::sleep(DEBOUNCE_WINDOW + Duration::from_millis(60)).await;
        drop(tx);
        drainer.await.unwrap();

        let frames_snap = frames.lock().unwrap();
        assert_eq!(frames_snap.len(), 2, "two distinct windows = two frames");
        assert_eq!(frames_snap[0], vec!["a: first".to_string()]);
        assert_eq!(frames_snap[1], vec!["a: second".to_string()]);
    }

    #[tokio::test]
    async fn t141c_drainer_skips_malformed_lines_silently() {
        // Garbage envelope MUST NOT crash the drainer. The next
        // valid envelope still gets rendered.
        let (tx, rx) = mpsc::channel::<String>(16);
        let frames: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
        let frames_clone = frames.clone();
        let drainer = tokio::spawn(run_drainer(rx, move |lines| {
            frames_clone.lock().unwrap().push(lines)
        }));

        tx.send("not json at all".into()).await.unwrap();
        tx.send(r#"{"type":"final","tool":"x","content":"y"}"#.into())
            .await
            .unwrap();
        tx.send(make_envelope("good", "ok", None)).await.unwrap();
        tokio::time::sleep(DEBOUNCE_WINDOW + Duration::from_millis(20)).await;
        drop(tx);
        drainer.await.unwrap();

        let frames_snap = frames.lock().unwrap();
        assert_eq!(frames_snap.len(), 1);
        assert_eq!(frames_snap[0], vec!["good: ok".to_string()]);
    }

    #[tokio::test]
    async fn t141c_drainer_flushes_pending_window_when_channel_closes_mid_window() {
        // An event arrives, then the sender drops BEFORE the 50 ms
        // window closes. The drainer must still emit a final frame
        // (otherwise the last partial would be lost).
        let (tx, rx) = mpsc::channel::<String>(8);
        let frames: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
        let frames_clone = frames.clone();
        let drainer = tokio::spawn(run_drainer(rx, move |lines| {
            frames_clone.lock().unwrap().push(lines)
        }));

        tx.send(make_envelope("a", "in flight", None)).await.unwrap();
        // Drop immediately — channel closes inside the window.
        drop(tx);
        drainer.await.unwrap();

        let frames_snap = frames.lock().unwrap();
        assert_eq!(frames_snap.len(), 1, "in-flight partial must be flushed");
        assert_eq!(frames_snap[0], vec!["a: in flight".to_string()]);
    }

    #[test]
    fn t141c_render_lines_orders_tools_alphabetically() {
        let mut latest = HashMap::new();
        latest.insert(
            "zeta".to_string(),
            Partial {
                tool: "zeta".into(),
                content: "z".into(),
                progress: None,
            },
        );
        latest.insert(
            "alpha".to_string(),
            Partial {
                tool: "alpha".into(),
                content: "a".into(),
                progress: None,
            },
        );
        latest.insert(
            "mid".to_string(),
            Partial {
                tool: "mid".into(),
                content: "m".into(),
                progress: None,
            },
        );
        let out = render_lines(&latest);
        assert_eq!(
            out,
            vec![
                "alpha: a".to_string(),
                "mid: m".to_string(),
                "zeta: z".to_string(),
            ],
        );
    }
}
