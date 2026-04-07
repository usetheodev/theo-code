//! Markdown parser: reads .md files and converts to HTML fragments.

use pulldown_cmark::{html, Options, Parser};
use std::path::Path;

/// A parsed markdown page.
#[derive(Debug, Clone)]
pub struct MarkdownPage {
    /// Unique slug (used as div id and nav target).
    pub slug: String,
    /// Display title (from first # heading or filename).
    pub title: String,
    /// Relative path from input dir.
    pub rel_path: String,
    /// HTML content (rendered from markdown).
    pub html_content: String,
    /// Plain text (for search index).
    pub plain_text: String,
    /// Nesting group (subdirectory name, or "root").
    pub group: String,
}

/// Parse all .md files in a directory (recursive).
pub fn parse_directory(dir: &Path) -> Result<Vec<MarkdownPage>, String> {
    let mut pages = Vec::new();
    collect_md_files(dir, dir, &mut pages)?;

    // Sort: index.md first, then alphabetically
    pages.sort_by(|a, b| {
        let a_idx = a.slug == "index";
        let b_idx = b.slug == "index";
        b_idx.cmp(&a_idx).then(a.group.cmp(&b.group)).then(a.title.cmp(&b.title))
    });

    Ok(pages)
}

fn collect_md_files(base: &Path, dir: &Path, pages: &mut Vec<MarkdownPage>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("Cannot read {}: {}", dir.display(), e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_md_files(base, &path, pages)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            // Skip enriched duplicates
            if path.to_string_lossy().contains(".enriched.") {
                continue;
            }
            if let Some(page) = parse_file(base, &path) {
                pages.push(page);
            }
        }
    }
    Ok(())
}

fn parse_file(base: &Path, path: &Path) -> Option<MarkdownPage> {
    let content = std::fs::read_to_string(path).ok()?;
    let rel_path = path.strip_prefix(base).ok()?.to_string_lossy().to_string();

    // Slug from filename
    let slug = path.file_stem()?.to_string_lossy().to_string();

    // Group from parent directory
    let group = path.parent()
        .and_then(|p| p.strip_prefix(base).ok())
        .and_then(|p| p.to_str())
        .unwrap_or("")
        .to_string();
    let group = if group.is_empty() { "root".to_string() } else { group };

    // Extract title from first heading
    let title = content.lines()
        .find(|l| l.starts_with("# "))
        .map(|l| l.trim_start_matches("# ").trim().to_string())
        .unwrap_or_else(|| slug.clone());

    // Convert markdown to HTML
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(&content, opts);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);

    // Resolve [[wiki-links]]
    html_output = resolve_wiki_links(&html_output);

    // Plain text for search
    let plain_text = content
        .lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with("```") && !l.starts_with('>') && !l.starts_with('|'))
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(500)
        .collect();

    Some(MarkdownPage {
        slug,
        title,
        rel_path,
        html_content: html_output,
        plain_text,
        group,
    })
}

/// Resolve [[wiki-links]] to clickable navigation.
fn resolve_wiki_links(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let bytes = html.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'[' && bytes[i + 1] == b'[' {
            if let Some(end) = html[i + 2..].find("]]") {
                let inner = &html[i + 2..i + 2 + end];
                let parts: Vec<&str> = inner.splitn(2, '|').collect();

                let (slug, title) = if parts.len() == 2 {
                    (parts[0].trim(), parts[1].trim())
                } else {
                    (parts[0].trim(), parts[0].trim())
                };

                result.push_str(&format!(
                    "<a class=\"wiki-link\" onclick=\"showPage('{}')\">{}</a>",
                    slug, title
                ));
                i = i + 2 + end + 2;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

/// Render all pages as hidden divs (JS switches visibility).
pub fn render_all_pages(pages: &[MarkdownPage]) -> String {
    let mut html = String::new();

    for (idx, page) in pages.iter().enumerate() {
        let display = if idx == 0 { "block" } else { "none" };
        html += &format!(
            "<article id=\"page-{}\" class=\"page\" style=\"display:{}\">\n{}\n</article>\n",
            page.slug, display, page.html_content
        );
    }

    html
}
