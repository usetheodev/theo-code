use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::permission::{PermissionRequest, PermissionType};
use theo_domain::tool::{
    FileAttachment, PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam,
    ToolSchema, optional_string, require_string,
};

pub struct WebFetchTool;

impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WebFetchTool {
    pub fn new() -> Self {
        Self
    }

    /// SSRF protection: reject dangerous URLs before fetching.
    fn validate_url(url: &str) -> Result<(), ToolError> {
        // Only allow http/https schemes
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(ToolError::Execution(format!(
                "URL scheme not allowed: {url}. Only http:// and https:// are permitted."
            )));
        }

        // Extract host from URL
        let host = url
            .strip_prefix("http://")
            .or_else(|| url.strip_prefix("https://"))
            .and_then(|rest| rest.split('/').next())
            .and_then(|host_port| host_port.split(':').next())
            .unwrap_or("");

        // Block private IP ranges and metadata endpoints
        let blocked_hosts = [
            "127.0.0.1",
            "localhost",
            "0.0.0.0",
            "169.254.169.254",          // AWS IMDS
            "metadata.google.internal", // GCP metadata
            "metadata.internal",
        ];
        if blocked_hosts.contains(&host) {
            return Err(ToolError::Execution(format!(
                "URL host blocked (SSRF protection): {host}"
            )));
        }

        // Block private IP ranges by prefix
        let blocked_prefixes = [
            "10.", "172.16.", "172.17.", "172.18.", "172.19.", "172.20.", "172.21.", "172.22.",
            "172.23.", "172.24.", "172.25.", "172.26.", "172.27.", "172.28.", "172.29.", "172.30.",
            "172.31.", "192.168.", "169.254.",
        ];
        for prefix in &blocked_prefixes {
            if host.starts_with(prefix) {
                return Err(ToolError::Execution(format!(
                    "URL host blocked (private IP): {host}"
                )));
            }
        }

        Ok(())
    }

    fn is_image_content_type(content_type: &str) -> bool {
        let ct = content_type.to_lowercase();
        ct.starts_with("image/") && !ct.contains("svg")
    }

    fn extract_mime(content_type: &str) -> String {
        content_type
            .split(';')
            .next()
            .unwrap_or(content_type)
            .trim()
            .to_lowercase()
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn id(&self) -> &str {
        "webfetch"
    }

    fn description(&self) -> &str {
        "Fetch a URL and return its content"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![ToolParam {
                name: "url".to_string(),
                param_type: "string".to_string(),
                description: "URL to fetch".to_string(),
                required: true,
            }],
        input_examples: Vec::new(),
    }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Web
    }

    /// Webpage bodies can be huge — cap at 10k chars, keep head (usually
    /// contains the structural content / title) plus the last kilobyte
    /// (often contains footer links and follow-up URLs).
    fn truncation_rule(&self) -> Option<theo_domain::tool::TruncationRule> {
        Some(theo_domain::tool::TruncationRule {
            max_chars: 10_000,
            strategy: theo_domain::tool::TruncationStrategy::HeadTail {
                head: 8_000,
                tail: 2_000,
            },
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
        permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let url = require_string(&args, "url")?;
        let _format = optional_string(&args, "format").unwrap_or_else(|| "markdown".to_string());

        // SSRF protection: validate URL before fetching
        Self::validate_url(&url)?;

        permissions.record(PermissionRequest {
            permission: PermissionType::WebFetch,
            patterns: vec![url.clone()],
            always: vec![],
            metadata: serde_json::json!({}),
        });

        let response = reqwest::get(&url)
            .await
            .map_err(|e| ToolError::Execution(format!("Fetch failed: {e}")))?;

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/plain")
            .to_string();

        let mime = Self::extract_mime(&content_type);

        // Handle image responses
        if Self::is_image_content_type(&content_type) {
            let bytes = response
                .bytes()
                .await
                .map_err(|e| ToolError::Execution(format!("Failed to read response: {e}")))?;

            let b64 = base64_encode(&bytes);

            return Ok(ToolOutput {
                title: url,
                output: "Image fetched successfully".to_string(),
                metadata: serde_json::json!({}),
                attachments: Some(vec![FileAttachment {
                    file_type: "file".to_string(),
                    mime: Some(mime),
                    url: format!("data:{};base64,{b64}", Self::extract_mime(&content_type)),
                }]),
                llm_suffix: None,
            });
        }

        // Handle SVG/HTML as text
        let text = response
            .text()
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to read response: {e}")))?;

        // Dynamic filtering: when the response looks like HTML, strip noise
        // blocks (scripts/styles/nav/header/footer) deterministically before
        // the body enters the context window. Anthropic "Dynamic Filtering"
        // ships ~24% token reduction on web fetches; our reducer is simpler
        // than a code-exec sandbox but follows the same "digest before
        // context" principle.
        let is_html = mime.contains("html") || text.trim_start().starts_with("<!DOCTYPE html")
            || text.trim_start().starts_with("<html");
        let (filtered, dropped_chars) = if is_html {
            filter_html(&text)
        } else {
            (text.clone(), 0)
        };
        let llm_suffix = if dropped_chars > 0 {
            Some(format!(
                "[html-filter] Removed {dropped_chars} chars of script/style/nav/header/footer noise from the response. Set fetch parameters (or target a specific section of the URL) if you need the raw HTML."
            ))
        } else {
            None
        };

        Ok(ToolOutput {
            title: url,
            output: filtered,
            metadata: serde_json::json!({"filtered_chars_removed": dropped_chars, "is_html": is_html}),
            attachments: None,
            llm_suffix,
        })
    }
}

/// Strip `<script>`, `<style>`, `<nav>`, `<header>`, `<footer>`, and inline
/// event handlers from an HTML document. Returns the filtered body and the
/// number of characters that were removed. Deterministic and allocation-light
/// — not a full HTML parser, just a targeted reducer for the common noise
/// blocks Anthropic cites in their Dynamic Filtering walkthrough.
pub(crate) fn filter_html(html: &str) -> (String, usize) {
    let original_len = html.chars().count();
    let mut out = html.to_string();
    for tag in ["script", "style", "nav", "header", "footer", "noscript"] {
        out = strip_tag_block(&out, tag);
    }
    // Strip inline on* event handlers ( on[a-z]+="..." or on[a-z]+='...' )
    out = strip_inline_event_handlers(&out);
    // Collapse runs of whitespace the stripping leaves behind
    out = collapse_whitespace(&out);
    let new_len = out.chars().count();
    let removed = original_len.saturating_sub(new_len);
    (out, removed)
}

fn strip_tag_block(input: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    loop {
        // Case-insensitive match of "<tag"
        let lower = rest.to_lowercase();
        let Some(open_idx) = lower.find(&open) else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..open_idx]);
        // Skip past the closing `>` of the opening tag
        let after_open = &rest[open_idx..];
        let Some(rel_gt) = after_open.find('>') else {
            // Malformed — bail out and append the rest
            out.push_str(after_open);
            break;
        };
        let search_start = open_idx + rel_gt + 1;
        let lower_remaining = rest[search_start..].to_lowercase();
        let Some(close_rel) = lower_remaining.find(&close) else {
            // No closing tag — drop everything from here on.
            break;
        };
        let close_abs = search_start + close_rel + close.len();
        rest = &rest[close_abs..];
    }
    out
}

fn strip_inline_event_handlers(input: &str) -> String {
    // Matches ` on<word>="..."` and ` on<word>='...'` conservatively.
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 3 < bytes.len() && bytes[i] == b' ' && bytes[i + 1] == b'o' && bytes[i + 2] == b'n'
            && bytes[i + 3].is_ascii_alphabetic()
        {
            // Find the `=` that names the handler attribute
            let mut j = i + 3;
            while j < bytes.len() && bytes[j].is_ascii_alphabetic() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'=' {
                let quote = bytes.get(j + 1).copied();
                if quote == Some(b'"') || quote == Some(b'\'') {
                    // Find the matching closing quote
                    let q = quote.unwrap();
                    if let Some(end) = bytes[j + 2..].iter().position(|&b| b == q) {
                        i = j + 2 + end + 1;
                        continue;
                    }
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn collapse_whitespace(input: &str) -> String {
    // Collapse runs of 3+ blank lines into exactly 2 (one blank line between
    // paragraphs). Leaves single/double newlines alone to preserve structure.
    let mut out = String::with_capacity(input.len());
    let mut blank_run = 0;
    for line in input.lines() {
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run <= 1 {
                out.push('\n');
            }
        } else {
            blank_run = 0;
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[(n >> 18 & 63) as usize] as char);
        result.push(CHARS[(n >> 12 & 63) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[(n >> 6 & 63) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(n & 63) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // Unit tests for content type detection (no network needed)

    #[test]
    fn detects_image_content_type() {
        assert!(WebFetchTool::is_image_content_type(
            "IMAGE/PNG; charset=binary"
        ));
        assert!(WebFetchTool::is_image_content_type("image/jpeg"));
        assert!(WebFetchTool::is_image_content_type("image/webp"));
    }

    #[test]
    fn svg_is_not_image_content_type() {
        assert!(!WebFetchTool::is_image_content_type(
            "image/svg+xml; charset=UTF-8"
        ));
    }

    #[test]
    fn extracts_mime_from_content_type() {
        assert_eq!(
            WebFetchTool::extract_mime("IMAGE/PNG; charset=binary"),
            "image/png"
        );
        assert_eq!(WebFetchTool::extract_mime("text/plain"), "text/plain");
    }

    #[test]
    fn base64_encode_roundtrip() {
        let input = &[137, 80, 78, 71, 13, 10, 26, 10]; // PNG magic bytes
        let encoded = base64_encode(input);
        assert!(encoded.starts_with("iVBOR"));
    }

    // ── Dynamic HTML filter tests ────────────────────────────────

    #[test]
    fn html_filter_strips_script_block() {
        let html = r#"<html><body><p>hello</p><script>alert('x');</script><p>world</p></body></html>"#;
        let (out, removed) = filter_html(html);
        assert!(!out.contains("alert"));
        assert!(out.contains("hello"));
        assert!(out.contains("world"));
        assert!(removed > 0);
    }

    #[test]
    fn html_filter_strips_style_block() {
        let html = r#"<html><head><style>body{color:red}</style></head><body>text</body></html>"#;
        let (out, _removed) = filter_html(html);
        assert!(!out.contains("color:red"));
        assert!(out.contains("text"));
    }

    #[test]
    fn html_filter_strips_nav_header_footer() {
        let html = r#"<html><body><nav>menu</nav><header>h</header><main>content</main><footer>f</footer></body></html>"#;
        let (out, _removed) = filter_html(html);
        assert!(!out.contains("menu"));
        assert!(!out.contains("<header>"));
        assert!(!out.contains("<footer>"));
        assert!(out.contains("content"));
    }

    #[test]
    fn html_filter_removes_inline_event_handlers() {
        let html = r#"<body><a href="/" onclick="trackClick()" onmouseover='pop()'>link</a></body>"#;
        let (out, _removed) = filter_html(html);
        assert!(!out.contains("onclick"));
        assert!(!out.contains("onmouseover"));
        assert!(out.contains("link"));
    }

    #[test]
    fn html_filter_returns_zero_removed_when_no_noise() {
        let html = "<p>clean text</p>";
        let (out, removed) = filter_html(html);
        assert_eq!(out.trim(), "<p>clean text</p>");
        assert_eq!(removed, 0);
    }

    #[test]
    fn html_filter_handles_case_insensitive_tags() {
        let html = "<SCRIPT>evil()</SCRIPT><p>ok</p>";
        let (out, removed) = filter_html(html);
        assert!(!out.to_lowercase().contains("evil"));
        assert!(out.contains("ok"));
        assert!(removed > 0);
    }
}
