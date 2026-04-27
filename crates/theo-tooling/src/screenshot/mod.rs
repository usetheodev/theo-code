//! T1.1 — Native screenshot tool.
//!
//! Captures the user's screen via the platform's native CLI:
//!
//!   macOS  → `screencapture -x /tmp/<rand>.png`
//!   Linux  → `gnome-screenshot -f /tmp/<rand>.png`
//!            falls back to `import -window root /tmp/<rand>.png`
//!            (ImageMagick) when gnome-screenshot is unavailable
//!   Other  → typed `Unsupported` error with platform name
//!
//! Returns the captured image as base64 in `metadata.data` so the
//! agent runtime's vision propagation lifts it into the next
//! assistant turn as a vision block. Same pattern as
//! `browser_screenshot` and `read_image`.
//!
//! No `xcap` dependency: shelling out is cross-platform without
//! Rust display bindings, and the agent can already see the
//! installed CLI tools via `env_info`. When no display is
//! available (headless CI, SSH without X-forward), the tool
//! surfaces an actionable error mentioning `webfetch` as a
//! fallback for accessing remote pages.

use std::path::PathBuf;
use std::process::Command;

use async_trait::async_trait;
use serde_json::{Value, json};

use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolSchema,
};

/// `screenshot` — capture the local screen.
pub struct ScreenshotTool;

impl Default for ScreenshotTool {
    fn default() -> Self {
        Self
    }
}

impl ScreenshotTool {
    pub fn new() -> Self {
        Self
    }
}

const PNG_MEDIA_TYPE: &str = "image/png";

#[async_trait]
impl Tool for ScreenshotTool {
    fn id(&self) -> &str {
        "screenshot"
    }

    fn description(&self) -> &str {
        "T1.1 — Capture the local screen as a PNG vision block. Uses the \
         platform's native CLI (macOS `screencapture`, Linux \
         `gnome-screenshot` or `import`); no Rust display dependency. \
         Returns the image base64-encoded in metadata for vision-block \
         propagation. Headless / SSH-without-X / CI sessions surface a \
         typed `no display available` error — fall back to `webfetch` \
         when a remote page is what you need to inspect. \
         Example: screenshot({})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![],
            input_examples: vec![json!({})],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    async fn execute(
        &self,
        _args: Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let target = pick_temp_path();
        let result = capture_to_path(&target);
        match result {
            Ok(()) => {
                let bytes = std::fs::read(&target).map_err(ToolError::Io)?;
                // Best-effort cleanup; failure here is fine.
                let _ = std::fs::remove_file(&target);
                let data = base64_encode(&bytes);
                let bytes_b64 = data.len();
                Ok(ToolOutput::new(
                    format!("screenshot: captured ({bytes_b64} base64 bytes)"),
                    "Screen captured. The image is attached as a vision block \
                     so the next assistant turn can read it visually."
                        .to_string(),
                )
                .with_metadata(json!({
                    "type": "screenshot",
                    "media_type": PNG_MEDIA_TYPE,
                    "base64_bytes": bytes_b64,
                    // `data` is what vision_propagation lifts into the
                    // assistant message as ContentBlock::ImageBase64.
                    "data": data,
                })))
            }
            Err(err) => Err(map_capture_error(err)),
        }
    }
}

/// Errors the platform-specific capture path can produce. Public so
/// the test module can match exhaustively.
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("no screenshot CLI available on this platform: tried {tried:?}")]
    NoCli { tried: Vec<&'static str> },
    #[error("screenshot CLI `{cli}` failed (exit {exit}): {stderr}")]
    CliFailed {
        cli: &'static str,
        exit: i32,
        stderr: String,
    },
    #[error("no display detected — screenshot requires an interactive desktop")]
    NoDisplay,
    #[error("unsupported platform `{0}`")]
    Unsupported(String),
    #[error("io error during screenshot: {0}")]
    Io(#[from] std::io::Error),
}

fn map_capture_error(err: CaptureError) -> ToolError {
    match err {
        CaptureError::NoCli { tried } => ToolError::Execution(format!(
            "no screenshot CLI installed (tried {tried:?}). Install one \
             (gnome-screenshot or ImageMagick `import` on Linux; \
             screencapture is built-in on macOS) or fall back to webfetch \
             for remote pages."
        )),
        CaptureError::NoDisplay => ToolError::Execution(
            "no display detected — screenshot requires an interactive \
             desktop session. In headless / SSH-without-X / container \
             contexts, fall back to webfetch for remote pages."
                .into(),
        ),
        CaptureError::CliFailed { cli, exit, stderr } => ToolError::Execution(format!(
            "screenshot CLI `{cli}` failed (exit {exit}): {stderr}"
        )),
        CaptureError::Unsupported(p) => ToolError::Execution(format!(
            "unsupported platform `{p}` — no native screenshot CLI wired"
        )),
        CaptureError::Io(e) => ToolError::Io(e),
    }
}

/// Pick a unique temp path — we use random hex over the raw nanos to
/// avoid races on shared `/tmp`.
fn pick_temp_path() -> PathBuf {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("theo_screenshot_{pid}_{nanos}.png"))
}

/// Capture to `path`. Returns `Ok(())` when the file was written;
/// errors otherwise. Public so tests can probe individual error
/// paths without invoking the full Tool::execute chain.
pub fn capture_to_path(path: &std::path::Path) -> Result<(), CaptureError> {
    #[cfg(target_os = "macos")]
    {
        return capture_macos(path);
    }
    #[cfg(target_os = "linux")]
    {
        return capture_linux(path);
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = path;
        Err(CaptureError::Unsupported(std::env::consts::OS.into()))
    }
}

#[cfg(target_os = "macos")]
fn capture_macos(path: &std::path::Path) -> Result<(), CaptureError> {
    let out = Command::new("screencapture")
        .arg("-x") // silent (no shutter sound)
        .arg(path)
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                CaptureError::NoCli {
                    tried: vec!["screencapture"],
                }
            } else {
                CaptureError::Io(e)
            }
        })?;
    if !out.status.success() {
        return Err(CaptureError::CliFailed {
            cli: "screencapture",
            exit: out.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn capture_linux(path: &std::path::Path) -> Result<(), CaptureError> {
    // Headless guard: a missing DISPLAY (and Wayland's
    // WAYLAND_DISPLAY) is the most common failure mode in
    // containers / SSH sessions. Catch it early with a typed
    // error instead of a confusing CLI exit code.
    if std::env::var_os("DISPLAY").is_none()
        && std::env::var_os("WAYLAND_DISPLAY").is_none()
    {
        return Err(CaptureError::NoDisplay);
    }

    // Try gnome-screenshot first (cleaner output, GNOME default).
    match try_gnome_screenshot(path) {
        Ok(()) => return Ok(()),
        Err(CaptureError::NoCli { .. }) => { /* fall through */ }
        Err(other) => return Err(other),
    }
    // Fall back to ImageMagick `import`.
    match try_imagemagick_import(path) {
        Ok(()) => Ok(()),
        Err(CaptureError::NoCli { .. }) => Err(CaptureError::NoCli {
            tried: vec!["gnome-screenshot", "import"],
        }),
        Err(other) => Err(other),
    }
}

#[cfg(target_os = "linux")]
fn try_gnome_screenshot(path: &std::path::Path) -> Result<(), CaptureError> {
    let out = Command::new("gnome-screenshot")
        .arg("-f")
        .arg(path)
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                CaptureError::NoCli {
                    tried: vec!["gnome-screenshot"],
                }
            } else {
                CaptureError::Io(e)
            }
        })?;
    if !out.status.success() {
        return Err(CaptureError::CliFailed {
            cli: "gnome-screenshot",
            exit: out.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn try_imagemagick_import(path: &std::path::Path) -> Result<(), CaptureError> {
    let out = Command::new("import")
        .arg("-window")
        .arg("root")
        .arg(path)
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                CaptureError::NoCli {
                    tried: vec!["import"],
                }
            } else {
                CaptureError::Io(e)
            }
        })?;
    if !out.status.success() {
        return Err(CaptureError::CliFailed {
            cli: "import",
            exit: out.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }
    Ok(())
}

/// Tiny base64 encoder. Avoids pulling the `base64` crate just for
/// one tool — same impl pattern as `webfetch::base64_encode`.
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(((bytes.len() + 2) / 3) * 4);
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let n = ((bytes[i] as u32) << 16)
            | ((bytes[i + 1] as u32) << 8)
            | (bytes[i + 2] as u32);
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        out.push(ALPHABET[(n & 0x3f) as usize] as char);
        i += 3;
    }
    let rem = bytes.len() - i;
    if rem == 1 {
        let n = (bytes[i] as u32) << 16;
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8);
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        out.push('=');
    }
    out
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
    fn t11_screenshot_id_and_category() {
        let t = ScreenshotTool::new();
        assert_eq!(t.id(), "screenshot");
        assert_eq!(t.category(), ToolCategory::Search);
    }

    #[test]
    fn t11_screenshot_schema_validates_with_no_args() {
        let t = ScreenshotTool::new();
        let schema = t.schema();
        schema.validate().unwrap();
        assert!(
            schema.params.is_empty(),
            "screenshot takes no arguments — schema must be empty"
        );
    }

    // ── pick_temp_path ────────────────────────────────────────────

    #[test]
    fn t11_pick_temp_path_lives_in_temp_dir_with_pid() {
        let p = pick_temp_path();
        let parent = p.parent().expect("temp_dir has a parent");
        assert_eq!(parent, std::env::temp_dir());
        let name = p.file_name().unwrap().to_string_lossy();
        // Name pattern: theo_screenshot_<pid>_<nanos>.png
        assert!(name.starts_with("theo_screenshot_"));
        assert!(name.ends_with(".png"));
        let pid_str = std::process::id().to_string();
        assert!(
            name.contains(&pid_str),
            "filename must include pid `{pid_str}` to avoid cross-process race"
        );
    }

    #[test]
    fn t11_pick_temp_path_two_calls_yield_different_paths() {
        // Same-process collision avoidance via the nanos suffix.
        // Sleep a nanosecond to be safe across very-fast clocks.
        let a = pick_temp_path();
        std::thread::sleep(std::time::Duration::from_nanos(50));
        let b = pick_temp_path();
        assert_ne!(a, b, "two consecutive temp paths must differ");
    }

    // ── base64 ────────────────────────────────────────────────────

    #[test]
    fn t11_base64_encode_known_vectors() {
        // RFC 4648 vectors.
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn t11_base64_encode_handles_full_byte_range() {
        let mut bytes = Vec::with_capacity(256);
        for i in 0..=255u8 {
            bytes.push(i);
        }
        let out = base64_encode(&bytes);
        // Length must be ((256 + 2) / 3) * 4 = 344.
        assert_eq!(out.len(), 344);
        // Output must round-trip through any standard decoder; we
        // check character set membership as a structural smoke test.
        for c in out.chars() {
            assert!(
                c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=',
                "unexpected char `{c}` in base64 output"
            );
        }
    }

    // ── error mapping ─────────────────────────────────────────────

    #[test]
    fn t11_map_capture_error_no_cli_mentions_install_options() {
        let err = map_capture_error(CaptureError::NoCli {
            tried: vec!["gnome-screenshot", "import"],
        });
        let msg = match err {
            ToolError::Execution(m) => m,
            other => panic!("expected Execution, got {other:?}"),
        };
        assert!(msg.contains("no screenshot CLI"));
        assert!(msg.contains("gnome-screenshot") || msg.contains("import"));
        // Mention webfetch fallback so the agent has somewhere to go.
        assert!(msg.contains("webfetch"));
    }

    #[test]
    fn t11_map_capture_error_no_display_mentions_headless_alternatives() {
        let err = map_capture_error(CaptureError::NoDisplay);
        let msg = match err {
            ToolError::Execution(m) => m,
            other => panic!("expected Execution, got {other:?}"),
        };
        assert!(msg.contains("no display"));
        assert!(msg.contains("headless") || msg.contains("SSH"));
        assert!(msg.contains("webfetch"));
    }

    #[test]
    fn t11_map_capture_error_cli_failed_includes_exit_and_stderr() {
        let err = map_capture_error(CaptureError::CliFailed {
            cli: "gnome-screenshot",
            exit: 1,
            stderr: "X11 not available".into(),
        });
        let msg = match err {
            ToolError::Execution(m) => m,
            other => panic!("expected Execution, got {other:?}"),
        };
        assert!(msg.contains("gnome-screenshot"));
        assert!(msg.contains("exit 1"));
        assert!(msg.contains("X11 not available"));
    }

    #[test]
    fn t11_map_capture_error_unsupported_platform_named() {
        let err = map_capture_error(CaptureError::Unsupported("haiku".into()));
        let msg = match err {
            ToolError::Execution(m) => m,
            other => panic!("expected Execution, got {other:?}"),
        };
        assert!(msg.contains("unsupported platform"));
        assert!(msg.contains("haiku"));
    }

    // ── headless integration ──────────────────────────────────────

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn t11_execute_in_headless_env_returns_no_display_error() {
        // In Linux CI / containers we expect DISPLAY to be unset.
        // Surface NoDisplay with the actionable webfetch hint.
        // We can't reliably set/unset env in this test (race with
        // sibling tests running tools concurrently), so we only
        // assert when both DISPLAY and WAYLAND_DISPLAY are unset
        // already — otherwise the test silently passes.
        if std::env::var_os("DISPLAY").is_some()
            || std::env::var_os("WAYLAND_DISPLAY").is_some()
        {
            eprintln!("skip: display detected — can't test NoDisplay path");
            return;
        }
        let t = ScreenshotTool::new();
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(json!({}), &make_ctx(), &mut perms)
            .await
            .unwrap_err();
        match err {
            ToolError::Execution(msg) => {
                assert!(
                    msg.contains("no display") || msg.contains("no screenshot CLI"),
                    "expected NoDisplay/NoCli, got `{msg}`"
                );
                assert!(msg.contains("webfetch"));
            }
            other => panic!("expected Execution error, got {other:?}"),
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    #[test]
    fn t11_capture_to_path_reports_unsupported_platform() {
        let p = pick_temp_path();
        let err = capture_to_path(&p).unwrap_err();
        match err {
            CaptureError::Unsupported(_) => {}
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }
}
