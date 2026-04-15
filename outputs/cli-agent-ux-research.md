---
type: report
question: "What are the best practices for Code Agent CLI/Terminal UX across modern AI coding assistants?"
generated_at: 2026-04-11T12:00:00Z
confidence: 0.88
sources_used: 34
---

# Report: Code Agent CLI/Terminal UX Best Practices

## Executive Summary

Modern AI coding CLIs have converged on a set of UX patterns that define what users expect from a terminal-based coding agent. Claude Code leads with the most sophisticated rendering engine (custom React reconciler + Yoga flexbox + ANSI parser, 389 components, 500K+ daily sessions). Codex CLI follows with a full-screen Rust TUI featuring themes, diffs, and approval modes. Aider takes the simplest approach with prompt-toolkit and Rich/Pygments for markdown. OpenCode pushes performance boundaries with SolidJS + 60fps dirty-rectangle rendering. The key takeaway for Theo Code: users expect streaming markdown with syntax highlighting, a rich slash-command system, persistent status/cost display, and clear permission UIs -- but the rendering approach (React-in-terminal vs native TUI vs simple ANSI) should match the team's velocity constraints.

---

## 1. Terminal Rendering & Visual Design

### 1.1 Rendering Engine Approaches

There are four distinct approaches in production today:

| Tool | Rendering Stack | Complexity | Maturity |
|------|----------------|-----------|----------|
| **Claude Code** | Custom React reconciler + Ink fork + Yoga (pure TS) + ANSI parser | Very High (~1,902 files, 389 components) | Battle-tested (500K+ daily sessions) |
| **Codex CLI** | Rust TUI (full-screen), syntax highlighting via `highlight.rs` | High | Production (rebuilt in Rust 2025) |
| **Aider** | prompt-toolkit (Python) + Rich/Pygments for markdown | Low-Medium | Stable, simple |
| **OpenCode** | SolidJS + @opentui (Yoga flexbox, 60fps, dirty-rectangle) | High | Active but rendering glitches reported |
| **Copilot CLI** | Standard terminal output + streaming | Medium | Production |

**Source:** [Claude Code leaked source analysis](https://dev.to/minnzen/i-studied-claude-codes-leaked-source-and-built-a-terminal-ui-toolkit-from-it-4poh), [OpenCode TUI Architecture](https://deepwiki.com/sst/opencode/6.2-cli-commands), [Codex CLI docs](https://developers.openai.com/codex/cli)

### 1.2 Claude Code's Rendering Pipeline (Deep Dive)

Claude Code's rendering is the most documented and sophisticated. The pipeline:

```
React Components
  -> Custom React Reconciler (ConcurrentRoot mode, React 18 features)
    -> Virtual DOM Tree (ink-box, ink-text, ink-root, ink-link)
      -> Yoga Layout Engine (pure TS, ~2700 lines, no WASM/native)
        -> Output Builder
          -> Screen Buffer (flat typed arrays, Int32Array, CharPool interning)
            -> Diff Engine (cell-by-cell, only emits changed cells)
              -> ANSI Escape Sequences -> TTY
```

Key performance characteristics:
- **Frame cap:** 10 FPS (100ms throttle) to prevent CPU waste
- **Idle optimization:** On unchanged frames, output drops from ~10KB to ~50 bytes
- **Cell packing:** Each cell = 2 Int32Array slots (character ID + packed style/hyperlink/width)
- **Color memoization:** Avoids re-sending ANSI codes if style hasn't changed
- **EL (Erase in Line):** Used instead of spaces for SSH performance
- **NO_FLICKER mode:** Alt-screen rendering via `CLAUDE_CODE_NO_FLICKER=1`, eliminates scroll-and-redraw jitter
- **DEC 2026 sync protocol:** Fixes scrolling in iTerm2, Ghostty

**Source:** [Claude Code Ink Renderer](https://deepwiki.com/alesha-pro/claude-code/7.1-ink-renderer-and-custom-tui-engine), [Claude Code TUI Architecture](https://deepwiki.com/flyboyer/claude-code/8-terminal-ui-(tui)-architecture)

### 1.3 Streaming Output

All tools stream LLM responses in real-time:

- **Claude Code:** StreamingText component renders token-by-token with markdown parsing mid-stream. Memory-optimized highlight render caches prevent growth from retaining full content strings.
- **Codex CLI:** Full-screen TUI renders streaming markdown with syntax highlighting as tokens arrive. Tool calls and diffs are formatted inline during streaming.
- **Aider:** Streams via `--stream` (default on). Displays markdown responses from LLM with Rich formatting.
- **OpenCode:** 60fps streaming with dirty-rectangle optimization, though performance bottlenecks reported during heavy LLM streaming (proposed fix: 75x faster).

**Source:** [OpenCode RFC](https://github.com/anomalyco/opencode/issues/13027)

### 1.4 Syntax Highlighting

| Tool | Approach | Themes |
|------|----------|--------|
| **Claude Code** | Custom ANSI component parses escape sequences into Ink Text spans | Design system colors via `colors.ts` |
| **Codex CLI** | `highlight.rs` with `.tmTheme` support | `/theme` picker with live preview, custom themes in `$CODEX_HOME/themes` |
| **Aider** | Pygments-based highlighting | `--code-theme` flag (default, monokai, solarized-dark, solarized-light, any Pygments style) |
| **OpenCode** | `code` and `diff` primitives in OpenTUI | Theme auto-detection via OSC 11 (light/dark) |

**Source:** [Codex CLI features](https://developers.openai.com/codex/cli/features), [Aider options](https://aider.chat/docs/config/options.html)

### 1.5 Color Systems

**Aider** has the most explicit and configurable color system:

| Role | Default Color | Env Variable |
|------|--------------|-------------|
| User input | Green `#00cc00` | `AIDER_USER_INPUT_COLOR` |
| Assistant output | Blue `#0088ff` | `AIDER_ASSISTANT_OUTPUT_COLOR` |
| Tool output | Terminal default | `AIDER_TOOL_OUTPUT_COLOR` |
| Tool errors | Red `#FF2222` | `AIDER_TOOL_ERROR_COLOR` |
| Tool warnings | Orange `#FFA500` | `AIDER_TOOL_WARNING_COLOR` |

Convenience modes: `--dark-mode`, `--light-mode`.

**Claude Code** uses a centralized design system (`src/constants/colors.ts`) with semantic color tokens. Colors are applied through Ink's `<Text color="..." />` props.

**Codex CLI** has been criticized for insufficient visual emphasis -- output appears "almost uniform and flat" compared to Claude Code. Community feedback requested semantic tokens (primary, accent, error) instead of hard-coded ANSI names. The `ansi` theme has a known bug where it emits RGB instead of palette indices.

**Source:** [Codex color emphasis issue](https://github.com/openai/codex/issues/6531), [Codex light theme issue](https://github.com/openai/codex/issues/2020)

### 1.6 Typography Patterns

Common patterns across all tools:

| Style | Usage |
|-------|-------|
| **Bold** | Headings, file names, important labels, tool names |
| **Dim** | Metadata, timestamps, secondary info, help text |
| **Italic** | Emphasis, notes, warnings |
| **Underline** | Links, file paths (clickable in some terminals) |
| **Strikethrough** | Removed code in diffs |
| **Inverse** | Selected items in pickers, active tab highlighting |

### 1.7 Box Drawing & Structured Output

**Claude Code** uses Ink's `<Box borderStyle="..." />` extensively:
- Permission dialogs rendered as bordered panels
- Diff output in structured bordered regions
- Tool results in collapsible bordered sections
- Spinner + verb rotation inside boxes for long operations

**Codex CLI** renders:
- Welcome box (criticized for being "plain white outline")
- Diff panels for code changes
- Status panels for `/status` output

**OpenCode** provides first-class `box`, `scrollbox`, `diff`, `line-number` primitives.

### 1.8 Handling Long Outputs

- **Claude Code:** Collapsed badges for search/read results, deduplicated in fullscreen scrollback. MCP tools can override truncation up to 500K chars via `_meta["anthropic/maxResultSizeChars"]`. Feature request open for collapsible sections and section navigation ([Issue #36462](https://github.com/anthropics/claude-code/issues/36462)).
- **Codex CLI:** Full-screen TUI with scrolling. `/ps` shows background terminal output.
- **Aider:** Standard terminal scrollback. `/tokens` to check context size.
- **OpenCode:** `scrollbox` primitive with scroll position tracking.

---

## 2. Slash Commands & Command Systems

### 2.1 Complete Command Inventory

#### Claude Code Commands

**Session Management:**
| Command | Description |
|---------|-------------|
| `/compact [focus]` | Compress conversation history, optionally specifying what to retain |
| `/clear` | Delete all conversation history |
| `/branch` | Fork conversation state (like git branch for sessions) |
| `/rewind` | Roll back conversation and code changes to checkpoint |
| `/context` | Visual grid of context window consumption |

**Model & Performance:**
| Command | Description |
|---------|-------------|
| `/model [name]` | Switch model (haiku, sonnet, opus) |
| `/effort [level]` | Set reasoning effort (low/medium/high) |
| `/fast` | Speed-optimized API settings for Opus |

**Integration:**
| Command | Description |
|---------|-------------|
| `/mcp` | Manage MCP server connections |
| `/install-github-app` | Setup GitHub Actions integration |
| `/install-slack-app` | Setup Slack integration |
| `/plugin` | Discover and install plugins |
| `/reload-plugins` | Hot-reload plugins without restart |

**Utility:**
| Command | Description |
|---------|-------------|
| `/help` | Show all commands including custom ones |
| `/plan` | Enter planning mode |
| `/hooks` | Manage HTTP webhooks on lifecycle events |
| `/keybindings` | Open keybinding config |
| `/statusline` | Generate/configure status line script |
| `/permissions` | Interactive permission management (4 tabs: Allow/Ask/Deny/Workspace) |

**Bundled Skills (prompt-based):**
| Command | Description |
|---------|-------------|
| `/simplify` | Three-agent parallel code review |
| `/batch` | Batch operations |
| `/debug` | Debug assistance |
| `/loop` | Iterative refinement |
| `/claude-api` | API-specific guidance |

**Source:** [Claude Code commands cheat sheet](https://www.scriptbyai.com/claude-code-commands-cheat-sheet/), [Claude Code docs](https://code.claude.com/docs/en/skills)

#### Aider Commands (38 commands)

**Chat Modes:** `/code`, `/architect`, `/ask`, `/context`, `/help`, `/chat-mode`
**File Management:** `/add`, `/read-only`, `/drop`, `/ls`
**Git:** `/commit`, `/diff`, `/git`, `/undo`
**Execution:** `/run` (alias: `!`), `/test`, `/lint`
**Session:** `/clear`, `/reset`, `/tokens`, `/model`, `/models`, `/settings`, `/exit`, `/quit`
**Input/Output:** `/editor` (alias: `/edit`), `/paste`, `/clipboard`, `/copy`, `/copy-context`, `/voice`, `/web`
**Advanced:** `/think-tokens`, `/reasoning-effort`, `/multiline-mode`, `/map`, `/map-refresh`, `/report`, `/save`, `/load`, `/ok`, `/editor-model`, `/weak-model`

**Source:** [Aider commands docs](https://aider.chat/docs/usage/commands.html)

#### Codex CLI Commands (25+ commands)

**Session:** `/clear`, `/new`, `/resume`, `/fork`, `/quit`, `/exit`
**Model:** `/model`, `/fast`, `/personality`
**Workflow:** `/plan`, `/review`, `/diff`, `/copy`, `/compact`
**Policy:** `/permissions` (alias: `/approval`), `/sandbox`, `/status`, `/debug-config`
**Config:** `/mcp`, `/apps`, `/experimental`, `/statusline`, `/theme`
**Agents:** `/agent`, `/ps`, `/mention`
**Utility:** `/init`, `/feedback`, `/logout`, `/sandbox-add-read-dir`

**Source:** [Codex CLI slash commands](https://developers.openai.com/codex/cli/slash-commands)

#### GitHub Copilot CLI Commands

**Session:** `/clear`, `/compact`, `/context`, `/share`, `/resume`
**Model:** `/model`
**Navigation:** `/add-dir`, `/cwd`
**Workflow:** `/plan`, `/delegate`, `/fleet`
**Config:** `/mcp`, `/experimental`, `/lsp`
**Utility:** `/help`, `/feedback`, `/login`

**Source:** [Copilot CLI docs](https://docs.github.com/en/copilot/how-tos/copilot-cli/use-copilot-cli)

### 2.2 Command Categories (Cross-Tool Pattern)

All tools converge on these categories:

| Category | Examples | Prevalence |
|----------|---------|------------|
| **Session management** | `/clear`, `/compact`, `/branch`, `/rewind` | Universal |
| **Model selection** | `/model`, `/fast`, `/effort` | Universal |
| **Context control** | `/context`, `/tokens`, `/add`, `/drop` | Universal |
| **Git integration** | `/diff`, `/commit`, `/undo` | Aider, Codex |
| **Execution** | `/run`, `/test`, `/lint` | Aider (strongest) |
| **Planning** | `/plan` | Claude, Codex, Copilot |
| **Review** | `/review`, `/simplify` | Claude, Codex |
| **Integration** | `/mcp`, `/plugin` | Claude, Codex, Copilot |
| **Configuration** | `/settings`, `/permissions`, `/theme` | Universal |
| **Help** | `/help` | Universal |

### 2.3 Auto-Completion Patterns

- **Claude Code:** Tab to accept autocomplete suggestion. `/` triggers slash menu. `@` triggers file/subagent mention typeahead.
- **Codex CLI:** Tab autocompletes slash commands. Up/Down navigates draft history.
- **Aider:** prompt-toolkit provides emacs/vi completion. Arrow keys and Ctrl-R for history.
- **Copilot CLI:** `/` shows available commands. Tab for completion.

### 2.4 Custom Commands

- **Claude Code:** `.claude/skills/` with `SKILL.md` (YAML frontmatter + markdown). Supports `allowed-tools`, `model`, `context: fork`, `disable-model-invocation`. Legacy `.claude/commands/` still works.
- **Aider:** No native custom commands, but community plugin (`slash`) provides external custom commands.
- **Codex CLI:** `/init` generates `AGENTS.md` scaffold. Custom MCP tools appear as commands.

### 2.5 Unknown Command Handling

- **Claude Code:** Unavailable commands display explanatory text rather than disappearing. Plan-restricted features show "unavailable for your plan" messaging.
- **Aider:** Unknown commands treated as chat input (no error, just sent to LLM).
- **Codex CLI:** Unrecognized commands show error feedback.

---

## 3. Interactive Patterns

### 3.1 REPL Design

**Prompt Formats:**

| Tool | Prompt | Style |
|------|--------|-------|
| **Claude Code** | `>` or custom prompt | Minimal, clean |
| **Aider** | `aider>` or mode-specific (`architect>`, `ask>`) | Mode-aware |
| **Codex CLI** | Full-screen TUI with input area at bottom | IDE-like |
| **Copilot CLI** | `>` with mode indicator | Clean |

**Multi-Line Input:**

| Tool | Method |
|------|--------|
| **Claude Code** | Shift+Enter or Meta+Enter for newline. Ctrl+G opens `$EDITOR`. |
| **Aider** | `/multiline-mode` toggles. Meta+Enter submits (swapped). `/editor` or Ctrl-X Ctrl-E opens editor. |
| **Codex CLI** | Ctrl+G opens `$VISUAL` or `$EDITOR` for long prompts. |
| **Copilot CLI** | Standard multi-line in TUI. |

**History:**

| Tool | Method |
|------|--------|
| **Claude Code** | Up/Down arrows. Ctrl+R for session history search. |
| **Aider** | Arrow keys. Ctrl-R history search (prompt-toolkit). |
| **Codex CLI** | Up/Down for draft history (restores text + image placeholders). |

### 3.2 File Selection UIs

- **Claude Code:** `@` mention with fuzzy typeahead. Named subagents also appear. Excludes `.jj`/`.sl` metadata.
- **Aider:** `/add` with tab completion on file paths. `/ls` shows all known files with chat inclusion status.
- **Codex CLI:** `/mention` to attach files. Fuzzy matching in TUI.
- **OpenCode:** Fuzzy picker component with keyboard navigation.

### 3.3 Confirmation Dialogs for Destructive Actions

- **Claude Code:** 51 dedicated permission UI components. Rules evaluated in order: deny -> ask -> allow. File edits shown as diffs before approval. Auto Mode (v2.1.85+) uses Sonnet 4.6 classifier for safety. Permission tabs navigated via arrow keys.
- **Codex CLI:** Three approval modes: read-only (explicit approvals), auto (workspace-scoped), full-access. `/approval` to change mid-session.
- **Aider:** Minimal confirmation. Git commits are auto-created (reversible via `/undo`).
- **Copilot CLI:** Trust confirmation on first run per directory.

### 3.4 Progress Indicators

| Tool | Pattern |
|------|---------|
| **Claude Code** | Spinner component with animated verb rotation (e.g., "Thinking...", "Reading...", "Writing..."). Collapsed badges for completed search/read operations. |
| **Codex CLI** | TUI shows running state for terminal commands. Fixed bug where stopped commands appeared as still running. |
| **Aider** | Streaming text serves as implicit progress. |
| **OpenCode** | Spinner and progress primitives in OpenTUI. |

### 3.5 Tool Use Display

- **Claude Code:** Read/search results shown as collapsed badges. Tool execution shows tool name in spinner. Diffs rendered as StructuredDiff component. MCP tool calls rendered with improved formatting.
- **Codex CLI:** Tool calls and diffs are "better formatted and easier to follow" after TUI rebuild. `/ps` shows background terminal processes.
- **Aider:** Tool output in terminal default color. Tool errors in red (`#FF2222`). Tool warnings in orange (`#FFA500`).

---

## 4. Context & Status Display

### 4.1 Context Window Visualization

**Claude Code** (`/context`):
- Visual grid showing context consumption by category
- Status line shows `used_percentage` as a visual progress bar (e.g., `▓▓▓▓▓▓░░░░ 60%`)
- JSON data available for custom scripts: `context_window.current_usage`, `total_input_tokens`, `total_output_tokens`
- Best practice: compact at 80%, alert at 70%

**Codex CLI** (`/status`):
- Displays active model, approval policy, writable roots, token usage
- `/statusline` picker to toggle and reorder items

**Aider** (`/tokens`):
- Reports token count for current chat context
- Shows per-file token breakdown

**Copilot CLI** (`/context`):
- Shows context window utilization
- Community requesting persistent indicator (currently manual check only)

**Source:** [Claude Code status line docs](https://code.claude.com/docs/en/statusline), [Copilot CLI context issue](https://github.com/github/copilot-cli/issues/2052)

### 4.2 Status Bars / Status Lines

**Claude Code** -- Most mature implementation:
- Customizable bar at bottom of terminal
- Runs any shell script, receives JSON on stdin
- Refreshes on message changes, max every 300ms
- Available fields: `model.display_name`, `context_window.*`, `cost.total_cost_usd`, `workspace.current_dir`, lines added/removed, session ID
- Community tools: [ccstatusline](https://github.com/sirmalloc/ccstatusline) (Powerline support), [CCometixLine](https://github.com/Haleclipse/CCometixLine) (Rust, interactive TUI config)

**Codex CLI:**
- Built-in status line with configurable items via `/statusline`
- Shows model, approval policy, token usage

**Temm1e** (open-source agent, notable pattern):
- 3-section layout: Left = state indicator (`idle / thinking / tool:name / cancelled`), Center = model/tokens/cost, Right = context meter + git repo/branch

**Source:** [Building custom statusline](https://www.dandoescode.com/blog/claude-code-custom-statusline)

### 4.3 Cost Tracking

| Tool | Cost Display |
|------|-------------|
| **Claude Code** | `cost.total_cost_usd` in status line JSON. Pro users see token estimates on session resume. |
| **Codex CLI** | Token usage in `/status`. No explicit cost display by default. |
| **Aider** | `/tokens` shows counts. No built-in cost display. |
| **Agent Deck** | Cross-agent cost tracking with daily/weekly/monthly budgets, TUI dashboard (`$` key). |

**Source:** [Agent Deck](https://github.com/asheshgoplani/agent-deck)

### 4.4 Session Metadata

- **Claude Code:** Session ID, version, model, cost, lines changed, current directory. All accessible via status line JSON.
- **Codex CLI:** `/status` shows model, approval policy, sandbox policy, writable roots, token usage. `/resume` lists saved sessions.
- **Aider:** `/settings` prints current configuration. Session history saved in `.aider.chat.history.md`.
- **Copilot CLI:** `/share` exports session as markdown for documentation.

---

## 5. Keyboard Shortcuts

### 5.1 Standard Keybindings (Cross-Tool)

| Shortcut | Claude Code | Codex CLI | Aider | Copilot CLI |
|----------|-------------|-----------|-------|-------------|
| **Cancel** | Ctrl+C | Esc (cancel request), Ctrl+C (exit) | Ctrl+C | Ctrl+C |
| **Exit** | Ctrl+D | Ctrl+C twice, `/quit` | Ctrl+D, `/exit` | Ctrl+C |
| **Submit** | Enter | Enter | Enter (or Meta+Enter in multiline) | Enter |
| **Newline** | Shift+Enter | -- | Meta+Enter (or Enter in multiline) | -- |
| **History up** | Up | Up | Up | Up |
| **History down** | Down | Down | Down | Down |
| **Editor** | Ctrl+G | Ctrl+G | Ctrl-X Ctrl-E, `/editor` | -- |
| **Clear screen** | -- | Ctrl+L | -- | -- |
| **Search history** | Ctrl+R | -- | Ctrl+R | -- |
| **Copy response** | -- | Ctrl+O | `/copy` | -- |

### 5.2 Claude Code-Specific Shortcuts

| Shortcut | Action |
|----------|--------|
| Shift+Tab | Toggle plan mode |
| Alt+T | Toggle extended thinking |
| Ctrl+F (twice in 3s) | Kill all background agents |
| Escape | Cancel input line |
| Escape Escape (double-tap) | Restore to previous point / generate summary |
| Ctrl+A / Ctrl+E | Start / end of line |
| Option+F / Option+B | Word forward / back |
| Ctrl+W | Delete previous word |
| Cmd+Delete | Delete to start of line (iTerm2, kitty, WezTerm, Ghostty) |

**Source:** [Claude Code keybindings docs](https://code.claude.com/docs/en/keybindings)

### 5.3 Vim/Emacs Mode Support

- **Claude Code:** Customizable keybindings via `/keybindings` -> `~/.claude/keybindings.json`. Context-aware bindings (Chat, Menu, etc.). Chord support (`ctrl+k ctrl+s`). In vim mode, Escape switches INSERT -> NORMAL (does not cancel).
- **Aider:** `--vim` flag for vi keybindings via prompt-toolkit. Default is emacs mode.
- **Codex CLI:** Standard readline-like input. No explicit vim mode documented.

### 5.4 Kitty Keyboard Protocol

Claude Code implements the Kitty keyboard protocol for enhanced key detection, which:
- Prevents raw key sequences leaking into prompts over SSH
- Enables reliable detection of modifier keys
- Supports the VS Code integrated terminal

---

## 6. Error & Feedback Patterns

### 6.1 Error Display Approaches

**Claude Code:**
- Typed errors with context (tool name, file path, operation)
- Permission denied shows clear message: "Claude requested permissions to write to [file], but you haven't granted it yet"
- Unavailable features show explanatory text, not silent failure
- Masked input for sensitive operations (OAuth tokens)

**Aider:**
- Color-coded by severity: errors in red `#FF2222`, warnings in orange `#FFA500`
- Tool errors and warnings have distinct visual treatment
- Output from `/run` and `/test` included in chat on non-zero exit codes

**Codex CLI:**
- Error/warning log patterns: `[WARN]`, `[ERROR]`, `[INFO]`
- Diagnostics via `/feedback` (records Request ID)
- `/debug-config` for configuration troubleshooting
- Known issue: insufficient visual emphasis makes errors blend with normal output

### 6.2 Warning vs Error Differentiation

| Level | Aider | Codex CLI | Claude Code |
|-------|-------|-----------|-------------|
| **Error** | Red `#FF2222` | `[ERROR]` prefix | Bordered error panel, red |
| **Warning** | Orange `#FFA500` | `[WARN]` prefix | Yellow/dim styling |
| **Info** | Terminal default | `[INFO]` prefix | Dim text |
| **Success** | Green (implicit) | -- | Green accents |

### 6.3 Permission Request UIs

**Claude Code** (most sophisticated):
- 51 dedicated permission UI components
- Diffs shown inline before approval
- 4-tab permission manager (Allow/Ask/Deny/Workspace)
- Rule evaluation order: deny -> ask -> allow (first match wins)
- Auto Mode: Sonnet 4.6 classifier reviews each action
- Read-only actions auto-approved, custom rules resolve first

**Codex CLI:**
- Three-level simplification: read-only, auto, full-access
- `/approval` to change mid-session
- `/sandbox` for sandbox policy control

**Copilot CLI:**
- Trust prompt on first directory entry
- Autopilot mode (Shift+Tab to cycle) for full autonomy

### 6.4 Success Confirmations

- **Claude Code:** Collapsed badges for completed operations (search, read). Tool result summaries.
- **Aider:** Git commit messages displayed after auto-commit. Diff shown after changes.
- **Codex CLI:** Completed tool calls show formatted results. Stopped commands no longer show as running (bug fix).

---

## 7. Synthesis: Patterns That Matter for Theo Code

### 7.1 Universal Patterns (Must-Have)

These are present in every tool and users expect them:

1. **Streaming markdown with syntax highlighting** -- Non-negotiable. Users will reject plain text output.
2. **Slash commands starting with `/`** -- Universal convention. Must include at minimum: `/clear`, `/compact`, `/model`, `/context`, `/help`.
3. **Ctrl+C to cancel, Ctrl+D to exit** -- Standard POSIX conventions.
4. **Color-coded output by role** -- User input, assistant output, tool output, errors must be visually distinct.
5. **Diff display for file changes** -- Before-approval review is expected.
6. **Context/token awareness** -- Users need to know how full the context window is.

### 7.2 Differentiating Patterns (Should-Have)

These separate good tools from great ones:

1. **Persistent status line** -- Claude Code's approach (shell script + JSON) is elegant and extensible.
2. **Plan mode** -- Claude Code, Codex, and Copilot all have `/plan`. Users want to review before execution.
3. **Custom commands/skills** -- Claude Code's `.claude/skills/` pattern is the most mature.
4. **@ mention for files** -- Faster than `/add`. Fuzzy typeahead is expected.
5. **Editor integration** -- Ctrl+G to open `$EDITOR` for long prompts.
6. **Session branching/forking** -- Claude Code's `/branch` and Codex's `/fork` for exploration.
7. **Cost tracking** -- Increasingly expected, especially for API-billed usage.

### 7.3 Rendering Strategy Recommendation

For Theo Code (Rust codebase with Tauri desktop):

| Approach | Pros | Cons | Fit for Theo |
|----------|------|------|-------------|
| **Ratatui (Rust-native TUI)** | Same language as codebase. Fast. No JS dependency. | Less flexible for complex layouts. | Good for CLI-only. |
| **Ink/React (TS)** | Proven by Claude Code. Rich component ecosystem. | Requires Node/Bun runtime. Separate from Rust codebase. | Good for desktop, complex for CLI. |
| **Simple ANSI (like Aider)** | Minimal complexity. Works everywhere. | Limited visual richness. | Good MVP approach. |
| **crossterm + custom renderer (Rust)** | Full control. Rust-native. Used by many Rust CLIs. | More work upfront. | Best long-term for Theo CLI. |

**Recommendation:** Start with crossterm (already a common Rust crate) + a simple markdown renderer. This matches Theo's Rust-first architecture. Upgrade to full TUI (Ratatui) when visual complexity demands it. Avoid the Claude Code path (custom React reconciler) unless building a dedicated rendering team.

### 7.4 Command System Recommendation

Minimum viable slash command set for Theo Code v1:

```
# Session
/clear          - Clear conversation
/compact        - Compress context
/context        - Show context usage

# Model
/model          - Switch model
/effort         - Reasoning effort

# Files
/add            - Add files to context
/drop           - Remove files
/diff           - Show changes

# Execution
/run            - Shell command
/test           - Run tests

# Navigation
/help           - Show commands
/plan           - Planning mode

# Config
/settings       - Current config
/exit           - Exit
```

---

## Gaps & Open Questions

1. **Accessibility:** None of the tools document screen reader compatibility or accessibility standards for terminal UIs. This is an industry gap.
2. **Light theme support:** Codex CLI struggles with light backgrounds. Claude Code handles it via design tokens. Aider has `--light-mode`. This is a common pain point.
3. **Remote/SSH performance:** Only Claude Code documents SSH-specific optimizations (EL sequences, color memoization). Important for remote development.
4. **Mobile terminal support:** No tool documents behavior on mobile terminal emulators (Blink, Termius). Growing use case.
5. **Terminal multiplexer compatibility:** Codex has specific Zellij fixes. Claude Code has tmux-specific flicker issues. Systematic testing needed.

---

## Sources

1. [Claude Code NO_FLICKER mode analysis](https://blockchain.news/ainews/claude-code-no-flicker-mode-latest-terminal-rendering-breakthrough-and-developer-ux-analysis)
2. [Claude Code leaked source analysis (DEV.to)](https://dev.to/minnzen/i-studied-claude-codes-leaked-source-and-built-a-terminal-ui-toolkit-from-it-4poh)
3. [Claude Code overview docs](https://code.claude.com/docs/en/overview)
4. [Claude Code keybindings docs](https://code.claude.com/docs/en/keybindings)
5. [Claude Code status line docs](https://code.claude.com/docs/en/statusline)
6. [Claude Code permissions docs](https://code.claude.com/docs/en/permissions)
7. [Claude Code UI/UX DeepWiki](https://deepwiki.com/anthropics/claude-code/3.9-uiux-and-terminal-integration)
8. [Claude Code Ink Renderer DeepWiki](https://deepwiki.com/alesha-pro/claude-code/7.1-ink-renderer-and-custom-tui-engine)
9. [Claude Code TUI Architecture DeepWiki](https://deepwiki.com/flyboyer/claude-code/8-terminal-ui-(tui)-architecture)
10. [Claude Code Core UI Components DeepWiki](https://deepwiki.com/mehmoodosman/claude-code/8.2-core-ui-components)
11. [Claude Code React Ink Architecture](https://zread.ai/instructkr/claude-code/16-react-ink-component-architecture)
12. [Claude Code commands cheat sheet](https://www.scriptbyai.com/claude-code-commands-cheat-sheet/)
13. [Claude Code slash commands 2026 edition](https://clskills.in/blog/claude-code-slash-commands-2026)
14. [Claude Code internals Part 11 (Medium)](https://kotrotsos.medium.com/claude-code-internals-part-11-terminal-ui-542fe17db016)
15. [ccstatusline community tool](https://github.com/sirmalloc/ccstatusline)
16. [CCometixLine (Rust statusline)](https://github.com/Haleclipse/CCometixLine)
17. [Building custom statusline](https://www.dandoescode.com/blog/claude-code-custom-statusline)
18. [Codex CLI overview](https://developers.openai.com/codex/cli)
19. [Codex CLI features](https://developers.openai.com/codex/cli/features)
20. [Codex CLI slash commands](https://developers.openai.com/codex/cli/slash-commands)
21. [Codex CLI cheat sheet](https://computingforgeeks.com/codex-cli-cheat-sheet/)
22. [Codex CLI color emphasis issue #6531](https://github.com/openai/codex/issues/6531)
23. [Codex CLI light theme issue #2020](https://github.com/openai/codex/issues/2020)
24. [Codex CLI ANSI theme issue #12890](https://github.com/openai/codex/issues/12890)
25. [Aider commands docs](https://aider.chat/docs/usage/commands.html)
26. [Aider options reference](https://aider.chat/docs/config/options.html)
27. [Aider DeepWiki commands](https://deepwiki.com/Aider-AI/aider/2.3-commands-and-user-interactions)
28. [GitHub Copilot CLI docs](https://docs.github.com/en/copilot/how-tos/copilot-cli/use-copilot-cli)
29. [Copilot CLI slash commands cheat sheet](https://github.blog/ai-and-ml/github-copilot/a-cheat-sheet-to-slash-commands-in-github-copilot-cli/)
30. [OpenCode TUI Architecture DeepWiki](https://deepwiki.com/anomalyco/opencode/5.1-tui-architecture)
31. [OpenCode TUI performance RFC #13027](https://github.com/anomalyco/opencode/issues/13027)
32. [Ink (React for CLI)](https://github.com/vadimdemedes/ink)
33. [assistant-ui for Terminal](https://www.assistant-ui.com/ink)
34. [Agent Deck (cost tracking)](https://github.com/asheshgoplani/agent-deck)
35. [Terminal AI coding agents arxiv paper](https://arxiv.org/html/2603.05344v1)
36. [Copilot CLI persistent token indicator issue #2052](https://github.com/github/copilot-cli/issues/2052)
37. [Temm1e (open-source agent with status bar)](https://github.com/temm1e-labs/temm1e)
