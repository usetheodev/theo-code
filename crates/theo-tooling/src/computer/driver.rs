//! T4.1 — Computer Use driver: subprocess executor.
//!
//! Maps a `ComputerAction` to an OS-specific CLI invocation:
//!
//!   Linux  → `xdotool`
//!   macOS  → `cliclick` (mouse) + `osascript` fallback for typing
//!   Win.   → `nircmd` (out of scope for first cut — surfaces as
//!            DriverMissing)
//!
//! All actions return either `ComputerResult::Empty` or
//! `ComputerResult::Screenshot { data, ... }`. The driver is pure
//! orchestration — the screenshot path delegates to the
//! `crate::screenshot` module so all image capture goes through
//! one well-tested implementation.
//!
//! Capability gate: callers MUST check `Capability::ComputerUse`
//! BEFORE invoking `execute`. The driver itself trusts its inputs
//! — gating happens at the tool boundary. Tests that exercise the
//! driver directly bypass the gate.
//!
//! Wayland / Windows are detected and surfaced as typed errors;
//! the agent sees the actionable hint instead of a confusing
//! subprocess failure.

use std::process::Command;

use crate::computer::protocol::{
    ComputerAction, ComputerError, ComputerResult, DriverFamily, MouseButton,
    ScrollDirection, detect_driver,
};

/// Execute one Computer Use action via the platform driver.
/// Tests can `force` a specific `DriverFamily` to exercise paths
/// without depending on the host's installed CLIs.
pub fn execute_action(
    action: &ComputerAction,
    force: Option<DriverFamily>,
) -> Result<ComputerResult, ComputerError> {
    let family = detect_driver(force);
    match family {
        DriverFamily::Xdotool => execute_xdotool(action),
        DriverFamily::Cliclick => execute_cliclick(action),
        DriverFamily::Nircmd => Err(ComputerError::DriverMissing(
            "nircmd".into(),
            "download from https://www.nirsoft.net/utils/nircmd.html".into(),
        )),
        DriverFamily::Unknown => Err(ComputerError::NoDisplay),
    }
}

// ---------------------------------------------------------------------------
// xdotool — Linux + X11
// ---------------------------------------------------------------------------

fn execute_xdotool(action: &ComputerAction) -> Result<ComputerResult, ComputerError> {
    match action {
        ComputerAction::Screenshot => screenshot_via_screen_module(),
        ComputerAction::MouseMove { x, y } => xdotool_run(&[
            "mousemove",
            "--sync",
            &x.to_string(),
            &y.to_string(),
        ])
        .map(|_| ComputerResult::Empty),
        ComputerAction::Click { x, y, button } => {
            xdotool_run(&[
                "mousemove",
                "--sync",
                &x.to_string(),
                &y.to_string(),
            ])?;
            xdotool_run(&["click", &xdotool_button(*button).to_string()])
                .map(|_| ComputerResult::Empty)
        }
        ComputerAction::DoubleClick { x, y } => {
            xdotool_run(&[
                "mousemove",
                "--sync",
                &x.to_string(),
                &y.to_string(),
            ])?;
            xdotool_run(&["click", "--repeat", "2", "1"])
                .map(|_| ComputerResult::Empty)
        }
        ComputerAction::MouseDown { x, y, button } => {
            xdotool_run(&[
                "mousemove",
                "--sync",
                &x.to_string(),
                &y.to_string(),
            ])?;
            xdotool_run(&["mousedown", &xdotool_button(*button).to_string()])
                .map(|_| ComputerResult::Empty)
        }
        ComputerAction::MouseUp { x, y, button } => {
            xdotool_run(&[
                "mousemove",
                "--sync",
                &x.to_string(),
                &y.to_string(),
            ])?;
            xdotool_run(&["mouseup", &xdotool_button(*button).to_string()])
                .map(|_| ComputerResult::Empty)
        }
        ComputerAction::Type { text } => {
            // Use --delay 0 for fast typing; xdotool default is 12ms.
            xdotool_run(&["type", "--delay", "0", "--", text])
                .map(|_| ComputerResult::Empty)
        }
        ComputerAction::Key { name } => {
            validate_xdotool_key(name)?;
            xdotool_run(&["key", "--", name]).map(|_| ComputerResult::Empty)
        }
        ComputerAction::Scroll {
            x,
            y,
            direction,
            clicks,
        } => {
            xdotool_run(&[
                "mousemove",
                "--sync",
                &x.to_string(),
                &y.to_string(),
            ])?;
            // Button 4 = scroll up; 5 = scroll down (X11 convention).
            // 6 / 7 = horizontal scroll.
            let button = match direction {
                ScrollDirection::Up => "4",
                ScrollDirection::Down => "5",
                ScrollDirection::Left => "6",
                ScrollDirection::Right => "7",
            };
            for _ in 0..*clicks {
                xdotool_run(&["click", button])?;
            }
            Ok(ComputerResult::Empty)
        }
        ComputerAction::Wait { ms } => {
            std::thread::sleep(std::time::Duration::from_millis(*ms));
            Ok(ComputerResult::Empty)
        }
    }
}

fn xdotool_run(args: &[&str]) -> Result<(), ComputerError> {
    let out = Command::new("xdotool").args(args).output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ComputerError::DriverMissing(
                "xdotool".into(),
                "sudo apt install xdotool  (or `brew install xdotool` for X-on-mac)"
                    .into(),
            )
        } else {
            ComputerError::Subprocess {
                exit_code: None,
                stderr: format!("io error: {e}"),
            }
        }
    })?;
    if !out.status.success() {
        return Err(ComputerError::Subprocess {
            exit_code: out.status.code(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }
    Ok(())
}

fn xdotool_button(b: MouseButton) -> u32 {
    match b {
        MouseButton::Left => 1,
        MouseButton::Middle => 2,
        MouseButton::Right => 3,
    }
}

/// Reject obviously malformed xdotool key strings before invoking
/// the subprocess. Catches the most-common typo (empty / space
/// inside, e.g. `"ctrl + a"` instead of `"ctrl+a"`).
pub fn validate_xdotool_key(name: &str) -> Result<(), ComputerError> {
    if name.is_empty() {
        return Err(ComputerError::InvalidKey("empty key name".into()));
    }
    if name.contains(' ') {
        return Err(ComputerError::InvalidKey(format!(
            "`{name}` contains a space — use `+` to combine modifiers \
             (e.g. `ctrl+a`, NOT `ctrl + a`)"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// cliclick — macOS
// ---------------------------------------------------------------------------

fn execute_cliclick(action: &ComputerAction) -> Result<ComputerResult, ComputerError> {
    match action {
        ComputerAction::Screenshot => screenshot_via_screen_module(),
        ComputerAction::MouseMove { x, y } => {
            cliclick_run(&[format!("m:{x},{y}")]).map(|_| ComputerResult::Empty)
        }
        ComputerAction::Click { x, y, button } => {
            let cmd = match button {
                MouseButton::Left => format!("c:{x},{y}"),
                MouseButton::Right => format!("rc:{x},{y}"),
                MouseButton::Middle => {
                    // cliclick doesn't expose middle-click directly;
                    // surface a typed error so the caller doesn't
                    // expect a silent fallback.
                    return Err(ComputerError::DriverMissing(
                        "cliclick middle-click".into(),
                        "macOS cliclick lacks middle-click support; \
                         use AppleScript or xdotool on Linux"
                            .into(),
                    ));
                }
            };
            cliclick_run(&[cmd]).map(|_| ComputerResult::Empty)
        }
        ComputerAction::DoubleClick { x, y } => {
            cliclick_run(&[format!("dc:{x},{y}")]).map(|_| ComputerResult::Empty)
        }
        ComputerAction::MouseDown { x, y, button } => {
            let cmd = match button {
                MouseButton::Left => format!("dd:{x},{y}"),
                _ => {
                    return Err(ComputerError::DriverMissing(
                        "cliclick non-left mouseDown".into(),
                        "cliclick `dd:` only supports left button; \
                         use AppleScript for right-down"
                            .into(),
                    ));
                }
            };
            cliclick_run(&[cmd]).map(|_| ComputerResult::Empty)
        }
        ComputerAction::MouseUp { x, y, button } => {
            let cmd = match button {
                MouseButton::Left => format!("du:{x},{y}"),
                _ => {
                    return Err(ComputerError::DriverMissing(
                        "cliclick non-left mouseUp".into(),
                        "cliclick `du:` only supports left button".into(),
                    ));
                }
            };
            cliclick_run(&[cmd]).map(|_| ComputerResult::Empty)
        }
        ComputerAction::Type { text } => {
            cliclick_run(&[format!("t:{text}")]).map(|_| ComputerResult::Empty)
        }
        ComputerAction::Key { name } => {
            // cliclick `kp:` accepts Apple key names. We pass through;
            // validation matches xdotool to keep the key catalogue
            // consistent across drivers.
            validate_xdotool_key(name)?;
            cliclick_run(&[format!("kp:{name}")]).map(|_| ComputerResult::Empty)
        }
        ComputerAction::Scroll { .. } => {
            // cliclick has no scroll command; surface a clear error
            // so the caller can fall back to AppleScript.
            Err(ComputerError::DriverMissing(
                "cliclick scroll".into(),
                "macOS cliclick lacks scroll; use AppleScript via osascript".into(),
            ))
        }
        ComputerAction::Wait { ms } => {
            std::thread::sleep(std::time::Duration::from_millis(*ms));
            Ok(ComputerResult::Empty)
        }
    }
}

fn cliclick_run(args: &[String]) -> Result<(), ComputerError> {
    let out = Command::new("cliclick").args(args).output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ComputerError::DriverMissing(
                "cliclick".into(),
                "brew install cliclick".into(),
            )
        } else {
            ComputerError::Subprocess {
                exit_code: None,
                stderr: format!("io error: {e}"),
            }
        }
    })?;
    if !out.status.success() {
        return Err(ComputerError::Subprocess {
            exit_code: out.status.code(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared: screenshot via the existing crate::screenshot module
// ---------------------------------------------------------------------------

fn screenshot_via_screen_module() -> Result<ComputerResult, ComputerError> {
    let path = std::env::temp_dir().join(format!(
        "theo_computer_{}.png",
        std::process::id()
    ));
    crate::screenshot::capture_to_path(&path).map_err(map_screenshot_error)?;
    let bytes = std::fs::read(&path).map_err(|e| ComputerError::Subprocess {
        exit_code: None,
        stderr: format!("could not read screenshot file: {e}"),
    })?;
    let _ = std::fs::remove_file(&path);
    let data = base64_encode(&bytes);
    // Width/height parsing from PNG IHDR chunk (bytes 16..24).
    // Falls back to (0, 0) when the buffer is too small or the
    // signature is wrong — the LLM still gets the image.
    let (width, height) = parse_png_dimensions(&bytes).unwrap_or((0, 0));
    Ok(ComputerResult::Screenshot {
        media_type: "image/png".into(),
        data,
        width,
        height,
    })
}

fn map_screenshot_error(e: crate::screenshot::CaptureError) -> ComputerError {
    use crate::screenshot::CaptureError;
    match e {
        CaptureError::NoCli { tried } => ComputerError::DriverMissing(
            format!("screenshot CLI ({tried:?})"),
            "install screencapture (macOS, built-in), gnome-screenshot, \
             or ImageMagick `import`"
                .into(),
        ),
        CaptureError::NoDisplay => ComputerError::NoDisplay,
        CaptureError::CliFailed { cli, exit, stderr } => ComputerError::Subprocess {
            exit_code: Some(exit),
            stderr: format!("{cli}: {stderr}"),
        },
        CaptureError::Unsupported(p) => ComputerError::DriverMissing(
            format!("platform `{p}`"),
            "no screenshot CLI wired for this OS".into(),
        ),
        CaptureError::Io(io) => ComputerError::Subprocess {
            exit_code: None,
            stderr: format!("io: {io}"),
        },
    }
}

/// Parse the width and height from a PNG header. PNG signature is
/// 8 bytes; IHDR length+type is 8 bytes; then the 4-byte width and
/// 4-byte height (big-endian) live at offsets 16..24.
fn parse_png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    const PNG_SIGNATURE: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.len() < 24 || !bytes.starts_with(PNG_SIGNATURE) {
        return None;
    }
    let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    Some((w, h))
}

/// Inline base64 encoder (same impl as crate::screenshot — keeping
/// it local avoids an import cycle and matches the rest of the
/// crate's "no extra deps for tiny helpers" policy).
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

    // ── Driver routing ────────────────────────────────────────────

    #[test]
    fn t41drv_unknown_driver_returns_no_display() {
        let result = execute_action(
            &ComputerAction::Screenshot,
            Some(DriverFamily::Unknown),
        );
        match result {
            Err(ComputerError::NoDisplay) => {}
            other => panic!("expected NoDisplay, got {other:?}"),
        }
    }

    #[test]
    fn t41drv_nircmd_driver_returns_driver_missing_with_install_hint() {
        // Windows is out-of-scope for the first cut; surface a clear
        // error mentioning where to get nircmd.
        let result = execute_action(
            &ComputerAction::Screenshot,
            Some(DriverFamily::Nircmd),
        );
        match result {
            Err(ComputerError::DriverMissing(name, hint)) => {
                assert_eq!(name, "nircmd");
                assert!(hint.contains("nirsoft"));
            }
            other => panic!("expected DriverMissing(nircmd, ..), got {other:?}"),
        }
    }

    // ── validate_xdotool_key ──────────────────────────────────────

    #[test]
    fn t41drv_validate_key_rejects_empty_string() {
        let err = validate_xdotool_key("").unwrap_err();
        match err {
            ComputerError::InvalidKey(msg) => assert!(msg.contains("empty")),
            other => panic!("expected InvalidKey, got {other:?}"),
        }
    }

    #[test]
    fn t41drv_validate_key_rejects_spaces_with_actionable_hint() {
        // Common typo: `"ctrl + a"` — should explain the canonical
        // form so the LLM doesn't repeat the mistake.
        let err = validate_xdotool_key("ctrl + a").unwrap_err();
        match err {
            ComputerError::InvalidKey(msg) => {
                assert!(msg.contains("space"));
                assert!(msg.contains("ctrl+a"));
            }
            other => panic!("expected InvalidKey, got {other:?}"),
        }
    }

    #[test]
    fn t41drv_validate_key_accepts_canonical_combos() {
        for k in ["Return", "ctrl+a", "ctrl+shift+t", "cmd+space", "F1"] {
            assert!(
                validate_xdotool_key(k).is_ok(),
                "`{k}` should be a valid key name"
            );
        }
    }

    // ── xdotool_button mapping ────────────────────────────────────

    #[test]
    fn t41drv_xdotool_button_mapping() {
        assert_eq!(xdotool_button(MouseButton::Left), 1);
        assert_eq!(xdotool_button(MouseButton::Middle), 2);
        assert_eq!(xdotool_button(MouseButton::Right), 3);
    }

    // ── PNG dimension parser ──────────────────────────────────────

    #[test]
    fn t41drv_parse_png_dimensions_known_signature() {
        // Synthetic PNG header: signature + IHDR with width=1920, height=1080.
        let mut bytes: Vec<u8> = vec![
            0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, // signature
            0x00, 0x00, 0x00, 0x0D, // IHDR length
            b'I', b'H', b'D', b'R', // chunk type
        ];
        bytes.extend_from_slice(&1920u32.to_be_bytes()); // width
        bytes.extend_from_slice(&1080u32.to_be_bytes()); // height
        let (w, h) = parse_png_dimensions(&bytes).unwrap();
        assert_eq!(w, 1920);
        assert_eq!(h, 1080);
    }

    #[test]
    fn t41drv_parse_png_dimensions_rejects_non_png() {
        let bytes = b"GIF89a not a PNG";
        assert!(parse_png_dimensions(bytes).is_none());
    }

    #[test]
    fn t41drv_parse_png_dimensions_too_short_returns_none() {
        let bytes = b"\x89PNG"; // only 4 bytes
        assert!(parse_png_dimensions(bytes).is_none());
    }

    // ── base64 (RFC 4648 vectors) ─────────────────────────────────

    #[test]
    fn t41drv_base64_encode_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    // ── map_screenshot_error → ComputerError ──────────────────────

    #[test]
    fn t41drv_map_screenshot_no_display_passes_through() {
        let err = map_screenshot_error(crate::screenshot::CaptureError::NoDisplay);
        assert!(matches!(err, ComputerError::NoDisplay));
    }

    #[test]
    fn t41drv_map_screenshot_no_cli_becomes_driver_missing() {
        let err = map_screenshot_error(crate::screenshot::CaptureError::NoCli {
            tried: vec!["gnome-screenshot"],
        });
        match err {
            ComputerError::DriverMissing(name, hint) => {
                assert!(name.contains("screenshot"));
                assert!(hint.contains("install"));
            }
            other => panic!("expected DriverMissing, got {other:?}"),
        }
    }

    #[test]
    fn t41drv_map_screenshot_unsupported_becomes_driver_missing_with_platform() {
        let err = map_screenshot_error(crate::screenshot::CaptureError::Unsupported(
            "haiku".into(),
        ));
        match err {
            ComputerError::DriverMissing(name, _) => assert!(name.contains("haiku")),
            other => panic!("expected DriverMissing, got {other:?}"),
        }
    }
}
