//! Persistent configuration for theo-cli.
//!
//! Loads `config.toml` from `$XDG_CONFIG_HOME/theo/` with fallback
//! defaults. Respects `THEO_HOME` env var for test isolation.
//!
//! See `docs/adr/ADR-003-xdg-paths.md` for the XDG rationale.

pub mod paths;

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use paths::TheoPaths;

/// Current config schema version. Bump when incompatible changes land.
pub const CURRENT_CONFIG_VERSION: u32 = 1;

/// The top-level theo-cli config.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct TheoConfig {
    /// Schema version. Must match `CURRENT_CONFIG_VERSION` or be migratable.
    pub config_version: u32,
    /// Default LLM model (e.g. "gpt-4", "claude-sonnet-4-6").
    pub model: Option<String>,
    /// Default LLM provider (e.g. "openai", "anthropic").
    pub provider: Option<String>,
    /// Default agent mode ("agent", "plan", or "ask").
    pub mode: String,
    /// Maximum agent loop iterations.
    pub max_iterations: usize,
    /// Maximum messages kept in a session before compaction.
    pub session_max_messages: usize,
    /// Permission mode ("interactive", "auto-accept", "deny-all").
    pub permission_mode: String,
    /// Color theme name for syntect highlighting.
    pub theme: String,
    /// Truncation widths per tool render.
    pub truncation: TruncationLimits,
}

/// Per-tool truncation widths.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct TruncationLimits {
    pub path: usize,
    pub command: usize,
    pub content: usize,
}

impl Default for TruncationLimits {
    fn default() -> Self {
        Self {
            path: 80,
            command: 70,
            content: 78,
        }
    }
}

impl Default for TheoConfig {
    fn default() -> Self {
        Self {
            config_version: CURRENT_CONFIG_VERSION,
            model: None,
            provider: None,
            mode: "agent".to_string(),
            max_iterations: 10,
            session_max_messages: 100,
            permission_mode: "interactive".to_string(),
            theme: "base16-ocean.dark".to_string(),
            truncation: TruncationLimits::default(),
        }
    }
}

/// Errors from config loading / parsing.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("io error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error(
        "config schema version mismatch at {path}: file has v{found}, this build expects v{expected}"
    )]
    VersionMismatch {
        path: PathBuf,
        found: u32,
        expected: u32,
    },
}

impl TheoConfig {
    /// Load from a specific path. Returns defaults if the file does not exist.
    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let bytes = fs::read_to_string(path).map_err(|e| ConfigError::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        let cfg: TheoConfig = toml::from_str(&bytes).map_err(|e| ConfigError::Parse {
            path: path.to_path_buf(),
            source: e,
        })?;
        if cfg.config_version != CURRENT_CONFIG_VERSION {
            return Err(ConfigError::VersionMismatch {
                path: path.to_path_buf(),
                found: cfg.config_version,
                expected: CURRENT_CONFIG_VERSION,
            });
        }
        Ok(cfg)
    }

    /// Serialize to a TOML string.
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_defaults_are_sensible() {
        let c = TheoConfig::default();
        assert_eq!(c.config_version, CURRENT_CONFIG_VERSION);
        assert_eq!(c.mode, "agent");
        assert_eq!(c.max_iterations, 10);
        assert_eq!(c.session_max_messages, 100);
        assert_eq!(c.permission_mode, "interactive");
        assert_eq!(c.theme, "base16-ocean.dark");
        assert_eq!(c.truncation.path, 80);
    }

    #[test]
    fn test_truncation_defaults() {
        let t = TruncationLimits::default();
        assert_eq!(t.path, 80);
        assert_eq!(t.command, 70);
        assert_eq!(t.content, 78);
    }

    #[test]
    fn test_load_missing_file_returns_defaults() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nonexistent.toml");
        let cfg = TheoConfig::load_from(&path).unwrap();
        assert_eq!(cfg, TheoConfig::default());
    }

    #[test]
    fn test_load_valid_file_parses_fields() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
config_version = 1
model = "gpt-4"
provider = "openai"
mode = "plan"
max_iterations = 42
session_max_messages = 50
permission_mode = "auto-accept"
theme = "Solarized (dark)"

[truncation]
path = 100
command = 90
content = 120
"#,
        )
        .unwrap();
        let cfg = TheoConfig::load_from(&path).unwrap();
        assert_eq!(cfg.model.as_deref(), Some("gpt-4"));
        assert_eq!(cfg.provider.as_deref(), Some("openai"));
        assert_eq!(cfg.mode, "plan");
        assert_eq!(cfg.max_iterations, 42);
        assert_eq!(cfg.session_max_messages, 50);
        assert_eq!(cfg.permission_mode, "auto-accept");
        assert_eq!(cfg.theme, "Solarized (dark)");
        assert_eq!(cfg.truncation.path, 100);
    }

    #[test]
    fn test_load_partial_file_fills_defaults() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("partial.toml");
        fs::write(&path, "config_version = 1\nmodel = \"claude-opus\"\n").unwrap();
        let cfg = TheoConfig::load_from(&path).unwrap();
        assert_eq!(cfg.model.as_deref(), Some("claude-opus"));
        // Defaults preserved
        assert_eq!(cfg.mode, "agent");
        assert_eq!(cfg.max_iterations, 10);
    }

    #[test]
    fn test_load_corrupt_file_returns_parse_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bad.toml");
        fs::write(&path, "this is not valid toml ====").unwrap();
        let err = TheoConfig::load_from(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
    }

    #[test]
    fn test_load_wrong_version_returns_version_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("old.toml");
        fs::write(&path, "config_version = 999\n").unwrap();
        let err = TheoConfig::load_from(&path).unwrap_err();
        assert!(matches!(err, ConfigError::VersionMismatch { found: 999, .. }));
    }

    #[test]
    fn test_load_unknown_field_rejected() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("alien.toml");
        fs::write(&path, "config_version = 1\nalien_field = 42\n").unwrap();
        let err = TheoConfig::load_from(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
    }

    #[test]
    fn test_to_toml_roundtrip() {
        let cfg = TheoConfig {
            model: Some("gpt-4".to_string()),
            max_iterations: 25,
            ..TheoConfig::default()
        };
        let s = cfg.to_toml();
        let parsed: TheoConfig = toml::from_str(&s).unwrap();
        assert_eq!(cfg, parsed);
    }
}
