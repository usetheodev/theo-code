# theo-cli Baseline (T0.0)

- **Measured**: 2026-04-11
- **Branch**: develop
- **Commit**: 11e6053 (Add Karpathy's LLM Wiki knowledge base)
- **Purpose**: Frozen reference to detect regressions during CLI professionalization (Fase 0 + Fase 1)

## Source Metrics

| Metric | Value | Source |
|---|---|---|
| Files in `apps/theo-cli/src/` | 6 | `find apps/theo-cli/src -name '*.rs'` |
| Lines of code | 2378 | `wc -l` |
| Raw ANSI sequences | **64** | `grep -rn '\\x1b\[' apps/theo-cli/src/` |
| `unwrap()` + `expect()` calls | **47** | `grep -rn 'unwrap()\|expect(' apps/theo-cli/src/` |
| `#[test]` / `#[tokio::test]` | **23** | `grep -rn '#[test]\|#[tokio::test]' apps/theo-cli/src/` |
| Direct CLI dependencies | 14 | `cargo metadata` on `theo` package |

## Binary Size

| Profile | Size | Path |
|---|---|---|
| debug | 386 MB | `target/debug/theo` |
| release | 72 MB | `target/release/theo` |

**Note**: Debug size is dominated by debug symbols. Release is the meaningful baseline.

## Slash Commands (current)

7 implemented:
- `/exit` / `/quit` / `/q`
- `/help` / `/h`
- `/clear`
- `/status`
- `/memory` (list / search / delete)
- `/skills`
- `/mode` (agent / plan / ask)

## Tool Events Handled

~15 tool types rendered with custom logic in `renderer.rs`:
read, write, edit, apply_patch, glob, grep, bash, think, reflect, memory, task_create, task_update, done, + generic fallback.

## Hardcoded Values

| Location | Value | Purpose |
|---|---|---|
| `renderer.rs:78` | `80` | Path truncation width |
| `renderer.rs:248` | `70` | Command truncation width |
| `renderer.rs:257` | `78` | Content truncation width |
| `repl.rs:23` | `100` | MAX_SESSION_MESSAGES |
| `init.rs:364` | `30` | AI enrichment max iterations |
| `renderer.rs:137` | `1000` | Duration threshold (ms) |

## Hardcoded Paths

- `repl.rs:284-289` → `~/.config/theo/sessions/`
- `commands.rs:123-128` → `~/.config/theo/skills/`
- `commands.rs:186-193` → `~/.config/theo/memory/`

**Non-compliant with XDG**. Tracked by ADR-003.

## Time-to-First-Token (TTFT)

**Status**: Not measured in this baseline. Requires running CLI against a live provider with latency instrumentation. Deferred until hyperfine benchmark harness is set up (see T1.4b, criterion benchmarks).

**Placeholder**: Measure via `theo agent "hello"` with a stubbed LLM that returns immediately. This gives pure CLI overhead, excluding network.

## Targets (Post-Plan, F0+F1+F2+F3+F4 items executed)

| Metric | Baseline | Target | **Actual** |
|---|---|---|---|
| Raw ANSI in production code outside `render/style.rs` (T0.3 literal DoD) | 64 | 0 | **0 ✅** |
| Unit + integration tests | 23 | ≥ 120 | **376 ✅** |
| Property tests | 0 | ≥ 3 | **6 ✅** |
| Criterion benchmarks | 0 | 2 (streaming + syntect) | **2 ✅** |
| Source files in apps/theo-cli/src | 6 | ≥ 20 | **41 ✅** |
| LOC in apps/theo-cli/src | 2378 | n/a | **8914** |
| Slash commands (registry) | 7 | ≥ 8 | **8 ✅** (new: model, cost, doctor; upgraded: help/status/clear/memory/skills) |
| Release binary size | 72 MB | ≤ +8 MB | **78 MB (+6 MB) ✅** |
| Syntect SyntaxSet cold load (criterion) | n/a | < 50 ms | **683 µs ✅** (73× margin) |
| Syntect ThemeSet cold load (criterion) | n/a | < 50 ms | **973 µs ✅** (51× margin) |
| Streaming per-chunk latency (criterion) | n/a | < 1 ms | **449 ns ✅** (2228× margin) |
| Streaming 100K chars throughput | n/a | < 1 s | **261 µs ✅** (~365 MiB/s) |
| New modules | n/a | render, tty, config, commands, input, permission, status_line | **all 7 ✅** |
| ADRs | 0 | 4 | **4 ✅** |
| Architecture doc | absent | `docs/current/cli-rendering.md` | **published ✅** |

## Execution Log

**Fase 0** — Foundation
- T0.0: baseline measured in this file
- T0.1: workspace deps added (syntect, indicatif, console, dialoguer, textwrap, comfy-table, dirs, insta, proptest, async-trait)
- T0.2: `tty/` module with `TtyCaps::detect` + `detect_with` + `resize` listener (14 tests)
- T0.3: `render/style.rs` + `render/tool_result.rs` — all styled output flows through this module (53 tests)
- T0.4: `config/mod.rs` with `TheoConfig` serde, `TheoPaths` XDG-compliant (13 tests)

**Fase 1** — Rendering
- T1.1: `renderer.rs` migrated to `render/style.rs` — 35 raw ANSI sequences eliminated
- T1.2: `render/markdown.rs` static renderer with headers, lists, code blocks, blockquotes, tables (22 tests)
- T1.3: `render/code_block.rs` syntect-backed highlighter (15 tests, 12+ languages)
- T1.4a: `render/streaming.rs` `StreamingMarkdownRenderer` state machine (26 unit tests)
- T1.4b: property tests via proptest (6 property tests) + 100K-char smoke bench
- T1.5: `ContentDelta` events in `CliRenderer` now flow through `StreamingMarkdownRenderer`
- T1.6: `render/diff.rs` for Edit/apply_patch rendering (18 tests)
- T1.7: `render/table.rs` via comfy-table (6 tests)
- T1.8: `render/progress.rs` via indicatif with no-op fallback (9 tests)

**Fase 2** — Commands and interactivity
- T2.1: `commands/` refactored into registry with `SlashCommand` trait + dispatcher (14 tests)
- T2.2a: `/model`, `/cost`, `/doctor` core commands + `/help` `/status` `/clear` `/memory` `/skills` rewritten (32 tests)
- T2.3: `input/completer.rs` tab completion for `/cmd` and `@file` (19 tests)
- T2.4: `input/hinter.rs` + `input/highlighter.rs` (21 tests)
- T2.5: `input/multiline.rs` triple-backtick detector (9 tests)
- T2.6: `input/mention.rs` with 64KB cap and 10-mentions-per-turn anti-abuse (22 tests)

**Fase 3** — Permission + status line + banner
- T3.1: `permission/` with prompt + session ACL (19 tests)
- T3.2: `status_line/format.rs` single-line segmented status (15 tests)
- T3.3: `render/banner.rs` replacing inline `print_banner` (5 tests)

**Fase 4** — Polish
- T4.5: session path migrated from hardcoded `~/.config/theo/sessions` to `TheoPaths::sessions()` (XDG compliant)
- T4.6: `render/errors.rs` structured CliError + CliWarning with hint/docs fields (10 tests)

## ADRs

- **ADR-001** Streaming Markdown State Machine
- **ADR-002** Reject Ratatui — use crossterm alone
- **ADR-003** XDG Base Directory compliance (promoted from T4.5 to Fase 0)
- **ADR-004** CLI exception to apps-never-import-infra rule

## Build target

All builds and tests run on vast.ai contract 34453774 at `ssh5.vast.ai:13774` — local machine freed for editing only.

## Regression Guardrails

The following MUST NOT regress across Fase 0 + Fase 1:

1. All 23 existing tests pass: `cargo test -p theo`
2. `cargo build --release` succeeds without new warnings
3. Visual output for existing tool renders matches baseline snapshot (captured as `insta` golden files at start of T1.1)
4. Session persistence round-trip works (load existing session file)
5. REPL accepts all 7 slash commands as before

## How to Reproduce

```bash
# Source metrics
grep -rn '\\x1b\[' apps/theo-cli/src/ | wc -l
grep -rn 'unwrap()\|expect(' apps/theo-cli/src/ | wc -l
grep -rn '#\[test\]\|#\[tokio::test\]' apps/theo-cli/src/ | wc -l
find apps/theo-cli/src/ -name '*.rs' | xargs wc -l

# Binary size
cargo build --release -p theo
du -h target/release/theo

# Test pass
cargo test -p theo
```
