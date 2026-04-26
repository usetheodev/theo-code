//! Secret / API-key redaction for tool output and persisted state.
//!
//! T4.5 / FIND-P6-008 (part 2). Sister module to
//! [`crate::tool_pair_integrity`]. Where `tool_pair_integrity` repairs
//! orphaned `tool_use`/`tool_result` pairs after compaction,
//! `secret_scrubber` removes obvious credentials from any string that
//! is about to be persisted (JSONL session-tree, snapshot store, OTel
//! span attributes) or rendered back to the LLM.
//!
//! The scrubber is **regex-based and best-effort** — it cannot catch
//! arbitrary secrets, only well-known patterns. The design favours
//! false negatives over false positives so legitimate code that
//! happens to look secret-shaped is not corrupted.
//!
//! # Patterns covered
//!
//! - Anthropic API keys (`sk-ant-…`)
//! - GitHub Personal Access Tokens (`ghp_…`, `gho_…`, `ghu_…`, `ghs_…`)
//! - AWS access key IDs (`AKIA…`)
//! - PEM private-key blocks (`-----BEGIN ... PRIVATE KEY-----`)
//!
//! All matches are replaced with the literal string `"[REDACTED]"` and
//! their original length is NOT preserved.

use regex::Regex;
use std::sync::OnceLock;

/// Marker substituted in place of any matched secret.
pub const REDACTED: &str = "[REDACTED]";

/// Lazily-compiled regex set. Compiling regexes is non-trivial, so we
/// initialise the set once per process and reuse it for every call to
/// [`scrub_secrets`].
fn patterns() -> &'static Vec<Regex> {
    static SET: OnceLock<Vec<Regex>> = OnceLock::new();
    SET.get_or_init(|| {
        // Each pattern is correct by construction (literal byte
        // classes + bounded repetition). Compile failure here is a
        // build-time bug, never user input — `expect` is appropriate.
        let raw = [
            // Anthropic API key — `sk-ant-` followed by ≥20 chars from
            // the URL-safe set the Anthropic console emits.
            r"sk-ant-[A-Za-z0-9_\-]{20,}",
            // GitHub PAT family — fixed prefix + base62 length.
            // ghp = personal, gho = OAuth, ghu = user-server, ghs = server.
            r"gh[pousr]_[A-Za-z0-9]{36,}",
            // AWS access key ID — `AKIA` + 16 uppercase alphanumerics.
            r"AKIA[0-9A-Z]{16}",
            // PEM private-key block — non-greedy match on body.
            // `(?s)` flag (s) lets `.` match `\n` for multiline keys.
            r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----",
        ];
        raw.iter()
            .map(|p| Regex::new(p).expect("scrubber regex constants are valid"))
            .collect()
    })
}

/// Replace every well-known secret pattern in `input` with [`REDACTED`].
///
/// Returns the redacted string. If no pattern matched, the function
/// allocates a fresh owned `String` equal to the input (kept simple
/// instead of returning `Cow<'_, str>` to avoid a viral type change at
/// the call sites).
pub fn scrub_secrets(input: &str) -> String {
    let mut out = input.to_string();
    for re in patterns() {
        if re.is_match(&out) {
            out = re.replace_all(&out, REDACTED).into_owned();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t45_scrubs_anthropic_key() {
        let input = "Authorization: Bearer sk-ant-api03-abcdefghijklmnopqrstuvwxyz123456";
        let out = scrub_secrets(input);
        assert!(!out.contains("sk-ant-api03-"), "key leaked: {out}");
        assert!(out.contains(REDACTED));
    }

    #[test]
    fn t45_scrubs_github_pat_all_prefixes() {
        for prefix in &["ghp_", "gho_", "ghu_", "ghs_"] {
            let input = format!(
                "token={}{}",
                prefix,
                "a".repeat(36)
            );
            let out = scrub_secrets(&input);
            assert!(
                !out.contains(&format!("{}{}", prefix, "a")),
                "prefix {prefix} not redacted: {out}"
            );
            assert!(out.contains(REDACTED));
        }
    }

    #[test]
    fn t45_scrubs_aws_access_key_id() {
        let input = "AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE";
        let out = scrub_secrets(input);
        assert!(!out.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(out.contains(REDACTED));
    }

    #[test]
    fn t45_scrubs_pem_block_multiline() {
        let pem = "before\n\
                   -----BEGIN RSA PRIVATE KEY-----\n\
                   MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcw\n\
                   ...redacted-body-bytes...\n\
                   -----END RSA PRIVATE KEY-----\n\
                   after";
        let out = scrub_secrets(pem);
        assert!(!out.contains("MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcw"));
        assert!(!out.contains("-----BEGIN"), "PEM header not redacted: {out}");
        assert!(out.contains("before"));
        assert!(out.contains("after"));
        assert!(out.contains(REDACTED));
    }

    #[test]
    fn t45_does_not_modify_unrelated_text() {
        // Strings that LOOK secret-shaped but do not match any pattern
        // must pass through unchanged.
        let input = "ordinary text with sk-something-else and ghp_short";
        let out = scrub_secrets(input);
        assert_eq!(out, input);
    }

    #[test]
    fn t45_idempotent_after_first_scrub() {
        let input = "k=sk-ant-api03-abcdefghijklmnopqrstuvwxyz123456";
        let first = scrub_secrets(input);
        let second = scrub_secrets(&first);
        assert_eq!(first, second, "scrubber must be idempotent");
    }

    #[test]
    fn t45_handles_multiple_distinct_secrets_in_one_string() {
        let input = format!(
            "ANTHROPIC_API_KEY=sk-ant-api03-{} GITHUB_TOKEN={}{}",
            "a".repeat(40),
            "ghp_",
            "b".repeat(40)
        );
        let out = scrub_secrets(&input);
        assert!(!out.contains("sk-ant-api03-aa"));
        assert!(!out.contains("ghp_bb"));
        // Both replaced — exactly two REDACTED tokens.
        assert_eq!(out.matches(REDACTED).count(), 2);
    }
}
