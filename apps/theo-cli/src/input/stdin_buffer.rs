//! Input batching — assembles fragmented terminal input into complete sequences.
//!
//! Terminal emulators may split escape sequences across multiple reads.
//! StdinBuffer accumulates bytes and emits complete sequences.
//!
//! Pi-mono ref: `packages/tui/src/stdin-buffer.ts`

#![allow(dead_code)] // Scaffolded helpers — kept for upcoming TUI features.
/// A complete input event extracted from the buffer.
#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    /// A complete key sequence (may be a single char or an escape sequence).
    Key(Vec<u8>),
    /// A bracketed paste (content between ESC[200~ and ESC[201~).
    Paste(String),
}

/// Buffers fragmented stdin input and emits complete sequences.
pub struct StdinBuffer {
    buffer: Vec<u8>,
}

impl StdinBuffer {
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(256),
        }
    }

    /// Feed raw bytes from stdin and extract complete events.
    pub fn feed(&mut self, data: &[u8]) -> Vec<InputEvent> {
        self.buffer.extend_from_slice(data);
        self.extract_events()
    }

    /// Flush any remaining bytes as a Key event (for timeout).
    pub fn flush(&mut self) -> Option<InputEvent> {
        if self.buffer.is_empty() {
            return None;
        }
        let data = std::mem::take(&mut self.buffer);
        Some(InputEvent::Key(data))
    }

    fn extract_events(&mut self) -> Vec<InputEvent> {
        let mut events = Vec::new();

        while !self.buffer.is_empty() {
            // Check for bracketed paste start: ESC[200~
            if self.buffer.starts_with(b"\x1b[200~") {
                if let Some(end) = find_subsequence(&self.buffer, b"\x1b[201~") {
                    let paste_start = 6; // len of ESC[200~
                    let paste_content =
                        String::from_utf8_lossy(&self.buffer[paste_start..end]).to_string();
                    let total_len = end + 6; // include ESC[201~
                    self.buffer.drain(..total_len);

                    // Large paste detection (T22)
                    if paste_content.len() > 5000 {
                        let truncated = paste_content[..5000].to_string();
                        events.push(InputEvent::Paste(format!(
                            "{}... [truncated, {} chars total]",
                            truncated,
                            paste_content.len()
                        )));
                    } else {
                        events.push(InputEvent::Paste(paste_content));
                    }
                    continue;
                } else {
                    // Paste not complete yet — wait for more data
                    break;
                }
            }

            // Check for CSI sequence: ESC[...final_byte
            if self.buffer.starts_with(b"\x1b[") {
                if let Some(len) = csi_sequence_len(&self.buffer) {
                    let seq: Vec<u8> = self.buffer.drain(..len).collect();
                    events.push(InputEvent::Key(seq));
                    continue;
                } else {
                    // Incomplete CSI — wait for more data
                    break;
                }
            }

            // Check for SS3 sequence: ESC O + one byte
            if self.buffer.starts_with(b"\x1bO") {
                if self.buffer.len() >= 3 {
                    let seq: Vec<u8> = self.buffer.drain(..3).collect();
                    events.push(InputEvent::Key(seq));
                    continue;
                } else {
                    break;
                }
            }

            // Check for meta/alt key: ESC + single char
            if self.buffer[0] == 0x1b
                && self.buffer.len() >= 2
                && self.buffer[1] != b'['
                && self.buffer[1] != b'O'
            {
                let seq: Vec<u8> = self.buffer.drain(..2).collect();
                events.push(InputEvent::Key(seq));
                continue;
            }

            // Lone ESC — might be start of sequence, wait for more
            if self.buffer[0] == 0x1b && self.buffer.len() == 1 {
                break;
            }

            // Regular byte (or UTF-8 sequence)
            let char_len = utf8_char_len(self.buffer[0]);
            if self.buffer.len() >= char_len {
                let seq: Vec<u8> = self.buffer.drain(..char_len).collect();
                events.push(InputEvent::Key(seq));
            } else {
                break; // Incomplete UTF-8
            }
        }

        events
    }
}

/// Find a subsequence in a byte slice.
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Get the length of a complete CSI sequence, or None if incomplete.
/// CSI = ESC \[ (parameter bytes 0x30-0x3F)* (intermediate bytes 0x20-0x2F)* (final byte 0x40-0x7E)
fn csi_sequence_len(data: &[u8]) -> Option<usize> {
    if data.len() < 3 {
        return None;
    }
    for (i, &b) in data.iter().enumerate().skip(2) {
        if (0x40..=0x7E).contains(&b) {
            return Some(i + 1);
        }
        // Still in parameter/intermediate bytes range
        if !(0x20..=0x3F).contains(&b) {
            // Invalid CSI sequence — treat as complete at this point
            return Some(i);
        }
    }
    None // Incomplete
}

/// Get the expected byte length of a UTF-8 character from its first byte.
fn utf8_char_len(first_byte: u8) -> usize {
    match first_byte {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1, // Invalid — consume as single byte
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_regular_ascii_chars_extracted_individually() {
        // Arrange
        let mut buf = StdinBuffer::new();

        // Act
        let events = buf.feed(b"abc");

        // Assert
        assert_eq!(
            events,
            vec![
                InputEvent::Key(vec![b'a']),
                InputEvent::Key(vec![b'b']),
                InputEvent::Key(vec![b'c']),
            ]
        );
    }

    #[test]
    fn test_csi_sequence_up_arrow_extracted_as_single_event() {
        // Arrange
        let mut buf = StdinBuffer::new();

        // Act — ESC[A = up arrow
        let events = buf.feed(b"\x1b[A");

        // Assert
        assert_eq!(events, vec![InputEvent::Key(b"\x1b[A".to_vec())]);
    }

    #[test]
    fn test_ss3_sequence_f1_extracted_as_single_event() {
        // Arrange
        let mut buf = StdinBuffer::new();

        // Act — ESC O P = F1
        let events = buf.feed(b"\x1bOP");

        // Assert
        assert_eq!(events, vec![InputEvent::Key(b"\x1bOP".to_vec())]);
    }

    #[test]
    fn test_meta_key_extracted_as_single_event() {
        // Arrange
        let mut buf = StdinBuffer::new();

        // Act — ESC a = Alt+a
        let events = buf.feed(b"\x1ba");

        // Assert
        assert_eq!(events, vec![InputEvent::Key(b"\x1ba".to_vec())]);
    }

    #[test]
    fn test_incomplete_csi_waits_for_more_data() {
        // Arrange
        let mut buf = StdinBuffer::new();

        // Act — incomplete CSI: ESC[ with no final byte
        let events1 = buf.feed(b"\x1b[");
        // Feed the final byte
        let events2 = buf.feed(b"A");

        // Assert
        assert!(events1.is_empty());
        assert_eq!(events2, vec![InputEvent::Key(b"\x1b[A".to_vec())]);
    }

    #[test]
    fn test_bracketed_paste_extracted_with_content() {
        // Arrange
        let mut buf = StdinBuffer::new();

        // Act — ESC[200~ hello world ESC[201~
        let events = buf.feed(b"\x1b[200~hello world\x1b[201~");

        // Assert
        assert_eq!(
            events,
            vec![InputEvent::Paste("hello world".to_string())]
        );
    }

    #[test]
    fn test_large_paste_truncated_with_warning() {
        // Arrange
        let mut buf = StdinBuffer::new();
        let large_content = "x".repeat(6000);
        let mut input = b"\x1b[200~".to_vec();
        input.extend_from_slice(large_content.as_bytes());
        input.extend_from_slice(b"\x1b[201~");

        // Act
        let events = buf.feed(&input);

        // Assert
        assert_eq!(events.len(), 1);
        match &events[0] {
            InputEvent::Paste(content) => {
                assert!(content.contains("... [truncated, 6000 chars total]"));
                assert!(content.starts_with(&"x".repeat(5000)));
            }
            _ => panic!("Expected Paste event"),
        }
    }

    #[test]
    fn test_flush_emits_remaining_bytes() {
        // Arrange
        let mut buf = StdinBuffer::new();
        buf.feed(b"\x1b"); // Lone ESC held back

        // Act
        let event = buf.flush();

        // Assert
        assert_eq!(event, Some(InputEvent::Key(vec![0x1b])));
    }

    #[test]
    fn test_empty_buffer_flush_returns_none() {
        // Arrange
        let mut buf = StdinBuffer::new();

        // Act
        let event = buf.flush();

        // Assert
        assert_eq!(event, None);
    }

    #[test]
    fn test_utf8_multibyte_character_extracted_correctly() {
        // Arrange
        let mut buf = StdinBuffer::new();

        // Act — é = 0xC3 0xA9 (2-byte UTF-8)
        let events = buf.feed("é".as_bytes());

        // Assert
        assert_eq!(events, vec![InputEvent::Key("é".as_bytes().to_vec())]);
    }

    #[test]
    fn test_mixed_input_regular_csi_regular_extracted_in_order() {
        // Arrange
        let mut buf = StdinBuffer::new();

        // Act — 'a' + ESC[B (down arrow) + 'z'
        let events = buf.feed(b"a\x1b[Bz");

        // Assert
        assert_eq!(
            events,
            vec![
                InputEvent::Key(vec![b'a']),
                InputEvent::Key(b"\x1b[B".to_vec()),
                InputEvent::Key(vec![b'z']),
            ]
        );
    }
}
