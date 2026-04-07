//! theo-marklive — Beautiful markdown wiki viewer.
//!
//! Renders a directory of markdown files into a single self-contained HTML page
//! with sidebar navigation, search, code highlighting, and dark theme.
//!
//! # Usage
//!
//! ```no_run
//! use std::path::Path;
//! use theo_marklive::{render, Config};
//!
//! let html = render(Path::new(".theo/wiki"), Config::default()).unwrap();
//! std::fs::write("wiki.html", html).unwrap();
//! ```

mod parser;
mod sidebar;
mod template;

use std::path::Path;

pub use parser::MarkdownPage;

/// Configuration for rendering.
#[derive(Debug, Clone)]
pub struct Config {
    pub title: String,
    pub search: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            title: "Code Wiki".to_string(),
            search: true,
        }
    }
}

/// Render a directory of markdown files into a single self-contained HTML string.
///
/// Reads all `.md` files from `input_dir` and its subdirectories,
/// generates a sidebar from the structure, and produces one HTML page
/// with inline CSS/JS (no external dependencies).
pub fn render(input_dir: &Path, config: Config) -> Result<String, String> {
    if !input_dir.exists() {
        return Err(format!("Directory not found: {}", input_dir.display()));
    }

    // Parse all markdown files
    let pages = parser::parse_directory(input_dir)?;

    if pages.is_empty() {
        return Err("No markdown files found".to_string());
    }

    // Build sidebar
    let sidebar_html = sidebar::build_sidebar(&pages);

    // Build page content (all pages as hidden divs, JS switches them)
    let pages_html = parser::render_all_pages(&pages);

    // Build search index
    let search_index = if config.search {
        sidebar::build_search_index(&pages)
    } else {
        String::new()
    };

    // Assemble final HTML
    Ok(template::build_html(&config.title, &sidebar_html, &pages_html, &search_index))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn render_empty_dir_errors() {
        let dir = tempfile::tempdir().unwrap();
        let result = render(dir.path(), Config::default());
        assert!(result.is_err());
    }

    #[test]
    fn render_single_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("index.md"), "# Hello\n\nWorld").unwrap();
        let html = render(dir.path(), Config::default()).unwrap();
        assert!(html.contains("Hello"));
        assert!(html.contains("World"));
        assert!(html.contains("<html"));
    }

    #[test]
    fn render_multiple_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("index.md"), "# Index\n\nMain page").unwrap();
        fs::create_dir(dir.path().join("modules")).unwrap();
        fs::write(dir.path().join("modules/auth.md"), "# Auth\n\nAuthentication module").unwrap();
        fs::write(dir.path().join("modules/search.md"), "# Search\n\nSearch engine").unwrap();

        let html = render(dir.path(), Config::default()).unwrap();
        assert!(html.contains("Auth"));
        assert!(html.contains("Search"));
        assert!(html.contains("sidebar"));
    }

    #[test]
    fn render_with_code_blocks() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("index.md"), "# Code\n\n```rust\nfn main() {}\n```").unwrap();
        let html = render(dir.path(), Config::default()).unwrap();
        assert!(html.contains("fn main"));
        assert!(html.contains("<code"));
    }
}
