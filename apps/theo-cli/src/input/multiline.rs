//! Triple-backtick multi-line input detection.
//!
//! Used by the REPL to detect when the user started a fenced block
//! and should keep reading lines until the fence closes.

/// State of the multi-line detector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Not inside a fenced block; a single `Enter` submits.
    Idle,
    /// Inside a fenced block; `Enter` adds a newline until the fence closes.
    Fenced,
}

/// Tracks whether the cumulative input has an open triple-backtick
/// fence. Each call to [`update`] returns the new state after feeding
/// a line.
#[derive(Debug, Default)]
pub struct MultilineDetector {
    state: State,
}

impl MultilineDetector {
    pub fn new() -> Self {
        Self { state: State::Idle }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn is_fenced(&self) -> bool {
        matches!(self.state, State::Fenced)
    }

    /// Consume a line and update state. Returns the new state.
    pub fn update(&mut self, line: &str) -> State {
        let fences = count_fences(line);
        // Each fence toggles the state.
        for _ in 0..fences {
            self.state = match self.state {
                State::Idle => State::Fenced,
                State::Fenced => State::Idle,
            };
        }
        self.state
    }

    /// Reset to Idle (e.g. on Ctrl+C).
    pub fn reset(&mut self) {
        self.state = State::Idle;
    }
}

impl Default for State {
    fn default() -> Self {
        State::Idle
    }
}

/// Count the number of triple-backtick fences in a line.
fn count_fences(line: &str) -> usize {
    let mut count = 0;
    let mut i = 0;
    let bytes = line.as_bytes();
    while i + 2 < bytes.len() + 1 {
        if i + 2 < bytes.len() && &bytes[i..i + 3] == b"```" {
            count += 1;
            i += 3;
        } else {
            i += 1;
        }
    }
    // Handle line ending exactly with ```
    if bytes.len() >= 3 && &bytes[bytes.len() - 3..] == b"```" {
        // Already counted in loop above
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_idle() {
        assert_eq!(MultilineDetector::new().state(), State::Idle);
    }

    #[test]
    fn test_plain_line_stays_idle() {
        let mut d = MultilineDetector::new();
        assert_eq!(d.update("hello world"), State::Idle);
    }

    #[test]
    fn test_opening_fence_enters_fenced() {
        let mut d = MultilineDetector::new();
        assert_eq!(d.update("```rust"), State::Fenced);
    }

    #[test]
    fn test_closing_fence_exits_fenced() {
        let mut d = MultilineDetector::new();
        d.update("```rust");
        assert_eq!(d.update("```"), State::Idle);
    }

    #[test]
    fn test_two_fences_same_line_stays_idle() {
        let mut d = MultilineDetector::new();
        // `foo` inline code: ``` ... ``` counts as 2 fences
        assert_eq!(d.update("```code```"), State::Idle);
    }

    #[test]
    fn test_three_fences_ends_fenced() {
        let mut d = MultilineDetector::new();
        assert_eq!(d.update("```one``` ```"), State::Fenced);
    }

    #[test]
    fn test_reset_clears_state() {
        let mut d = MultilineDetector::new();
        d.update("```");
        assert!(d.is_fenced());
        d.reset();
        assert_eq!(d.state(), State::Idle);
    }

    #[test]
    fn test_is_fenced_matches_state() {
        let mut d = MultilineDetector::new();
        assert!(!d.is_fenced());
        d.update("```");
        assert!(d.is_fenced());
    }

    #[test]
    fn test_sequence_of_lines() {
        let mut d = MultilineDetector::new();
        assert_eq!(d.update("```rust"), State::Fenced);
        assert_eq!(d.update("fn main() {}"), State::Fenced);
        assert_eq!(d.update("```"), State::Idle);
        assert_eq!(d.update("next prompt"), State::Idle);
    }
}
