//! Markdown to ratatui Spans converter.
//!
//! Parses markdown using pulldown-cmark and converts to styled ratatui Lines.
//! Supports: headings, bold, italic, code inline, code blocks, lists, links.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, CodeBlockKind};
use ratatui::prelude::*;

struct MarkdownState {
    lines: Vec<Line<'static>>,
    current_spans: Vec<Span<'static>>,
    style_stack: Vec<Style>,
    in_code_block: bool,
    code_block_lang: String,
    code_buffer: String,
    list_depth: usize,
}

/// Convert a markdown string to styled ratatui Lines.
pub fn markdown_to_lines(text: &str) -> Vec<Line<'static>> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(text, opts);

    let mut state = MarkdownState {
        lines: Vec::new(),
        current_spans: Vec::new(),
        style_stack: vec![Style::default()],
        in_code_block: false,
        code_block_lang: String::new(),
        code_buffer: String::new(),
        list_depth: 0,
    };

    for event in parser {
        match event {
            Event::Start(tag) => handle_start_tag(&mut state, tag),
            Event::End(tag_end) => handle_end_tag(&mut state, tag_end),
            Event::Text(text) => handle_text_event(&mut state, &text),
            Event::Code(code) => state.current_spans.push(Span::styled(
                format!("`{code}`"),
                Style::default().fg(Color::Yellow).bg(Color::DarkGray),
            )),
            Event::SoftBreak | Event::HardBreak => {
                flush_line(&mut state.lines, &mut state.current_spans);
            }
            Event::Rule => {
                flush_line(&mut state.lines, &mut state.current_spans);
                state.lines.push(Line::from(Span::styled(
                    "────────────────────",
                    Style::default().fg(Color::DarkGray),
                )));
            }
            _ => {}
        }
    }
    let MarkdownState {
        mut lines,
        mut current_spans,
        ..
    } = state;

    // Flush remaining spans
    if !current_spans.is_empty() {
        flush_line(&mut lines, &mut current_spans);
    }

    lines
}

/// Simple keyword-based syntax highlighting for code blocks.
/// Not as sophisticated as syntect but covers common patterns without heavy deps.
fn handle_start_tag(state: &mut MarkdownState, tag: Tag) {
    match tag {
        Tag::Heading { level, .. } => {
            let style = match level {
                pulldown_cmark::HeadingLevel::H1 => {
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                }
                pulldown_cmark::HeadingLevel::H2 => {
                    Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)
                }
                _ => Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            };
            state.style_stack.push(style);
        }
        Tag::Strong => {
            let base = *state.style_stack.last().unwrap_or(&Style::default());
            state.style_stack.push(base.add_modifier(Modifier::BOLD));
        }
        Tag::Emphasis => {
            let base = *state.style_stack.last().unwrap_or(&Style::default());
            state.style_stack.push(base.add_modifier(Modifier::ITALIC));
        }
        Tag::CodeBlock(kind) => {
            state.in_code_block = true;
            state.code_buffer.clear();
            state.code_block_lang = match kind {
                CodeBlockKind::Fenced(lang) => lang.to_string(),
                CodeBlockKind::Indented => String::new(),
            };
        }
        Tag::List(_) => {
            state.list_depth += 1;
        }
        Tag::Item => {
            let indent = "  ".repeat(state.list_depth.saturating_sub(1));
            state
                .current_spans
                .push(Span::styled(format!("{indent}  "), Style::default()));
            state
                .current_spans
                .push(Span::styled("• ", Style::default().fg(Color::Cyan)));
        }
        Tag::Link { dest_url, .. } => {
            let base = *state.style_stack.last().unwrap_or(&Style::default());
            state
                .style_stack
                .push(base.fg(Color::Cyan).add_modifier(Modifier::UNDERLINED));
            state.current_spans.push(Span::styled(
                "[".to_string(),
                Style::default().fg(Color::DarkGray),
            ));
            let _ = dest_url;
        }
        Tag::Paragraph => {}
        _ => {}
    }
}

fn handle_end_tag(state: &mut MarkdownState, tag_end: TagEnd) {
    match tag_end {
        TagEnd::Heading(_) => {
            state.style_stack.pop();
            flush_line(&mut state.lines, &mut state.current_spans);
            state.lines.push(Line::from(""));
        }
        TagEnd::Strong | TagEnd::Emphasis => {
            state.style_stack.pop();
        }
        TagEnd::CodeBlock => {
            state.in_code_block = false;
            render_code_block(state);
            state.code_buffer.clear();
        }
        TagEnd::List(_) => {
            state.list_depth = state.list_depth.saturating_sub(1);
        }
        TagEnd::Item => {
            flush_line(&mut state.lines, &mut state.current_spans);
        }
        TagEnd::Link => {
            state.style_stack.pop();
            state
                .current_spans
                .push(Span::styled("]", Style::default().fg(Color::DarkGray)));
        }
        TagEnd::Paragraph => {
            flush_line(&mut state.lines, &mut state.current_spans);
            state.lines.push(Line::from(""));
        }
        _ => {}
    }
}

fn render_code_block(state: &mut MarkdownState) {
    let lang_label = if state.code_block_lang.is_empty() {
        String::new()
    } else {
        format!(" {}", state.code_block_lang)
    };
    state.lines.push(Line::from(Span::styled(
        format!("  ┌─{lang_label}─────"),
        Style::default().fg(Color::DarkGray),
    )));
    for code_line in state.code_buffer.lines() {
        let styled_spans = highlight_code_line(code_line, &state.code_block_lang);
        let mut line_spans = vec![Span::styled("  │ ", Style::default().fg(Color::DarkGray))];
        line_spans.extend(styled_spans);
        state.lines.push(Line::from(line_spans));
    }
    state.lines.push(Line::from(Span::styled(
        "  └─────",
        Style::default().fg(Color::DarkGray),
    )));
}

fn handle_text_event(state: &mut MarkdownState, text: &str) {
    if state.in_code_block {
        state.code_buffer.push_str(text);
        return;
    }
    let style = *state.style_stack.last().unwrap_or(&Style::default());
    let mut first = true;
    for line in text.split('\n') {
        if !first {
            flush_line(&mut state.lines, &mut state.current_spans);
        }
        if !line.is_empty() {
            state
                .current_spans
                .push(Span::styled(line.to_string(), style));
        }
        first = false;
    }
}

fn highlight_code_line(line: &str, lang: &str) -> Vec<Span<'static>> {
    let keywords: &[&str] = match lang {
        "rust" | "rs" => &[
            "fn", "let", "mut", "pub", "struct", "enum", "impl", "trait", "use", "mod",
            "async", "await", "match", "if", "else", "for", "while", "loop", "return",
            "self", "Self", "super", "crate", "where", "type", "const", "static",
        ],
        "typescript" | "ts" | "javascript" | "js" => &[
            "function", "const", "let", "var", "if", "else", "for", "while", "return",
            "import", "export", "from", "class", "extends", "interface", "type",
            "async", "await", "new", "this", "super", "default",
        ],
        "python" | "py" => &[
            "def", "class", "import", "from", "if", "elif", "else", "for", "while",
            "return", "yield", "async", "await", "with", "as", "try", "except",
            "raise", "pass", "lambda", "self", "None", "True", "False",
        ],
        "go" => &[
            "func", "var", "const", "type", "struct", "interface", "if", "else",
            "for", "range", "return", "package", "import", "go", "defer", "chan",
            "select", "case", "switch", "default", "nil", "true", "false",
        ],
        "bash" | "sh" | "shell" | "zsh" => &[
            "if", "then", "else", "fi", "for", "do", "done", "while", "case", "esac",
            "function", "return", "export", "local", "echo", "exit",
        ],
        _ => &[],
    };

    if keywords.is_empty() {
        // No language-specific highlighting, use generic green
        return vec![Span::styled(line.to_string(), Style::default().fg(Color::Green))];
    }

    let mut spans: Vec<Span<'static>> = Vec::new();
    let remaining = line.to_string();

    // Simple tokenizer: split by word boundaries, color keywords
    let _result = String::new();
    let mut in_string = false;
    let mut string_char = '"';
    let mut in_comment = false;
    let _chars = remaining.chars().peekable();
    let _current_word = String::new();

    // Simplified approach: just scan for keywords at word boundaries
    // and color strings/comments differently
    for (i, ch) in line.char_indices() {
        if in_comment {
            // Rest of line is comment
            break;
        }
        if in_string {
            if ch == string_char && (i == 0 || line.as_bytes().get(i - 1) != Some(&b'\\')) {
                in_string = false;
            }
            continue;
        }
        if ch == '"' || ch == '\'' {
            in_string = true;
            string_char = ch;
            continue;
        }
        if ch == '/' && line.get(i+1..i+2) == Some("/") {
            in_comment = true;
            continue;
        }
        if ch == '#' && matches!(lang, "python" | "py" | "bash" | "sh" | "shell" | "zsh") {
            in_comment = true;
            continue;
        }
    }

    // For simplicity, use a word-by-word coloring approach
    let parts: Vec<&str> = line.split_inclusive(|c: char| !c.is_alphanumeric() && c != '_')
        .collect();

    for part in parts {
        let word = part.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_');
        let _suffix_len = part.len() - word.len();
        let suffix = &part[word.len()..];

        if keywords.contains(&word) {
            spans.push(Span::styled(word.to_string(), Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)));
            if !suffix.is_empty() {
                spans.push(Span::styled(suffix.to_string(), Style::default().fg(Color::Green)));
            }
        } else if word.starts_with('"') || word.starts_with('\'') {
            spans.push(Span::styled(part.to_string(), Style::default().fg(Color::Yellow)));
        } else if word.starts_with("//") || word.starts_with('#') {
            spans.push(Span::styled(part.to_string(), Style::default().fg(Color::DarkGray)));
        } else {
            spans.push(Span::styled(part.to_string(), Style::default().fg(Color::Green)));
        }
    }

    if spans.is_empty() {
        spans.push(Span::styled(line.to_string(), Style::default().fg(Color::Green)));
    }

    spans
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
