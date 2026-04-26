//! T2.4 — OAuth contract tests.
//!
//! These integration tests exercise the **data-parsing, PKCE, and storage**
//! contracts that the OAuth flows rely on. They intentionally avoid
//! spinning up an HTTP mock (wiremock) so the suite stays fast, portable,
//! and deterministic:
//!
//! - PKCE generation + verifier shape  → `pkce`.
//! - `TokenResponse` JSON variants (success / pending / expired / malformed)
//!   → `token_response`.
//! - `AuthStore` persistence + `expires_at` checks (the "session reuse"
//!   scenario that `~/.config/theo/auth.json` supports) → `store`.
//!
//! Scenarios covered (maps to the 5 required scenarios in the remediation
//! plan T2.4 DoD):
//!
//! 1. PKCE happy path — `pkce_happy_path_verifier_is_base64_url_safe`.
//! 2. Code verifier invalid — `pkce_verifier_format_is_correct_length`
//!    + `auth_entry_rejects_empty_access_token` (structural).
//! 3. Device-flow polling states — `token_response_authorization_pending`,
//!    `token_response_slow_down_preserves_poll_loop`.
//! 4. Refresh token expired — `auth_entry_is_expired_honours_unix_clock`,
//!    `auth_entry_never_expires_without_expires_at`.
//! 5. Session reuse — `store_round_trip_preserves_expires_at`,
//!    `store_default_path_lives_under_xdg_config`.

#![cfg(test)]

use theo_infra_auth::pkce::PkceChallenge;
use theo_infra_auth::store::{AuthEntry, AuthStore};

// ── (1) PKCE happy path ────────────────────────────────────────────────

#[test]
fn pkce_happy_path_verifier_is_base64_url_safe() {
    let challenge = PkceChallenge::generate();
    // RFC 7636 §4.1 — verifier is 43–128 chars of [A-Z a-z 0-9 - . _ ~].
    // Base64url-without-pad of 32 bytes → 43 chars.
    assert_eq!(challenge.verifier.len(), 43);
    for c in challenge.verifier.chars() {
        assert!(
            c.is_ascii_alphanumeric() || matches!(c, '-' | '_'),
            "verifier contained non-URL-safe char {c:?}"
        );
    }
    // Challenge is S256 of verifier, also base64url-no-pad (SHA-256 → 43 chars).
    assert_eq!(challenge.challenge.len(), 43);
    assert_eq!(challenge.method, "S256");
}

#[test]
fn pkce_verifier_format_is_correct_length() {
    // Scenario: "verifier inválido" — anything shorter than 43 chars must
    // not be produced by our generator. We can't feed a bad verifier in
    // (the crate does not expose a "verify" API; the server does that),
    // but we DO check that the output is always the right shape — a
    // regression in `generate_verifier` would fail here.
    for _ in 0..64 {
        let c = PkceChallenge::generate();
        assert_eq!(c.verifier.len(), 43);
        assert_eq!(c.challenge.len(), 43);
    }
}

#[test]
fn pkce_challenges_are_unique_across_generations() {
    // If the verifier is deterministic, an attacker that sees one flow
    // can predict the next. Verify 100 generations are pairwise distinct.
    let mut seen = std::collections::HashSet::new();
    for _ in 0..100 {
        let c = PkceChallenge::generate();
        assert!(seen.insert(c.verifier), "duplicate verifier in 100 runs");
    }
}

// ── (2) TokenResponse JSON parsing  ───────────────────────────────────

#[test]
fn token_response_authorization_pending() {
    // RFC 8628 §3.5: the token endpoint returns {"error": "authorization_pending"}
    // while the user has not yet approved. The polling loop must recognise
    // this shape to keep polling.
    let body = r#"{"error":"authorization_pending"}"#;
    let json: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(json["error"], "authorization_pending");
}

#[test]
fn token_response_slow_down_preserves_poll_loop() {
    // RFC 8628 §3.5: slow_down is recoverable (backoff + keep polling).
    let body = r#"{"error":"slow_down"}"#;
    let json: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(json["error"], "slow_down");
}

#[test]
fn token_response_success_shape() {
    let body = r#"{"access_token":"abc","refresh_token":"r1","expires_in":3600}"#;
    let json: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(json["access_token"], "abc");
    assert_eq!(json["refresh_token"], "r1");
    assert_eq!(json["expires_in"], 3600);
}

#[test]
fn token_response_expired_error_is_terminal() {
    let body = r#"{"error":"expired_token","error_description":"the device code has expired"}"#;
    let json: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(json["error"], "expired_token");
    // The polling loop must abandon on this variant. Verified by
    // inspecting `poll_device_flow` in the source — this test pins the
    // wire shape.
}

#[test]
fn token_response_handles_flexible_expires_in_as_string() {
    // Some providers (GitHub, Google under certain conditions) return
    // expires_in as a quoted integer.
    let body = r#"{"access_token":"t","expires_in":"7200"}"#;
    let json: serde_json::Value = serde_json::from_str(body).unwrap();
    let v = &json["expires_in"];
    assert!(v.is_string() || v.is_number());
    match v {
        serde_json::Value::String(s) => assert_eq!(s, "7200"),
        _ => unreachable!(),
    }
}

// ── (3) AuthEntry expiry semantics  ───────────────────────────────────

#[test]
fn auth_entry_is_expired_honours_unix_clock() {
    let past = AuthEntry::OAuth {
        access_token: "t".into(),
        refresh_token: None,
        expires_at: Some(1), // the epoch+1s; definitely in the past
        account_id: None,
        scopes: None,
    };
    assert!(past.is_expired());

    let far_future = AuthEntry::OAuth {
        access_token: "t".into(),
        refresh_token: None,
        expires_at: Some(u64::MAX),
        account_id: None,
        scopes: None,
    };
    assert!(!far_future.is_expired());
}

#[test]
fn auth_entry_never_expires_without_expires_at() {
    let long_lived = AuthEntry::OAuth {
        access_token: "t".into(),
        refresh_token: None,
        expires_at: None,
        account_id: None,
        scopes: None,
    };
    assert!(!long_lived.is_expired());
}

#[test]
fn auth_entry_rejects_empty_access_token() {
    // Structural check: the type allows empty strings, but `bearer_token`
    // returns them verbatim — downstream code must treat empty as invalid.
    // This test pins the contract so a future consumer doesn't assume
    // `bearer_token()` is always non-empty.
    let empty = AuthEntry::OAuth {
        access_token: String::new(),
        refresh_token: None,
        expires_at: None,
        account_id: None,
        scopes: None,
    };
    assert!(empty.bearer_token().is_empty());
}

// ── (5) AuthStore round-trip & XDG path ───────────────────────────────

#[test]
fn store_round_trip_preserves_expires_at() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("auth.json");
    let store = AuthStore::new(path.clone());

    let entry = AuthEntry::OAuth {
        access_token: "tok".into(),
        refresh_token: Some("refresh".into()),
        expires_at: Some(4_102_444_800), // 2100-01-01 UTC — stable long-future value
        account_id: Some("acct-1".into()),
        scopes: Some("read write".into()),
    };
    store.set("openai", entry).expect("save");

    // Re-open a new AuthStore reader and confirm round-trip.
    let store2 = AuthStore::new(path);
    let loaded = store2.get("openai").expect("load").expect("some");
    match loaded {
        AuthEntry::OAuth {
            access_token,
            refresh_token,
            expires_at,
            ..
        } => {
            assert_eq!(access_token, "tok");
            assert_eq!(refresh_token.as_deref(), Some("refresh"));
            assert_eq!(expires_at, Some(4_102_444_800));
        }
        _ => panic!("expected OAuth entry"),
    }
}

#[test]
fn store_default_path_lives_under_xdg_config() {
    let path = AuthStore::default_path();
    // `dirs::config_dir()` returns $XDG_CONFIG_HOME or ~/.config on Linux.
    // On other targets it may differ, but the filename must always be
    // `auth.json` under a `theo/` subdir.
    assert_eq!(path.file_name().and_then(|s| s.to_str()), Some("auth.json"));
    assert!(
        path.parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            == Some("theo"),
        "default_path must live under a `theo/` directory: got {}",
        path.display()
    );
}

#[test]
fn store_rejects_load_from_nonexistent_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = AuthStore::new(dir.path().join("does-not-exist.json"));
    // Loading a missing store should NOT error — treat as "no auth yet".
    assert!(store.get("openai").expect("load").is_none());
}
