//! Resolve per-user config paths without falling back to `/tmp`.
//!
//! Previously four sites (`run_engine.rs`, `plugin.rs`, `hooks.rs`,
//! `memory_lifecycle.rs`) each read `HOME` and defaulted to `/tmp` on
//! failure. That is a TOCTOU / privilege-escalation vector on containers
//! without `HOME`: `/tmp/.config/theo/**` is world-writable and shared
//! across processes.
//!
//! This module centralizes the lookup and returns `None` on failure —
//! callers must decide whether to skip the feature (preferred) or return
//! an error.

use std::path::PathBuf;

/// Return `$HOME` as a `PathBuf`, or `None` if the env var is missing.
/// Does NOT fall back to `/tmp`.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Return `$HOME/.config/theo` if `HOME` is set, else `None`.
pub fn theo_config_dir() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".config").join("theo"))
}

/// Return `$HOME/.config/theo/<subdir>` if `HOME` is set, else `None`.
pub fn theo_config_subdir(subdir: &str) -> Option<PathBuf> {
    theo_config_dir().map(|d| d.join(subdir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theo_config_dir_matches_home_plus_config_theo() {
        // Guard against missing HOME in CI sandboxes.
        if let Some(home) = home_dir() {
            let expected = home.join(".config").join("theo");
            assert_eq!(theo_config_dir().unwrap(), expected);
        }
    }

    #[test]
    fn theo_config_subdir_appends_given_name() {
        if let Some(home) = home_dir() {
            let expected = home.join(".config").join("theo").join("skills");
            assert_eq!(theo_config_subdir("skills").unwrap(), expected);
        }
    }
}
