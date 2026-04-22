//! Markdown parser: reads .md files and converts to HTML fragments.

use pulldown_cmark::{Options, Parser, html};
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
        b_idx
            .cmp(&a_idx)
            .then(a.group.cmp(&b.group))
            .then(a.title.cmp(&b.title))
    });

    Ok(pages)
}

fn collect_md_files(base: &Path, dir: &Path, pages: &mut Vec<MarkdownPage>) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("Cannot read {}: {}", dir.display(), e))?;

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
    let raw = std::fs::read_to_string(path).ok()?;
    let without_fence = strip_wrapping_code_fence(&raw);
    let content = strip_yaml_frontmatter(&without_fence);
    let rel_path = path.strip_prefix(base).ok()?.to_string_lossy().to_string();

    // Slug from filename
    let slug = path.file_stem()?.to_string_lossy().to_string();

    // Group from parent directory
    let group = path
        .parent()
        .and_then(|p| p.strip_prefix(base).ok())
        .and_then(|p| p.to_str())
        .unwrap_or("")
        .to_string();
    let group = if group.is_empty() {
        "root".to_string()
    } else {
        group
    };

    // Extract title from first heading
    let title = content
        .lines()
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
        .filter(|l| {
            !l.starts_with('#')
                && !l.starts_with("```")
                && !l.starts_with('>')
                && !l.starts_with('|')
        })
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

/// Strip wrapping code fences that LLMs often add (```markdown ... ```).
fn strip_wrapping_code_fence(content: &str) -> String {
    let trimmed = content.trim();
    // Check if the entire file is wrapped in ```markdown or ```md
    if (trimmed.starts_with("```markdown") || trimmed.starts_with("```md"))
        && trimmed.ends_with("```")
    {
        let first_newline = trimmed.find('\n').unwrap_or(0);
        let inner = &trimmed[first_newline + 1..trimmed.len() - 3];
        return inner.trim().to_string();
    }
    content.to_string()
}

/// Strip YAML frontmatter (---\n...\n---\n) from markdown content.
fn strip_yaml_frontmatter(content: &str) -> String {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return content.to_string();
    }
    let after_open = &trimmed[3..];
    if let Some(close_pos) = after_open.find("\n---") {
        let rest = &after_open[close_pos + 4..];
        return rest.trim_start_matches('\n').to_string();
    }
    content.to_string()
}

/// Resolve [[wiki-links]] to clickable navigation.
fn resolve_wiki_links(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let bytes = html.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'[' && bytes[i + 1] == b'['
            && let Some(end) = html[i + 2..].find("]]") {
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
        let content = if is_overview_page(page) {
            transform_overview_html(&page.html_content)
        } else if is_architecture_page(page) {
            transform_mermaid_blocks(&page.html_content)
        } else {
            page.html_content.clone()
        };
        html += &format!(
            "<article id=\"page-{}\" class=\"page\" style=\"display:{}\">\n{}\n</article>\n",
            page.slug, display, content
        );
    }

    html
}

/// Detect if this is the overview/index page.
fn is_overview_page(page: &MarkdownPage) -> bool {
    page.slug == "overview"
        || page.slug == "index"
        || page.title.contains("Project Overview")
        || page.title.contains("Overview")
}

/// Detect architecture page.
fn is_architecture_page(page: &MarkdownPage) -> bool {
    page.slug == "architecture" || page.title.contains("Architecture")
}

/// Transform mermaid code blocks into renderable divs.
fn transform_mermaid_blocks(html: &str) -> String {
    // Replace <pre><code class="language-mermaid">...</code></pre> with mermaid div
    let mut result = html.to_string();

    // Pattern: <code class="language-mermaid">CONTENT</code>
    while let Some(start) = result.find("<code class=\"language-mermaid\">") {
        let code_start = start + "<code class=\"language-mermaid\">".len();
        if let Some(end) = result[code_start..].find("</code>") {
            let mermaid_content = &result[code_start..code_start + end];
            let decoded = html_decode(mermaid_content);

            // Find the wrapping <pre> tag
            let pre_start = result[..start].rfind("<pre>").unwrap_or(start);
            let post_code = code_start + end + "</code>".len();
            let pre_end = if result[post_code..].starts_with("</pre>") {
                post_code + "</pre>".len()
            } else {
                post_code
            };

            let replacement = format!(
                "<div class=\"mermaid-container\"><div class=\"mermaid\">\n{}\n</div></div>",
                decoded
            );
            result = format!(
                "{}{}{}",
                &result[..pre_start],
                replacement,
                &result[pre_end..]
            );
        } else {
            break;
        }
    }

    result
}

/// Decode basic HTML entities for mermaid content.
fn html_decode(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
}

/// Transform overview HTML into visual components.
fn transform_overview_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len() * 2);

    // Split by <h2> sections
    let sections = split_by_h2(html);

    // Hero = intro (h1) + "What is" section merged together
    let mut hero_parts = String::new();
    let mut skip_next = false;

    if let Some(intro) = sections.first() {
        hero_parts += intro;
    }

    // Check if second section is "What is..." — merge into hero
    if sections.len() > 1 {
        let second_lower = sections[1].to_lowercase();
        if second_lower.contains("what is") || second_lower.contains("about") {
            // Extract description paragraph, add as hero subtitle
            let desc = extract_paragraphs(&sections[1]);
            hero_parts += &format!("<div class=\"hero-subtitle\">{}</div>\n", desc);
            skip_next = true;
        }
    }

    out += "<div class=\"overview-hero\">\n";
    out += &hero_parts;
    out += "</div>\n";

    // Process remaining sections
    let start_idx = if skip_next { 2 } else { 1 };
    for section in sections.iter().skip(start_idx) {
        let lower = section.to_lowercase();
        if lower.contains("key features") || lower.contains("features") {
            out += &transform_features_section(section);
        } else if lower.contains("architecture") || lower.contains("diagram") {
            out += "<div class=\"overview-section\">\n";
            out += &transform_mermaid_blocks(section);
            out += "\n</div>\n";
        } else if lower.contains("quick links") || lower.contains("links") {
            out += &transform_quick_links_section(section);
        } else {
            out += "<div class=\"overview-section\">\n";
            out += section;
            out += "\n</div>\n";
        }
    }

    out
}

/// Extract paragraph text from an HTML section (strip h2 header).
fn extract_paragraphs(section: &str) -> String {
    let mut result = String::new();
    for line in section.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("<h2") || trimmed.starts_with("</h2") {
            continue;
        }
        if trimmed.starts_with("<p>") {
            // Strip <p> tags and keep content
            let content = trimmed.trim_start_matches("<p>").trim_end_matches("</p>");
            result += content;
            result += " ";
        }
    }
    result.trim().to_string()
}

/// Split HTML content by <h2> headers, keeping the h2 with its section.
fn split_by_h2(html: &str) -> Vec<String> {
    let mut sections = Vec::new();
    let mut current = String::new();
    let mut first = true;

    for line in html.lines() {
        if line.starts_with("<h2") && !first {
            if !current.trim().is_empty() {
                sections.push(current);
            }
            current = String::new();
        }
        first = false;
        current += line;
        current += "\n";
    }

    if !current.trim().is_empty() {
        sections.push(current);
    }

    sections
}

/// Transform a "Key Features" bullet list into feature cards.
fn transform_features_section(section: &str) -> String {
    let mut out = String::new();
    out += "<div class=\"overview-section\">\n";

    // Extract h2
    if let Some(h2_end) = section.find("</h2>") {
        let h2_end = h2_end + "</h2>".len();
        out += &section[..h2_end];
        out += "\n";
    }

    // Parse list items into cards
    let icons = ["→", "◆", "●", "■", "▲", "◇", "★", "▸", "◉", "▪"];
    let mut cards = Vec::new();
    let mut icon_idx = 0;

    // Find <li> items
    let mut remaining = section.to_string();
    while let Some(start) = remaining.find("<li>") {
        let content_start = start + "<li>".len();
        if let Some(end) = remaining[content_start..].find("</li>") {
            let item = remaining[content_start..content_start + end]
                .trim()
                .to_string();
            // Split on first — or - or : for title/desc
            let (title, desc) = split_feature_item(&item);
            cards.push((icons[icon_idx % icons.len()], title, desc));
            icon_idx += 1;
            remaining = remaining[content_start + end + "</li>".len()..].to_string();
        } else {
            break;
        }
    }

    if !cards.is_empty() {
        out += "<div class=\"feature-grid\">\n";
        for (icon, title, desc) in &cards {
            out += &format!(
                "<div class=\"feature-card\">\
                <div class=\"card-icon\">{}</div>\
                <div class=\"card-title\">{}</div>\
                <div class=\"card-desc\">{}</div>\
                </div>\n",
                icon, title, desc
            );
        }
        out += "</div>\n";
    }

    out += "</div>\n";
    out
}

/// Split a feature list item into title and description.
fn split_feature_item(item: &str) -> (String, String) {
    // Try splitting on " — ", " - ", or ": "
    for sep in &[" — ", " – ", " - ", ": "] {
        if let Some(pos) = item.find(sep) {
            let title = item[..pos].trim().to_string();
            let desc = item[pos + sep.len()..].trim().to_string();
            if !title.is_empty() && !desc.is_empty() {
                return (strip_tags(&title), strip_tags(&desc));
            }
        }
    }
    // No separator found — use whole text as title
    (strip_tags(item), String::new())
}

/// Strip HTML tags from a string (simple).
fn strip_tags(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
}

/// Transform "Quick Links" section into card grid.
fn transform_quick_links_section(section: &str) -> String {
    let mut out = String::new();
    out += "<div class=\"quick-links-section\">\n";

    // Extract h2
    if let Some(h2_end) = section.find("</h2>") {
        let h2_end = h2_end + "</h2>".len();
        out += &section[..h2_end];
        out += "\n";
    }

    // Find wiki-links or regular links
    let mut links = Vec::new();

    // Pattern: <a class="wiki-link" onclick="showPage('SLUG')">TITLE</a>
    let mut search = section.to_string();
    while let Some(start) = search.find("onclick=\"showPage('") {
        let slug_start = start + "onclick=\"showPage('".len();
        if let Some(slug_end) = search[slug_start..].find("')\"") {
            let slug = search[slug_start..slug_start + slug_end].to_string();
            // Find the title text after >
            let after = &search[slug_start + slug_end..];
            if let Some(title_start) = after.find('>')
                && let Some(title_end) = after[title_start + 1..].find('<') {
                    let title = after[title_start + 1..title_start + 1 + title_end]
                        .trim()
                        .to_string();
                    if !title.is_empty() {
                        links.push((slug, title));
                    }
                }
            search = search[slug_start + slug_end + 3..].to_string();
        } else {
            break;
        }
    }

    if !links.is_empty() {
        out += "<div class=\"quick-links-grid\">\n";
        let link_icons = ["→", "◆", "●", "■", "▲", "◇", "★", "▸"];
        for (idx, (slug, title)) in links.iter().enumerate() {
            let icon = link_icons[idx % link_icons.len()];
            out += &format!(
                "<a class=\"quick-link-card\" onclick=\"showPage('{}')\">\
                <div class=\"link-icon\">{}</div>\
                <div class=\"link-title\">{}</div>\
                </a>\n",
                slug, icon, title
            );
        }
        out += "</div>\n";
    }

    out += "</div>\n";
    out
}
