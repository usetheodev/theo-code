//! T4.1 — Computer Use wire format.
//!
//! Mirrors Anthropic's `computer_20250124` action set so the same
//! types serialize for both:
//! - Anthropic's API (when delegating decision-making to the model
//!   via the `computer_20250124` tool definition).
//! - Theo's local OS-driver subprocess (`xdotool`, `cliclick`,
//!   `nircmd`).
//!
//! Pure code: no IO, no subprocess. Tests prove the wire shape end-
//! to-end without any OS tool installed.
//!
//! Coordinate system: Anthropic uses (x, y) in pixels with origin at
//! TOP-LEFT of the captured display. Theo translates to whatever the
//! local driver expects (xdotool uses the same convention; cliclick
//! and nircmd also TOP-LEFT).

use serde::{Deserialize, Serialize};

/// Mouse buttons supported by `Click` / `MouseDown` / `MouseUp`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Scroll direction for `Scroll` action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

/// One Computer Use action. Variants map to Anthropic's
/// `computer_20250124` enum.
///
/// Conventions:
/// - Coordinates are pixels, origin top-left of the captured display.
/// - `Key`'s `name` follows Anthropic's xdotool-compatible naming:
///   `Return`, `Tab`, `cmd+tab`, `ctrl+shift+t`, etc.
/// - `Wait` is in milliseconds — useful between actions to let the
///   UI settle before the next observation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ComputerAction {
    /// Capture the entire active display. Returns base64 image.
    Screenshot,

    /// Move the cursor to the absolute (x, y) without clicking.
    MouseMove { x: u32, y: u32 },

    /// Click at (x, y) with the given button. Default Left.
    Click {
        x: u32,
        y: u32,
        #[serde(default = "default_button")]
        button: MouseButton,
    },

    /// Double-click at (x, y) with Left button (the canonical use).
    DoubleClick { x: u32, y: u32 },

    /// Press button down (without releasing) for drag operations.
    MouseDown {
        x: u32,
        y: u32,
        #[serde(default = "default_button")]
        button: MouseButton,
    },

    /// Release a previously pressed button.
    MouseUp {
        x: u32,
        y: u32,
        #[serde(default = "default_button")]
        button: MouseButton,
    },

    /// Type literal text. Caller is responsible for keyboard layout
    /// (e.g. `'Hello'` types lowercase letters; for `'HELLO'` use
    /// shift via `Key`).
    Type { text: String },

    /// Press a key combination (xdotool naming, e.g. `"Return"`,
    /// `"ctrl+a"`, `"cmd+shift+t"`).
    Key { name: String },

    /// Scroll wheel motion at (x, y). `clicks` is the number of
    /// scroll wheel ticks.
    Scroll {
        x: u32,
        y: u32,
        direction: ScrollDirection,
        #[serde(default = "default_scroll_clicks")]
        clicks: u32,
    },

    /// Sleep for `ms` milliseconds. Useful to let the UI settle
    /// after a click before screenshotting.
    Wait { ms: u64 },
}

fn default_button() -> MouseButton {
    MouseButton::Left
}

fn default_scroll_clicks() -> u32 {
    3
}

/// Result variants. Matches the shape Anthropic's tool returns to
/// the LLM.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ComputerResult {
    /// Action succeeded with no content (move, click, type, etc.).
    Empty,
    /// Screenshot result — base64 PNG of the active display.
    Screenshot {
        media_type: String,
        data: String,
        /// Display dimensions in pixels — useful for the LLM's
        /// next coordinate calculation.
        width: u32,
        height: u32,
    },
}

/// Errors specific to Computer Use.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, thiserror::Error)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ComputerError {
    #[error("no display server detected (X11/Wayland on Linux, GUI session on macOS/Windows)")]
    NoDisplay,
    #[error("OS driver not installed: {0} — install via: {hint}", hint = .1)]
    DriverMissing(String, String),
    #[error("coordinate out of display bounds: ({x},{y}) exceeds {width}x{height}")]
    CoordinateOutOfBounds {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    #[error("invalid key name: {0}")]
    InvalidKey(String),
    #[error("driver subprocess failed (exit {exit_code:?}): {stderr}")]
    Subprocess {
        exit_code: Option<i32>,
        stderr: String,
    },
    #[error("operation forbidden by capability gate (Capability::ComputerUse off)")]
    CapabilityDenied,
}

/// Best-effort OS detection for picking the right driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverFamily {
    /// Linux + X11 → `xdotool`
    Xdotool,
    /// macOS → `cliclick`
    Cliclick,
    /// Windows → `nircmd`
    Nircmd,
    /// Unknown / unsupported.
    Unknown,
}

/// Pick a driver based on the current OS. Hands off to runtime
/// detection (env vars `WAYLAND_DISPLAY`/`DISPLAY`) when available;
/// caller passes `force` to bypass detection in tests.
pub fn detect_driver(force: Option<DriverFamily>) -> DriverFamily {
    if let Some(f) = force {
        return f;
    }
    #[cfg(target_os = "linux")]
    {
        // Prefer xdotool; Wayland support varies. The caller can
        // force a specific driver via `force` when running under
        // wlroots.
        if std::env::var_os("DISPLAY").is_some() {
            return DriverFamily::Xdotool;
        }
        DriverFamily::Unknown
    }
    #[cfg(target_os = "macos")]
    {
        return DriverFamily::Cliclick;
    }
    #[cfg(target_os = "windows")]
    {
        return DriverFamily::Nircmd;
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        DriverFamily::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ---- Action ----

    #[test]
    fn t41proto_screenshot_action_serializes_with_kind_only() {
        let a = ComputerAction::Screenshot;
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["action"], "screenshot");
        // Only the discriminator — no payload fields.
        assert_eq!(json.as_object().unwrap().len(), 1);
    }

    #[test]
    fn t41proto_mouse_move_carries_xy() {
        let a = ComputerAction::MouseMove { x: 100, y: 200 };
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["action"], "mouse_move");
        assert_eq!(json["x"], 100);
        assert_eq!(json["y"], 200);
    }

    #[test]
    fn t41proto_click_default_button_is_left() {
        let a = ComputerAction::Click {
            x: 10,
            y: 10,
            button: default_button(),
        };
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["button"], "left");
    }

    #[test]
    fn t41proto_click_right_button_serialized() {
        let a = ComputerAction::Click {
            x: 0,
            y: 0,
            button: MouseButton::Right,
        };
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["button"], "right");
    }

    #[test]
    fn t41proto_type_action_carries_text() {
        let a = ComputerAction::Type {
            text: "Hello".into(),
        };
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["action"], "type");
        assert_eq!(json["text"], "Hello");
    }

    #[test]
    fn t41proto_key_action_carries_name() {
        let a = ComputerAction::Key {
            name: "ctrl+a".into(),
        };
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["action"], "key");
        assert_eq!(json["name"], "ctrl+a");
    }

    #[test]
    fn t41proto_scroll_default_clicks_is_3() {
        let a = ComputerAction::Scroll {
            x: 100,
            y: 100,
            direction: ScrollDirection::Down,
            clicks: default_scroll_clicks(),
        };
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["clicks"], 3);
        assert_eq!(json["direction"], "down");
    }

    #[test]
    fn t41proto_scroll_direction_all_variants_lowercase() {
        for (dir, expected) in [
            (ScrollDirection::Up, "up"),
            (ScrollDirection::Down, "down"),
            (ScrollDirection::Left, "left"),
            (ScrollDirection::Right, "right"),
        ] {
            assert_eq!(
                serde_json::to_value(dir).unwrap(),
                json!(expected),
                "wrong serialization for {dir:?}"
            );
        }
    }

    #[test]
    fn t41proto_wait_action_carries_ms() {
        let a = ComputerAction::Wait { ms: 250 };
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["action"], "wait");
        assert_eq!(json["ms"], 250);
    }

    #[test]
    fn t41proto_action_serde_roundtrip_all_variants() {
        for a in [
            ComputerAction::Screenshot,
            ComputerAction::MouseMove { x: 1, y: 2 },
            ComputerAction::Click {
                x: 1,
                y: 2,
                button: MouseButton::Middle,
            },
            ComputerAction::DoubleClick { x: 5, y: 6 },
            ComputerAction::MouseDown {
                x: 0,
                y: 0,
                button: MouseButton::Left,
            },
            ComputerAction::MouseUp {
                x: 0,
                y: 0,
                button: MouseButton::Left,
            },
            ComputerAction::Type { text: "x".into() },
            ComputerAction::Key { name: "Tab".into() },
            ComputerAction::Scroll {
                x: 1,
                y: 1,
                direction: ScrollDirection::Up,
                clicks: 5,
            },
            ComputerAction::Wait { ms: 100 },
        ] {
            let json = serde_json::to_string(&a).unwrap();
            let back: ComputerAction = serde_json::from_str(&json).unwrap();
            assert_eq!(a, back, "roundtrip failed for {a:?}");
        }
    }

    // ---- Result ----

    #[test]
    fn t41proto_empty_result_serializes_as_kind_only() {
        let r = ComputerResult::Empty;
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["kind"], "empty");
    }

    #[test]
    fn t41proto_screenshot_result_carries_dimensions() {
        let r = ComputerResult::Screenshot {
            media_type: "image/png".into(),
            data: "AAAA".into(),
            width: 1920,
            height: 1080,
        };
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["kind"], "screenshot");
        assert_eq!(json["media_type"], "image/png");
        assert_eq!(json["width"], 1920);
        assert_eq!(json["height"], 1080);
    }

    // ---- Error ----

    #[test]
    fn t41proto_error_no_display_displays_actionable_message() {
        let e = ComputerError::NoDisplay;
        let s = e.to_string();
        assert!(s.contains("display server"));
    }

    #[test]
    fn t41proto_error_coordinate_out_of_bounds_includes_coords_and_dimensions() {
        let e = ComputerError::CoordinateOutOfBounds {
            x: 2000,
            y: 100,
            width: 1920,
            height: 1080,
        };
        let s = e.to_string();
        assert!(s.contains("(2000,100)"));
        assert!(s.contains("1920x1080"));
    }

    #[test]
    fn t41proto_error_subprocess_carries_exit_code_and_stderr() {
        let e = ComputerError::Subprocess {
            exit_code: Some(127),
            stderr: "command not found".into(),
        };
        let s = e.to_string();
        assert!(s.contains("127"));
        assert!(s.contains("command not found"));
    }

    #[test]
    fn t41proto_error_capability_denied_explains_gate() {
        let e = ComputerError::CapabilityDenied;
        let s = e.to_string();
        assert!(s.contains("Capability::ComputerUse"));
    }

    #[test]
    fn t41proto_error_serde_roundtrip_all_variants() {
        for e in [
            ComputerError::NoDisplay,
            ComputerError::DriverMissing("xdotool".into(), "apt install xdotool".into()),
            ComputerError::CoordinateOutOfBounds {
                x: 0,
                y: 0,
                width: 100,
                height: 100,
            },
            ComputerError::InvalidKey("bogus_key".into()),
            ComputerError::Subprocess {
                exit_code: Some(1),
                stderr: "x".into(),
            },
            ComputerError::CapabilityDenied,
        ] {
            let json = serde_json::to_string(&e).unwrap();
            let back: ComputerError = serde_json::from_str(&json).unwrap();
            assert_eq!(e, back);
        }
    }

    // ---- Driver detection ----

    #[test]
    fn t41proto_detect_driver_force_overrides_runtime() {
        // The most important property: tests can force a specific
        // driver to exercise per-platform code paths.
        assert_eq!(
            detect_driver(Some(DriverFamily::Xdotool)),
            DriverFamily::Xdotool
        );
        assert_eq!(
            detect_driver(Some(DriverFamily::Cliclick)),
            DriverFamily::Cliclick
        );
        assert_eq!(
            detect_driver(Some(DriverFamily::Nircmd)),
            DriverFamily::Nircmd
        );
    }
}
