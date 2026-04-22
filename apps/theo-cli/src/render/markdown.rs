//! Static (non-streaming) markdown → terminal renderer.
//!
//! Uses `pulldown-cmark` to parse CommonMark + GFM extensions and emits
//! styled text via [`crate::render::style`]. All escape sequences flow
//! through the style module; this file contains no raw ANSI.
//!
//! For **streaming** LLM output, use [`crate::render::streaming`] which
//! is built on top of these primitives. See ADR-001 for the rationale.

#![allow(dead_code)] // Scaffolded helpers — kept for upcoming TUI features.
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::render::style::{self, StyleCaps, accent, bold, code_bg, dim, warn};

/// The set of pulldown-cmark options used consistently across theo
/// (CLI and theo-marklive). Keeping this in one place prevents drift
/// between the two renderers (wiki-expert concern from meeting
/// 20260411-103954).
pub fn default_options() -> Options {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts
}

/// Render a markdown string to terminal-styled text.
///
/// This is a one-shot renderer — the full text must be known upfront.
/// For incremental rendering during streaming, use the streaming module.
pub fn render(markdown: &str, caps: StyleCaps) -> String {
    let mut r = MarkdownRenderer::new(caps);
    let parser = Parser::new_ext(markdown, default_options());
    for event in parser {
        r.handle(event);
    }
    r.finish()
}

struct MarkdownRenderer {
    caps: StyleCaps,
    out: String,
    /// Current list indent stack. Each entry is a bullet string.
    list_stack: Vec<ListKind>,
    /// Whether we are currently inside a code block.
    in_code_block: bool,
    /// Current code block language (if any).
    code_lang: String,
    /// Buffered code content.
    code_buf: String,
    /// Currently inside emphasis (italic).
    italic_depth: u32,
    /// Currently inside strong (bold).
    bold_depth: u32,
    /// Inside a blockquote.
    blockquote_depth: u32,
    /// Inside a heading; heading level when active.
    heading_level: Option<HeadingLevel>,
    /// Inside a link; collect label.
    link_stack: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
enum ListKind {
    Unordered,
    Ordered(u64),
}

impl MarkdownRenderer {
    fn new(caps: StyleCaps) -> Self {
        Self {
            caps,
            out: String::new(),
            list_stack: Vec::new(),
            in_code_block: false,
            code_lang: String::new(),
            code_buf: String::new(),
            italic_depth: 0,
            bold_depth: 0,
            blockquote_depth: 0,
            heading_level: None,
            link_stack: Vec::new(),
        }
    }

    fn finish(mut self) -> String {
        self.flush_code();
        // Strip trailing newline if present.
        while self.out.ends_with('\n') {
            self.out.pop();
        }
        self.out
    }

    fn handle(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            Event::Code(code) => self.inline_code(&code),
            Event::Html(html) | Event::InlineHtml(html) => {
                // Preserve HTML as plain text; terminal has no renderer.
                self.push_plain(&html);
            }
            Event::SoftBreak => self.out.push(' '),
            Event::HardBreak => self.out.push('\n'),
            Event::Rule => self.rule(),
            Event::TaskListMarker(done) => {
                let marker = if done { "[x] " } else { "[ ] " };
                self.out.push_str(marker);
            }
            Event::FootnoteReference(label) => {
                self.out.push_str(&format!("[^{label}]"));
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => {
                self.newline_if_needed();
                self.heading_level = Some(level);
            }
            Tag::BlockQuote(_) => {
                self.blockquote_depth += 1;
                self.newline_if_needed();
            }
            Tag::CodeBlock(kind) => {
                self.newline_if_needed();
                self.in_code_block = true;
                self.code_buf.clear();
                self.code_lang = match kind {
                    CodeBlockKind::Fenced(lang) => lang.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
            }
            Tag::List(first) => {
                let kind = match first {
                    Some(n) => ListKind::Ordered(n),
                    None => ListKind::Unordered,
                };
                self.list_stack.push(kind);
                self.newline_if_needed();
            }
            Tag::Item => {
                let indent = "  ".repeat(self.list_stack.len().saturating_sub(1));
                let marker = match self.list_stack.last_mut() {
                    Some(ListKind::Unordered) => format!("{} ", style::bullet(self.caps)),
                    Some(ListKind::Ordered(n)) => {
                        let marker = format!("{n}. ");
                        if let Some(ListKind::Ordered(n)) = self.list_stack.last_mut() {
                            *n += 1;
                        }
                        marker
                    }
                    None => String::new(),
                };
                self.push_indent_marker(&indent, &marker);
            }
            Tag::Emphasis => self.italic_depth += 1,
            Tag::Strong => self.bold_depth += 1,
            Tag::Strikethrough => {}
            Tag::Link { .. } => self.link_stack.push(String::new()),
            Tag::Paragraph
                if !self.out.is_empty() && !self.out.ends_with('\n') => {
                    self.out.push('\n');
                }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => {
                self.heading_level = None;
                self.out.push('\n');
            }
            TagEnd::BlockQuote(_) => {
                self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
                self.out.push('\n');
            }
            TagEnd::CodeBlock => {
                self.flush_code();
                self.in_code_block = false;
                self.code_lang.clear();
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
                self.out.push('\n');
            }
            TagEnd::Item => self.out.push('\n'),
            TagEnd::Emphasis => self.italic_depth = self.italic_depth.saturating_sub(1),
            TagEnd::Strong => self.bold_depth = self.bold_depth.saturating_sub(1),
            TagEnd::Link => {
                if let Some(label) = self.link_stack.pop() {
                    // We already appended the label text via Text events
                    // into self.out, not into the link_stack frame. The
                    // frame acts as a sentinel. We do nothing extra here;
                    // a full implementation would track href too.
                    let _ = label;
                }
            }
            TagEnd::Paragraph => self.out.push('\n'),
            _ => {}
        }
    }

    fn text(&mut self, text: &str) {
        if self.in_code_block {
            self.code_buf.push_str(text);
            return;
        }
        if let Some(level) = self.heading_level {
            let styled = match level {
                HeadingLevel::H1 => bold(text.to_string(), self.caps).to_string(),
                HeadingLevel::H2 => accent(text.to_string(), self.caps).to_string(),
                _ => dim(text.to_string(), self.caps).to_string(),
            };
            self.out.push_str(&styled);
            return;
        }
        let mut rendered = text.to_string();
        if self.bold_depth > 0 {
            rendered = bold(rendered, self.caps).to_string();
        }
        if self.italic_depth > 0 {
            rendered = dim(rendered, self.caps).to_string();
        }
        if self.blockquote_depth > 0 {
            // Prefix with quote marker on each newline
            let prefix = dim("│ ", self.caps).to_string();
            let prefixed: Vec<String> = rendered
                .lines()
                .map(|l| format!("{prefix}{l}"))
                .collect();
            rendered = prefixed.join("\n");
        }
        self.out.push_str(&rendered);
    }

    fn inline_code(&mut self, code: &str) {
        self.out.push_str(&code_bg(code.to_string(), self.caps).to_string());
    }

    fn push_plain(&mut self, s: &str) {
        self.out.push_str(s);
    }

    fn push_indent_marker(&mut self, indent: &str, marker: &str) {
        self.out.push_str(indent);
        self.out
            .push_str(&accent(marker.to_string(), self.caps).to_string());
    }

    fn newline_if_needed(&mut self) {
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.out.push('\n');
        }
    }

    fn rule(&mut self) {
        self.newline_if_needed();
        let width = 40_usize;
        let line = style::hline_char(self.caps).repeat(width);
        self.out.push_str(&dim(line, self.caps).to_string());
        self.out.push('\n');
    }

    fn flush_code(&mut self) {
        if self.code_buf.is_empty() {
            return;
        }
        // Header with language label
        let lang_label = if self.code_lang.is_empty() {
            "code".to_string()
        } else {
            self.code_lang.clone()
        };
        let border_char = style::hline_char(self.caps);
        let header = format!("{} {}", lang_label, border_char.repeat(40));
        self.out.push_str(&dim(header, self.caps).to_string());
        self.out.push('\n');
        // Body (each line prefixed)
        for line in self.code_buf.lines() {
            self.out
                .push_str(&dim("│ ", self.caps).to_string());
            self.out.push_str(&code_bg(line.to_string(), self.caps).to_string());
            self.out.push('\n');
        }
        // Footer
        let footer = border_char.repeat(42);
        self.out.push_str(&dim(footer, self.caps).to_string());
        self.out.push('\n');
        // Warn if language unknown in TTY mode (helps debug wrong fences).
        if self.code_lang == "???" {
            self.out
                .push_str(&warn("(unknown language)", self.caps).to_string());
            self.out.push('\n');
        }
        self.code_buf.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> StyleCaps {
        StyleCaps::plain()
    }

    #[test]
    fn test_default_options_enables_tables_and_strikethrough() {
        let opts = default_options();
        assert!(opts.contains(Options::ENABLE_TABLES));
        assert!(opts.contains(Options::ENABLE_STRIKETHROUGH));
        assert!(opts.contains(Options::ENABLE_TASKLISTS));
    }

    #[test]
    fn test_empty_input_returns_empty_output() {
        assert_eq!(render("", plain()), "");
    }

    #[test]
    fn test_plain_paragraph_round_trips() {
        let out = render("hello world", plain());
        assert_eq!(out, "hello world");
    }

    #[test]
    fn test_bold_text_in_plain_mode_is_raw() {
        let out = render("**hello**", plain());
        assert_eq!(out, "hello");
    }

    #[test]
    fn test_italic_text_in_plain_mode_is_raw() {
        let out = render("*hello*", plain());
        assert_eq!(out, "hello");
    }

    #[test]
    fn test_inline_code_rendered() {
        let out = render("use `println!`", plain());
        assert!(out.contains("println!"));
    }

    #[test]
    fn test_h1_heading_rendered() {
        let out = render("# Title", plain());
        assert!(out.contains("Title"));
    }

    #[test]
    fn test_h2_heading_rendered() {
        let out = render("## Subtitle", plain());
        assert!(out.contains("Subtitle"));
    }

    #[test]
    fn test_unordered_list_has_bullets() {
        let out = render("- one\n- two", plain());
        assert!(out.contains("* one"));
        assert!(out.contains("* two"));
    }

    #[test]
    fn test_ordered_list_has_numbers() {
        let out = render("1. first\n2. second", plain());
        assert!(out.contains("1. first"));
        assert!(out.contains("2. second"));
    }

    #[test]
    fn test_nested_list_indents() {
        let md = "- outer\n  - inner";
        let out = render(md, plain());
        assert!(out.contains("outer"));
        assert!(out.contains("inner"));
        // The inner item should be indented more than the outer
        let outer_line = out
            .lines()
            .find(|l| l.contains("outer"))
            .expect("outer line");
        let inner_line = out
            .lines()
            .find(|l| l.contains("inner"))
            .expect("inner line");
        let outer_indent = outer_line.len() - outer_line.trim_start().len();
        let inner_indent = inner_line.len() - inner_line.trim_start().len();
        assert!(inner_indent > outer_indent);
    }

    #[test]
    fn test_blockquote_prefixes_line() {
        let out = render("> quoted", plain());
        assert!(out.contains("│ quoted"));
    }

    #[test]
    fn test_fenced_code_block_keeps_content() {
        let md = "```rust\nfn main() {}\n```";
        let out = render(md, plain());
        assert!(out.contains("fn main() {}"));
        assert!(out.contains("rust"));
    }

    #[test]
    fn test_code_block_without_language_labels_as_code() {
        let md = "```\nhello\n```";
        let out = render(md, plain());
        assert!(out.contains("code"));
        assert!(out.contains("hello"));
    }

    #[test]
    fn test_horizontal_rule_renders_hline() {
        let out = render("before\n\n---\n\nafter", plain());
        assert!(out.contains("-")); // ASCII fallback in plain caps
        assert!(out.contains("before"));
        assert!(out.contains("after"));
    }

    #[test]
    fn test_strikethrough_preserves_text() {
        let out = render("~~gone~~", plain());
        assert!(out.contains("gone"));
    }

    #[test]
    fn test_task_list_renders_checkboxes() {
        let out = render("- [x] done\n- [ ] todo", plain());
        assert!(out.contains("[x] done"));
        assert!(out.contains("[ ] todo"));
    }

    #[test]
    fn test_link_text_preserved() {
        let out = render("[theo](https://example.com)", plain());
        assert!(out.contains("theo"));
    }

    #[test]
    fn test_trailing_newline_stripped() {
        let out = render("hello\n\n", plain());
        assert_eq!(out, "hello");
    }

    #[test]
    fn test_multiline_paragraph_joined_softly() {
        let out = render("line one\nline two", plain());
        // Soft break becomes space
        assert!(out.contains("line one line two"));
    }

    #[test]
    fn test_tty_mode_emits_ansi_for_bold() {
        let out = render("**bold**", StyleCaps::full());
        assert!(out.contains("\x1b["));
        assert!(out.contains("bold"));
    }

    #[test]
    fn test_render_is_deterministic() {
        let md = "# Title\n\nSome **bold** and *italic*.\n\n- one\n- two\n";
        let a = render(md, plain());
        let b = render(md, plain());
        assert_eq!(a, b);
    }
}
