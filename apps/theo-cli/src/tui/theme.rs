//! Theme system — configurable color schemes for the TUI.
//!
//! Supports dark, light, and high-contrast presets.
//! Can be extended with custom themes via ~/.config/theo/tui.toml.

use ratatui::prelude::*;

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub bg: Color,
    pub fg: Color,
    pub header_fg: Color,
    pub status_fg: Color,
    pub status_bg: Color,
    pub user_fg: Color,
    pub assistant_fg: Color,
    pub tool_border: Color,
    pub tool_running: Color,
    pub tool_success: Color,
    pub tool_failed: Color,
    pub code_fg: Color,
    pub code_bg: Color,
    pub inline_code_fg: Color,
    pub search_highlight: Color,
    pub accent: Color,
    pub dim: Color,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            name: "dark".to_string(),
            bg: Color::Reset,
            fg: Color::White,
            header_fg: Color::Cyan,
            status_fg: Color::DarkGray,
            status_bg: Color::Black,
            user_fg: Color::Cyan,
            assistant_fg: Color::White,
            tool_border: Color::DarkGray,
            tool_running: Color::Yellow,
            tool_success: Color::Green,
            tool_failed: Color::Red,
            code_fg: Color::Green,
            code_bg: Color::Reset,
            inline_code_fg: Color::Yellow,
            search_highlight: Color::Yellow,
            accent: Color::Cyan,
            dim: Color::DarkGray,
        }
    }

    pub fn all() -> Vec<Self> {
        vec![Self::dark(), Self::light(), Self::high_contrast(), Self::dracula(), Self::tokyo_night()]
    }

    pub fn light() -> Self {
        Self {
            name: "light".to_string(),
            bg: Color::White,
            fg: Color::Black,
            header_fg: Color::Blue,
            status_fg: Color::DarkGray,
            status_bg: Color::White,
            user_fg: Color::Blue,
            assistant_fg: Color::Black,
            tool_border: Color::Gray,
            tool_running: Color::Yellow,
            tool_success: Color::Green,
            tool_failed: Color::Red,
            code_fg: Color::DarkGray,
            code_bg: Color::White,
            inline_code_fg: Color::Magenta,
            search_highlight: Color::Yellow,
            accent: Color::Blue,
            dim: Color::Gray,
        }
    }

    pub fn high_contrast() -> Self {
        Self {
            name: "high-contrast".to_string(),
            bg: Color::Black,
            fg: Color::White,
            header_fg: Color::LightCyan,
            status_fg: Color::White,
            status_bg: Color::DarkGray,
            user_fg: Color::LightCyan,
            assistant_fg: Color::White,
            tool_border: Color::White,
            tool_running: Color::LightYellow,
            tool_success: Color::LightGreen,
            tool_failed: Color::LightRed,
            code_fg: Color::LightGreen,
            code_bg: Color::Black,
            inline_code_fg: Color::LightYellow,
            search_highlight: Color::LightYellow,
            accent: Color::LightCyan,
            dim: Color::Gray,
        }
    }

    pub fn dracula() -> Self {
        Self {
            name: "dracula".to_string(),
            bg: Color::Rgb(40, 42, 54),
            fg: Color::Rgb(248, 248, 242),
            header_fg: Color::Rgb(139, 233, 253),
            status_fg: Color::Rgb(98, 114, 164),
            status_bg: Color::Rgb(40, 42, 54),
            user_fg: Color::Rgb(139, 233, 253),
            assistant_fg: Color::Rgb(248, 248, 242),
            tool_border: Color::Rgb(68, 71, 90),
            tool_running: Color::Rgb(241, 250, 140),
            tool_success: Color::Rgb(80, 250, 123),
            tool_failed: Color::Rgb(255, 85, 85),
            code_fg: Color::Rgb(80, 250, 123),
            code_bg: Color::Rgb(40, 42, 54),
            inline_code_fg: Color::Rgb(241, 250, 140),
            search_highlight: Color::Rgb(241, 250, 140),
            accent: Color::Rgb(189, 147, 249),
            dim: Color::Rgb(98, 114, 164),
        }
    }

    pub fn tokyo_night() -> Self {
        Self {
            name: "tokyo-night".to_string(),
            bg: Color::Rgb(26, 27, 38),
            fg: Color::Rgb(169, 177, 214),
            header_fg: Color::Rgb(125, 207, 255),
            status_fg: Color::Rgb(86, 95, 137),
            status_bg: Color::Rgb(26, 27, 38),
            user_fg: Color::Rgb(125, 207, 255),
            assistant_fg: Color::Rgb(169, 177, 214),
            tool_border: Color::Rgb(59, 66, 97),
            tool_running: Color::Rgb(224, 175, 104),
            tool_success: Color::Rgb(158, 206, 106),
            tool_failed: Color::Rgb(247, 118, 142),
            code_fg: Color::Rgb(158, 206, 106),
            code_bg: Color::Rgb(26, 27, 38),
            inline_code_fg: Color::Rgb(224, 175, 104),
            search_highlight: Color::Rgb(224, 175, 104),
            accent: Color::Rgb(187, 154, 247),
            dim: Color::Rgb(86, 95, 137),
        }
    }

    pub fn available_themes() -> Vec<String> {
        vec![
            "dark".to_string(),
            "light".to_string(),
            "high-contrast".to_string(),
            "dracula".to_string(),
            "tokyo-night".to_string(),
        ]
    }

    pub fn by_name(name: &str) -> Self {
        match name {
            "light" => Self::light(),
            "high-contrast" => Self::high_contrast(),
            "dracula" => Self::dracula(),
            "tokyo-night" => Self::tokyo_night(),
            _ => Self::dark(),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_themes_construct() {
        for name in Theme::available_themes() {
            let theme = Theme::by_name(&name);
            assert_eq!(theme.name, name);
        }
    }

    #[test]
    fn default_is_dark() {
        let theme = Theme::default();
        assert_eq!(theme.name, "dark");
    }

    #[test]
    fn unknown_theme_falls_back_to_dark() {
        let theme = Theme::by_name("nonexistent");
        assert_eq!(theme.name, "dark");
    }
}
