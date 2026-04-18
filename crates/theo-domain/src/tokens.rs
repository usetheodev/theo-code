//! Unified token estimation — single source of truth for token counting.
//!
//! Used by both the retrieval assembly (context budget) and the agent runtime
//! (compaction threshold). Having a single function prevents divergence.

/// Estimate token count for a text string.
///
/// Uses a hybrid heuristic: `max(chars/4, words * 1.3) + overhead`.
/// This balances accuracy for code (short identifiers, symbols) vs prose (longer words).
///
/// More accurate than chars/4 alone (underestimates code) or words*1.3 alone
/// (underestimates minified/JSON content).
pub fn estimate_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }

    let char_estimate = text.len() / 4;
    let word_count = text.split_whitespace().count();
    let word_estimate = (word_count as f64 * 1.3) as usize;

    // Take the higher estimate for safety (better to overcount than undercount).
    char_estimate.max(word_estimate)
}

/// Estimate tokens for a text string with per-message overhead.
///
/// Each message has ~10 tokens of overhead (role, formatting, separators).
pub fn estimate_message_tokens(text: &str) -> usize {
    estimate_tokens(text) + 10
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string_returns_zero() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn short_prose() {
        let tokens = estimate_tokens("Hello world");
        assert!(tokens > 0);
        assert!(tokens < 10);
    }

    #[test]
    fn code_with_short_identifiers() {
        // Code has many short tokens — chars/4 should dominate
        let code = "fn f(x: i32) -> bool { x > 0 }";
        let tokens = estimate_tokens(code);
        assert!(
            tokens >= 5,
            "Expected at least 5 tokens for short code, got {tokens}"
        );
    }

    #[test]
    fn long_text_reasonable_estimate() {
        // ~100 words of prose
        let text = "The quick brown fox ".repeat(25);
        let tokens = estimate_tokens(&text);
        // 100 words * 1.3 ≈ 130 tokens, or 500 chars / 4 = 125
        assert!(tokens >= 100, "Expected 100+ tokens, got {tokens}");
        assert!(tokens <= 200, "Expected under 200 tokens, got {tokens}");
    }

    #[test]
    fn json_content() {
        // JSON has lots of punctuation — chars/4 should dominate
        let json = r#"{"key":"value","nested":{"a":1,"b":2}}"#;
        let tokens = estimate_tokens(json);
        assert!(
            tokens >= 5,
            "Expected at least 5 tokens for JSON, got {tokens}"
        );
    }

    #[test]
    fn message_overhead_adds_ten() {
        let text = "hello";
        let base = estimate_tokens(text);
        let with_overhead = estimate_message_tokens(text);
        assert_eq!(with_overhead, base + 10);
    }

    #[test]
    fn unicode_handled_correctly() {
        // Emoji: 4 bytes each, but should still produce reasonable estimate
        let text = "🎉🎉🎉🎉🎉";
        let tokens = estimate_tokens(text);
        assert!(tokens > 0);
    }
}
