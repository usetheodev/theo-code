//! Persistent TUI configuration — ~/.config/theo/tui.toml
//!
//! Stores theme, keybinds, and display preferences.

#![allow(dead_code)] // Scaffolded helpers — kept for upcoming TUI features.
use std::path::PathBuf;

/// TUI configuration loaded from disk.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TuiConfig {
    pub theme: String,
    pub sidebar_visible: bool,
    pub show_tokens: bool,
    pub fps: u16,
    pub max_scroll_buffer: usize,
    pub keybinds: KeybindConfig,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            sidebar_visible: true,
            show_tokens: true,
            fps: 30,
            max_scroll_buffer: 10_000,
            keybinds: KeybindConfig::default(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct KeybindConfig {
    pub quit: String,
    pub search: String,
    pub help: String,
    pub new_tab: String,
    pub close_tab: String,
    pub sidebar_toggle: String,
    pub mode_cycle: String,
    pub model_picker: String,
    pub export: String,
}

impl Default for KeybindConfig {
    fn default() -> Self {
        Self {
            quit: "ctrl+c".to_string(),
            search: "ctrl+f".to_string(),
            help: "esc".to_string(),
            new_tab: "ctrl+t".to_string(),
            close_tab: "ctrl+w".to_string(),
            sidebar_toggle: "tab".to_string(),
            mode_cycle: "shift+tab".to_string(),
            model_picker: "ctrl+m".to_string(),
            export: "/export".to_string(),
        }
    }
}

impl TuiConfig {
    /// Load config from ~/.config/theo/tui.toml, creating defaults if missing.
    pub fn load() -> Self {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(content) => toml::from_str(&content).unwrap_or_default(),
            Err(_) => {
                let config = Self::default();
                config.save(); // Create default file
                config
            }
        }
    }

    /// Save config to disk.
    pub fn save(&self) {
        let path = config_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(toml) = toml::to_string_pretty(self) {
            let _ = std::fs::write(&path, toml);
        }
    }

    /// Get theme by name from config.
    pub fn resolve_theme(&self) -> super::theme::Theme {
        match self.theme.as_str() {
            "light" => super::theme::Theme::light(),
            "high_contrast" | "high-contrast" => super::theme::Theme::high_contrast(),
            "dracula" => super::theme::Theme::dracula(),
            _ => super::theme::Theme::dark(),
        }
    }
}

fn config_path() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
        .join(".config")
        .join("theo")
        .join("tui.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_serializes() {
        let config = TuiConfig::default();
        let toml = toml::to_string_pretty(&config).unwrap();
        assert!(toml.contains("theme"));
        assert!(toml.contains("dark"));
    }

    #[test]
    fn config_roundtrips() {
        let config = TuiConfig::default();
        let toml = toml::to_string_pretty(&config).unwrap();
        let back: TuiConfig = toml::from_str(&toml).unwrap();
        assert_eq!(back.theme, "dark");
        assert_eq!(back.fps, 30);
    }

    #[test]
    fn resolve_theme_dark() {
        let config = TuiConfig { theme: "dark".into(), ..Default::default() };
        let theme = config.resolve_theme();
        assert_eq!(theme.name, "dark");
    }

    #[test]
    fn resolve_theme_unknown_falls_back() {
        let config = TuiConfig { theme: "nonexistent".into(), ..Default::default() };
        let theme = config.resolve_theme();
        assert_eq!(theme.name, "dark");
    }
}
