# theo-cli Rendering Architecture

> **Status**: Implemented — Fase 0 + Fase 1 + Fase 2 + Fase 3 + Fase 4 of
> `docs/roadmap/cli-professionalization.md` executed and validated on
> vast.ai (contract 34453774).

This document is the authoritative reference for how `apps/theo-cli`
renders output to the terminal. It is the post-implementation sibling of
`docs/roadmap/cli-professionalization.md` and should be updated whenever
the rendering pipeline changes.

## 1. Invariants

These are enforced by CI / tests and cannot be broken without an ADR:

1. **Single source of ANSI emission**: only `apps/theo-cli/src/render/style.rs`
   is allowed to emit raw `\x1b[...]` sequences. Enforced by:
   ```bash
   grep -rn '\\x1b\[' apps/theo-cli/src/ \
     | grep -v '/render/' \
     | grep -v 'assert' \
     | grep -v '//' \
     | wc -l   # must be 0
   ```
2. **TTY awareness**: every styled output respects [`StyleCaps`], which
   disables colors and unicode for piped output and `NO_COLOR=1`.
3. **Streaming idempotency**: `StreamingMarkdownRenderer` produces
   identical output regardless of how input is chunked. Verified by
   6 `proptest` cases in `src/render/streaming.rs`.
4. **No engine/infra in presentation**: the `render/` subsystem depends
   only on `pulldown-cmark`, `syntect`, `crossterm`, `comfy-table`,
   `indicatif`, and internal `theo-domain` / `theo-cli` types.
   `theo-tooling` and `theo-agent-runtime` are not imported from render.
5. **Non-crashing**: property tests prove the streaming renderer never
   panics on arbitrary UTF-8 or ASCII input.

## 2. Module layout

```
apps/theo-cli/src/
├── render/
│   ├── mod.rs              # Re-exports
│   ├── style.rs            # StyleCaps, color/attribute primitives, ansi_reset()
│   ├── tool_result.rs      # Pure per-tool formatters (read, write, edit, bash, ...)
│   ├── markdown.rs         # Static pulldown-cmark → ANSI renderer
│   ├── code_block.rs       # Syntect syntax highlighting with lazy OnceLock
│   ├── streaming.rs        # Incremental state-machine markdown renderer
│   ├── diff.rs             # Edit / unified-diff rendering
│   ├── table.rs            # comfy-table wrapper for /status, /cost, etc.
│   ├── progress.rs         # indicatif spinners + bars (no-op in piped mode)
│   ├── banner.rs           # Startup banner
│   └── errors.rs           # CliError / CliWarning structured messages
├── tty/
│   ├── caps.rs             # TtyCaps::detect / detect_with
│   └── resize.rs           # AtomicU16 width cache + SIGWINCH listener
├── config/
│   ├── mod.rs              # TheoConfig + TOML load/save + ConfigError
│   └── paths.rs            # TheoPaths (XDG via `dirs`, THEO_HOME override)
├── commands/
│   ├── mod.rs              # SlashCommand trait + CommandRegistry
│   ├── help.rs status.rs clear.rs model.rs cost.rs doctor.rs
│   └── memory.rs skills.rs
├── input/
│   ├── completer.rs        # /cmd and @file tab completion
│   ├── hinter.rs           # rustyline-style unique-prefix hints
│   ├── highlighter.rs      # Token-aware prompt highlighting
│   ├── mention.rs          # @file parsing + 64KB-capped reading
│   └── multiline.rs        # Triple-backtick multi-line detector
├── permission/
│   ├── prompt.rs           # dialoguer y/n/always/deny-always prompt
│   └── session.rs          # PermissionSession ACL (Always / DenyAlways persisted)
├── status_line/
│   └── format.rs           # Segmented status-line string builder
├── renderer.rs             # CliRenderer: EventListener → delegates to render::*
├── repl.rs                 # REPL loop, session persistence, streaming flush
├── pilot.rs                # Autonomous pilot loop
├── init.rs                 # Project initialization
└── main.rs                 # Clap CLI + subcommands
```

## 3. Rendering pipeline

```
Agent Runtime
  │
  ▼ DomainEvent
EventBus ──▶ CliRenderer::on_event (src/renderer.rs)
                │
                ├─ ContentDelta  ─▶ StreamingMarkdownRenderer.push(text)
                │                   └─ eprint!(chunk) (drained per event)
                │
                ├─ ReasoningDelta ▶ render::tool_result::render_reasoning_chunk
                │                    (dim text, streamed directly)
                │
                ├─ RunStateChanged ▶ flush_streaming() + optional banner
                │                    (via render::tool_result::render_subagent_banner)
                │
                ├─ ToolCallCompleted ▶ render_tool_completed()
                │                       (dispatches to per-tool functions in
                │                        render::tool_result, rendering via
                │                        render::diff for Edit/Patch)
                │
                ├─ BudgetExceeded ▶ render::tool_result::render_budget_warning
                │
                └─ Error ─▶ render::tool_result::render_denied / render_error
```

### Streaming markdown state machine

`StreamingMarkdownRenderer` is the highest-risk component (see ADR-001).
It consumes text chunk-by-chunk and emits styled output as soon as
tokens can be safely resolved.

States:

```
Plain → Star1 → BoldOpen → BoldClosing → BoldOpen … (close on **)
Plain → Star1 → ItalicOpen → Plain (close on *)
Plain → Backtick1 → InlineCode → Plain (close on `)
Plain → Backtick1 → Backtick2 → FenceLang → CodeBlock → CodeBacktick1
      → CodeBacktick2 → Plain (close on triple backtick)
```

Flushing rules: `flush()` emits any open tokens as **plain text**
(not styled) so the renderer never leaks unclosed markdown across
turn boundaries. Called from `renderer.rs` on `RunStateChanged` and
before `ToolCallCompleted` events.

Property tests (in `src/render/streaming.rs`) prove:

- Any chunk size produces identical output to a single push.
- Renderer never panics on arbitrary ASCII or Unicode input.
- `flush()` separates independent segments cleanly.

## 4. Benchmarks (validated on vast.ai)

| Benchmark | Result | DoD |
|---|---|---|
| `streaming_per_chunk/single_push` | **449 ns / chunk** | T1.4b: < 1 ms ✅ (~2000× margin) |
| `streaming_100k_plain/push_all` | **261 µs for 100 KB** | T1.4b: < 1 s ✅ |
| `streaming_chunk_size/*` (1→256) | **2.7–3.0 µs** (flat) | No chunk-size regression |
| `syntect_cold_load/SyntaxSet` | **683 µs** | T1.3: < 50 ms ✅ (~60× margin) |
| `syntect_cold_load/ThemeSet` | **973 µs** | T1.3: < 50 ms ✅ |
| `syntect_lazy_access/*` | **~450 ps** | Essentially free |
| `syntect_highlight_lang/rust` | **132 µs** for 5 lines | Real-time |
| `syntect_highlight_lang/python` | **202 µs** for 2 lines | Real-time |
| `syntect_render_block/rust_five_lines` | **421 µs** (with borders) | Real-time |

Benchmarks live at `apps/theo-cli/benches/streaming_markdown.rs` and
`apps/theo-cli/benches/syntect_load.rs`. Run with:

```bash
cargo bench -p theo --bench streaming_markdown
cargo bench -p theo --bench syntect_load
```

## 5. Style module API

`render::style` exposes **eight** styled variants and a handful of
capability-aware symbols:

| Function | Intent |
|---|---|
| `success(s, caps)` | Green — successful operations, done markers |
| `error(s, caps)` | Red — failures, errors, denials |
| `warn(s, caps)` | Yellow — warnings, `/mode` hints |
| `dim(s, caps)` | Dark grey — secondary text, prefixes, borders |
| `accent(s, caps)` | Cyan — prompts, command names, key labels |
| `tool_name(s, caps)` | Magenta bold — sub-agent prefixes |
| `code_bg(s, caps)` | Grey background — inline code snippets |
| `bold(s, caps)` | Bold — headings, emphasis |

Symbol helpers adapt to `caps.unicode`:

| Function | Unicode | ASCII |
|---|---|---|
| `check_symbol(caps)` | `✓` | `OK` |
| `cross_symbol(caps)` | `✗` | `X` |
| `bullet(caps)` | `•` | `*` |
| `hline_char(caps)` | `─` | `-` |

`ansi_reset()` returns `"\x1b[0m"` and is the **single** sanctioned
raw-ANSI literal in the crate, used by `render::code_block` to close
syntect's pre-colored output.

## 6. TtyCaps → StyleCaps flow

```
TtyCaps::detect()          # Queries stderr TTY, NO_COLOR, terminal size
    │
    ▼
TtyCaps { is_tty, colors, unicode, width }
    │
    ▼
TtyCaps::style_caps()      # Projects to the render::style surface
    │
    ▼
StyleCaps { colors, unicode }  # Used by every render function
```

- `NO_COLOR=1` in env: `colors = false`, `unicode` unchanged.
- Non-TTY (pipes, `2>/dev/null`): `is_tty = false`, `colors = false`,
  `unicode = false`.
- `THEO_HOME=/path`: rooted path override for sessions, config, cache.

## 7. Permission integration

`permission/session.rs` tracks "Always" and "Deny always" decisions
during the REPL lifetime. Keys are `(tool_name, first-word-of-summary)`
so related calls share a decision without being identical.

When the REPL wants to run a tool:

```
PermissionSession::check(req)
    │
    ├─ Allow        → run immediately (prior Always)
    ├─ Deny         → skip (prior Deny always)
    └─ NeedsPrompt  → permission::prompt::prompt_for(req)
                        │
                        ├─ THEO_AUTO_ACCEPT=1 → Always
                        ├─ not a TTY           → No
                        └─ dialoguer::Select  → user choice
                            │
                            ▼
                       session.remember(req, decision)
```

The actual wiring into `theo-governance::PolicyEngine` is out of scope
for the CLI-professionalization milestone but the presentation surface
is fully tested.

## 8. Test coverage

| Category | Count |
|---|---|
| Unit tests (in-crate) | ~361 |
| Property tests (`proptest`) | 6 |
| Integration tests (`tests/`) | 0 (intentional — in-crate tests cover the public surface) |
| Criterion benchmarks | 9 groups across 2 binaries |
| **Total `cargo test -p theo`** | **375 passing, 0 failing** |

Measurements taken 2026-04-11 on vast.ai contract 34453774.

## 9. Related ADRs

- [ADR-001 Streaming Markdown State Machine](../adr/ADR-001-streaming-markdown.md)
- [ADR-002 Reject Ratatui](../adr/ADR-002-reject-ratatui.md)
- [ADR-003 XDG Base Directory](../adr/ADR-003-xdg-paths.md)
- [ADR-004 CLI Infra Exception](../adr/ADR-004-cli-infra-exception.md)

## 10. How to extend

### Add a new slash command

1. Create `apps/theo-cli/src/commands/<name>.rs` with a
   `#[derive(…)] pub struct FooCommand` and `impl SlashCommand for FooCommand`.
2. Register it in `apps/theo-cli/src/commands/mod.rs` inside `build_registry`.
3. Add 3+ unit tests (name, category, behavior).

### Add a new render primitive

1. If it's a raw-ANSI helper, it MUST live in `render::style`.
2. If it's a composite renderer (markdown, diff, table, etc.), create
   a new `render/<name>.rs` module, delegate all styling to `render::style`,
   and add it to `render/mod.rs`.
3. Add unit tests covering both `StyleCaps::plain()` and
   `StyleCaps::full()` paths.

### Add a new language to syntax highlighting

Syntect bundles defaults; unknown languages fall back to plain text.
To add a non-default language, extend `code_block::highlight` to load
additional `.sublime-syntax` definitions from
`TheoPaths::syntect_cache()` at startup.

### Add a new benchmark

1. Create `apps/theo-cli/benches/<name>.rs`.
2. Add `[[bench]] name = "<name>" harness = false` to `apps/theo-cli/Cargo.toml`.
3. Import internal modules via `#[path = "../src/…"] pub mod …`.
4. Run with `cargo bench -p theo --bench <name>`.
