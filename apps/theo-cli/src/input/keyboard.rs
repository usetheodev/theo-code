//! Keyboard protocol detection and parsing.
//!
//! Supports Kitty keyboard protocol (CSI u encoding) with xterm fallback.
//! Pi-mono ref: `packages/tui/src/keys.ts`

#![allow(dead_code)] // Scaffolded helpers — kept for upcoming TUI features.
/// Detected keyboard protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardProtocol {
    /// Kitty keyboard protocol (CSI u encoding).
    Kitty,
    /// Standard xterm encoding (legacy).
    Xterm,
}

/// A parsed key event with modifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    pub modifiers: Modifiers,
}

/// Key identifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Tab,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    F(u8),
}

/// Modifier keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Modifiers {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn ctrl() -> Self {
        Self {
            ctrl: true,
            ..Self::default()
        }
    }

    pub fn alt() -> Self {
        Self {
            alt: true,
            ..Self::default()
        }
    }

    pub fn shift() -> Self {
        Self {
            shift: true,
            ..Self::default()
        }
    }
}

/// Parse a raw byte sequence into a KeyEvent.
/// Tries Kitty CSI-u format first, falls back to xterm.
pub fn parse_key(data: &[u8]) -> Option<KeyEvent> {
    if data.is_empty() {
        return None;
    }

    // Kitty CSI-u: ESC [ <codepoint> ; <modifiers> u
    if data.len() >= 4 && data[0] == 0x1b && data[1] == b'[' && data.last() == Some(&b'u') {
        return parse_kitty_csi_u(data);
    }

    // Standard xterm sequences
    parse_xterm(data)
}

fn parse_kitty_csi_u(data: &[u8]) -> Option<KeyEvent> {
    // ESC [ <number> ; <modifiers> u
    let inner = std::str::from_utf8(&data[2..data.len() - 1]).ok()?;
    let parts: Vec<&str> = inner.split(';').collect();

    let codepoint: u32 = parts.first()?.parse().ok()?;
    let mods_raw: u32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);

    let modifiers = decode_modifiers(mods_raw);
    let key = char::from_u32(codepoint).map(Key::Char)?;

    Some(KeyEvent { key, modifiers })
}

fn parse_xterm(data: &[u8]) -> Option<KeyEvent> {
    match data {
        [0x1b, b'[', b'A'] => Some(KeyEvent {
            key: Key::Up,
            modifiers: Modifiers::none(),
        }),
        [0x1b, b'[', b'B'] => Some(KeyEvent {
            key: Key::Down,
            modifiers: Modifiers::none(),
        }),
        [0x1b, b'[', b'C'] => Some(KeyEvent {
            key: Key::Right,
            modifiers: Modifiers::none(),
        }),
        [0x1b, b'[', b'D'] => Some(KeyEvent {
            key: Key::Left,
            modifiers: Modifiers::none(),
        }),
        [0x1b, b'[', b'H'] => Some(KeyEvent {
            key: Key::Home,
            modifiers: Modifiers::none(),
        }),
        [0x1b, b'[', b'F'] => Some(KeyEvent {
            key: Key::End,
            modifiers: Modifiers::none(),
        }),
        [0x1b, b'O', b'P'] => Some(KeyEvent {
            key: Key::F(1),
            modifiers: Modifiers::none(),
        }),
        [0x1b, b'O', b'Q'] => Some(KeyEvent {
            key: Key::F(2),
            modifiers: Modifiers::none(),
        }),
        [0x1b, b'O', b'R'] => Some(KeyEvent {
            key: Key::F(3),
            modifiers: Modifiers::none(),
        }),
        [0x1b, b'O', b'S'] => Some(KeyEvent {
            key: Key::F(4),
            modifiers: Modifiers::none(),
        }),
        [0x1b] => Some(KeyEvent {
            key: Key::Escape,
            modifiers: Modifiers::none(),
        }),
        [0x0d] => Some(KeyEvent {
            key: Key::Enter,
            modifiers: Modifiers::none(),
        }),
        [0x09] => Some(KeyEvent {
            key: Key::Tab,
            modifiers: Modifiers::none(),
        }),
        [0x7f] => Some(KeyEvent {
            key: Key::Backspace,
            modifiers: Modifiers::none(),
        }),
        [b] if *b < 0x20 => {
            // Ctrl+letter (0x01=A, 0x02=B, ...)
            let ch = (b + 0x60) as char;
            Some(KeyEvent {
                key: Key::Char(ch),
                modifiers: Modifiers::ctrl(),
            })
        }
        [b] if *b >= 0x20 && *b < 0x7f => Some(KeyEvent {
            key: Key::Char(*b as char),
            modifiers: Modifiers::none(),
        }),
        _ => None,
    }
}

/// Decode modifier bitmask (Kitty/xterm: 1=none, 2=shift, 3=alt, etc.)
fn decode_modifiers(raw: u32) -> Modifiers {
    let m = raw.saturating_sub(1); // Kitty convention: mods = raw - 1
    Modifiers {
        shift: m & 1 != 0,
        alt: m & 2 != 0,
        ctrl: m & 4 != 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_input_returns_none() {
        assert!(parse_key(&[]).is_none());
    }

    #[test]
    fn test_parse_regular_char() {
        let event = parse_key(b"a").expect("should parse 'a'");
        assert_eq!(event.key, Key::Char('a'));
        assert_eq!(event.modifiers, Modifiers::none());
    }

    #[test]
    fn test_parse_space() {
        let event = parse_key(b" ").expect("should parse space");
        assert_eq!(event.key, Key::Char(' '));
        assert_eq!(event.modifiers, Modifiers::none());
    }

    #[test]
    fn test_parse_arrow_up() {
        let event = parse_key(&[0x1b, b'[', b'A']).expect("should parse arrow up");
        assert_eq!(event.key, Key::Up);
        assert_eq!(event.modifiers, Modifiers::none());
    }

    #[test]
    fn test_parse_arrow_down() {
        let event = parse_key(&[0x1b, b'[', b'B']).expect("should parse arrow down");
        assert_eq!(event.key, Key::Down);
    }

    #[test]
    fn test_parse_arrow_right() {
        let event = parse_key(&[0x1b, b'[', b'C']).expect("should parse arrow right");
        assert_eq!(event.key, Key::Right);
    }

    #[test]
    fn test_parse_arrow_left() {
        let event = parse_key(&[0x1b, b'[', b'D']).expect("should parse arrow left");
        assert_eq!(event.key, Key::Left);
    }

    #[test]
    fn test_parse_home_end() {
        let home = parse_key(&[0x1b, b'[', b'H']).expect("home");
        assert_eq!(home.key, Key::Home);

        let end = parse_key(&[0x1b, b'[', b'F']).expect("end");
        assert_eq!(end.key, Key::End);
    }

    #[test]
    fn test_parse_function_keys() {
        let f1 = parse_key(&[0x1b, b'O', b'P']).expect("F1");
        assert_eq!(f1.key, Key::F(1));

        let f4 = parse_key(&[0x1b, b'O', b'S']).expect("F4");
        assert_eq!(f4.key, Key::F(4));
    }

    #[test]
    fn test_parse_enter_tab_backspace_escape() {
        assert_eq!(parse_key(&[0x0d]).map(|e| e.key), Some(Key::Enter));
        assert_eq!(parse_key(&[0x09]).map(|e| e.key), Some(Key::Tab));
        assert_eq!(parse_key(&[0x7f]).map(|e| e.key), Some(Key::Backspace));
        assert_eq!(parse_key(&[0x1b]).map(|e| e.key), Some(Key::Escape));
    }

    #[test]
    fn test_parse_ctrl_c() {
        // Ctrl+C = 0x03
        let event = parse_key(&[0x03]).expect("Ctrl+C");
        assert_eq!(event.key, Key::Char('c'));
        assert_eq!(event.modifiers, Modifiers::ctrl());
    }

    #[test]
    fn test_parse_ctrl_a() {
        // Ctrl+A = 0x01
        let event = parse_key(&[0x01]).expect("Ctrl+A");
        assert_eq!(event.key, Key::Char('a'));
        assert!(event.modifiers.ctrl);
        assert!(!event.modifiers.alt);
        assert!(!event.modifiers.shift);
    }

    #[test]
    fn test_parse_kitty_csi_u_plain_char() {
        // ESC [ 97 u  (97 = 'a', no modifiers)
        let data = b"\x1b[97u";
        let event = parse_key(data).expect("Kitty 'a'");
        assert_eq!(event.key, Key::Char('a'));
        assert_eq!(event.modifiers, Modifiers::none());
    }

    #[test]
    fn test_parse_kitty_csi_u_with_shift() {
        // ESC [ 97 ; 2 u  (shift modifier = 2)
        let data = b"\x1b[97;2u";
        let event = parse_key(data).expect("Kitty Shift+a");
        assert_eq!(event.key, Key::Char('a'));
        assert!(event.modifiers.shift);
        assert!(!event.modifiers.ctrl);
        assert!(!event.modifiers.alt);
    }

    #[test]
    fn test_parse_kitty_csi_u_with_ctrl() {
        // ESC [ 97 ; 5 u  (ctrl modifier = 5)
        let data = b"\x1b[97;5u";
        let event = parse_key(data).expect("Kitty Ctrl+a");
        assert_eq!(event.key, Key::Char('a'));
        assert!(event.modifiers.ctrl);
        assert!(!event.modifiers.shift);
        assert!(!event.modifiers.alt);
    }

    #[test]
    fn test_parse_kitty_csi_u_with_alt() {
        // ESC [ 97 ; 3 u  (alt modifier = 3)
        let data = b"\x1b[97;3u";
        let event = parse_key(data).expect("Kitty Alt+a");
        assert_eq!(event.key, Key::Char('a'));
        assert!(event.modifiers.alt);
        assert!(!event.modifiers.ctrl);
        assert!(!event.modifiers.shift);
    }

    #[test]
    fn test_decode_modifiers_none() {
        let m = decode_modifiers(1);
        assert_eq!(m, Modifiers::none());
    }

    #[test]
    fn test_decode_modifiers_shift() {
        let m = decode_modifiers(2);
        assert!(m.shift);
        assert!(!m.alt);
        assert!(!m.ctrl);
    }

    #[test]
    fn test_decode_modifiers_alt() {
        let m = decode_modifiers(3); // 3-1=2, bit 1 = alt
        assert!(m.alt);
        assert!(!m.shift);
        assert!(!m.ctrl);
    }

    #[test]
    fn test_decode_modifiers_ctrl() {
        let m = decode_modifiers(5);
        assert!(m.ctrl);
        assert!(!m.shift);
        assert!(!m.alt);
    }

    #[test]
    fn test_decode_modifiers_ctrl_shift() {
        let m = decode_modifiers(6);
        // 6-1=5: bit 0 (shift) + bit 2 (ctrl)
        assert!(m.shift);
        assert!(m.ctrl);
        assert!(!m.alt);
    }

    #[test]
    fn test_unknown_sequence_returns_none() {
        // Multi-byte sequence that doesn't match any pattern
        assert!(parse_key(&[0x1b, b'[', b'9', b'9']).is_none());
    }

    #[test]
    fn test_keyboard_protocol_variants() {
        // Just verify the enum variants exist and are distinct.
        assert_ne!(KeyboardProtocol::Kitty, KeyboardProtocol::Xterm);
    }
}
