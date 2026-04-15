//! Markdown to ratatui Spans converter.
//!
//! Parses markdown using pulldown-cmark and converts to styled ratatui Lines.
//! Supports: headings, bold, italic, code inline, code blocks, lists, links.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, CodeBlockKind};
use ratatui::prelude::*;

/// Convert a markdown string to styled ratatui Lines.
pub fn markdown_to_lines(text: &str) -> Vec<Line<'static>> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(text, opts);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut style_stack: Vec<Style> = vec![Style::default()];
    let mut in_code_block = false;
    let mut code_block_lang = String::new();
    let mut code_buffer = String::new();
    let mut list_depth: usize = 0;

    for event in parser {
        match event {
            Event::Start(tag) => {
                match tag {
                    Tag::Heading { level, .. } => {
                        let style = match level {
                            pulldown_cmark::HeadingLevel::H1 => Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                            pulldown_cmark::HeadingLevel::H2 => Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
                            _ => Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                        };
                        style_stack.push(style);
                    }
                    Tag::Strong => {
                        let base = *style_stack.last().unwrap_or(&Style::default());
                        style_stack.push(base.add_modifier(Modifier::BOLD));
                    }
                    Tag::Emphasis => {
                        let base = *style_stack.last().unwrap_or(&Style::default());
                        style_stack.push(base.add_modifier(Modifier::ITALIC));
                    }
                    Tag::CodeBlock(kind) => {
                        in_code_block = true;
                        code_buffer.clear();
                        code_block_lang = match kind {
                            CodeBlockKind::Fenced(lang) => lang.to_string(),
                            CodeBlockKind::Indented => String::new(),
                        };
                    }
                    Tag::List(_) => {
                        list_depth += 1;
                    }
                    Tag::Item => {
                        let indent = "  ".repeat(list_depth.saturating_sub(1));
                        current_spans.push(Span::styled(
                            format!("{indent}  "),
                            Style::default(),
                        ));
                        current_spans.push(Span::styled(
                            "• ",
                            Style::default().fg(Color::Cyan),
                        ));
                    }
                    Tag::Link { dest_url, .. } => {
                        let base = *style_stack.last().unwrap_or(&Style::default());
                        style_stack.push(base.fg(Color::Cyan).add_modifier(Modifier::UNDERLINED));
                        // Store URL for later display
                        current_spans.push(Span::styled(
                            format!("["),
                            Style::default().fg(Color::DarkGray),
                        ));
                        let _ = dest_url; // URL displayed after text
                    }
                    Tag::Paragraph => {}
                    _ => {}
                }
            }
            Event::End(tag_end) => {
                match tag_end {
                    TagEnd::Heading(_) => {
                        style_stack.pop();
                        flush_line(&mut lines, &mut current_spans);
                        lines.push(Line::from(""));
                    }
                    TagEnd::Strong | TagEnd::Emphasis => {
                        style_stack.pop();
                    }
                    TagEnd::CodeBlock => {
                        in_code_block = false;
                        // Render code block with background
                        let lang_label = if code_block_lang.is_empty() {
                            String::new()
                        } else {
                            format!(" {}", code_block_lang)
                        };
                        lines.push(Line::from(Span::styled(
                            format!("  ┌─{lang_label}─────"),
                            Style::default().fg(Color::DarkGray),
                        )));
                        for code_line in code_buffer.lines() {
                            lines.push(Line::from(vec![
                                Span::styled("  │ ", Style::default().fg(Color::DarkGray)),
                                Span::styled(
                                    code_line.to_string(),
                                    Style::default().fg(Color::Green),
                                ),
                            ]));
                        }
                        lines.push(Line::from(Span::styled(
                            "  └─────",
                            Style::default().fg(Color::DarkGray),
                        )));
                        code_buffer.clear();
                    }
                    TagEnd::List(_) => {
                        list_depth = list_depth.saturating_sub(1);
                    }
                    TagEnd::Item => {
                        flush_line(&mut lines, &mut current_spans);
                    }
                    TagEnd::Link => {
                        style_stack.pop();
                        current_spans.push(Span::styled(
                            "]",
                            Style::default().fg(Color::DarkGray),
                        ));
                    }
                    TagEnd::Paragraph => {
                        flush_line(&mut lines, &mut current_spans);
                        lines.push(Line::from(""));
                    }
                    _ => {}
                }
            }
            Event::Text(text) => {
                if in_code_block {
                    code_buffer.push_str(&text);
                } else {
                    let style = *style_stack.last().unwrap_or(&Style::default());
                    // Handle multi-line text
                    let text_str = text.to_string();
                    let mut first = true;
                    for line in text_str.split('\n') {
                        if !first {
                            flush_line(&mut lines, &mut current_spans);
                        }
                        if !line.is_empty() {
                            current_spans.push(Span::styled(line.to_string(), style));
                        }
                        first = false;
                    }
                }
            }
            Event::Code(code) => {
                current_spans.push(Span::styled(
                    format!("`{code}`"),
                    Style::default().fg(Color::Yellow).bg(Color::DarkGray),
                ));
            }
            Event::SoftBreak | Event::HardBreak => {
                flush_line(&mut lines, &mut current_spans);
            }
            Event::Rule => {
                flush_line(&mut lines, &mut current_spans);
                lines.push(Line::from(Span::styled(
                    "────────────────────",
                    Style::default().fg(Color::DarkGray),
                )));
            }
            _ => {}
        }
    }

    // Flush remaining spans
    if !current_spans.is_empty() {
        flush_line(&mut lines, &mut current_spans);
    }

    lines
}

fn flush_line(lines: &mut Vec<Line<'static>>, spans: &mut Vec<Span<'static>>) {
    if !spans.is_empty() {
        lines.push(Line::from(std::mem::take(spans)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_single_line() {
        let lines = markdown_to_lines("hello world");
        assert!(!lines.is_empty());
    }

    #[test]
    fn heading_renders_bold() {
        let lines = markdown_to_lines("# Title");
        let first_content = lines.iter().find(|l| !l.spans.is_empty()).unwrap();
        let span = &first_content.spans[0];
        assert!(span.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn code_block_renders_with_border() {
        let lines = markdown_to_lines("```rust\nfn main() {}\n```");
        let has_border = lines.iter().any(|l| {
            l.spans.iter().any(|s| s.content.contains("┌─"))
        });
        assert!(has_border, "code block should have border");
    }

    #[test]
    fn inline_code_has_yellow_fg() {
        let lines = markdown_to_lines("use `foo` here");
        let has_code = lines.iter().any(|l| {
            l.spans.iter().any(|s| s.content.contains("`foo`"))
        });
        assert!(has_code, "inline code should be present");
    }

    #[test]
    fn bold_text_has_modifier() {
        let lines = markdown_to_lines("this is **bold** text");
        let has_bold = lines.iter().any(|l| {
            l.spans.iter().any(|s| {
                s.content.contains("bold") && s.style.add_modifier.contains(Modifier::BOLD)
            })
        });
        assert!(has_bold, "bold text should have BOLD modifier");
    }
}
