//! XDG-compliant path resolution.
//!
//! See `docs/adr/ADR-003-xdg-paths.md`.

use std::path::PathBuf;

/// Resolved directories for theo-cli.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TheoPaths {
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
}

impl TheoPaths {
    /// Resolve using XDG env vars / `dirs` fallback.
    /// Honors `THEO_HOME` to force a single root (for tests, Docker, CI).
    pub fn resolve() -> Self {
        if let Ok(home) = std::env::var("THEO_HOME") {
            return Self::rooted(PathBuf::from(home));
        }
        Self::from_dirs()
    }

    /// Build from `dirs` crate (Linux/macOS/Windows defaults).
    pub fn from_dirs() -> Self {
        Self {
            config: dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("theo"),
            data: dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("theo"),
            cache: dirs::cache_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("theo"),
        }
    }

    /// Build from a single root directory (for `THEO_HOME` or tests).
    pub fn rooted(root: PathBuf) -> Self {
        Self {
            config: root.join("config"),
            data: root.join("data"),
            cache: root.join("cache"),
        }
    }

    pub fn sessions(&self) -> PathBuf {
        self.data.join("sessions")
    }
    pub fn memory(&self) -> PathBuf {
        self.data.join("memory")
    }
    pub fn skills(&self) -> PathBuf {
        self.data.join("skills")
    }
    pub fn config_file(&self) -> PathBuf {
        self.config.join("config.toml")
    }
    pub fn syntect_cache(&self) -> PathBuf {
        self.cache.join("syntect")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rooted_uses_explicit_root() {
        let p = TheoPaths::rooted(PathBuf::from("/tmp/theo-test"));
        assert_eq!(p.config, PathBuf::from("/tmp/theo-test/config"));
        assert_eq!(p.data, PathBuf::from("/tmp/theo-test/data"));
        assert_eq!(p.cache, PathBuf::from("/tmp/theo-test/cache"));
    }

    #[test]
    fn test_rooted_derives_all_paths() {
        let p = TheoPaths::rooted(PathBuf::from("/root"));
        assert_eq!(p.sessions(), PathBuf::from("/root/data/sessions"));
        assert_eq!(p.memory(), PathBuf::from("/root/data/memory"));
        assert_eq!(p.skills(), PathBuf::from("/root/data/skills"));
        assert_eq!(p.config_file(), PathBuf::from("/root/config/config.toml"));
        assert_eq!(p.syntect_cache(), PathBuf::from("/root/cache/syntect"));
    }

    #[test]
    fn test_from_dirs_paths_end_with_theo() {
        let p = TheoPaths::from_dirs();
        assert!(p.config.ends_with("theo"));
        assert!(p.data.ends_with("theo"));
        assert!(p.cache.ends_with("theo"));
    }

    #[test]
    fn test_paths_equality() {
        let a = TheoPaths::rooted(PathBuf::from("/x"));
        let b = TheoPaths::rooted(PathBuf::from("/x"));
        assert_eq!(a, b);
    }
}
