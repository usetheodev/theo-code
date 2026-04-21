//! Authentication use cases — headless `login` / `logout` flows.
//!
//! `theo-application` is the composition root where `theo-infra-auth`
//! primitives become usable by the CLI without breaking the boundary
//! rule (apps/* → theo-application only).
//!
//! Design reference: opencode (`opencode login <url>`) and Claude Code
//! (`claude login`). Both treat auth as a headless subcommand separate
//! from the interactive agent surface, so CI/scripts can automate
//! credential provisioning and the TUI stays focused on agent work.

use theo_infra_auth::AuthError;
use theo_infra_auth::store::{AuthEntry, AuthStore};

const OPENAI_PROVIDER_ID: &str = "openai";

/// Minimum length for an API key we will accept. Catches obvious
/// empty/whitespace input before writing garbage to disk. Provider-side
/// validation is authoritative — this is just a sanity gate.
const MIN_API_KEY_CHARS: usize = 12;

/// Persist `key` as an API-key entry for the OpenAI provider in `store`.
/// Returns the saved length so callers can emit a safe mask.
pub fn save_api_key(store: &AuthStore, key: &str) -> Result<usize, AuthError> {
    let trimmed = key.trim();
    if trimmed.len() < MIN_API_KEY_CHARS {
        return Err(AuthError::OAuth(format!(
            "api key must be at least {MIN_API_KEY_CHARS} chars (got {})",
            trimmed.len()
        )));
    }
    store.set(
        OPENAI_PROVIDER_ID,
        AuthEntry::ApiKey {
            key: trimmed.to_string(),
        },
    )?;
    Ok(trimmed.len())
}

/// Remove (or clear) the OpenAI entry from `store`. Idempotent:
/// `Ok(false)` when there was nothing to clear, `Ok(true)` on success.
///
/// AuthStore does not expose a delete yet; we overwrite with an empty
/// ApiKey entry so consumers that fall through to env-var fallback
/// still do the right thing.
pub fn logout(store: &AuthStore) -> Result<bool, AuthError> {
    match store.get(OPENAI_PROVIDER_ID)? {
        None => Ok(false),
        Some(_) => {
            store.set(
                OPENAI_PROVIDER_ID,
                AuthEntry::ApiKey {
                    key: String::new(),
                },
            )?;
            Ok(true)
        }
    }
}

/// Short human-friendly mask: `first-6 … last-4`.
pub fn mask_key(key: &str) -> String {
    if key.len() <= 10 {
        return "***".to_string();
    }
    format!("{}…{}", &key[..6], &key[key.len() - 4..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (tempfile::TempDir, AuthStore) {
        let dir = tempfile::tempdir().expect("t");
        let path = dir.path().join("auth.json");
        (dir, AuthStore::new(path))
    }

    // AAA pattern, one behaviour per test, independent.

    #[test]
    fn test_save_api_key_persists_value_in_store() {
        // Arrange
        let (_dir, store) = temp_store();
        let key = "sk-test-abcdef123456";

        // Act
        let saved = save_api_key(&store, key).expect("save ok");

        // Assert
        assert_eq!(saved, key.len());
        match store.get(OPENAI_PROVIDER_ID).expect("t") {
            Some(AuthEntry::ApiKey { key: stored }) => assert_eq!(stored, key),
            other => panic!("expected ApiKey, got {other:?}"),
        }
    }

    #[test]
    fn test_save_api_key_rejects_short_input() {
        let (_dir, store) = temp_store();
        let err = save_api_key(&store, "short").unwrap_err();
        assert!(
            format!("{err}").to_lowercase().contains("at least"),
            "error must name the minimum length, got: {err}"
        );
        assert!(store.get(OPENAI_PROVIDER_ID).expect("t").is_none());
    }

    #[test]
    fn test_save_api_key_trims_whitespace() {
        let (_dir, store) = temp_store();
        save_api_key(&store, "   sk-with-padding-xyz   ").expect("t");
        match store.get(OPENAI_PROVIDER_ID).expect("t") {
            Some(AuthEntry::ApiKey { key }) => {
                assert!(!key.starts_with(' '));
                assert!(!key.ends_with(' '));
                assert_eq!(key.as_str(), "sk-with-padding-xyz");
            }
            _ => panic!("expected ApiKey"),
        }
    }

    #[test]
    fn test_logout_on_empty_store_returns_false() {
        let (_dir, store) = temp_store();
        let cleared = logout(&store).expect("t");
        assert!(!cleared);
    }

    #[test]
    fn test_logout_clears_saved_api_key() {
        let (_dir, store) = temp_store();
        save_api_key(&store, "sk-prod-key-xxxxxxxx").expect("t");
        assert!(store.get(OPENAI_PROVIDER_ID).expect("t").is_some());

        let cleared = logout(&store).expect("t");
        assert!(cleared);

        match store.get(OPENAI_PROVIDER_ID).expect("t") {
            Some(AuthEntry::ApiKey { key }) => assert!(key.is_empty()),
            None => {}
            other => panic!("unexpected state: {other:?}"),
        }
    }

    #[test]
    fn test_mask_key_shows_only_head_and_tail() {
        let full = "sk-abcdefghijklmnopqrstuvwxyz1234";
        let masked = mask_key(full);
        assert!(masked.starts_with("sk-abc"));
        assert!(masked.ends_with("1234"));
        assert!(!masked.contains("ijkl"));
    }

    #[test]
    fn test_mask_key_short_input_is_fully_opaque() {
        assert_eq!(mask_key("short"), "***");
        assert_eq!(mask_key(""), "***");
    }
}
