//! Pure helper functions extracted from `run_engine.rs` as part of the
//! Fase 4 split (REMEDIATION_PLAN T4.2). This module is intentionally
//! side-effect-free (no fs, no network, no shared state) so it composes
//! cleanly into any future sub-module layout.
//!
//! Keeping these helpers in a separate file shrinks `run_engine.rs` by
//! ~80 LOC and makes the split boundary easier to reason about.

use theo_domain::error_class::ErrorClass;

/// Map an `LlmError` to the high-level `ErrorClass` used in the agent
/// run outcome. Conservative fallback is `Aborted`.
pub(crate) fn llm_error_to_class(e: &theo_infra_llm::LlmError) -> ErrorClass {
    use theo_infra_llm::LlmError;
    match e {
        LlmError::RateLimited { .. } => ErrorClass::RateLimited,
        // Distinct from RateLimited: retry doesn't help for quota
        // exhaustion — only the billing cycle reset clears it.
        LlmError::QuotaExceeded { .. } => ErrorClass::QuotaExceeded,
        LlmError::AuthFailed(_) => ErrorClass::AuthFailed,
        LlmError::ContextOverflow { .. } => ErrorClass::ContextOverflow,
        // Network / Timeout / ServiceUnavailable / Parse / Api / etc.
        // — none of these match a more specific class, so they fall
        // into the catch-all "internal abort".
        _ => ErrorClass::Aborted,
    }
}

/// Truncate a handoff objective string to 200 characters max, replacing
/// the tail with an ellipsis. Char-boundary safe (counts chars, not bytes).
pub(crate) fn truncate_handoff_objective(s: &str) -> String {
    if s.chars().count() <= 200 {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(199).collect();
        t.push('…');
        t
    }
}

/// Render a short, human-readable summary of a single batch tool call's
/// arguments — shown in the batch-result preview sent back to the LLM.
pub(crate) fn truncate_batch_args(args: &serde_json::Value) -> String {
    if let Some(path) = args.get("filePath").and_then(|v| v.as_str()) {
        return path.to_string();
    }
    if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
        let short = if cmd.len() > 40 { &cmd[..40] } else { cmd };
        return short.to_string();
    }
    if let Some(pattern) = args.get("pattern").and_then(|v| v.as_str()) {
        return format!("\"{}\"", pattern);
    }
    "...".to_string()
}

/// Heuristic mapping `base_url → provider` for the OTel `gen_ai.system`
/// attribute. Returns `"openai_compatible"` for unknown URLs since
/// theo's protocol is OpenAI-compatible across providers.
pub(crate) fn derive_provider_hint(base_url: &str) -> &'static str {
    let lower = base_url.to_ascii_lowercase();
    if lower.contains("api.openai.com") || lower.contains("chatgpt.com") {
        "openai"
    } else if lower.contains("api.anthropic.com") {
        "anthropic"
    } else if lower.contains("googleapis.com") || lower.contains("gemini") {
        "gemini"
    } else if lower.contains("groq.com") {
        "groq"
    } else if lower.contains("mistral.ai") {
        "mistral"
    } else if lower.contains("deepseek") {
        "deepseek"
    } else if lower.contains("together.ai") {
        "together"
    } else if lower.contains("xai") || lower.contains("x.ai") {
        "xai"
    } else if lower.contains("localhost") || lower.contains("127.0.0.1") {
        "openai_compatible_local"
    } else {
        "openai_compatible"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // llm_error_to_class
    #[test]
    fn llm_error_rate_limited_maps_to_rate_limited() {
        let e = theo_infra_llm::LlmError::RateLimited {
            retry_after: Some(60),
        };
        assert_eq!(llm_error_to_class(&e), ErrorClass::RateLimited);
    }

    #[test]
    fn llm_error_auth_maps_to_auth_failed() {
        let e = theo_infra_llm::LlmError::AuthFailed("bad token".into());
        assert_eq!(llm_error_to_class(&e), ErrorClass::AuthFailed);
    }

    #[test]
    fn llm_error_generic_parse_falls_back_to_aborted() {
        let e = theo_infra_llm::LlmError::Parse("bad json".into());
        assert_eq!(llm_error_to_class(&e), ErrorClass::Aborted);
    }

    // truncate_handoff_objective
    #[test]
    fn truncate_handoff_short_string_unchanged() {
        assert_eq!(truncate_handoff_objective("short"), "short");
    }

    #[test]
    fn truncate_handoff_long_string_truncated_at_199_plus_ellipsis() {
        let input: String = "a".repeat(300);
        let out = truncate_handoff_objective(&input);
        assert_eq!(out.chars().count(), 200);
        assert!(out.ends_with('…'));
    }

    // truncate_batch_args
    #[test]
    fn truncate_batch_args_prefers_file_path() {
        let args = serde_json::json!({"filePath": "/tmp/a.rs"});
        assert_eq!(truncate_batch_args(&args), "/tmp/a.rs");
    }

    #[test]
    fn truncate_batch_args_shortens_long_commands() {
        let args = serde_json::json!({"command": "a".repeat(100)});
        assert_eq!(truncate_batch_args(&args).len(), 40);
    }

    #[test]
    fn truncate_batch_args_falls_back_to_ellipsis() {
        let args = serde_json::json!({"unknown_field": "x"});
        assert_eq!(truncate_batch_args(&args), "...");
    }

    // derive_provider_hint
    #[test]
    fn derive_provider_hint_recognizes_openai() {
        assert_eq!(derive_provider_hint("https://api.openai.com/v1"), "openai");
    }

    #[test]
    fn derive_provider_hint_recognizes_anthropic() {
        assert_eq!(derive_provider_hint("https://api.anthropic.com"), "anthropic");
    }

    #[test]
    fn derive_provider_hint_recognizes_localhost_as_local() {
        assert_eq!(
            derive_provider_hint("http://localhost:8000"),
            "openai_compatible_local"
        );
    }

    #[test]
    fn derive_provider_hint_falls_back_for_unknown_url() {
        assert_eq!(
            derive_provider_hint("https://my-private-llm.corp"),
            "openai_compatible"
        );
    }
}
