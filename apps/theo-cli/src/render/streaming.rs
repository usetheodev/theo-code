//! Incremental markdown renderer for LLM streaming output.
//!
//! Buffers partial text chunks and emits styled output as soon as
//! markdown primitives can be safely resolved (e.g. a closing `**`,
//! a newline, or a closing code fence).
//!
//! The key constraint: same logical input must produce identical
//! output regardless of how it is chunked. This is verified via
//! property tests in T1.4b.
//!
//! See ADR-001: Streaming Markdown State Machine.
//!
//! ## State machine
//!
//! ```text
//! Plain            -- default: append char, emit on flush boundary
//! Star1            -- saw one `*`; could be italic or first of bold
//! Star2            -- saw two `*`; entering bold
//! BoldOpen         -- inside **...**, accumulate until closing **
//! BoldClosing1     -- inside bold, saw one `*` (could be closing)
//! ItalicOpen       -- inside *...*, accumulate until closing *
//! Backtick1        -- saw one ` ` (could be inline or code fence)
//! Backtick2        -- saw two backticks (abandoned — not valid md)
//! FenceOpen        -- saw three backticks, reading language
//! CodeBlock        -- inside fenced code block, accumulate lines
//! FenceClosing     -- inside code block, saw backtick(s) on line start
//! InlineCode       -- inside `...` inline code
//! ```
//!
//! For simplicity in this first cut, we handle the most common
//! productions explicitly: bold, italic, inline code, fenced code
//! blocks. Headers and lists fall through to plain text; a full
//! implementation would pipe complete blocks through `markdown::render`.

#![allow(dead_code)] // Scaffolded helpers — kept for upcoming TUI features.
use crate::render::code_block;
use crate::render::style::{StyleCaps, bold, code_bg, dim};

/// Incremental markdown renderer.
///
/// Feed text chunks with [`push`]. Finished styled output is pulled
/// from [`take_output`]. Call [`flush`] at turn boundaries to emit any
/// remaining buffered content as plain text.
#[derive(Debug)]
pub struct StreamingMarkdownRenderer {
    caps: StyleCaps,
    state: State,
    /// Output accumulator (caller drains via `take_output`).
    out: String,
    /// Buffer for the current token being resolved (e.g. bold body).
    token_buf: String,
    /// Code block language detected after triple backticks.
    code_lang: String,
    /// Code block body.
    code_buf: String,
    // When we see a `*` we do not know yet if it starts italic or bold.
    // Same for backticks. These are tracked via the state enum.
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Plain,
    Star1,           // saw one *
    BoldOpen,        // inside ** ... **
    BoldClosing,     // inside bold, saw one * (waiting for second)
    ItalicOpen,      // inside * ... *
    Backtick1,       // saw one ` (could be inline or fence)
    Backtick2,       // saw two ` (ambiguous, discarded as literal)
    FenceLang,       // saw ```, reading lang until newline
    CodeBlock,       // inside ``` ... ```
    CodeBacktick1,   // in code block, saw ` on some position
    CodeBacktick2,   // in code block, saw ``
    InlineCode,      // inside ` ... `
}

impl StreamingMarkdownRenderer {
    pub fn new(caps: StyleCaps) -> Self {
        Self {
            caps,
            state: State::Plain,
            out: String::new(),
            token_buf: String::new(),
            code_lang: String::new(),
            code_buf: String::new(),
        }
    }

    /// Feed a chunk of text. Internally processes char by char; order
    /// of chunks must not affect final output (verified by proptest).
    pub fn push(&mut self, chunk: &str) {
        for ch in chunk.chars() {
            self.push_char(ch);
        }
    }

    /// Drain the accumulated styled output. Resets the output buffer
    /// but preserves parser state (so a subsequent `push` continues
    /// the current token).
    pub fn take_output(&mut self) -> String {
        std::mem::take(&mut self.out)
    }

    /// Flush any in-flight tokens as plain text. Use this at turn
    /// boundaries (e.g. `RunStateChanged::Idle`) to avoid leaking
    /// unclosed markdown. Resets to `Plain` state.
    pub fn flush(&mut self) {
        match self.state {
            State::Plain => {}
            State::Star1 => self.out.push('*'),
            State::BoldOpen => {
                // Emit as plain: ** + buffered body
                self.out.push_str("**");
                self.out.push_str(&self.token_buf);
                self.token_buf.clear();
            }
            State::BoldClosing => {
                self.out.push_str("**");
                self.out.push_str(&self.token_buf);
                self.out.push('*');
                self.token_buf.clear();
            }
            State::ItalicOpen => {
                self.out.push('*');
                self.out.push_str(&self.token_buf);
                self.token_buf.clear();
            }
            State::Backtick1 => self.out.push('`'),
            State::Backtick2 => self.out.push_str("``"),
            State::FenceLang => {
                self.out.push_str("```");
                self.out.push_str(&self.code_lang);
                self.code_lang.clear();
            }
            State::CodeBlock | State::CodeBacktick1 | State::CodeBacktick2 => {
                // Emit the unfinished code block as-is, no highlighting.
                self.out.push_str("```");
                if !self.code_lang.is_empty() {
                    self.out.push_str(&self.code_lang);
                    self.out.push('\n');
                }
                self.out.push_str(&self.code_buf);
                self.code_lang.clear();
                self.code_buf.clear();
            }
            State::InlineCode => {
                self.out.push('`');
                self.out.push_str(&self.token_buf);
                self.token_buf.clear();
            }
        }
        self.state = State::Plain;
    }

    fn push_char(&mut self, ch: char) {
        match self.state {
            State::Plain => self.plain(ch),
            State::Star1 => self.star1(ch),
            State::BoldOpen => self.bold_open(ch),
            State::BoldClosing => self.bold_closing(ch),
            State::ItalicOpen => self.italic_open(ch),
            State::Backtick1 => self.backtick1(ch),
            State::Backtick2 => self.backtick2(ch),
            State::FenceLang => self.fence_lang(ch),
            State::CodeBlock => self.code_block(ch),
            State::CodeBacktick1 => self.code_backtick1(ch),
            State::CodeBacktick2 => self.code_backtick2(ch),
            State::InlineCode => self.inline_code(ch),
        }
    }

    fn plain(&mut self, ch: char) {
        match ch {
            '*' => self.state = State::Star1,
            '`' => self.state = State::Backtick1,
            _ => self.out.push(ch),
        }
    }

    fn star1(&mut self, ch: char) {
        match ch {
            '*' => {
                // Second star → opens bold
                self.state = State::BoldOpen;
            }
            _ => {
                // First star was italic opener
                self.state = State::ItalicOpen;
                self.token_buf.push(ch);
            }
        }
    }

    fn bold_open(&mut self, ch: char) {
        if ch == '*' {
            self.state = State::BoldClosing;
        } else {
            self.token_buf.push(ch);
        }
    }

    fn bold_closing(&mut self, ch: char) {
        if ch == '*' {
            // Close the bold span, emit styled
            let styled = bold(std::mem::take(&mut self.token_buf), self.caps).to_string();
            self.out.push_str(&styled);
            self.state = State::Plain;
        } else {
            // False alarm — the single star was literal
            self.token_buf.push('*');
            self.token_buf.push(ch);
            self.state = State::BoldOpen;
        }
    }

    fn italic_open(&mut self, ch: char) {
        if ch == '*' {
            // Close italic
            let styled = dim(std::mem::take(&mut self.token_buf), self.caps).to_string();
            self.out.push_str(&styled);
            self.state = State::Plain;
        } else {
            self.token_buf.push(ch);
        }
    }

    fn backtick1(&mut self, ch: char) {
        match ch {
            '`' => self.state = State::Backtick2,
            _ => {
                // Start inline code
                self.state = State::InlineCode;
                self.token_buf.push(ch);
            }
        }
    }

    fn backtick2(&mut self, ch: char) {
        match ch {
            '`' => {
                // Triple backtick → fence open
                self.state = State::FenceLang;
            }
            _ => {
                // Only two backticks — emit literally and re-enter plain
                self.out.push_str("``");
                self.state = State::Plain;
                self.push_char(ch);
            }
        }
    }

    fn fence_lang(&mut self, ch: char) {
        if ch == '\n' {
            self.state = State::CodeBlock;
        } else {
            self.code_lang.push(ch);
        }
    }

    fn code_block(&mut self, ch: char) {
        if ch == '`' {
            self.state = State::CodeBacktick1;
        } else {
            self.code_buf.push(ch);
        }
    }

    fn code_backtick1(&mut self, ch: char) {
        if ch == '`' {
            self.state = State::CodeBacktick2;
        } else {
            self.code_buf.push('`');
            self.code_buf.push(ch);
            self.state = State::CodeBlock;
        }
    }

    fn code_backtick2(&mut self, ch: char) {
        if ch == '`' {
            // Fence close — render block and return to Plain
            let code: String = std::mem::take(&mut self.code_buf);
            let lang: String = std::mem::take(&mut self.code_lang);
            // Remove a trailing newline before the closing fence.
            let code_trimmed = code.strip_suffix('\n').unwrap_or(code.as_str());
            let rendered = code_block::render_block(code_trimmed, lang.trim(), self.caps);
            self.out.push('\n');
            self.out.push_str(&rendered);
            self.out.push('\n');
            self.state = State::Plain;
        } else {
            self.code_buf.push_str("``");
            self.code_buf.push(ch);
            self.state = State::CodeBlock;
        }
    }

    fn inline_code(&mut self, ch: char) {
        if ch == '`' {
            let styled = code_bg(std::mem::take(&mut self.token_buf), self.caps).to_string();
            self.out.push_str(&styled);
            self.state = State::Plain;
        } else {
            self.token_buf.push(ch);
        }
    }
}

/// Convenience helper: render a complete string via the streaming
/// renderer (for tests and parity with `markdown::render`).
pub fn render_complete(text: &str, caps: StyleCaps) -> String {
    let mut r = StreamingMarkdownRenderer::new(caps);
    r.push(text);
    r.flush();
    r.take_output()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> StyleCaps {
        StyleCaps::plain()
    }

    // ---- Plain passthrough ----

    #[test]
    fn test_plain_text_unchanged() {
        let mut r = StreamingMarkdownRenderer::new(plain());
        r.push("hello world");
        r.flush();
        assert_eq!(r.take_output(), "hello world");
    }

    #[test]
    fn test_empty_input_empty_output() {
        let mut r = StreamingMarkdownRenderer::new(plain());
        r.flush();
        assert_eq!(r.take_output(), "");
    }

    // ---- Bold ----

    #[test]
    fn test_bold_emitted_on_close() {
        let out = render_complete("**bold**", plain());
        assert_eq!(out, "bold");
    }

    #[test]
    fn test_bold_chunked_by_char() {
        let mut r = StreamingMarkdownRenderer::new(plain());
        for ch in "**bold**".chars() {
            r.push(&ch.to_string());
        }
        r.flush();
        assert_eq!(r.take_output(), "bold");
    }

    #[test]
    fn test_bold_unclosed_flushes_as_plain() {
        // An opened but unclosed ** should flush as literal.
        let out = render_complete("**unclosed", plain());
        assert_eq!(out, "**unclosed");
    }

    #[test]
    fn test_bold_with_surrounding_text() {
        let out = render_complete("say **hi** there", plain());
        assert_eq!(out, "say hi there");
    }

    // ---- Italic ----

    #[test]
    fn test_italic_emitted_on_close() {
        let out = render_complete("*italic*", plain());
        assert_eq!(out, "italic");
    }

    #[test]
    fn test_italic_unclosed_flushes_as_plain() {
        let out = render_complete("*unclosed", plain());
        assert_eq!(out, "*unclosed");
    }

    // ---- Inline code ----

    #[test]
    fn test_inline_code_emitted_on_close() {
        let out = render_complete("`let x = 1`", plain());
        assert_eq!(out, "let x = 1");
    }

    #[test]
    fn test_inline_code_unclosed_flushes_as_plain() {
        let out = render_complete("`broken", plain());
        assert_eq!(out, "`broken");
    }

    // ---- Code block (fenced) ----

    #[test]
    fn test_code_block_renders_on_close() {
        let md = "```rust\nfn main() {}\n```";
        let out = render_complete(md, plain());
        assert!(out.contains("fn main() {}"));
        assert!(out.contains("rust"));
    }

    #[test]
    fn test_code_block_chunked_char_by_char() {
        let md = "```rust\nfn x() {}\n```";
        let mut r = StreamingMarkdownRenderer::new(plain());
        for ch in md.chars() {
            r.push(&ch.to_string());
        }
        r.flush();
        let out = r.take_output();
        assert!(out.contains("fn x() {}"));
    }

    #[test]
    fn test_code_block_no_language_uses_code_label() {
        let md = "```\nhello\n```";
        let out = render_complete(md, plain());
        assert!(out.contains("code"));
        assert!(out.contains("hello"));
    }

    #[test]
    fn test_code_block_unclosed_flushes_as_plain() {
        let md = "```rust\nfn main() {}";
        let out = render_complete(md, plain());
        // Should contain the raw text since fence never closed.
        assert!(out.contains("fn main() {}"));
    }

    // ---- Mixed content ----

    #[test]
    fn test_mixed_bold_and_italic() {
        let out = render_complete("**b** and *i*", plain());
        assert_eq!(out, "b and i");
    }

    #[test]
    fn test_mixed_bold_inline_code() {
        let out = render_complete("**hi** `code`", plain());
        assert_eq!(out, "hi code");
    }

    // ---- Idempotency across chunk boundaries ----

    #[test]
    fn test_chunking_does_not_affect_output() {
        let input = "hello **world** `code` end";
        let one_shot = render_complete(input, plain());

        let mut char_by_char = StreamingMarkdownRenderer::new(plain());
        for ch in input.chars() {
            char_by_char.push(&ch.to_string());
        }
        char_by_char.flush();
        let piecewise = char_by_char.take_output();

        assert_eq!(one_shot, piecewise);
    }

    #[test]
    fn test_take_output_resets_buffer() {
        let mut r = StreamingMarkdownRenderer::new(plain());
        r.push("hello");
        r.flush();
        let first = r.take_output();
        assert_eq!(first, "hello");
        let second = r.take_output();
        assert_eq!(second, "");
    }

    #[test]
    fn test_push_after_flush_continues_plain() {
        let mut r = StreamingMarkdownRenderer::new(plain());
        r.push("first ");
        r.flush();
        r.push("second");
        r.flush();
        assert_eq!(r.take_output(), "first second");
    }

    #[test]
    fn test_flush_after_bold_open_emits_literal() {
        let mut r = StreamingMarkdownRenderer::new(plain());
        r.push("**unclosed");
        r.flush();
        assert_eq!(r.take_output(), "**unclosed");
    }

    #[test]
    fn test_tty_bold_contains_ansi() {
        let mut r = StreamingMarkdownRenderer::new(StyleCaps::full());
        r.push("**hi**");
        r.flush();
        let out = r.take_output();
        assert!(out.contains("\x1b["));
        assert!(out.contains("hi"));
    }

    #[test]
    fn test_consecutive_bold_spans() {
        let out = render_complete("**a** **b**", plain());
        assert_eq!(out, "a b");
    }

    #[test]
    fn test_bold_with_spaces_inside() {
        let out = render_complete("**hi there**", plain());
        assert_eq!(out, "hi there");
    }

    #[test]
    fn test_chunking_by_two_chars() {
        let input = "hello **bold** world `code` end";
        let one_shot = render_complete(input, plain());

        let mut r = StreamingMarkdownRenderer::new(plain());
        let chars: Vec<char> = input.chars().collect();
        for pair in chars.chunks(2) {
            let s: String = pair.iter().collect();
            r.push(&s);
        }
        r.flush();

        assert_eq!(r.take_output(), one_shot);
    }

    #[test]
    fn test_rapid_push_is_linear_time() {
        // Smoke test: feeding 10K chars should complete quickly.
        let input = "a".repeat(10_000);
        let mut r = StreamingMarkdownRenderer::new(plain());
        let start = std::time::Instant::now();
        r.push(&input);
        r.flush();
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_millis() < 500,
            "streaming 10K chars took {elapsed:?}"
        );
        assert_eq!(r.take_output().len(), 10_000);
    }

    // ---- Property tests (T1.4b) ----
    //
    // These prove chunk-order idempotency: same input → same output
    // regardless of how it is split into push() calls.

    use proptest::prelude::*;

    fn feed_chunked(input: &str, chunk_size: usize, caps: StyleCaps) -> String {
        let mut r = StreamingMarkdownRenderer::new(caps);
        let bytes = input.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let mut end = (i + chunk_size).min(bytes.len());
            while end > i && !input.is_char_boundary(end) {
                end -= 1;
            }
            if end == i {
                end = i + 1;
                while end < bytes.len() && !input.is_char_boundary(end) {
                    end += 1;
                }
            }
            r.push(&input[i..end]);
            i = end;
        }
        r.flush();
        r.take_output()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn prop_chunk_size_does_not_change_output(
            input in prop::string::string_regex("[a-zA-Z0-9 *`\n]{0,60}").unwrap(),
            chunk_size in 1usize..12,
        ) {
            let one_shot = render_complete(&input, plain());
            let chunked = feed_chunked(&input, chunk_size, plain());
            prop_assert_eq!(one_shot, chunked);
        }

        #[test]
        fn prop_char_by_char_matches_one_shot(
            input in prop::string::string_regex("[a-zA-Z0-9 *`\n_-]{0,40}").unwrap(),
        ) {
            let one_shot = render_complete(&input, plain());
            let piecewise = feed_chunked(&input, 1, plain());
            prop_assert_eq!(one_shot, piecewise);
        }

        #[test]
        fn prop_never_panics_on_ascii(
            input in prop::string::string_regex("[\\x20-\\x7e\n]{0,150}").unwrap(),
        ) {
            let _ = render_complete(&input, plain());
        }

        #[test]
        fn prop_never_panics_on_unicode(
            input in "\\PC{0,80}",
        ) {
            let _ = render_complete(&input, plain());
        }

        #[test]
        fn prop_alphanumeric_output_size_equals_input(
            input in prop::string::string_regex("[a-z0-9 ]{0,80}").unwrap(),
        ) {
            let out = render_complete(&input, plain());
            prop_assert_eq!(out.len(), input.len());
        }

        #[test]
        fn prop_flush_separates_independent_segments(
            a in prop::string::string_regex("[a-z ]{0,20}").unwrap(),
            b in prop::string::string_regex("[a-z ]{0,20}").unwrap(),
        ) {
            let mut r = StreamingMarkdownRenderer::new(plain());
            r.push(&a);
            r.flush();
            r.push(&b);
            r.flush();
            let combined = r.take_output();
            let expected = format!(
                "{}{}",
                render_complete(&a, plain()),
                render_complete(&b, plain())
            );
            prop_assert_eq!(combined, expected);
        }
    }

    #[test]
    fn test_streaming_100k_chars_under_1s() {
        let input = "a".repeat(100_000);
        let start = std::time::Instant::now();
        let _ = render_complete(&input, plain());
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_millis() < 1000,
            "streaming 100K chars took {elapsed:?}, expected < 1s"
        );
    }
}
