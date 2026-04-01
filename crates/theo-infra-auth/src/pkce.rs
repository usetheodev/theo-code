use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::Rng;
use sha2::{Digest, Sha256};

/// PKCE (Proof Key for Code Exchange) parameters for OAuth 2.0.
#[derive(Debug, Clone)]
pub struct PkceChallenge {
    /// The random verifier string (sent during token exchange).
    pub verifier: String,
    /// SHA-256 hash of verifier, base64url-encoded (sent during authorization).
    pub challenge: String,
    /// Always "S256".
    pub method: &'static str,
}

impl PkceChallenge {
    /// Generate a new PKCE challenge pair.
    pub fn generate() -> Self {
        let verifier = generate_verifier();
        let challenge = compute_challenge(&verifier);
        Self {
            verifier,
            challenge,
            method: "S256",
        }
    }
}

/// Generate a 43-character URL-safe random verifier.
fn generate_verifier() -> String {
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.random::<u8>()).collect();
    URL_SAFE_NO_PAD.encode(&bytes)
}

/// Compute the S256 challenge from a verifier.
fn compute_challenge(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}

/// Generate a random hex string for OAuth state parameter (CSRF protection).
pub fn generate_state() -> String {
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.random::<u8>()).collect();
    hex::encode(&bytes)
}

/// Minimal hex encoding (avoids adding the `hex` crate).
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkce_verifier_length() {
        let pkce = PkceChallenge::generate();
        // Base64url of 32 bytes = 43 chars
        assert_eq!(pkce.verifier.len(), 43);
    }

    #[test]
    fn test_pkce_challenge_is_base64url() {
        let pkce = PkceChallenge::generate();
        // SHA-256 = 32 bytes → base64url = 43 chars
        assert_eq!(pkce.challenge.len(), 43);
        assert!(!pkce.challenge.contains('+'));
        assert!(!pkce.challenge.contains('/'));
        assert!(!pkce.challenge.contains('='));
    }

    #[test]
    fn test_pkce_method_is_s256() {
        let pkce = PkceChallenge::generate();
        assert_eq!(pkce.method, "S256");
    }

    #[test]
    fn test_pkce_challenge_matches_verifier() {
        let pkce = PkceChallenge::generate();
        let recomputed = compute_challenge(&pkce.verifier);
        assert_eq!(pkce.challenge, recomputed);
    }

    #[test]
    fn test_pkce_unique_each_time() {
        let a = PkceChallenge::generate();
        let b = PkceChallenge::generate();
        assert_ne!(a.verifier, b.verifier);
    }

    #[test]
    fn test_state_is_64_hex_chars() {
        let state = generate_state();
        assert_eq!(state.len(), 64);
        assert!(state.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
