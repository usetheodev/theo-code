//! Sidebar navigation and search index generation.

use crate::parser::MarkdownPage;
use std::collections::BTreeMap;

/// Build sidebar HTML with grouped navigation.
pub fn build_sidebar(pages: &[MarkdownPage]) -> String {
    let mut html = String::new();

    // Group pages by directory
    let mut groups: BTreeMap<String, Vec<&MarkdownPage>> = BTreeMap::new();
    for page in pages {
        groups.entry(page.group.clone()).or_default().push(page);
    }

    // Render groups
    for (group, group_pages) in &groups {
        let group_label = if group == "root" {
            "Overview".to_string()
        } else {
            group.replace('/', " / ")
        };

        html += &format!("<div class=\"nav-group\">\n");
        html += &format!("  <div class=\"nav-group-label\">{}</div>\n", group_label);

        for page in group_pages {
            let active = if page.slug == "index" { " active" } else { "" };
            let title_short = if page.title.len() > 35 {
                format!("{}...", &page.title[..32])
            } else {
                page.title.clone()
            };

            html += &format!(
                "  <a class=\"nav-item{}\" onclick=\"showPage('{}')\" data-slug=\"{}\">{}</a>\n",
                active, page.slug, page.slug, title_short
            );
        }

        html += "</div>\n";
    }

    html
}

/// Build search index as JSON for client-side search.
pub fn build_search_index(pages: &[MarkdownPage]) -> String {
    let entries: Vec<serde_json::Value> = pages.iter().map(|p| {
        serde_json::json!({
            "slug": p.slug,
            "title": p.title,
            "text": p.plain_text,
        })
    }).collect();

    serde_json::to_string(&entries).unwrap_or_else(|_| "[]".to_string())
}
