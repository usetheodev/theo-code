//! Centralized reads of process environment variables.
//!
//! REMEDIATION_PLAN T3.3 — before this module, 17+ call sites across
//! `theo-agent-runtime` read `std::env::var` directly. That scatters
//! policy (prefix conventions, sanitization, defaults), prevents unit
//! tests from mocking the environment, and violates DIP (library code
//! reaching into process-global state).
//!
//! This module:
//! 1. Exposes a small set of free functions for the common reads
//!    (`theo_var`, `home_dir`, `bool_var`, `parse_var`). Every call site
//!    funnels through one of them — grep/audit becomes tractable.
//! 2. Defines the `Environment` trait so future refactors can inject a
//!    mock (see `MapEnvironment` for tests). Default impl delegates to
//!    the process environment.
//!
//! The binary target (`bin/theo-agent.rs`) is allowed to keep
//! `std::env::var` direct — CLI-layer policy, not library concern.

use std::collections::HashMap;
use std::path::PathBuf;

/// Reads a boolean-ish env var. Treats `""`, `"0"`, `"false"`, `"no"`,
/// `"off"` (case-insensitive) as `false`; anything else present as `true`;
/// absence as `default`.
pub fn bool_var(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(raw) => {
            let v = raw.trim().to_ascii_lowercase();
            !matches!(v.as_str(), "" | "0" | "false" | "no" | "off")
        }
        Err(_) => default,
    }
}

/// Reads an env var and parses it via `FromStr`. Returns `None` if
/// the var is absent or parsing fails.
pub fn parse_var<T: std::str::FromStr>(name: &str) -> Option<T> {
    std::env::var(name).ok().and_then(|v| v.parse::<T>().ok())
}

/// Reads an env var and returns it trimmed. Returns `None` if absent
/// or empty after trim.
pub fn theo_var(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(v) => {
            let trimmed = v.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Err(_) => None,
    }
}

/// Return `$HOME` as `PathBuf`, or `None` if the var is missing.
/// Mirrors `crate::user_paths::home_dir` — re-exported here so the
/// single trait implementation below has one canonical source.
pub fn home_dir() -> Option<PathBuf> {
    crate::user_paths::home_dir()
}

/// Trait for environment access. Default impl delegates to the process
/// environment via the free functions above.
///
/// Production callers should accept `&dyn Environment` (or
/// `Arc<dyn Environment>`) rather than reaching into `std::env` directly.
pub trait Environment: Send + Sync {
    fn var(&self, name: &str) -> Option<String>;

    fn bool(&self, name: &str, default: bool) -> bool {
        match self.var(name) {
            Some(raw) => {
                let v = raw.trim().to_ascii_lowercase();
                !matches!(v.as_str(), "" | "0" | "false" | "no" | "off")
            }
            None => default,
        }
    }

    fn home(&self) -> Option<PathBuf> {
        self.var("HOME").map(PathBuf::from)
    }
}

/// Default process-environment implementation. Zero-sized; construct via
/// `SystemEnvironment` and pass as `&dyn Environment`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemEnvironment;

impl Environment for SystemEnvironment {
    fn var(&self, name: &str) -> Option<String> {
        theo_var(name)
    }

    fn home(&self) -> Option<PathBuf> {
        home_dir()
    }
}

/// Test double: look up env vars in a `HashMap` instead of the process
/// env. Callers inject `Arc::new(MapEnvironment::from([("THEO_X", "1")]))`
/// to produce deterministic behaviour in unit tests.
#[derive(Debug, Default, Clone)]
pub struct MapEnvironment {
    vars: HashMap<String, String>,
}

impl MapEnvironment {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.vars.insert(key.into(), value.into());
        self
    }
}

impl<K, V, const N: usize> From<[(K, V); N]> for MapEnvironment
where
    K: Into<String>,
    V: Into<String>,
{
    fn from(arr: [(K, V); N]) -> Self {
        let mut vars = HashMap::with_capacity(N);
        for (k, v) in arr {
            vars.insert(k.into(), v.into());
        }
        Self { vars }
    }
}

impl Environment for MapEnvironment {
    fn var(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theo_var_returns_none_for_empty_trimmed_string() {
        // SAFETY: tests manipulate env; this is test-only isolated.
        unsafe { std::env::set_var("THEO_TEST_EMPTY", "   ") };
        assert_eq!(theo_var("THEO_TEST_EMPTY"), None);
        unsafe { std::env::remove_var("THEO_TEST_EMPTY") };
    }

    #[test]
    fn bool_var_recognizes_falsey_values() {
        for falsey in ["0", "false", "FALSE", "no", "NO", "off", "OFF", ""] {
            // SAFETY: test mutates a uniquely-named THEO_TEST_* env var (no other test reads it), so a data race on it is impossible even without a global lock.
            unsafe { std::env::set_var("THEO_BOOL_FALSEY", falsey) };
            assert!(
                !bool_var("THEO_BOOL_FALSEY", true),
                "expected false for {falsey:?}"
            );
        }
        // SAFETY: test mutates a uniquely-named THEO_TEST_* env var (no other test reads it), so a data race on it is impossible even without a global lock.
        unsafe { std::env::remove_var("THEO_BOOL_FALSEY") };
    }

    #[test]
    fn bool_var_returns_true_for_nonempty_truthy() {
        // SAFETY: test mutates a uniquely-named THEO_TEST_* env var (no other test reads it), so a data race on it is impossible even without a global lock.
        unsafe { std::env::set_var("THEO_BOOL_TRUTHY", "1") };
        assert!(bool_var("THEO_BOOL_TRUTHY", false));
        // SAFETY: test mutates a uniquely-named THEO_TEST_* env var (no other test reads it), so a data race on it is impossible even without a global lock.
        unsafe { std::env::set_var("THEO_BOOL_TRUTHY", "yes") };
        assert!(bool_var("THEO_BOOL_TRUTHY", false));
        // SAFETY: test mutates a uniquely-named THEO_TEST_* env var (no other test reads it), so a data race on it is impossible even without a global lock.
        unsafe { std::env::remove_var("THEO_BOOL_TRUTHY") };
    }

    #[test]
    fn bool_var_returns_default_when_absent() {
        // SAFETY: test mutates a uniquely-named THEO_TEST_* env var (no other test reads it), so a data race on it is impossible even without a global lock.
        unsafe { std::env::remove_var("THEO_BOOL_ABSENT") };
        assert!(bool_var("THEO_BOOL_ABSENT", true));
        assert!(!bool_var("THEO_BOOL_ABSENT", false));
    }

    #[test]
    fn parse_var_parses_integers() {
        // SAFETY: test mutates a uniquely-named THEO_TEST_* env var (no other test reads it), so a data race on it is impossible even without a global lock.
        unsafe { std::env::set_var("THEO_INT_TEST", "42") };
        assert_eq!(parse_var::<u32>("THEO_INT_TEST"), Some(42));
        // SAFETY: test mutates a uniquely-named THEO_TEST_* env var (no other test reads it), so a data race on it is impossible even without a global lock.
        unsafe { std::env::set_var("THEO_INT_TEST", "not-an-int") };
        assert_eq!(parse_var::<u32>("THEO_INT_TEST"), None);
        // SAFETY: test mutates a uniquely-named THEO_TEST_* env var (no other test reads it), so a data race on it is impossible even without a global lock.
        unsafe { std::env::remove_var("THEO_INT_TEST") };
    }

    #[test]
    fn map_environment_round_trips_inserted_keys() {
        let env = MapEnvironment::new()
            .with("FOO", "bar")
            .with("BAZ", "qux");
        assert_eq!(env.var("FOO").as_deref(), Some("bar"));
        assert_eq!(env.var("MISSING"), None);
    }

    #[test]
    fn map_environment_supports_from_array_literal() {
        let env = MapEnvironment::from([("A", "1"), ("B", "2")]);
        assert_eq!(env.var("A").as_deref(), Some("1"));
        assert_eq!(env.var("B").as_deref(), Some("2"));
    }

    #[test]
    fn map_environment_bool_default_semantics() {
        let env = MapEnvironment::from([("SET_TRUE", "1"), ("SET_FALSE", "0")]);
        assert!(env.bool("SET_TRUE", false));
        assert!(!env.bool("SET_FALSE", true));
        assert!(env.bool("ABSENT", true));
        assert!(!env.bool("ABSENT", false));
    }
}
