//! Bounded JSON deserialization helpers (T2.7).
//!
//! `serde_json::from_str` and friends are unbounded: a malicious or
//! misconfigured producer (upstream LLM provider, wiki export pipeline,
//! tooling consumer) can force the parser to allocate hundreds of MB
//! before the caller realises the input is oversized.
//!
//! This module centralises a size-checked wrapper:
//!
//! ```ignore
//! use theo_domain::safe_json::{from_str_bounded, DEFAULT_JSON_LIMIT};
//!
//! let dto: MyDto = from_str_bounded(payload, DEFAULT_JSON_LIMIT)?;
//! ```
//!
//! The wrapper short-circuits with `SafeJsonError::PayloadTooLarge`
//! **before** calling into `serde_json`, so the memory cost is O(1) for
//! oversized payloads.
//!
//! Callers that legitimately accept larger payloads (batch imports,
//! snapshots) should pick a higher explicit limit rather than calling the
//! unbounded `serde_json` API.

use serde::de::DeserializeOwned;
use thiserror::Error;

/// Default upper bound used by [`from_str_bounded`] / [`from_slice_bounded`]
/// callers that do not set their own limit.
///
/// 10 MiB matches the documented upper bound for our LLM streaming chunks
/// and is within an order of magnitude of every non-archive payload the
/// workspace produces. Bumping this constant intentionally widens attack
/// surface — prefer passing a per-call limit where possible.
pub const DEFAULT_JSON_LIMIT: usize = 10 * 1024 * 1024;

/// Errors surfaced by the bounded-JSON helpers.
#[derive(Debug, Error)]
pub enum SafeJsonError {
    /// The input is larger than the caller-specified limit.
    #[error("JSON payload too large: {actual} bytes exceeds limit {limit} bytes")]
    PayloadTooLarge {
        actual: usize,
        limit: usize,
    },

    /// `serde_json` failed to deserialize the (size-bounded) payload.
    #[error("JSON parse error: {0}")]
    Parse(#[from] serde_json::Error),
}

/// Deserialize `T` from a string, rejecting payloads larger than `max_bytes`.
///
/// Checks the byte length before handing off to `serde_json`, so an
/// oversized payload costs O(1).
pub fn from_str_bounded<T>(input: &str, max_bytes: usize) -> Result<T, SafeJsonError>
where
    T: DeserializeOwned,
{
    let len = input.len();
    if len > max_bytes {
        return Err(SafeJsonError::PayloadTooLarge {
            actual: len,
            limit: max_bytes,
        });
    }
    Ok(serde_json::from_str(input)?)
}

/// Same as [`from_str_bounded`] but accepts a byte slice.
pub fn from_slice_bounded<T>(input: &[u8], max_bytes: usize) -> Result<T, SafeJsonError>
where
    T: DeserializeOwned,
{
    let len = input.len();
    if len > max_bytes {
        return Err(SafeJsonError::PayloadTooLarge {
            actual: len,
            limit: max_bytes,
        });
    }
    Ok(serde_json::from_slice(input)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Sample {
        name: String,
        n: u32,
    }

    #[test]
    fn rejects_payload_larger_than_limit() {
        let payload = r#"{"name":"x","n":1}"#;
        let err = from_str_bounded::<Sample>(payload, 5).unwrap_err();
        match err {
            SafeJsonError::PayloadTooLarge { actual, limit } => {
                assert_eq!(limit, 5);
                assert_eq!(actual, payload.len());
            }
            other => panic!("expected PayloadTooLarge, got {other:?}"),
        }
    }

    #[test]
    fn accepts_payload_exactly_at_limit() {
        let payload = r#"{"name":"x","n":1}"#;
        let result: Sample = from_str_bounded(payload, payload.len()).unwrap();
        assert_eq!(
            result,
            Sample {
                name: "x".into(),
                n: 1
            }
        );
    }

    #[test]
    fn accepts_payload_under_limit() {
        let payload = r#"{"name":"theo","n":42}"#;
        let result: Sample = from_str_bounded(payload, 1024).unwrap();
        assert_eq!(result.n, 42);
    }

    #[test]
    fn parse_error_bubbles_up_when_size_ok() {
        let err = from_str_bounded::<Sample>("not json", 1024).unwrap_err();
        assert!(matches!(err, SafeJsonError::Parse(_)));
    }

    #[test]
    fn from_slice_bounded_rejects_oversized() {
        let payload = br#"{"name":"x","n":1}"#;
        let err = from_slice_bounded::<Sample>(payload, 3).unwrap_err();
        assert!(matches!(err, SafeJsonError::PayloadTooLarge { .. }));
    }

    #[test]
    fn from_slice_bounded_accepts_payload_under_limit() {
        let payload = br#"{"name":"y","n":7}"#;
        let result: Sample = from_slice_bounded(payload, DEFAULT_JSON_LIMIT).unwrap();
        assert_eq!(result.name, "y");
    }

    #[test]
    fn default_limit_accepts_reasonable_payload() {
        // Emit ~1 MiB of valid JSON and verify the default limit accepts it.
        let payload = format!(r#"{{"name":"{}","n":1}}"#, "a".repeat(1024 * 1024 - 50));
        let result: Sample = from_str_bounded(&payload, DEFAULT_JSON_LIMIT).unwrap();
        assert_eq!(result.n, 1);
    }

    #[test]
    fn payload_of_exactly_default_limit_plus_one_is_rejected() {
        let payload = "a".repeat(DEFAULT_JSON_LIMIT + 1);
        let err = from_str_bounded::<serde_json::Value>(&payload, DEFAULT_JSON_LIMIT).unwrap_err();
        match err {
            SafeJsonError::PayloadTooLarge { actual, limit } => {
                assert_eq!(actual, DEFAULT_JSON_LIMIT + 1);
                assert_eq!(limit, DEFAULT_JSON_LIMIT);
            }
            other => panic!("expected PayloadTooLarge, got {other:?}"),
        }
    }
}
