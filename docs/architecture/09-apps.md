# 09 — Surface Applications

Five applications that consume `theo-application` and `theo-api-contracts`. None import engine/infra crates directly.

Only three of these are Rust workspace members (`theo-cli`, `theo-desktop`, `theo-marklive`). `theo-ui` is a separate npm project embedded by `theo-desktop` at build time, and `theo-benchmark` is an isolated Python harness — explicitly **not** in the Rust workspace to keep benchmark code from accidentally becoming a production dependency.

---

## theo-cli (`apps/theo-cli`)

Primary interface. Rust binary with Clap argument parsing.

### Subcommands

| Command | Purpose |
|---|---|
| `theo` (default) | Interactive REPL or single-shot task via `--prompt` |
| `theo init` | Initialize project, generate `.theo/theo.md` via AI analysis |
| `theo pilot` | Autonomous loop: work until promise fulfilled |
| `theo context` | Query GRAPHCTX for a code question |
| `theo impact` | Analyze impact of editing a file |
| `theo stats` | Show graph statistics |

### Key Flags

| Flag | Effect |
|---|---|
| `--headless` / `-p` | No REPL, emit single JSON line (`theo.headless.v2`) for CI/benchmark |
| `--tui` | Launch ratatui TUI mode (experimental) |
| `--model <model>` | Override LLM model |
| `--continue` | Resume from previous session |
| `--plan` | Plan mode (read-only, write only to `.theo/plans/`) |

### Module Structure

```
theo-cli/src/
├── main.rs              # Clap CLI, subcommand dispatch
├── repl.rs              # Interactive REPL loop
├── pilot.rs             # Autonomous pilot loop
├── init.rs              # Project initialization
├── json_output.rs       # Headless JSON serialization
├── renderer.rs          # Rendering orchestration
│
├── commands/            # Slash commands: /clear, /cost, /doctor, /help,
│                        #   /memory, /model, /session, /skills, /status
│
├── config/              # Config loading + path resolution
│
├── input/               # REPL input handling
│   ├── completer.rs     # Tab completion
│   ├── highlighter.rs   # Syntax highlighting in input
│   ├── hinter.rs        # Input hints
│   ├── keyboard.rs      # Key bindings
│   ├── mention.rs       # @file mention expansion
│   ├── model_selector.rs # Interactive model picker
│   ├── multiline.rs     # Multi-line input handling
│   └── stdin_buffer.rs  # Stdin buffering for piped input
│
├── permission/          # Permission prompt UI + session state
│
├── render/              # Terminal rendering
│   ├── banner.rs        # Startup banner
│   ├── code_block.rs    # Syntax-highlighted code blocks
│   ├── diff.rs          # Diff rendering
│   ├── markdown.rs      # Streaming markdown rendering
│   ├── progress.rs      # Progress indicators
│   ├── streaming.rs     # Token-by-token streaming output
│   ├── style.rs         # ANSI style capabilities
│   ├── table.rs         # Table rendering
│   └── tool_result.rs   # Tool output formatting
│
├── status_line/         # Bottom status bar
│
├── tty/                 # Terminal capability detection
│   ├── caps.rs          # Style capabilities (color, unicode, width)
│   └── resize.rs        # Terminal resize handling
│
└── tui/                 # Ratatui TUI (experimental)
    ├── app.rs           # TUI application state machine
    ├── autocomplete.rs  # Input autocomplete
    ├── bench.rs         # TUI benchmark mode
    ├── commands.rs      # TUI slash command handling
    ├── config.rs        # TUI configuration
    ├── events.rs        # Event handling (keyboard, resize)
    ├── input.rs         # Input widget
    ├── markdown.rs      # Markdown rendering to ratatui widgets
    ├── theme.rs         # Color theme
    ├── view.rs          # Main view layout
    └── widgets/         # diff_viewer, sidebar
```

### Headless Mode

For CI and benchmarks. Emits a single JSON line on stdout:

```json
{
  "schema": "theo.headless.v2",
  "success": true,
  "summary": "Fixed the bug in auth.rs",
  "files_edited": ["src/auth.rs"],
  "iterations_used": 5,
  "tokens_used": 12000,
  "input_tokens": 8000,
  "output_tokens": 4000,
  "tool_calls_total": 8,
  "tool_calls_success": 7,
  "llm_calls": 5,
  "retries": 0,
  "duration_ms": 45000
}
```

---

## theo-desktop (`apps/theo-desktop`)

Tauri v2 desktop application with a React frontend.

### Rust Backend (`src/`)

| File | Purpose |
|---|---|
| `main.rs` | Tauri binary entry |
| `lib.rs` | Plugin + invoke handler registration |
| `state.rs` | `AppState` — shared mutable state (`Mutex`) |
| `events.rs` | Tauri event definitions for backend → frontend |
| `commands/chat.rs` | `send_message`, `cancel_agent`, `set_project_dir`, `get/update_config` |
| `commands/auth.rs` | OpenAI OAuth device flow |
| `commands/copilot.rs` | GitHub Copilot device flow |
| `commands/anthropic_auth.rs` | Anthropic Console device flow |

### Frontend Bridge

Backend publishes `FrontendEvent` (from `theo-api-contracts`) via Tauri event system. Frontend subscribes via `useAgentEvents` hook.

---

## theo-ui (`apps/theo-ui`)

React 18 + TypeScript + Tailwind + Radix UI. Separate app that serves as the frontend for `theo-desktop`.

### Structure

```
theo-ui/src/
├── main.tsx                    # App entry
├── app/
│   ├── routes.tsx              # Router
│   ├── AppLayout.tsx           # Shell layout
│   └── AppSidebar.tsx          # Navigation sidebar
│
├── features/
│   ├── assistant/              # Main chat UI
│   │   ├── AssistantPage.tsx   # Page wrapper
│   │   ├── AgentView.tsx       # Agent conversation view
│   │   ├── AssistantMessage.tsx # Message rendering
│   │   ├── CommandComposer.tsx # Input area
│   │   ├── ToolCallDisplay.tsx # Tool call visualization
│   │   ├── AgentPlanView.tsx   # Plan mode view
│   │   ├── AgentReviewView.tsx # Review view
│   │   ├── AgentSecurityView.tsx # Security review view
│   │   └── AgentTestsView.tsx  # Test results view
│   │
│   ├── code/                   # Code viewer (stub)
│   ├── database/               # Database viewer
│   ├── deploys/                # Deploy management
│   ├── monitoring/             # Monitoring dashboard
│   └── settings/               # Settings page
│
├── components/
│   ├── auth/                   # Auth dialogs
│   └── ui/                     # shadcn/ui primitives
│
├── hooks/
│   ├── useAgentEvents.ts       # Tauri event subscription
│   ├── useAnthropicAuth.ts     # Anthropic auth flow
│   ├── useDeviceAuth.ts        # Device flow auth
│   └── use-mobile.ts           # Mobile detection
│
└── types.ts                    # Shared TypeScript types
```

---

## theo-marklive (`apps/theo-marklive`)

Standalone markdown wiki renderer. Converts a directory of `.md` files into a single self-contained HTML page with sidebar navigation, full-text search, code highlighting, and dark theme.

Primary use: rendering `.theo/wiki/` directories generated by the Code Wiki pipeline.

```rust
pub fn render(dir: &Path, config: Config) -> Result<String, String>;
```

CLI: `theo-marklive <dir> [-o output.html] [--title "My Wiki"]`

### Modules

| Module | Purpose |
|---|---|
| `parser.rs` | Walk directory, parse `.md` → `MarkdownPage`, render HTML |
| `sidebar.rs` | Build sidebar HTML + JS search index |
| `template.rs` | Assemble single-page HTML with inline CSS/JS |

---

## theo-benchmark (`apps/theo-benchmark`)

Multi-mode benchmark harness. **Isolated from production runtime** — never imported by other crates.

### Benchmark Modes

| Runner | Purpose | Input |
|---|---|---|
| `runner/smoke.py` | 20 TOML scenario tests | `scenarios/smoke/*.toml` |
| `runner/evolve.py` | Mutation-based prompt evolution (Karpathy ratchet) | `mutation_bank.json` |
| `swe_bench_harness.py` | SWE-bench Lite integration | SWE-bench instances |
| `run_benchmark.py` | GRAPHCTX context-engineering A/B test | Coding tasks |

### Smoke Scenarios (20)

| # | Scenario | Tests |
|---|---|---|
| 01 | read-answer | Read file, answer question |
| 02 | grep-search | Find pattern in codebase |
| 03 | fix-typo | Fix spelling error |
| 04 | fix-return | Fix wrong return value |
| 05 | add-function | Add new function |
| 06 | rename-var | Rename variable |
| 07 | count-files | Count files matching pattern |
| 08 | multi-file-edit | Edit across files |
| 09 | plan-mode | Plan without executing |
| 10 | logic-bug | Fix logical error |
| 11 | python-fix | Fix Python code |
| 12 | bash-command | Execute shell command |
| 13 | multi-step-fix | Multi-step bug fix |
| 14 | add-import | Add missing import |
| 15 | cross-file-search | Search across files |
| 16 | cross-file-bug | Fix bug spanning files |
| 17 | missing-error-handling | Add error handling |
| 18 | three-file-feature | Feature across 3 files |
| 19 | off-by-one | Fix off-by-one error |
| 20 | class-inheritance-bug | Fix class hierarchy bug |

### Core Module (`_headless.py`)

All benchmark modes use `_headless.py` to invoke `theo --headless`:
- Parses `theo.headless.v2` JSON output
- Handles rate limit retry with exponential backoff
- Cost estimation from token counts
- Multi-run statistics aggregation

### Evolution Loop (`runner/evolve.py`)

Implements the Karpathy ratchet pattern:
1. Mutate prompt (from `mutation_bank.json`)
2. Run smoke suite
3. If score improves → accept mutation
4. If score degrades → revert
5. Repeat

> This is **not** the same as `theo-agent-runtime::evolution` — that module operates in-run over a single task's attempts. Karpathy ratchet operates across runs and mutates the system prompt itself. See `04-agent-runtime.md` §Evolution Loop Integration for the distinction.

---

## Harness Templates — Not Yet

Böckeler `harness-engineering.md` §5 proposes **harness templates**: bundled guides and sensors that come with a chosen application topology (business service with API, event processor, data dashboard). Teams instantiate a template and get the baseline harness for free.

Theo has five surface applications, but each currently carries its own ad-hoc configuration for hooks, skills, sandbox policy, and sensors. There is no shared template that a new project consuming Theo Code can instantiate to get a coherent starting harness.

This is a known gap, tied to Theo's role as "the Vercel for developers who have a backend": the platform should eventually ship harness templates aligned to the service topologies it supports. See `README.md` → Gaps vs Research.
