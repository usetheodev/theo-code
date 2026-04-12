---
type: report
question: "What is the Rust terminal rendering ecosystem for building a professional AI coding CLI?"
generated_at: 2026-04-11T12:00:00Z
confidence: 0.92
sources_used: 24
---

# Report: Rust Terminal Rendering Ecosystem for AI Coding CLIs

## Executive Summary

The Rust terminal ecosystem is mature and well-layered. For Theo's CLI (an AI coding agent, not a full TUI), **crossterm** (low-level terminal control) + **pulldown-cmark** (markdown parsing) + **syntect** (syntax highlighting) is the correct stack. Ratatui is overkill for a streaming-first agent CLI. The current Theo CLI already uses crossterm 0.28 and pulldown-cmark 0.13 but renders with raw ANSI escape codes -- upgrading to structured rendering through syntect and proper markdown-to-terminal conversion is the high-value next step.

## 1. Crossterm Deep Dive

### What It Is

Crossterm (latest: **0.29.0**, 73M+ downloads) is a pure-Rust, cross-platform terminal manipulation library. It is the de facto standard backend for terminal apps in Rust [1][2].

### Full API Surface

Crossterm is organized into four modules:

| Module | Purpose | Key Types |
|---|---|---|
| `crossterm::style` | Text styling: colors (16/256/RGB), bold, italic, underline, strikethrough, dimmed | `SetForegroundColor`, `SetBackgroundColor`, `SetAttribute`, `ResetColor`, `Print`, `Stylize` trait |
| `crossterm::cursor` | Cursor movement and visibility | `MoveTo`, `MoveUp/Down/Left/Right`, `SavePosition`, `RestorePosition`, `Hide`, `Show` |
| `crossterm::terminal` | Terminal state: size, raw mode, alternate screen, clear, scroll | `size()`, `enable_raw_mode()`, `disable_raw_mode()`, `EnterAlternateScreen`, `LeaveAlternateScreen`, `Clear`, `SetSize` |
| `crossterm::event` | Input events: keyboard, mouse, paste, resize | `Event`, `KeyEvent`, `MouseEvent`, `KeyCode`, `KeyModifiers`, `read()`, `poll()`, `EventStream` |

### Command Execution Model

Crossterm uses a **command pattern** with two execution modes:

```rust
use crossterm::{execute, queue};
use crossterm::style::{SetForegroundColor, Color, Print, ResetColor};
use std::io::{stdout, Write};

// Immediate execution (flushes immediately)
execute!(
    stdout(),
    SetForegroundColor(Color::Green),
    Print("success"),
    ResetColor
)?;

// Queued execution (batched, flush manually -- better performance)
queue!(
    stdout(),
    SetForegroundColor(Color::Cyan),
    Print("queued output"),
    ResetColor
)?;
stdout().flush()?;
```

**Performance implication**: `queue!` + manual flush is significantly faster for multiple operations because it batches ANSI sequences into a single write syscall. For streaming LLM output, this matters.

### Raw Mode vs Cooked Mode

| Mode | Behavior | When to Use |
|---|---|---|
| **Cooked (default)** | OS handles line editing, echo, signals (Ctrl+C = SIGINT) | Normal CLI output, streaming text, non-interactive commands |
| **Raw** | Every keystroke goes directly to app, no echo, no line buffering | Interactive REPL input, key-by-key capture, permission prompts |

**For Theo CLI**: Stay in cooked mode for LLM streaming output. Switch to raw mode only for interactive prompts (tool approval, y/n confirmations). The current rustyline-based REPL handles this correctly -- rustyline manages its own raw mode internally.

```rust
// Only enter raw mode for specific interactions
crossterm::terminal::enable_raw_mode()?;
// ... capture keystrokes ...
crossterm::terminal::disable_raw_mode()?;
```

### Async Event Loop with Tokio

The `event-stream` feature enables async event reading via `EventStream`:

```rust
// Cargo.toml
// crossterm = { version = "0.29", features = ["event-stream"] }
// futures = "0.3"

use crossterm::event::{EventStream, Event, KeyCode};
use futures::StreamExt;

async fn event_loop() {
    let mut reader = EventStream::new();

    loop {
        tokio::select! {
            // Wait for terminal events
            maybe_event = reader.next() => {
                match maybe_event {
                    Some(Ok(Event::Key(key))) => {
                        if key.code == KeyCode::Esc { break; }
                    }
                    Some(Ok(Event::Resize(w, h))) => {
                        // Handle terminal resize
                        handle_resize(w, h);
                    }
                    _ => {}
                }
            }
            // Concurrently wait for LLM stream chunks
            chunk = llm_stream.next() => {
                if let Some(text) = chunk {
                    render_streaming_text(&text);
                }
            }
        }
    }
}
```

**Critical constraint**: You cannot mix synchronous `read()`/`poll()` with `EventStream` -- pick one approach per application [3].

### Terminal Resize Handling

Crossterm captures `SIGWINCH` on Unix via `signal-hook-mio` and emits `Event::Resize(cols, rows)`. On Windows, resize events come from `INPUT_RECORD` [4].

```rust
// Get current terminal size (synchronous)
let (cols, rows) = crossterm::terminal::size()?;

// Or react to resize events in the event loop
match event {
    Event::Resize(width, height) => {
        // Re-wrap text, adjust layout
    }
    _ => {}
}
```

### Performance Characteristics

- **Startup**: Near-zero overhead (pure Rust, no C bindings)
- **Write operations**: `queue!` batching reduces syscalls substantially
- **Event reading**: mio-based, efficient epoll/kqueue on Unix
- **Memory**: Minimal allocations in the command pipeline
- **Caveat**: `terminal::size()` can spawn a `tput` process on some systems (slow in tight loops) -- cache the value and update on `Event::Resize` [4]

---

## 2. Ratatui: When You Need It (and When You Do Not)

### The Decision

| Scenario | Use |
|---|---|
| Full TUI with panels, tabs, scrollable lists, split panes | Ratatui + Crossterm |
| Streaming text output with interleaved tool results (like Claude Code) | **Crossterm alone** |
| Interactive selection menus within a streaming CLI | Crossterm + dialoguer |

**Verdict for Theo CLI: Crossterm alone is sufficient.** Ratatui's immediate-mode rendering (redraw entire UI every frame) conflicts with a streaming-output model where you append text incrementally. Ratatui is designed for apps like `gitui` or `lazygit` where the entire screen is redrawn 30+ times per second [5][6].

### When Ratatui Would Become Relevant

If Theo CLI evolves to have:
- A persistent status bar showing token count / model / mode
- Split-pane view (code on left, agent output on right)
- Scrollable history with vim keybindings

Then Ratatui becomes the right choice. Current architecture (streaming append-only output) does not need it.

### Ratatui's Rendering Model (for reference)

Ratatui maintains two buffers (current + previous). On each `terminal.draw()` call, it diffs them and only writes changed cells. This is why it is fast for full-screen TUIs but wasteful for append-only CLIs [6].

```rust
// Ratatui pattern (NOT recommended for streaming CLI)
terminal.draw(|frame| {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(frame.area());

    frame.render_widget(Paragraph::new(output_text), layout[0]);
    frame.render_widget(status_bar, layout[1]);
})?;
```

---

## 3. Markdown Terminal Rendering

### 3.1 pulldown-cmark (already in use)

**Version**: 0.13.1 (latest). Already a workspace dependency in Theo [7].

pulldown-cmark is an event-based CommonMark parser. It emits a stream of `Event` variants that you consume to build output:

```rust
use pulldown_cmark::{Parser, Event, Tag, Options, CodeBlockKind};

let mut opts = Options::empty();
opts.insert(Options::ENABLE_TABLES);
opts.insert(Options::ENABLE_STRIKETHROUGH);
opts.insert(Options::ENABLE_TASKLISTS);

let parser = Parser::new_ext(markdown_text, opts);

for event in parser {
    match event {
        Event::Start(Tag::Heading { level, .. }) => {
            // Emit bold + color for headers
        }
        Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(lang))) => {
            // Start syntax-highlighted code block
            // `lang` contains "rust", "python", etc.
        }
        Event::Text(text) => {
            // Emit styled text
        }
        Event::Code(code) => {
            // Inline code: render with background color
        }
        Event::Start(Tag::List(_)) => { /* indent */ }
        Event::Start(Tag::BlockQuote(_)) => { /* dim + border */ }
        Event::Start(Tag::Table(_)) => { /* table rendering */ }
        _ => {}
    }
}
```

**For Theo CLI**: The key insight is that LLM output arrives as streaming text, not complete markdown. You need an **incremental** approach: buffer text, detect when markdown blocks are complete, then render them. For inline formatting (bold, code), you can render in real-time. For code blocks, you must buffer until the closing fence.

### 3.2 termimad

**Version**: latest uses crossterm 0.27 (behind Theo's 0.28). Built by the author of `broot` [8].

termimad renders markdown directly to terminal with crossterm styling. It handles wrapping, table balancing, and scrolling. However:

**Limitations for Theo**:
- No syntax highlighting for code blocks (only monospace rendering)
- Crossterm version may lag behind
- Designed for static text rendering, not streaming
- Re-exports crossterm (version coupling risk)

**Verdict**: Not a good fit. Theo should use pulldown-cmark (already imported) + syntect for a more controlled pipeline.

### 3.3 syntect: Syntax Highlighting Engine

**Version**: 5.2+ (latest). Used by bat, delta, xi-editor, Zola [9][10].

syntect uses Sublime Text syntax definitions (`.sublime-syntax`) and themes (`.tmTheme`) to produce highlighted output. For terminal output, it generates 24-bit ANSI escape sequences.

```rust
use syntect::easy::HighlightLines;
use syntect::parsing::SyntaxSet;
use syntect::highlighting::{ThemeSet, Style};
use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};

// Load once at startup (23ms), cache as &'static or Arc
let syntax_set = SyntaxSet::load_defaults_newlines();
let theme_set = ThemeSet::load_defaults();
let theme = &theme_set.themes["base16-ocean.dark"];

fn highlight_code(code: &str, lang: &str, ss: &SyntaxSet, theme: &Theme) -> String {
    let syntax = ss.find_syntax_by_token(lang)
        .unwrap_or_else(|| ss.find_syntax_plain_text());
    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut output = String::new();

    for line in LinesWithEndings::from(code) {
        let ranges: Vec<(Style, &str)> = highlighter
            .highlight_line(line, ss)
            .unwrap();
        let escaped = as_24_bit_terminal_escaped(&ranges, false);
        output.push_str(&escaped);
    }
    output.push_str("\x1b[0m"); // Reset
    output
}
```

**Performance notes**:
- `SyntaxSet::load_defaults_newlines()` takes ~23ms. Load once at startup.
- The `fancy-regex` feature (pure Rust, no C deps) is **extremely slow in debug mode**. Use `--release` for testing or use `default-onig` for development.
- `SyntaxSet` is `Send + Sync + Clone` -- safe for multi-threaded use.
- Supports 170+ languages out of the box.

**Recommended Cargo config**:
```toml
# Pure Rust (no C dependency), good for cross-compilation
syntect = { version = "5", default-features = false, features = ["default-fancy"] }
```

### 3.4 How to Render Code Blocks with Syntax Highlighting

The complete pipeline for Theo:

```
LLM stream --> buffer text --> detect code fence
  --> if code block complete:
       pulldown-cmark parses --> extract lang + code
       syntect highlights --> ANSI-colored output
       render with box border + language label
  --> if inline markdown:
       pulldown-cmark parses --> crossterm styled output
```

Example rendering target:

```
  rust ─────────────────────────────
  │ fn main() {                    │
  │     println!("Hello, world!"); │
  │ }                              │
  ──────────────────────────────────
```

### 3.5 How to Render Tables, Lists, Headers

| Element | Terminal Rendering Strategy |
|---|---|
| `# Header` | Bold + bright color + newline padding |
| `## Header` | Bold + dimmer color |
| `**bold**` | `crossterm::style::Attribute::Bold` |
| `` `inline code` `` | Dim background (gray) or different foreground color |
| `- list item` | Indent + bullet character (`  *` or `  -`) |
| `> blockquote` | Dim foreground + left border (`  |`) |
| Tables | Measure column widths, render with Unicode box-drawing chars (`+---+---+`) |
| `---` (hr) | Full-width dim line (`───────`) |
| Links `[text](url)` | Underlined text + dim URL in parentheses (or OSC 8 hyperlinks for supporting terminals) |

---

## 4. Other Key Crates

### 4.1 indicatif (Progress Bars and Spinners)

**Version**: 0.18.4, 136M+ downloads [11].

Already transitively available in Theo's dependency tree (via Cargo.lock). Useful for:
- Spinner during GRAPHCTX index build
- Progress bar during file scanning / embedding generation
- Multi-progress for parallel sub-agent execution

```rust
use indicatif::{ProgressBar, ProgressStyle, MultiProgress};
use std::time::Duration;

// Spinner for ongoing operations
let spinner = ProgressBar::new_spinner();
spinner.set_style(
    ProgressStyle::default_spinner()
        .tick_strings(&["   ", ".  ", ".. ", "...", " ..", "  .", "   "])
        .template("{spinner} {msg}")
        .unwrap()
);
spinner.set_message("Building code graph...");
spinner.enable_steady_tick(Duration::from_millis(120));

// ... do work ...
spinner.finish_with_message("Code graph ready (1,247 symbols)");

// Multi-progress for parallel sub-agents
let multi = MultiProgress::new();
let agent1 = multi.add(ProgressBar::new_spinner());
let agent2 = multi.add(ProgressBar::new_spinner());
agent1.set_message("[Researcher] analyzing codebase...");
agent2.set_message("[Implementer] writing tests...");
```

**Integration note**: indicatif writes to stderr by default. It uses the `console` crate internally for TTY detection. Plays well with crossterm as long as you do not mix raw mode with indicatif output.

### 4.2 console (High-Level Terminal Styling)

**Version**: 0.16.0, by the same author as indicatif (Armin Ronacher / console-rs) [12].

The `console` crate provides:
- **TTY detection**: `console::Term::stdout().is_term()` -- determines if output goes to a terminal or pipe
- **ANSI stripping**: `strip_ansi_codes(&str)` -- removes color codes for piped output
- **Text width**: `measure_text_width(&str)` -- accounts for ANSI codes and Unicode width
- **Style API**: Higher-level than raw crossterm escapes

```rust
use console::{style, Term, measure_text_width};

// Style with automatic TTY detection
println!("{} Reading file...", style("*").cyan());
println!("{}", style("Error: file not found").red().bold());

// Detect piped output
let term = Term::stdout();
if !term.is_term() {
    // Strip colors, simplify output
}

// Measure actual display width (ignoring ANSI escapes)
let text = "\x1b[32mHello\x1b[0m";
let width = measure_text_width(text); // Returns 5, not 14
```

**For Theo CLI**: `console` is the right way to handle TTY detection and piped output. The current renderer uses hard-coded `\x1b[...]` sequences -- migrating to `console::style()` adds automatic color stripping when piped.

### 4.3 dialoguer (Interactive Prompts)

**Version**: 0.11.x, by console-rs (same ecosystem) [13].

Provides ready-made interactive prompts:

```rust
use dialoguer::{Confirm, Select, Input, theme::ColorfulTheme};

// Yes/No confirmation (for tool approval)
let approved = Confirm::with_theme(&ColorfulTheme::default())
    .with_prompt("Allow bash execution: `rm -rf target/`?")
    .default(false)
    .interact()?;

// Selection menu (for mode switching, model selection)
let modes = &["Agent", "Plan", "Ask"];
let selection = Select::with_theme(&ColorfulTheme::default())
    .with_prompt("Select mode")
    .items(modes)
    .default(0)
    .interact()?;

// Text input with validation
let path: String = Input::with_theme(&ColorfulTheme::default())
    .with_prompt("Project directory")
    .validate_with(|input: &String| {
        if std::path::Path::new(input).exists() {
            Ok(())
        } else {
            Err("Directory does not exist")
        }
    })
    .interact_text()?;
```

**For Theo CLI**: dialoguer is ideal for tool approval prompts (replacing raw y/n). It handles raw mode internally and plays well with rustyline.

### 4.4 textwrap (Text Wrapping)

**Version**: 0.16.2 [14].

Handles word wrapping respecting terminal width:

```rust
use textwrap::{wrap, Options};

// Basic wrapping to terminal width
let text = "Long explanation from the LLM about what it plans to do...";
let options = Options::new(80)
    .initial_indent("  ")      // First line indent
    .subsequent_indent("  ");  // Continuation indent

for line in wrap(text, &options) {
    println!("{}", line);
}

// Auto-detect terminal width
let options = Options::with_termwidth(); // Requires "terminal_size" feature
```

**For Theo CLI**: Useful for wrapping LLM explanation text within tool result boxes and for the summary output. Not needed for code blocks (those should not be wrapped).

---

## 5. Architecture Patterns

### 5.1 Recommended Module Structure for Theo CLI Renderer

```
apps/theo-cli/src/
  renderer/
    mod.rs          // TerminalRenderer trait + factory
    streaming.rs    // Real-time streaming text with incremental markdown
    markdown.rs     // Complete markdown-to-terminal conversion
    code_block.rs   // Syntax-highlighted code block rendering
    tool_result.rs  // Tool call/result display (current render_tool_completed)
    style.rs        // Color theme, style constants, TTY detection
    table.rs        // Terminal table rendering
```

### 5.2 Streaming Architecture (Event-Driven)

The current Theo CLI already uses an event-driven pattern (EventBus + EventListener). The rendering architecture should be:

```
Agent Runtime
  |
  v
EventBus ──> CliRenderer (EventListener)
               |
               +-- ContentDelta: StreamingMarkdownRenderer
               |     |-- buffers partial text
               |     |-- detects complete blocks
               |     |-- renders inline formatting immediately
               |     |-- renders code blocks when fence closes
               |
               +-- ToolCallCompleted: ToolResultRenderer
               |     |-- formats per-tool output (read, write, bash, etc.)
               |
               +-- ReasoningDelta: DimmedTextRenderer
               |
               +-- RunStateChanged: StatusRenderer
```

### 5.3 Buffered Output vs Direct Write

| Approach | When to Use |
|---|---|
| **Direct write** (`eprint!`) | Streaming text deltas (low latency matters) |
| **Buffered write** (`BufWriter` + flush) | Tool results, code blocks, tables (multiple ANSI sequences) |
| **queue! + flush** | Any multi-command crossterm sequence |

```rust
use std::io::{BufWriter, Write};
use crossterm::{queue, style::*};

fn render_code_block(code: &str, lang: &str) -> std::io::Result<()> {
    let mut buf = BufWriter::new(std::io::stderr());

    // Header line
    queue!(buf, SetForegroundColor(Color::DarkGrey), Print(format!("  {} ", lang)))?;
    queue!(buf, Print("─".repeat(60)), Print("\n"), ResetColor)?;

    // Highlighted code (syntect output already contains ANSI)
    let highlighted = highlight_code(code, lang);
    for line in highlighted.lines() {
        queue!(buf, SetForegroundColor(Color::DarkGrey), Print("  | "))?;
        queue!(buf, ResetColor, Print(line), Print("\n"))?;
    }

    // Footer line
    queue!(buf, SetForegroundColor(Color::DarkGrey))?;
    queue!(buf, Print(format!("  {}\n", "─".repeat(62))), ResetColor)?;

    buf.flush()
}
```

### 5.4 Handling Terminal Resize

```rust
// Cache terminal width, update on resize
use std::sync::atomic::{AtomicU16, Ordering};

static TERM_WIDTH: AtomicU16 = AtomicU16::new(80);

fn update_term_width() {
    if let Ok((w, _)) = crossterm::terminal::size() {
        TERM_WIDTH.store(w, Ordering::Relaxed);
    }
}

fn term_width() -> u16 {
    TERM_WIDTH.load(Ordering::Relaxed)
}
```

### 5.5 Supporting Piped Output (TTY Detection)

```rust
use console::Term;

pub struct RenderConfig {
    pub colors: bool,
    pub unicode: bool,
    pub width: u16,
}

impl RenderConfig {
    pub fn detect() -> Self {
        let term = Term::stderr(); // Theo outputs to stderr
        let is_tty = term.is_term();

        RenderConfig {
            colors: is_tty,
            unicode: is_tty,
            width: if is_tty {
                crossterm::terminal::size().map(|(w, _)| w).unwrap_or(80)
            } else {
                80
            },
        }
    }
}

// When not a TTY: strip colors, use ASCII borders, skip spinners
fn render_tool_result(tool: &str, success: bool, config: &RenderConfig) {
    if config.colors {
        eprintln!("  \x1b[36m*\x1b[0m {} \x1b[32mok\x1b[0m", tool);
    } else {
        eprintln!("  * {} ok", tool);
    }
}
```

---

## 6. Real-World Examples

### 6.1 How `bat` Renders Syntax-Highlighted Files

**Pipeline**: file input -> language detection (by extension/shebang) -> syntect parsing with Sublime grammars -> theme-based colorization -> ANSI escape sequences -> optional paging via `less` [10].

Key architectural decisions:
- syntect's `SyntaxSet` and `ThemeSet` loaded once at startup
- Falls back to 8-bit colors if `COLORTERM` is not set to `truecolor`
- Strips decorations (line numbers, grid) when output is piped
- The `bat` crate (library version) can be used as a dependency for other tools

### 6.2 How `delta` Renders Git Diffs

**Pipeline**: git diff input -> hunk parsing -> line buffering into "subhunks" -> syntect highlighting per language -> within-line diff computation -> style superimposition (diff colors + syntax colors) -> ANSI output [15].

Key architectural decisions:
- Buffers minus/plus lines separately, processes at subhunk boundary
- Style superimposition: diff emphasis (red/green) overlaid on syntax highlighting foreground colors
- Uses `ansi_term` crate (older) for style composition
- Smart paging (auto-detects when to use `less`)

### 6.3 How `gitui` Handles Its TUI

**Architecture**: crossterm (raw mode, alternate screen, events) -> ratatui (double-buffered rendering, widget layout) -> custom components (diff viewer, file tree, commit list) [6].

Event loop pattern:
- `QueueEvent` enum: `Tick`, `InputEvent`, `AsyncEvent`, `SpinnerUpdate`
- Background git operations via async tasks
- 80ms spinner tick interval
- 5-second periodic refresh tick
- Full-screen redraw on every frame (immediate mode)

**Lesson for Theo**: gitui's full-screen approach is appropriate for a git client but wrong for a streaming CLI. The event-driven pattern (separating input events from async work) is the transferable lesson.

---

## Gaps

1. **Streaming markdown renderer**: No existing crate handles incremental markdown-to-terminal rendering (all assume complete input). This must be built custom for Theo.

2. **OSC 8 hyperlink support**: Modern terminals support clickable links via `\x1b]8;;URL\x07text\x1b]8;;\x07`. No research done on terminal coverage for this.

3. **Image rendering**: Some terminals support Kitty/iTerm2 image protocol. Could be relevant for rendering diagrams. Not explored.

4. **Accessibility**: Screen reader compatibility with ANSI-styled output is under-explored in the Rust ecosystem.

5. **syntect startup cost**: 23ms load time is fine for CLI startup but could matter if lazy-loaded. No profiling data for Theo's specific startup path.

---

## Recommendations

### Immediate (P0) -- Upgrade renderer with structured styling

1. **Add `console` crate** for TTY detection and ANSI stripping. Replace hard-coded `\x1b[...]` in `renderer.rs` with `console::style()` or at minimum use it for detection.

2. **Add `syntect` crate** (with `default-fancy` feature) for code block highlighting. Load `SyntaxSet` and `ThemeSet` once at CLI startup, share via `Arc`.

3. **Build incremental markdown renderer**: Use pulldown-cmark events to render LLM streaming output. Buffer code blocks until fence closes, render inline formatting immediately.

### Short-term (P1) -- Interactive improvements

4. **Add `dialoguer`** for tool approval prompts (replace raw y/n input).

5. **Add `indicatif`** spinners for GRAPHCTX build and long-running operations.

6. **Implement terminal resize handling**: Cache width in `AtomicU16`, update on `Event::Resize`.

### Medium-term (P2) -- Polish

7. **Add `textwrap`** for wrapping LLM explanation text to terminal width.

8. **Code block rendering with language labels and box borders** (see example in section 5.3).

9. **Table rendering** for structured LLM output (grep results, file lists).

### Not Recommended

- **Ratatui**: Overkill for current streaming-first architecture. Revisit only if Theo CLI evolves to full-screen TUI.
- **termimad**: Version coupling with crossterm, no syntax highlighting, designed for static rendering.
- **ansi_term**: Unmaintained, replaced by `console` crate ecosystem.

---

## Sources

1. [crossterm GitHub repository](https://github.com/crossterm-rs/crossterm) -- Source code and examples
2. [crossterm on crates.io](https://crates.io/crates/crossterm) -- v0.29.0, 73M+ downloads
3. [crossterm EventStream docs](https://docs.rs/crossterm/latest/crossterm/event/struct.EventStream.html) -- Async event reading API
4. [crossterm terminal resize source](https://github.com/crossterm-rs/crossterm/blob/master/src/event/source/unix/mio.rs) -- SIGWINCH handling
5. [Ratatui documentation](https://ratatui.rs/) -- When to use Ratatui vs crossterm alone
6. [gitui source code](https://github.com/gitui-org/gitui) -- Full TUI architecture reference
7. [pulldown-cmark on crates.io](https://crates.io/crates/pulldown-cmark) -- v0.13.1, CommonMark parser
8. [termimad GitHub](https://github.com/Canop/termimad) -- Terminal markdown rendering
9. [syntect GitHub](https://github.com/trishume/syntect/) -- Syntax highlighting engine
10. [bat GitHub](https://github.com/sharkdp/bat) -- Reference implementation using syntect
11. [indicatif on crates.io](https://crates.io/crates/indicatif) -- v0.18.4, progress bars
12. [console crate GitHub](https://github.com/console-rs/console) -- TTY detection, styling
13. [dialoguer docs](https://docs.rs/dialoguer/latest/dialoguer/) -- Interactive prompts
14. [textwrap GitHub](https://github.com/mgeisler/textwrap) -- v0.16.2, text wrapping
15. [delta ARCHITECTURE.md](https://github.com/dandavison/delta/blob/main/ARCHITECTURE.md) -- Diff rendering pipeline
16. [Crossterm Rust Guide 2025](https://generalistprogrammer.com/tutorials/crossterm-rust-crate-guide) -- Tutorial
17. [crossterm event-stream-tokio example](https://github.com/crossterm-rs/crossterm/blob/master/examples/event-stream-tokio.rs) -- Async pattern
18. [Ratatui backend comparison](https://ratatui.rs/concepts/backends/comparison/) -- Backend options
19. [syntect docs.rs](https://docs.rs/syntect) -- API documentation
20. [anstream blog post](https://epage.github.io/blog/2023/03/anstream-simplifying-terminal-styling/) -- Modern ANSI handling
21. [Rust Markdown Syntax Highlighting guide](https://bandarra.me/posts/Rust-Markdown-Syntax-Highlighting-A-Practical-Guide) -- pulldown-cmark + syntect integration
22. [tui-markdown crate](https://docs.rs/tui-markdown/latest/tui_markdown/) -- Ratatui markdown widget (reference only)
23. [Terminal markdown rendering research gist](https://gist.github.com/nelson-ddatalabs/21290f85c8bd13bb56676560c114980d) -- Comprehensive patterns
24. [Indicatif Rust Guide 2025](https://generalistprogrammer.com/tutorials/indicatif-rust-crate-guide) -- Tutorial
