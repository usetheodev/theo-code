//! Theme system — configurable color schemes for the TUI.
//!
//! Only the theme `name` is read today (surfaced in status and /theme output).
//! Color fields were present but never consumed by the renderer; they were
//! dropped to keep the module honest about what it actually delivers.

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
}

impl Theme {
    pub fn dark() -> Self {
        Self { name: "dark".to_string() }
    }

    pub fn light() -> Self {
        Self { name: "light".to_string() }
    }

    pub fn high_contrast() -> Self {
        Self { name: "high-contrast".to_string() }
    }

    pub fn dracula() -> Self {
        Self { name: "dracula".to_string() }
    }

    pub fn tokyo_night() -> Self {
        Self { name: "tokyo-night".to_string() }
    }

    pub fn all() -> Vec<Self> {
        vec![
            Self::dark(),
            Self::light(),
            Self::high_contrast(),
            Self::dracula(),
            Self::tokyo_night(),
        ]
    }

    pub fn available_themes() -> Vec<String> {
        Self::all().into_iter().map(|t| t.name).collect()
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
