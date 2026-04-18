//! Sidebar navigation and search index generation.

use crate::parser::MarkdownPage;
use std::collections::BTreeMap;

/// Build sidebar HTML with grouped navigation.
/// Root/overview pages always appear FIRST at the top.
pub fn build_sidebar(pages: &[MarkdownPage]) -> String {
    let mut html = String::new();

    // Group pages by directory
    let mut groups: BTreeMap<String, Vec<&MarkdownPage>> = BTreeMap::new();
    for page in pages {
        groups.entry(page.group.clone()).or_default().push(page);
    }

    // Render root group FIRST (overview, architecture, getting-started)
    if let Some(root_pages) = groups.remove("root") {
        html += "<div class=\"nav-group\">\n";
        html += "  <div class=\"nav-group-label\">Overview</div>\n";
        for page in &root_pages {
            let active = if page.slug == "index" || page.slug == "overview" {
                " active"
            } else {
                ""
            };
            html += &format!(
                "  <a class=\"nav-item{}\" onclick=\"showPage('{}')\" data-slug=\"{}\">{}</a>\n",
                active,
                page.slug,
                page.slug,
                truncate_title(&page.title)
            );
        }
        html += "</div>\n";
    }

    // Then render remaining groups alphabetically
    for (group, group_pages) in &groups {
        let group_label = group.replace('/', " / ");

        html += "<div class=\"nav-group\">\n";
        html += &format!("  <div class=\"nav-group-label\">{}</div>\n", group_label);

        for page in group_pages {
            html += &format!(
                "  <a class=\"nav-item\" onclick=\"showPage('{}')\" data-slug=\"{}\">{}</a>\n",
                page.slug,
                page.slug,
                truncate_title(&page.title)
            );
        }

        html += "</div>\n";
    }

    html
}

fn truncate_title(title: &str) -> String {
    if title.len() > 35 {
        format!("{}...", &title[..32])
    } else {
        title.to_string()
    }
}

/// Build search index as JSON for client-side search.
pub fn build_search_index(pages: &[MarkdownPage]) -> String {
    let entries: Vec<serde_json::Value> = pages
        .iter()
        .map(|p| {
            serde_json::json!({
                "slug": p.slug,
                "title": p.title,
                "text": p.plain_text,
            })
        })
        .collect();

    serde_json::to_string(&entries).unwrap_or_else(|_| "[]".to_string())
}
