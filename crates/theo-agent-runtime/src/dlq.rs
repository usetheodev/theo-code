use serde::{Deserialize, Serialize};

use theo_domain::identifiers::CallId;

/// A dead letter — a tool call that failed permanently after exhausting retries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadLetter {
    pub call_id: CallId,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub error: String,
    pub attempts: u32,
    pub created_at: u64,
}

/// In-memory dead-letter queue for permanently failed operations.
///
/// Operations that exhaust their retry policy are pushed here for
/// later inspection or manual retry.
pub struct DeadLetterQueue {
    letters: Vec<DeadLetter>,
}

impl DeadLetterQueue {
    pub fn new() -> Self {
        Self {
            letters: Vec::new(),
        }
    }

    pub fn push(&mut self, letter: DeadLetter) {
        self.letters.push(letter);
    }

    /// Drains all dead letters, returning them and emptying the queue.
    pub fn drain(&mut self) -> Vec<DeadLetter> {
        std::mem::take(&mut self.letters)
    }

    pub fn len(&self) -> usize {
        self.letters.len()
    }

    pub fn is_empty(&self) -> bool {
        self.letters.is_empty()
    }

    /// Returns a snapshot of all dead letters without draining.
    pub fn peek(&self) -> &[DeadLetter] {
        &self.letters
    }
}

impl Default for DeadLetterQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_letter(tool: &str, error: &str) -> DeadLetter {
        DeadLetter {
            call_id: CallId::new("c-1"),
            tool_name: tool.to_string(),
            input: serde_json::json!({}),
            error: error.to_string(),
            attempts: 3,
            created_at: 1000,
        }
    }

    #[test]
    fn push_increments_len() {
        let mut dlq = DeadLetterQueue::new();
        assert!(dlq.is_empty());
        assert_eq!(dlq.len(), 0);

        dlq.push(make_letter("bash", "timeout"));
        assert_eq!(dlq.len(), 1);
        assert!(!dlq.is_empty());
    }

    #[test]
    fn drain_returns_all_and_empties() {
        let mut dlq = DeadLetterQueue::new();
        dlq.push(make_letter("bash", "timeout"));
        dlq.push(make_letter("read", "permission denied"));

        let letters = dlq.drain();
        assert_eq!(letters.len(), 2);
        assert!(dlq.is_empty());
        assert_eq!(dlq.len(), 0);
    }

    #[test]
    fn peek_does_not_drain() {
        let mut dlq = DeadLetterQueue::new();
        dlq.push(make_letter("edit", "conflict"));

        assert_eq!(dlq.peek().len(), 1);
        assert_eq!(dlq.len(), 1); // still there
    }

    #[test]
    fn dead_letter_serde_roundtrip() {
        let letter = make_letter("bash", "command not found");
        let json = serde_json::to_string(&letter).unwrap();
        let back: DeadLetter = serde_json::from_str(&json).unwrap();
        assert_eq!(back.call_id, letter.call_id);
        assert_eq!(back.tool_name, "bash");
        assert_eq!(back.error, "command not found");
        assert_eq!(back.attempts, 3);
    }
}
