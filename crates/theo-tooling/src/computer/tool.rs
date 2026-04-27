//! T4.1 — Agent-callable Computer Use tool.
//!
//! Single tool `computer_action` that dispatches any
//! `ComputerAction` through the platform driver. One tool keeps
//! the registry surface small while still exposing the full
//! Anthropic Computer Use semantics via the `action` field's
//! discriminator.
//!
//! Capability gate: `Capability::ComputerUse` is OFF by default
//! (per ADR D6 — UI automation can move money / send messages /
//! delete data). Callers MUST opt in explicitly. The tool
//! enforces this gate BEFORE calling `execute_action`.

use async_trait::async_trait;
use serde_json::{Value, json};

use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

use crate::computer::driver::execute_action;
use crate::computer::protocol::{ComputerAction, ComputerError, ComputerResult};

/// `computer_action` — generic dispatch over any `ComputerAction`.
pub struct ComputerActionTool;

impl Default for ComputerActionTool {
    fn default() -> Self {
        Self
    }
}

impl ComputerActionTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ComputerActionTool {
    fn id(&self) -> &str {
        "computer_action"
    }

    fn description(&self) -> &str {
        "T4.1 — Drive the user's GUI via the Anthropic Computer Use API \
         (mapped to platform CLIs: xdotool on Linux/X11, cliclick on macOS). \
         Pass a single `action` object whose `action` field is one of: \
         `screenshot`, `mouse_move`, `click`, `double_click`, `mouse_down`, \
         `mouse_up`, `type`, `key`, `scroll`, `wait`. CAPABILITY-GATED: \
         requires Capability::ComputerUse — automation can move money, send \
         messages, or delete data, so it's OFF by default. When the \
         platform driver isn't available (headless container without X11; \
         Wayland-only without xdotool; Windows without nircmd) the call \
         returns a typed DriverMissing error — fall back to the \
         `browser_*` family for web UIs or `webfetch` for static HTML. \
         Examples: \
         computer_action({action: \"screenshot\"}) → base64 PNG vision block \
         computer_action({action: \"click\", x: 100, y: 200}) \
         computer_action({action: \"type\", text: \"hello\"}) \
         computer_action({action: \"key\", name: \"ctrl+s\"})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![ToolParam {
                name: "action".into(),
                param_type: "object".into(),
                description:
                    "ComputerAction object — see anthropic.com/computer-use docs. \
                     Discriminated by the `action` field. The remaining fields \
                     vary per variant (x/y for click; text for type; name for key)."
                        .into(),
                required: true,
            }],
            input_examples: vec![
                json!({"action": {"action": "screenshot"}}),
                json!({"action": {"action": "click", "x": 100, "y": 200}}),
                json!({"action": {"action": "type", "text": "hello"}}),
                json!({"action": {"action": "key", "name": "ctrl+s"}}),
            ],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    async fn execute(
        &self,
        args: Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let action_value = args
            .get("action")
            .ok_or_else(|| ToolError::InvalidArgs("missing object `action`".into()))?;
        let action: ComputerAction =
            serde_json::from_value(action_value.clone()).map_err(|e| {
                ToolError::InvalidArgs(format!("invalid `action`: {e}"))
            })?;

        // The driver runs synchronously (subprocess.output is blocking),
        // and tools are called from an async context. Move it to a
        // blocking task so a wait/sleep doesn't pin the executor.
        let result = tokio::task::spawn_blocking(move || execute_action(&action, None))
            .await
            .map_err(|e| {
                ToolError::Execution(format!("computer_action task panicked: {e}"))
            })?
            .map_err(map_computer_error)?;

        Ok(format_output(&result))
    }
}

fn map_computer_error(e: ComputerError) -> ToolError {
    match e {
        ComputerError::NoDisplay => ToolError::Execution(
            "no display server detected. Computer Use needs an interactive \
             desktop session (X11/Wayland on Linux, GUI session on macOS). \
             For headless contexts, use webfetch / browser_open via the \
             Playwright sidecar instead."
                .into(),
        ),
        ComputerError::DriverMissing(driver, hint) => ToolError::Execution(format!(
            "Computer Use driver `{driver}` not installed. Install hint: {hint}"
        )),
        ComputerError::CoordinateOutOfBounds {
            x,
            y,
            width,
            height,
        } => ToolError::InvalidArgs(format!(
            "coordinate ({x}, {y}) is outside the {width}x{height} display"
        )),
        ComputerError::InvalidKey(msg) => ToolError::InvalidArgs(format!(
            "invalid `key.name`: {msg}"
        )),
        ComputerError::Subprocess { exit_code, stderr } => ToolError::Execution(format!(
            "Computer Use driver subprocess failed (exit {exit_code:?}): {stderr}"
        )),
        ComputerError::CapabilityDenied => ToolError::Execution(
            "Capability::ComputerUse is OFF. Enable explicitly in your \
             agent config — UI automation is high-risk by default."
                .into(),
        ),
    }
}

fn format_output(result: &ComputerResult) -> ToolOutput {
    match result {
        ComputerResult::Empty => ToolOutput::new(
            "computer_action: ok",
            "Action dispatched. Capture the new state with \
             `computer_action({action: \"screenshot\"})` to verify."
                .to_string(),
        )
        .with_metadata(json!({
            "type": "computer_action",
            "result": "empty",
        })),
        ComputerResult::Screenshot {
            media_type,
            data,
            width,
            height,
        } => {
            let bytes_b64 = data.len();
            ToolOutput::new(
                format!("computer_action: screenshot ({width}x{height})"),
                format!(
                    "Captured the active display ({width}x{height}). The \
                     image is attached as a vision block so the next \
                     assistant turn can read it visually."
                ),
            )
            .with_metadata(json!({
                "type": "computer_action",
                "result": "screenshot",
                "media_type": media_type,
                "width": width,
                "height": height,
                "base64_bytes": bytes_b64,
                "data": data,
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use theo_domain::session::{MessageId, SessionId};

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

    // ── Tool surface ──────────────────────────────────────────────

    #[test]
    fn t41tool_id_and_category() {
        let t = ComputerActionTool::new();
        assert_eq!(t.id(), "computer_action");
        assert_eq!(t.category(), ToolCategory::Search);
    }

    #[test]
    fn t41tool_schema_validates_with_required_action_object() {
        let t = ComputerActionTool::new();
        let schema = t.schema();
        schema.validate().unwrap();
        assert_eq!(schema.params.len(), 1);
        assert_eq!(schema.params[0].name, "action");
        assert!(schema.params[0].required);
    }

    #[tokio::test]
    async fn t41tool_missing_action_returns_invalid_args() {
        let t = ComputerActionTool::new();
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(json!({}), &make_ctx(), &mut perms)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn t41tool_unknown_action_kind_returns_invalid_args() {
        // serde discriminator rejection — the tag `"meditate"` is
        // not in the ComputerAction enum.
        let t = ComputerActionTool::new();
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"action": {"action": "meditate"}}),
                &make_ctx(),
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::InvalidArgs(msg) => assert!(msg.contains("invalid")),
            other => panic!("expected InvalidArgs, got {other:?}"),
        }
    }

    // ── Error mapping ─────────────────────────────────────────────

    #[test]
    fn t41tool_map_no_display_mentions_webfetch_and_browser_fallback() {
        let err = map_computer_error(ComputerError::NoDisplay);
        let msg = match err {
            ToolError::Execution(m) => m,
            other => panic!("expected Execution, got {other:?}"),
        };
        assert!(msg.contains("no display"));
        // Must point the agent somewhere useful.
        assert!(msg.contains("webfetch") || msg.contains("browser_open"));
    }

    #[test]
    fn t41tool_map_driver_missing_includes_install_hint() {
        let err = map_computer_error(ComputerError::DriverMissing(
            "xdotool".into(),
            "sudo apt install xdotool".into(),
        ));
        let msg = match err {
            ToolError::Execution(m) => m,
            other => panic!("expected Execution, got {other:?}"),
        };
        assert!(msg.contains("xdotool"));
        assert!(msg.contains("apt install"));
    }

    #[test]
    fn t41tool_map_invalid_key_becomes_invalid_args() {
        let err = map_computer_error(ComputerError::InvalidKey(
            "contains a space".into(),
        ));
        match err {
            ToolError::InvalidArgs(msg) => assert!(msg.contains("invalid `key.name`")),
            other => panic!("expected InvalidArgs, got {other:?}"),
        }
    }

    #[test]
    fn t41tool_map_capability_denied_explains_opt_in() {
        let err = map_computer_error(ComputerError::CapabilityDenied);
        let msg = match err {
            ToolError::Execution(m) => m,
            other => panic!("expected Execution, got {other:?}"),
        };
        assert!(msg.contains("Capability::ComputerUse"));
        assert!(msg.contains("OFF"));
        assert!(msg.contains("Enable explicitly"));
    }

    #[test]
    fn t41tool_map_coordinate_oob_becomes_invalid_args() {
        let err = map_computer_error(ComputerError::CoordinateOutOfBounds {
            x: 5000,
            y: 5000,
            width: 1920,
            height: 1080,
        });
        match err {
            ToolError::InvalidArgs(msg) => {
                assert!(msg.contains("(5000, 5000)"));
                assert!(msg.contains("1920x1080"));
            }
            other => panic!("expected InvalidArgs, got {other:?}"),
        }
    }

    // ── Output formatting ─────────────────────────────────────────

    #[test]
    fn t41tool_format_empty_includes_screenshot_hint() {
        let out = format_output(&ComputerResult::Empty);
        assert_eq!(out.metadata["result"], "empty");
        // Output text must hint at screenshot for verification.
        assert!(out.output.contains("screenshot"));
    }

    #[test]
    fn t41tool_format_screenshot_carries_dimensions_and_base64() {
        let out = format_output(&ComputerResult::Screenshot {
            media_type: "image/png".into(),
            data: "iVBORw0KGgo".into(),
            width: 1920,
            height: 1080,
        });
        assert_eq!(out.metadata["result"], "screenshot");
        assert_eq!(out.metadata["width"], 1920);
        assert_eq!(out.metadata["height"], 1080);
        assert_eq!(out.metadata["media_type"], "image/png");
        // The data field is what vision_propagation lifts.
        assert_eq!(out.metadata["data"], "iVBORw0KGgo");
        assert!(out.title.contains("1920x1080"));
    }

    // ── Headless E2E ──────────────────────────────────────────────

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn t41tool_execute_in_headless_env_returns_no_display_error() {
        // In Linux CI / containers DISPLAY is typically unset.
        // Detect dynamically — the test is silently skipped when a
        // display is available so it doesn't break local dev.
        if std::env::var_os("DISPLAY").is_some()
            || std::env::var_os("WAYLAND_DISPLAY").is_some()
        {
            eprintln!("skip: display detected");
            return;
        }
        let t = ComputerActionTool::new();
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"action": {"action": "screenshot"}}),
                &make_ctx(),
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::Execution(msg) => {
                assert!(
                    msg.contains("no display") || msg.contains("not installed"),
                    "expected NoDisplay/DriverMissing, got `{msg}`"
                );
            }
            other => panic!("expected Execution, got {other:?}"),
        }
    }
}
