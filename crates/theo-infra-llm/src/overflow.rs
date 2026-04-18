//! Context window overflow detection for multiple LLM providers.
//!
//! Each provider reports context-length errors differently. This module
//! provides pattern-based detection so the agent runtime can trigger
//! emergency compaction and retry instead of aborting.
//!
//! **Pi-mono ref:** `packages/ai/src/utils/overflow.ts`

/// Lowercase patterns that indicate a context window overflow.
const OVERFLOW_PATTERNS: &[&str] = &[
    // OpenAI / OA-compatible
    "context_length_exceeded",
    "maximum context length",
    "please reduce the length",
    // Anthropic
    "prompt is too long",
    "request too large",
    // Google / Vertex
    "exceeds the context window",
    "input token count",
    // xAI / Grok
    "maximum prompt length",
    // Generic
    "token limit exceeded",
    "too many tokens",
    "context window exceeds limit",
];

/// Lowercase patterns that look like overflow but are actually rate-limit errors.
const NON_OVERFLOW_PATTERNS: &[&str] = &["rate limit", "too many requests"];

/// Check whether an error message indicates a context window overflow.
pub fn is_context_overflow(error_message: &str) -> bool {
    let lower = error_message.to_lowercase();

    // Exclude rate-limit false positives first
    if NON_OVERFLOW_PATTERNS.iter().any(|p| lower.contains(p)) {
        return false;
    }

    OVERFLOW_PATTERNS.iter().any(|p| lower.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── OpenAI ──────────────────────────────────────────────────

    #[test]
    fn detects_openai_context_length_exceeded() {
        assert!(is_context_overflow(
            "This model's maximum context length is 128000 tokens. \
             However, your messages resulted in 140000 tokens. \
             Please reduce the length of the messages."
        ));
    }

    #[test]
    fn detects_openai_context_length_exceeded_code() {
        assert!(is_context_overflow(
            "Error code: context_length_exceeded"
        ));
    }

    // ── Anthropic ───────────────────────────────────────────────

    #[test]
    fn detects_anthropic_prompt_too_long() {
        assert!(is_context_overflow(
            "prompt is too long: 210000 tokens > 200000 maximum"
        ));
    }

    #[test]
    fn detects_anthropic_request_too_large() {
        assert!(is_context_overflow("request too large for model"));
    }

    // ── Google ──────────────────────────────────────────────────

    #[test]
    fn detects_google_exceeds_context_window() {
        assert!(is_context_overflow(
            "Input exceeds the context window of the model"
        ));
    }

    #[test]
    fn detects_google_input_token_count() {
        assert!(is_context_overflow(
            "input token count of 150000 exceeds the limit of 128000"
        ));
    }

    // ── Generic ─────────────────────────────────────────────────

    #[test]
    fn detects_generic_token_limit() {
        assert!(is_context_overflow("token limit exceeded"));
    }

    #[test]
    fn detects_generic_too_many_tokens() {
        assert!(is_context_overflow("Request has too many tokens"));
    }

    // ── Negative cases ──────────────────────────────────────────

    #[test]
    fn does_not_match_rate_limit() {
        assert!(!is_context_overflow(
            "rate limit exceeded, retry after 30s"
        ));
    }

    #[test]
    fn does_not_match_too_many_requests() {
        assert!(!is_context_overflow("429 Too Many Requests"));
    }

    #[test]
    fn does_not_match_unrelated_error() {
        assert!(!is_context_overflow(
            "authentication failed: invalid API key"
        ));
    }

    #[test]
    fn does_not_match_empty_string() {
        assert!(!is_context_overflow(""));
    }
}
