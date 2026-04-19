//! Preemptive context window overflow detection.
//!
//! Reactive detection lives in `overflow.rs` (parses API error messages).
//! This module provides the opposite: check BEFORE sending whether the
//! estimated request will fit.
//!
//! Reference: `referencias/gemini-cli/packages/core/src/core/client.ts:617-655`
//!
//! Pattern: `remaining = model_limit - last_prompt_tokens`; if estimated
//! request > remaining, emit `ContextWindowWillOverflow` without sending.
//! Integrates with the compaction pipeline: if overflow is imminent, the
//! caller should force `OptimizationLevel::Compact` before aborting.

/// Default context window when a model is not in the known table.
/// 200k matches the target budget documented in `.theo/evolution_criteria.md` C1.
pub const DEFAULT_CONTEXT_WINDOW: u64 = 200_000;

/// Default tokens reserved for the model's output when computing remaining budget.
pub const DEFAULT_OUTPUT_RESERVATION: u64 = 8_192;

/// Return the known context window (in tokens) for a model identifier.
///
/// Matching is case-insensitive and substring-based so variations like
/// `gpt-4o-2024-11-20` and `gpt-4o-mini` both map correctly. Unknown models
/// fall back to `DEFAULT_CONTEXT_WINDOW`.
pub fn model_token_limit(model: &str) -> u64 {
    let m = model.to_lowercase();

    // Anthropic
    if m.contains("claude-opus-4")
        || m.contains("claude-sonnet-4")
        || m.contains("claude-haiku-4")
    {
        return 200_000;
    }
    if m.contains("claude-3-5-sonnet") || m.contains("claude-3-5-haiku") {
        return 200_000;
    }
    if m.contains("claude-3-opus") || m.contains("claude-3-sonnet") || m.contains("claude-3-haiku") {
        return 200_000;
    }

    // OpenAI
    if m.contains("gpt-4o") || m.contains("gpt-4-turbo") {
        return 128_000;
    }
    if m.contains("gpt-4.1") {
        return 1_047_576;
    }
    if m.contains("o1") || m.contains("o3") {
        return 200_000;
    }
    if m.contains("gpt-3.5") {
        return 16_385;
    }

    // Google
    if m.contains("gemini-2.5") || m.contains("gemini-2.0") {
        return 2_000_000;
    }
    if m.contains("gemini-1.5-pro") {
        return 2_000_000;
    }
    if m.contains("gemini-1.5-flash") {
        return 1_000_000;
    }

    // xAI
    if m.contains("grok-2") || m.contains("grok-3") {
        return 128_000;
    }

    DEFAULT_CONTEXT_WINDOW
}

/// Compute the remaining budget for input tokens.
///
/// `limit - last_prompt_tokens - output_reservation`, floored at 0.
pub fn remaining_budget(model: &str, last_prompt_tokens: u64, output_reservation: u64) -> u64 {
    model_token_limit(model)
        .saturating_sub(last_prompt_tokens)
        .saturating_sub(output_reservation)
}

/// Preemptive check: would sending `estimated_tokens` overflow the model's window?
///
/// Returns `Some((estimated, remaining))` if overflow imminent, `None` otherwise.
/// Callers should invoke compaction or abort when this returns `Some`.
pub fn would_overflow(
    model: &str,
    estimated_tokens: u64,
    last_prompt_tokens: u64,
    output_reservation: u64,
) -> Option<(u64, u64)> {
    let remaining = remaining_budget(model, last_prompt_tokens, output_reservation);
    if estimated_tokens > remaining {
        Some((estimated_tokens, remaining))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_anthropic_models_return_200k() {
        assert_eq!(model_token_limit("claude-opus-4-7"), 200_000);
        assert_eq!(model_token_limit("claude-sonnet-4-6"), 200_000);
        assert_eq!(model_token_limit("claude-3-5-sonnet-20241022"), 200_000);
    }

    #[test]
    fn known_openai_models_return_correct_limits() {
        assert_eq!(model_token_limit("gpt-4o-2024-11-20"), 128_000);
        assert_eq!(model_token_limit("gpt-4o-mini"), 128_000);
        assert_eq!(model_token_limit("gpt-3.5-turbo"), 16_385);
        assert_eq!(model_token_limit("gpt-4.1"), 1_047_576);
    }

    #[test]
    fn known_gemini_models_return_correct_limits() {
        assert_eq!(model_token_limit("gemini-2.5-pro"), 2_000_000);
        assert_eq!(model_token_limit("gemini-1.5-flash-latest"), 1_000_000);
    }

    #[test]
    fn unknown_model_falls_back_to_default() {
        assert_eq!(model_token_limit("custom-fine-tune-xyz"), DEFAULT_CONTEXT_WINDOW);
        assert_eq!(model_token_limit(""), DEFAULT_CONTEXT_WINDOW);
    }

    #[test]
    fn case_insensitive_matching() {
        assert_eq!(model_token_limit("GPT-4O"), 128_000);
        assert_eq!(model_token_limit("Claude-Opus-4-7"), 200_000);
    }

    #[test]
    fn remaining_budget_subtracts_last_prompt_and_reservation() {
        let r = remaining_budget("gpt-4o", 100_000, 8_192);
        assert_eq!(r, 128_000 - 100_000 - 8_192);
    }

    #[test]
    fn remaining_budget_saturates_at_zero() {
        let r = remaining_budget("gpt-3.5-turbo", 20_000, 0);
        assert_eq!(r, 0);
    }

    #[test]
    fn would_overflow_returns_none_when_under_limit() {
        let result = would_overflow("gpt-4o", 50_000, 0, DEFAULT_OUTPUT_RESERVATION);
        assert!(result.is_none());
    }

    #[test]
    fn would_overflow_returns_some_when_exceeds() {
        let result = would_overflow("gpt-3.5-turbo", 20_000, 0, 0);
        assert_eq!(result, Some((20_000, 16_385)));
    }

    #[test]
    fn would_overflow_accounts_for_last_prompt() {
        // After a previous turn consumed 100k, only 28k-8k=20k remain.
        let result = would_overflow("gpt-4o", 25_000, 100_000, 8_192);
        assert!(result.is_some());
    }
}
