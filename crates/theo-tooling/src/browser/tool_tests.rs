// Sibling test body of `browser/tool.rs` re-attached via
// `#[cfg(test)] #[path = "tool_tests.rs"] mod tests;`. The inner
// attribute below is redundant for the compiler (the `mod` decl
// already cfg-gates this file) but signals to scripts/check-unwrap.sh
// and scripts/check-panic.sh that every line is test-only — so the
// production-only filter excludes the entire file from violation
// counts. Only test code lives here.
#![cfg(test)]

#![allow(unused_imports)]

use super::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};

use theo_domain::error::ToolError;
use theo_domain::session::{MessageId, SessionId};
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput,
};

use crate::browser::tool_common::*;
use crate::browser::click::*;
use crate::browser::close::*;
use crate::browser::eval::*;
use crate::browser::open::*;
use crate::browser::screenshot::*;
use crate::browser::status::*;
use crate::browser::type_text::*;
use crate::browser::wait_for_selector::*;

fn make_ctx() -> ToolContext {
    let (_tx, rx) = tokio::sync::watch::channel(false);
    ToolContext {
        session_id: SessionId::new("ses_test"),
        message_id: MessageId::new(""),
        call_id: "call_test".into(),
        agent: "build".into(),
        abort: rx,
        project_dir: PathBuf::from("/tmp"),
        graph_context: None,
        stdout_tx: None,
    }
}

fn missing_script_manager() -> Arc<BrowserSessionManager> {
    // A path that is guaranteed not to exist — every ensure_client
    // call surfaces ScriptMissing, which the tools should map to
    // ToolError::Execution with an actionable message.
    Arc::new(BrowserSessionManager::new(
        "node",
        PathBuf::from("/nonexistent/playwright_sidecar.js"),
    ))
}

// ── browser_status ───────────────────────────────────────────

#[test]
fn t21btool_status_id_and_category() {
    let t = BrowserStatusTool::new(missing_script_manager());
    assert_eq!(t.id(), "browser_status");
    assert_eq!(t.category(), ToolCategory::Search);
}

#[test]
fn t21btool_status_schema_validates_with_no_args() {
    let t = BrowserStatusTool::new(missing_script_manager());
    let s = t.schema();
    assert!(s.params.is_empty());
    s.validate().unwrap();
}

#[tokio::test]
async fn t21btool_status_missing_script_steers_to_webfetch_fallback() {
    let t = BrowserStatusTool::new(missing_script_manager());
    let mut perms = PermissionCollector::new();
    let out = t
        .execute(json!({}), &make_ctx(), &mut perms)
        .await
        .unwrap();
    assert_eq!(out.metadata["type"], "browser_status");
    assert_eq!(out.metadata["script_present"], false);
    assert_eq!(out.metadata["session_active"], false);
    assert!(out.output.contains("webfetch"));
    assert!(out.output.contains("not reachable"));
}

#[tokio::test]
async fn t21btool_status_present_script_steers_to_browser_open() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("playwright_sidecar.js");
    std::fs::write(&script, b"// stub").unwrap();
    let mgr = Arc::new(BrowserSessionManager::new("node", script.clone()));
    let t = BrowserStatusTool::new(mgr);
    let mut perms = PermissionCollector::new();
    let out = t
        .execute(json!({}), &make_ctx(), &mut perms)
        .await
        .unwrap();
    assert_eq!(out.metadata["script_present"], true);
    assert_eq!(out.metadata["session_active"], false);
    assert_eq!(out.metadata["script_path"], script.display().to_string());
    assert!(
        out.output.contains("Open one with browser_open"),
        "should steer agent to browser_open when script ready but no session yet"
    );
}

// ── browser_open ──────────────────────────────────────────────

#[test]
fn t21btool_open_id_and_category() {
    let t = BrowserOpenTool::new(missing_script_manager());
    assert_eq!(t.id(), "browser_open");
    assert_eq!(t.category(), ToolCategory::Search);
}

#[test]
fn t21btool_open_schema_validates() {
    let t = BrowserOpenTool::new(missing_script_manager());
    t.schema().validate().unwrap();
}

#[tokio::test]
async fn t21btool_open_missing_url_returns_invalid_args() {
    let t = BrowserOpenTool::new(missing_script_manager());
    let mut perms = PermissionCollector::new();
    let err = t.execute(json!({}), &make_ctx(), &mut perms).await.unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn t21btool_open_empty_url_returns_invalid_args() {
    let t = BrowserOpenTool::new(missing_script_manager());
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(json!({"url": "  "}), &make_ctx(), &mut perms)
        .await
        .unwrap_err();
    match err {
        ToolError::InvalidArgs(msg) => assert!(msg.contains("`url` is empty")),
        other => panic!("expected InvalidArgs, got {other:?}"),
    }
}

#[tokio::test]
async fn t21btool_open_missing_script_returns_actionable_error() {
    let t = BrowserOpenTool::new(missing_script_manager());
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"url": "https://example.com"}),
            &make_ctx(),
            &mut perms,
        )
        .await
        .unwrap_err();
    match err {
        ToolError::Execution(msg) => {
            assert!(msg.contains("playwright_sidecar.js"));
            assert!(msg.contains("crates/theo-tooling/scripts"));
        }
        other => panic!("expected Execution error, got {other:?}"),
    }
}

// ── browser_click ─────────────────────────────────────────────

#[test]
fn t21btool_click_id_and_category() {
    let t = BrowserClickTool::new(missing_script_manager());
    assert_eq!(t.id(), "browser_click");
    assert_eq!(t.category(), ToolCategory::Search);
}

#[test]
fn t21btool_click_schema_validates() {
    let t = BrowserClickTool::new(missing_script_manager());
    t.schema().validate().unwrap();
}

#[tokio::test]
async fn t21btool_click_missing_selector_returns_invalid_args() {
    let t = BrowserClickTool::new(missing_script_manager());
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(json!({}), &make_ctx(), &mut perms)
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn t21btool_click_empty_selector_returns_invalid_args() {
    let t = BrowserClickTool::new(missing_script_manager());
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"selector": "   "}),
            &make_ctx(),
            &mut perms,
        )
        .await
        .unwrap_err();
    match err {
        ToolError::InvalidArgs(msg) => assert!(msg.contains("`selector` is empty")),
        other => panic!("expected InvalidArgs, got {other:?}"),
    }
}

// ── browser_screenshot ────────────────────────────────────────

#[test]
fn t21btool_screenshot_id_and_category() {
    let t = BrowserScreenshotTool::new(missing_script_manager());
    assert_eq!(t.id(), "browser_screenshot");
    assert_eq!(t.category(), ToolCategory::Search);
}

#[test]
fn t21btool_screenshot_schema_validates_with_optional_args() {
    let t = BrowserScreenshotTool::new(missing_script_manager());
    let schema = t.schema();
    schema.validate().unwrap();
    // Both fields are optional — caller can omit both.
    for p in &schema.params {
        assert!(!p.required, "{} should be optional", p.name);
    }
}

#[tokio::test]
async fn t21btool_screenshot_invalid_format_returns_invalid_args() {
    let t = BrowserScreenshotTool::new(missing_script_manager());
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"format": "webp"}),
            &make_ctx(),
            &mut perms,
        )
        .await
        .unwrap_err();
    match err {
        ToolError::InvalidArgs(msg) => {
            assert!(msg.contains("`format`"));
            assert!(msg.contains("png"));
            assert!(msg.contains("jpeg"));
        }
        other => panic!("expected InvalidArgs, got {other:?}"),
    }
}

#[tokio::test]
async fn t21btool_screenshot_jpg_alias_accepted() {
    // `jpg` is the common typo for `jpeg`. Tools accept both
    // for ergonomics; map to ScreenshotFormat::Jpeg internally.
    // We can't run the full request without a sidecar, so the
    // test only verifies args parsing — the actual sidecar
    // call surfaces ScriptMissing as expected.
    let t = BrowserScreenshotTool::new(missing_script_manager());
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(json!({"format": "jpg"}), &make_ctx(), &mut perms)
        .await
        .unwrap_err();
    // The arg parser accepted `jpg` (no InvalidArgs); the failure
    // is the downstream missing-script error.
    match err {
        ToolError::Execution(msg) => assert!(msg.contains("playwright_sidecar.js")),
        other => panic!("expected downstream Execution error, got {other:?}"),
    }
}

// ── browser_close ─────────────────────────────────────────────

#[test]
fn t21btool_close_id_and_category() {
    let t = BrowserCloseTool::new(missing_script_manager());
    assert_eq!(t.id(), "browser_close");
    assert_eq!(t.category(), ToolCategory::Search);
}

#[test]
fn t21btool_close_schema_validates_with_no_args() {
    let t = BrowserCloseTool::new(missing_script_manager());
    let schema = t.schema();
    schema.validate().unwrap();
    assert!(schema.params.is_empty());
}

#[tokio::test]
async fn t21btool_close_no_active_session_returns_was_active_false() {
    // Idempotency invariant: closing without an open session is
    // a no-op success, NOT an error. Cleanup routines might call
    // this without knowing the state.
    let t = BrowserCloseTool::new(missing_script_manager());
    let mut perms = PermissionCollector::new();
    let out = t
        .execute(json!({}), &make_ctx(), &mut perms)
        .await
        .unwrap();
    assert_eq!(out.metadata["was_active"], false);
    assert!(out.title.contains("no session was active"));
}

// ── browser_type ──────────────────────────────────────────────

#[test]
fn t21btool_type_id_and_category() {
    let t = BrowserTypeTool::new(missing_script_manager());
    assert_eq!(t.id(), "browser_type");
    assert_eq!(t.category(), ToolCategory::Search);
}

#[test]
fn t21btool_type_schema_validates_with_required_args() {
    let t = BrowserTypeTool::new(missing_script_manager());
    let schema = t.schema();
    schema.validate().unwrap();
    for p in &schema.params {
        assert!(p.required, "{} should be required", p.name);
    }
}

#[tokio::test]
async fn t21btool_type_missing_selector_returns_invalid_args() {
    let t = BrowserTypeTool::new(missing_script_manager());
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(json!({"text": "hi"}), &make_ctx(), &mut perms)
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn t21btool_type_missing_text_returns_invalid_args() {
    let t = BrowserTypeTool::new(missing_script_manager());
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"selector": "input"}),
            &make_ctx(),
            &mut perms,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn t21btool_type_empty_selector_returns_invalid_args() {
    let t = BrowserTypeTool::new(missing_script_manager());
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"selector": "  ", "text": "hi"}),
            &make_ctx(),
            &mut perms,
        )
        .await
        .unwrap_err();
    match err {
        ToolError::InvalidArgs(msg) => assert!(msg.contains("`selector` is empty")),
        other => panic!("expected InvalidArgs, got {other:?}"),
    }
}

#[tokio::test]
async fn t21btool_type_empty_text_is_accepted() {
    // Empty text is the "clear field" idiom — must NOT be rejected
    // as InvalidArgs. The downstream missing-script error confirms
    // the args parsed correctly.
    let t = BrowserTypeTool::new(missing_script_manager());
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"selector": "input", "text": ""}),
            &make_ctx(),
            &mut perms,
        )
        .await
        .unwrap_err();
    match err {
        ToolError::Execution(msg) => assert!(msg.contains("playwright_sidecar.js")),
        other => panic!("expected downstream Execution error, got {other:?}"),
    }
}

// ── browser_eval ──────────────────────────────────────────────

#[test]
fn t21btool_eval_id_and_category() {
    let t = BrowserEvalTool::new(missing_script_manager());
    assert_eq!(t.id(), "browser_eval");
    assert_eq!(t.category(), ToolCategory::Search);
}

#[test]
fn t21btool_eval_schema_validates_with_js_required() {
    let t = BrowserEvalTool::new(missing_script_manager());
    let schema = t.schema();
    schema.validate().unwrap();
    let js = schema.params.iter().find(|p| p.name == "js").unwrap();
    assert!(js.required);
}

#[tokio::test]
async fn t21btool_eval_missing_js_returns_invalid_args() {
    let t = BrowserEvalTool::new(missing_script_manager());
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(json!({}), &make_ctx(), &mut perms)
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn t21btool_eval_empty_js_returns_invalid_args() {
    let t = BrowserEvalTool::new(missing_script_manager());
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"js": "   "}),
            &make_ctx(),
            &mut perms,
        )
        .await
        .unwrap_err();
    match err {
        ToolError::InvalidArgs(msg) => assert!(msg.contains("`js` is empty")),
        other => panic!("expected InvalidArgs, got {other:?}"),
    }
}

// ── browser_wait_for_selector ─────────────────────────────────

#[test]
fn t21btool_wait_id_and_category() {
    let t = BrowserWaitForSelectorTool::new(missing_script_manager());
    assert_eq!(t.id(), "browser_wait_for_selector");
    assert_eq!(t.category(), ToolCategory::Search);
}

#[test]
fn t21btool_wait_schema_validates_with_optional_timeout() {
    let t = BrowserWaitForSelectorTool::new(missing_script_manager());
    let schema = t.schema();
    schema.validate().unwrap();
    let sel = schema.params.iter().find(|p| p.name == "selector").unwrap();
    let to = schema
        .params
        .iter()
        .find(|p| p.name == "timeout_ms")
        .unwrap();
    assert!(sel.required);
    assert!(!to.required);
}

#[tokio::test]
async fn t21btool_wait_missing_selector_returns_invalid_args() {
    let t = BrowserWaitForSelectorTool::new(missing_script_manager());
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(json!({}), &make_ctx(), &mut perms)
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn t21btool_wait_clamps_timeout_to_max() {
    // 9_999_999_999 is way over MAX_WAIT_MS (60s). The tool
    // accepts it (parses it) but clamps internally. The args
    // parser doesn't surface InvalidArgs — verified indirectly
    // by reaching the downstream missing-script error.
    let t = BrowserWaitForSelectorTool::new(missing_script_manager());
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"selector": ".x", "timeout_ms": 9_999_999_999u64}),
            &make_ctx(),
            &mut perms,
        )
        .await
        .unwrap_err();
    match err {
        ToolError::Execution(msg) => assert!(msg.contains("playwright_sidecar.js")),
        other => panic!("expected downstream Execution error, got {other:?}"),
    }
}
