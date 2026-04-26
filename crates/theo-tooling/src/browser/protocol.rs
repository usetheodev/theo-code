//! T2.1 — Browser sidecar JSON-RPC protocol.
//!
//! Serializes the Rust client's `BrowserAction` into a JSON request
//! the Node sidecar (`scripts/playwright_sidecar.js`) understands, and
//! deserializes the sidecar's response into `BrowserResult`. The
//! sidecar implementation matches this protocol verbatim — the JSON
//! shape IS the API contract.
//!
//! Pure code — no IO. Tests prove the wire format end-to-end without
//! Node installed.

use serde::{Deserialize, Serialize};

/// Image format for `Screenshot` actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ScreenshotFormat {
    Png,
    Jpeg,
}

impl Default for ScreenshotFormat {
    fn default() -> Self {
        Self::Png
    }
}

/// One action the Rust client asks the sidecar to perform.
///
/// Every variant maps 1:1 to a Playwright API:
/// - `Open` → `page.goto(url)`
/// - `Click` → `page.click(selector)`
/// - `Type` → `page.fill(selector, text)` (faster than typeKeys for forms)
/// - `Screenshot` → `page.screenshot({fullPage, type})` returning base64
/// - `Eval` → `page.evaluate(js)` (returns the JSON-serialised result)
/// - `WaitForSelector` → `page.waitForSelector(selector, {timeoutMs})`
/// - `Close` → `browser.close()`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case")]
#[non_exhaustive]
pub enum BrowserAction {
    Open {
        url: String,
    },
    Click {
        selector: String,
    },
    Type {
        selector: String,
        text: String,
    },
    Screenshot {
        #[serde(default)]
        full_page: bool,
        #[serde(default)]
        format: ScreenshotFormat,
    },
    Eval {
        js: String,
    },
    WaitForSelector {
        selector: String,
        #[serde(default = "default_wait_ms")]
        timeout_ms: u64,
    },
    Close,
}

fn default_wait_ms() -> u64 {
    5_000
}

/// Wire-format request the Rust client writes to the sidecar's stdin.
/// `id` correlates request → response (sidecar echoes it back).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BrowserRequest {
    pub id: u64,
    #[serde(flatten)]
    pub action: BrowserAction,
}

/// Result variants. `Empty` is the response shape for actions that
/// don't return content (Click, Type, Close, WaitForSelector when
/// the selector is found in time).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum BrowserResult {
    /// Action succeeded with no content (click, type, close).
    Empty,
    /// `Open` confirmation: the URL the page actually landed on
    /// (after redirects) and the document title.
    Navigated { final_url: String, title: String },
    /// Base64-encoded image (PNG or JPEG per the requesting action's
    /// `format`).
    Screenshot {
        media_type: String,
        data: String,
    },
    /// `Eval` result. The sidecar JSON-serialises whatever the JS
    /// expression returns; complex objects round-trip through
    /// `JSON.stringify` on the JS side.
    EvalResult { value: serde_json::Value },
    /// `WaitForSelector` finished (the element became visible).
    SelectorFound,
}

/// Wire-format response the sidecar writes to its stdout.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BrowserResponse {
    pub id: u64,
    #[serde(flatten)]
    pub outcome: ResponseOutcome,
}

/// Top-level success/error union. Exposed for clarity on the wire;
/// real Rust callers prefer `BrowserResponse::result()` which returns
/// `Result<BrowserResult, BrowserError>`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ResponseOutcome {
    Ok { result: BrowserResult },
    Err { error: BrowserError },
}

impl BrowserResponse {
    /// Convert the outcome union into a Rust-friendly `Result`.
    pub fn result(self) -> Result<BrowserResult, BrowserError> {
        match self.outcome {
            ResponseOutcome::Ok { result } => Ok(result),
            ResponseOutcome::Err { error } => Err(error),
        }
    }
}

/// Errors the sidecar may surface.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, thiserror::Error)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BrowserError {
    #[error("playwright not installed: {0}")]
    PlaywrightMissing(String),
    #[error("navigation failed: {0}")]
    NavigationFailed(String),
    #[error("selector not found within timeout: {selector} (waited {timeout_ms}ms)")]
    SelectorTimeout { selector: String, timeout_ms: u64 },
    #[error("javascript evaluation failed: {0}")]
    EvalFailed(String),
    #[error("browser session not open — call `open` first")]
    NotOpen,
    #[error("internal sidecar error: {0}")]
    Internal(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ---- BrowserAction ----

    #[test]
    fn t21proto_open_action_serializes_with_url() {
        let a = BrowserAction::Open {
            url: "https://e.x".into(),
        };
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["action"], "open");
        assert_eq!(json["url"], "https://e.x");
    }

    #[test]
    fn t21proto_click_action_serializes_with_selector() {
        let a = BrowserAction::Click {
            selector: "#btn".into(),
        };
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["action"], "click");
        assert_eq!(json["selector"], "#btn");
    }

    #[test]
    fn t21proto_type_action_serializes_with_selector_and_text() {
        let a = BrowserAction::Type {
            selector: "input[name=q]".into(),
            text: "hello".into(),
        };
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["action"], "type");
        assert_eq!(json["selector"], "input[name=q]");
        assert_eq!(json["text"], "hello");
    }

    #[test]
    fn t21proto_screenshot_action_default_format_is_png() {
        let a = BrowserAction::Screenshot {
            full_page: true,
            format: ScreenshotFormat::default(),
        };
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["action"], "screenshot");
        assert_eq!(json["full_page"], true);
        assert_eq!(json["format"], "png");
    }

    #[test]
    fn t21proto_eval_action_serializes_with_js_string() {
        let a = BrowserAction::Eval {
            js: "document.title".into(),
        };
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["action"], "eval");
        assert_eq!(json["js"], "document.title");
    }

    #[test]
    fn t21proto_wait_action_default_timeout_is_5s() {
        let a = BrowserAction::WaitForSelector {
            selector: ".loading".into(),
            timeout_ms: default_wait_ms(),
        };
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["timeout_ms"], 5_000);
    }

    #[test]
    fn t21proto_close_action_serializes_with_no_extra_fields() {
        let a = BrowserAction::Close;
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["action"], "close");
        assert_eq!(json.as_object().unwrap().len(), 1);
    }

    #[test]
    fn t21proto_action_serde_roundtrip_all_variants() {
        for a in [
            BrowserAction::Open { url: "u".into() },
            BrowserAction::Click { selector: "s".into() },
            BrowserAction::Type {
                selector: "s".into(),
                text: "t".into(),
            },
            BrowserAction::Screenshot {
                full_page: false,
                format: ScreenshotFormat::Jpeg,
            },
            BrowserAction::Eval { js: "1+1".into() },
            BrowserAction::WaitForSelector {
                selector: "x".into(),
                timeout_ms: 1234,
            },
            BrowserAction::Close,
        ] {
            let json = serde_json::to_string(&a).unwrap();
            let back: BrowserAction = serde_json::from_str(&json).unwrap();
            assert_eq!(a, back, "roundtrip failed for {a:?}");
        }
    }

    // ---- BrowserRequest ----

    #[test]
    fn t21proto_request_flattens_action_with_id() {
        let req = BrowserRequest {
            id: 42,
            action: BrowserAction::Click {
                selector: "#x".into(),
            },
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["id"], 42);
        assert_eq!(json["action"], "click");
        assert_eq!(json["selector"], "#x");
    }

    // ---- BrowserResult ----

    #[test]
    fn t21proto_navigated_result_carries_url_and_title() {
        let r = BrowserResult::Navigated {
            final_url: "https://e.x/landed".into(),
            title: "Landing".into(),
        };
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["kind"], "navigated");
        assert_eq!(json["final_url"], "https://e.x/landed");
        assert_eq!(json["title"], "Landing");
    }

    #[test]
    fn t21proto_screenshot_result_carries_base64_and_mime() {
        let r = BrowserResult::Screenshot {
            media_type: "image/png".into(),
            data: "AAAA".into(),
        };
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["kind"], "screenshot");
        assert_eq!(json["media_type"], "image/png");
        assert_eq!(json["data"], "AAAA");
    }

    #[test]
    fn t21proto_eval_result_passes_arbitrary_json() {
        let r = BrowserResult::EvalResult {
            value: json!({"answer": 42, "items": [1,2,3]}),
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: BrowserResult = serde_json::from_str(&s).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn t21proto_empty_result_serializes_as_kind_only() {
        let r = BrowserResult::Empty;
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["kind"], "empty");
    }

    // ---- Response (success / error union) ----

    #[test]
    fn t21proto_success_response_decodes_into_ok_result() {
        let raw = json!({
            "id": 1,
            "result": {"kind": "navigated", "final_url": "u", "title": "t"}
        });
        let resp: BrowserResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(resp.id, 1);
        match resp.result() {
            Ok(BrowserResult::Navigated { final_url, title }) => {
                assert_eq!(final_url, "u");
                assert_eq!(title, "t");
            }
            other => panic!("expected Navigated, got {other:?}"),
        }
    }

    #[test]
    fn t21proto_error_response_decodes_into_err_variant() {
        let raw = json!({
            "id": 2,
            "error": {"selector_timeout": {"selector": ".x", "timeout_ms": 5000}}
        });
        let resp: BrowserResponse = serde_json::from_value(raw).unwrap();
        match resp.result() {
            Err(BrowserError::SelectorTimeout {
                selector,
                timeout_ms,
            }) => {
                assert_eq!(selector, ".x");
                assert_eq!(timeout_ms, 5000);
            }
            other => panic!("expected SelectorTimeout, got {other:?}"),
        }
    }

    #[test]
    fn t21proto_error_playwright_missing_displays_actionable_message() {
        let e = BrowserError::PlaywrightMissing("install via npx".into());
        let s = e.to_string();
        assert!(s.contains("playwright not installed"));
        assert!(s.contains("install via npx"));
    }

    #[test]
    fn t21proto_error_serde_roundtrip_all_variants() {
        for e in [
            BrowserError::PlaywrightMissing("x".into()),
            BrowserError::NavigationFailed("net::ERR".into()),
            BrowserError::SelectorTimeout {
                selector: ".x".into(),
                timeout_ms: 1000,
            },
            BrowserError::EvalFailed("ReferenceError".into()),
            BrowserError::NotOpen,
            BrowserError::Internal("oops".into()),
        ] {
            let json = serde_json::to_string(&e).unwrap();
            let back: BrowserError = serde_json::from_str(&json).unwrap();
            assert_eq!(e, back);
        }
    }

    // ---- ScreenshotFormat ----

    #[test]
    fn t21proto_screenshot_format_default_is_png() {
        assert_eq!(ScreenshotFormat::default(), ScreenshotFormat::Png);
    }

    #[test]
    fn t21proto_screenshot_format_serde_lowercase() {
        assert_eq!(
            serde_json::to_value(ScreenshotFormat::Png).unwrap(),
            json!("png")
        );
        assert_eq!(
            serde_json::to_value(ScreenshotFormat::Jpeg).unwrap(),
            json!("jpeg")
        );
    }
}
