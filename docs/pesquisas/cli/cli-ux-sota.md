---
type: report
question: "What is the state-of-the-art in CLI UX for AI coding agents, including TUI design, operation modes, session management, shell integration, and benchmark results?"
generated_at: 2026-04-29T12:00:00-03:00
confidence: 0.86
sources_used: 24
supplements: cli-agent-ux-research.md
---

# CLI UX for AI Coding Agents: State of the Art

## Executive Summary

The CLI UX landscape for AI coding agents has matured rapidly in 2026, with terminal-native agents decisively winning the market over IDE-integrated alternatives. OpenCode leads with 150K GitHub stars and a dual-mode (Plan/Build) TUI built on Bubble Tea, demonstrating that decoupling from editors creates broader adoption. The benchmark picture is sobering: Terminal-Bench 2.0 shows even frontier agents resolve fewer than 65% of realistic CLI tasks (Claude Opus 4.7 leads at 68.54%), while LongCLI-Bench reveals pass rates below 20% for long-horizon, multi-category tasks. Key UX innovations include: OpenDev's dual-path input dispatch (slash commands to REPL handler, NL queries to agent loop), opencode's 5 operation modes (interactive/print/JSON/RPC/SDK), pi-mono's session tree with fork capability, and session management with auto-generated titles and cost tracking. Shell integration (completions for bash/zsh/fish, aliases, .env protection) and startup performance (<500ms target) are now table stakes. This research supplements cli-agent-ux-research.md, which covers rendering engines, slash commands, keyboard shortcuts, and color systems in depth.

---

## Part 1: Terminal Agent Architectures

### 1.1 OpenDev -- TUI + Web UI Dual Interface (arXiv:2603.05344)

OpenDev is an open-source coding agent written in Rust with both a TUI (Textual-based) and a Web UI (FastAPI/WebSocket).

**Dual-Path Input Dispatch:**

```
User Input
    |
    v
Input Router
    |
    +--> Starts with "/" ? --> Slash Command Handler
    |                              |
    |                              v
    |                         REPL Handler
    |                         (9 command categories)
    |
    +--> Natural language --> Agent Loop
                                |
                                v
                           Model Call -> Tool Use -> Repeat
```

**9 Command Handler Categories:**

| Category | Commands | Description |
|----------|----------|-------------|
| **Session** | `/new`, `/clear`, `/compact` | Session lifecycle |
| **Model** | `/model`, `/thinking` | Model selection and configuration |
| **Context** | `/context`, `/add`, `/drop` | Context window management |
| **Files** | `/files`, `/diff` | File management and diffs |
| **Tools** | `/tools`, `/mcp` | Tool listing and MCP management |
| **Execution** | `/run`, `/test` | Shell execution |
| **Navigation** | `/help`, `/plan` | Help and planning mode |
| **Config** | `/settings`, `/permissions` | Configuration management |
| **Debug** | `/debug`, `/logs` | Debugging and diagnostics |

**Multi-Model Workflow (5 specialized roles):**

| Role | Purpose | Tool Access |
|------|---------|-------------|
| Action Model | Primary execution | Full tool access |
| Thinking Model | Extended reasoning | No tools (reasoning only) |
| Critique Model | Self-evaluation (Reflexion-inspired) | Read-only |
| Vision Model | Screenshots, images | Image processing |
| Compact Model | Fast summarization for context compression | Summarization only |

**Key Architectural Decision:** Using API-reported `prompt_tokens` for context calibration rather than local estimates. Providers inject invisible content (safety preambles, tool-schema serialization) causing local counts to systematically underestimate actual usage.

### 1.2 opencode -- 5 Operation Modes

opencode (150K+ GitHub stars, Go-based) provides the broadest mode coverage of any CLI agent:

| Mode | Interface | Use Case |
|------|-----------|----------|
| **Interactive** | TUI (Bubble Tea) | Primary development work |
| **Print** | Stdout | Scripting, piping output |
| **JSON** | Structured JSON to stdout | Machine-readable output for tooling |
| **RPC** | JSON-RPC server | Remote control from other processes |
| **SDK** | Go library | Embedding in other applications |

**Dual-Mode Agent System:**

- **Build mode** (default): Full tool access for development work
- **Plan mode**: Read-only tools for analysis and code exploration
- Tab key toggles between modes

**Key UX Insight:** The friction of approving a plan before execution is low enough that developers do it for anything bigger than a one-file change, and the errors it catches would otherwise cost 40 minutes of `git reset --hard`.

**Client/Server Architecture:** The TUI frontend is just one possible client. opencode can run on a dev box while driven remotely from a mobile app.

**TUI Features:**
- Bubble Tea framework for smooth terminal experience
- Vim-like editor integration
- `ctrl+t` to cycle through model variants and toggle reasoning capabilities
- Toggle visibility of thinking/reasoning blocks
- Persistent SQLite storage for sessions
- LSP integration for language-aware features

### 1.3 pi-mono -- Advanced Session Management

pi-mono provides the most sophisticated session management among CLI agents:

| Feature | Description |
|---------|-------------|
| **Session tree view** | `/tree` command shows full session hierarchy |
| **Fork** | Create a branch of the current session state |
| **40+ commands** | Most extensive command set of any CLI agent |
| **Editor** | Built-in TUI editor for multi-line input |
| **Session persistence** | Full history across restarts |

### 1.4 Why Terminal Agents Won 2026

The interface is whatever your terminal supports -- text, a TUI, keybindings. There is no editor integration because the editor is wherever you want it to be. It can be Neovim on a remote dev box, VS Code on a laptop, Helix in a tmux session, or no editor at all. The agent operates on files, not on buffers.

This decoupling is the thing that Cline and Cursor explicitly rejected. Both bet that deep IDE integration (selection-aware context, inline diffs, click-to-apply) would be the defining UX. They were not wrong about the UX. They were wrong about the market.

**The Broader CLI Agent Ecosystem (April 2026):**

| Agent | Stars | Language | Key Feature |
|-------|-------|----------|-------------|
| **OpenCode** | 150K | Go | 75+ provider support, LSP integration, privacy-first |
| **Gemini CLI** | 103K | TypeScript | Google-powered, tools for repo work and research |
| **Codex CLI** | 78K | Rust | OpenAI-powered, interactive TUI, tool execution |
| **Claude Code** | -- | TypeScript | Anthropic-powered, 389 components, 500K+ daily sessions |
| **amux** | -- | -- | Terminal UI for running multiple agents in parallel |
| **AgentPipe** | -- | -- | Multi-agent conversations in shared rooms |

---

## Part 2: Benchmark Results

### 2.1 Terminal-Bench 2.0 (ICLR 2026)

Terminal-Bench 2.0 is the primary benchmark for evaluating agents on realistic CLI tasks. Published at ICLR 2026, it includes 89 tasks across categories ranging from model training to system administration.

**Key Finding:** Frontier agents resolve fewer than 65% of realistic CLI tasks.

**Leaderboard (April 2026, vals.ai):**

| Rank | Model/Agent | Score |
|------|-------------|-------|
| 1 | Claude Opus 4.7 | **68.54%** |
| 2 | Gemini 3.1 Pro Preview | 67.42% |
| 3 | GPT 5.3 Codex | 64.05% |
| 4 | GPT 5.5 | 62.92% |
| 5 | Muse Spark / Claude Sonnet 4.6 | 59.55% (tied) |
| 6 | Claude Opus 4.5 (Nonthinking) | 58.43% |
| 7 | Claude Opus 4.6 (Thinking) | 58.43% |

**Evaluation Setup:**
- 6 state-of-the-art agents evaluated across 16 frontier models
- 32,155 total trials
- Harbor framework for building and running agent evaluations at scale
- Daytona used to run 32-100 containers in parallel
- Tasks range from training ML models to building Linux from source to reverse engineering binaries
- 93 contributors created 229 tasks (crowd-sourced)

**Terminal-Bench Hard (Artificial Analysis):**

| Model | Score |
|-------|-------|
| GPT-5.5 (xhigh) | 60.6% |
| GPT-5.5 (high) | 59.8% |
| GPT-5.5 (medium) | 57.6% |

### 2.2 LongCLI-Bench (arXiv:2602.14337)

LongCLI-Bench evaluates long-horizon, multi-category CLI tasks -- a significantly harder benchmark.

**Key Finding:** All agents yield pass rates below 20%. Most tasks stall at less than 30% completion.

**Task Categories:**

| Category | Description | Example |
|----------|-------------|---------|
| **From Scratch** | Build a complete application from nothing | Build a REST API with auth, tests, docs |
| **Feature Addition** | Add features to existing codebase | Add pagination + search to existing API |
| **Bug Fixing** | Diagnose and fix bugs | Fix race condition in event processing |
| **Refactoring** | Restructure code preserving behavior | Extract microservice from monolith |

**Evaluation Methodology:**
- 20 high-quality tasks curated from 1,000+ CS assignments and real-world workflows
- Dual-set testing: fail-to-pass (requirement fulfillment) + pass-to-pass (regression avoidance)
- Step-level scoring to pinpoint where execution fails
- Expert completion time: 1,000+ minutes average (vs. 206.7 min for Terminal-Bench)

**Agents Evaluated:**
- Commercial: Codex (GPT-5.x), Claude Code (claude-sonnet/opus-4.x)
- Open-source: OpenHands with DeepSeek-V3.1, GLM-4.6, Qwen3-235B-A22B

**Root Cause Analysis:** Planning and execution proficiency are the key limitations, not model capability. Future research should prioritize core engineering proficiencies, long-horizon contextual consistency maintenance, and effective human collaboration.

### 2.3 Benchmark Comparison

| Benchmark | Tasks | Avg. Expert Time | Best Agent Score | What It Measures |
|-----------|-------|------------------|-----------------|-----------------|
| **Terminal-Bench 2.0** | 89 | 206.7 min | 68.54% (Opus 4.7) | Single-task CLI competence |
| **Terminal-Bench Hard** | Subset | Higher | 60.6% (GPT-5.5) | Hardest CLI tasks |
| **LongCLI-Bench** | 20 | 1,000+ min | <20% | Long-horizon, multi-category |
| **SWE-Bench Verified** | 500 | -- | 89.1% (MCE) | Bug fixing in real repos |

**Implications for Theo Code:** The gap between SWE-Bench performance (89%) and LongCLI-Bench (<20%) reveals that current agents are competent at isolated tasks but struggle with sustained, multi-step engineering work. Theo's CLI UX must support long-horizon sessions with robust context management, session persistence, and plan/execute separation.

---

## Part 3: Shell Integration

### 3.1 Shell Completions

All production CLI agents provide shell completions for at least bash and zsh:

| Agent | Bash | Zsh | Fish | Installation |
|-------|------|-----|------|-------------|
| **Claude Code** | Yes | Yes | Yes | `claude completion bash/zsh/fish` |
| **Codex CLI** | Yes | Yes | Yes | `codex completion bash/zsh/fish` |
| **opencode** | Yes | Yes | Yes | `opencode completion bash/zsh/fish` |
| **Gemini CLI** | Yes | Yes | No | `gemini completion bash/zsh` |

Completions cover: subcommands, flags, file paths, model names, and slash commands.

### 3.2 Aliases and Environment Variables

**Common alias patterns:**

```bash
# Quick access
alias cc="claude"
alias oc="opencode"
alias cx="codex"

# Mode shortcuts
alias plan="claude --plan"
alias ask="claude --print"  # non-interactive mode
```

**Environment variable patterns:**

| Variable | Purpose | Example |
|----------|---------|---------|
| `CLAUDE_MODEL` / `OPENAI_MODEL` | Default model | `claude-opus-4.6` |
| `*_API_KEY` | Authentication | `ANTHROPIC_API_KEY=sk-...` |
| `EDITOR` / `VISUAL` | External editor for long prompts | `nvim` |
| `NO_COLOR` | Disable colors (standard) | `1` |
| `TERM` | Terminal capability detection | `xterm-256color` |
| `CLAUDE_CODE_NO_FLICKER` | Alt-screen rendering | `1` |

### 3.3 .env Protection

All production agents implement .env file protection:

| Agent | Strategy |
|-------|----------|
| **Claude Code** | .env files excluded from context by default. Permission system blocks reads of sensitive files. |
| **Codex CLI** | Sandbox policy prevents reading .env unless explicitly allowed. |
| **opencode** | .env files in .gitignore are auto-excluded. Warning on sensitive file access. |

### 3.4 Non-Interactive Modes

For scripting and CI/CD integration:

```bash
# Claude Code
echo "explain this error" | claude --print
claude -p "fix the failing test" --output-format json

# opencode
opencode "explain this code" --print
opencode "generate types" --json

# Codex CLI
codex "fix lint errors" --approval=auto
```

---

## Part 4: Session Management

### 4.1 Session Titles (Auto-Generated)

| Agent | Title Generation | Storage |
|-------|-----------------|---------|
| **Claude Code** | LLM-generated from first message | SQLite / filesystem |
| **opencode** | LLM-generated, shown in session list | SQLite |
| **Codex CLI** | LLM-generated, shown in `/resume` | Local filesystem |
| **Aider** | Not auto-titled | `.aider.chat.history.md` |

### 4.2 Session Resumption

| Agent | Resume Command | What's Preserved |
|-------|---------------|-----------------|
| **Claude Code** | `claude --continue` or `claude --resume <id>` | Full conversation, context, cost |
| **Codex CLI** | `/resume` | Conversation history, model settings |
| **opencode** | Automatic (persistent SQLite) | Full state |
| **Aider** | `--restore-chat-history` | Chat messages (not context) |

**Session Index (Self-Healing):**

Claude Code implements a self-healing session index:
- Sessions stored in `~/.claude/sessions/`
- Index file tracks session metadata (title, model, cost, last activity)
- On corruption, index is rebuilt from session files
- Session pruning: old sessions auto-archived after configurable period

### 4.3 Cost Tracking Across Sessions

| Agent | Cost Display | Granularity |
|-------|-------------|-------------|
| **Claude Code** | `cost.total_cost_usd` in status line JSON | Per-session, cumulative |
| **Codex CLI** | Token count in `/status` | Per-session |
| **opencode** | Token count in TUI status bar | Per-session |
| **Agent Deck** | Cross-agent dashboard (`$` key) | Daily/weekly/monthly budgets |

### 4.4 Session Branching and Forking

| Agent | Feature | Command |
|-------|---------|---------|
| **Claude Code** | Branch conversation state | `/branch` |
| **Codex CLI** | Fork session | `/fork` |
| **pi-mono** | Fork + session tree view | `/tree`, fork |
| **opencode** | Not supported | -- |

---

## Part 5: Startup Performance

### 5.1 Startup Time Targets

| Agent | Target | Approach |
|-------|--------|----------|
| **OpenDev** | 4.3ms | Rust binary, lazy loading, no runtime |
| **opencode** | ~100ms | Go binary, lazy loading |
| **Claude Code** | ~500ms | Node.js, lazy imports, caching |
| **Codex CLI** | ~200ms | Rust binary |
| **Aider** | ~1-2s | Python, pip dependencies |

**Lazy Loading Reduces Startup:**
- OpenDev: tool schemas loaded on first use, not at startup
- Claude Code: 54 tools registered but schemas loaded lazily
- opencode: provider initialization deferred until first model call

### 5.2 Startup Sequence (Best Practice)

```
1. Parse CLI arguments          [<1ms]
2. Load config file             [<5ms]
3. Initialize TUI framework     [10-50ms]
4. Register tool schemas        [lazy -- 0ms at startup]
5. Connect to providers         [lazy -- 0ms at startup]
6. Load session (if --continue) [50-200ms]
7. Display prompt               [total: <500ms target]
```

---

## Part 6: Subcommand Architecture

### 6.1 Subcommand Inventory (Cross-Tool)

Production CLI agents converge on approximately 17 subcommands:

| Subcommand | Claude Code | Codex CLI | opencode | Description |
|------------|-------------|-----------|----------|-------------|
| `run` (default) | Yes | Yes | Yes | Start interactive session |
| `completion` | Yes | Yes | Yes | Generate shell completions |
| `config` | Yes | Yes | Yes | Manage configuration |
| `auth` / `login` | Yes | Yes | Yes | Authentication |
| `doctor` | Yes | No | No | Diagnose installation issues |
| `mcp` | Yes | Yes | Yes | MCP server management |
| `version` | Yes | Yes | Yes | Show version |
| `help` | Yes | Yes | Yes | Show help |
| `init` | No | Yes | Yes | Initialize project config |
| `update` | Yes | Yes | No | Self-update |
| `api` | Yes | No | No | Direct API access |
| `print` / `-p` | Yes | Yes | Yes | Non-interactive mode |
| `review` | No | Yes | No | Code review mode |
| `resume` | Yes | Yes | No | Resume session |
| `list` | No | No | Yes | List sessions |
| `serve` | No | No | Yes | Start RPC server |
| `export` | No | No | Yes | Export session data |

### 6.2 Help System Requirements

Every subcommand must support `--help`:

```
$ theo --help
theo-code v0.1.0 -- AI coding agent for the terminal

USAGE:
    theo [SUBCOMMAND] [OPTIONS] [PROMPT]

SUBCOMMANDS:
    run          Start interactive session (default)
    config       Manage configuration
    auth         Authentication
    doctor       Diagnose installation issues
    mcp          MCP server management
    completion   Generate shell completions
    version      Show version information

OPTIONS:
    -p, --print         Non-interactive mode (print response and exit)
    -m, --model MODEL   Override default model
    --continue          Resume last session
    --resume ID         Resume specific session
    --json              Output in JSON format
    -h, --help          Show this help
    -V, --version       Show version

EXAMPLES:
    theo                          Start interactive session
    theo "fix the failing test"   Non-interactive mode
    theo --continue               Resume last session
    theo config set model opus    Set default model
```

---

## Part 7: Evidence Table

| System | TUI Framework | Operation Modes | Slash Commands | Shell Completions | Session Resume | Startup Time | Terminal-Bench Score |
|--------|--------------|----------------|---------------|-------------------|---------------|-------------|---------------------|
| **Claude Code** | Custom React/Ink | 2 (interactive, print) | 30+ | bash/zsh/fish | `--continue`, `--resume` | ~500ms | 58-68% (model dependent) |
| **opencode** | Bubble Tea (Go) | 5 (interactive, print, JSON, RPC, SDK) | 20+ | bash/zsh/fish | Automatic (SQLite) | ~100ms | -- |
| **Codex CLI** | Rust TUI | 2 (interactive, print) | 25+ | bash/zsh/fish | `/resume` | ~200ms | 64% (GPT 5.3) |
| **OpenDev** | Textual + FastAPI | 2 (TUI, Web UI) | 15+ | bash/zsh | `--continue` | 4.3ms | -- |
| **Aider** | prompt-toolkit | 2 (interactive, scripting) | 38 | No native | `--restore-chat-history` | ~1-2s | -- |
| **pi-mono** | Custom TUI | 1 (interactive) | 40+ | bash/zsh | Full tree | -- | -- |
| **Gemini CLI** | TypeScript TUI | 2 (interactive, print) | 15+ | bash/zsh | -- | ~300ms | 67% (Gemini 3.1) |

---

## Part 8: Thresholds and Targets

### CLI UX Performance Targets

| Metric | Current (Theo) | SOTA Target | Gap |
|--------|---------------|-------------|-----|
| Subcommands | -- | 17 (industry convergence) | Needs full CLI |
| Startup time | -- | <500ms | Rust should achieve this |
| All subcommands `--help` | -- | 100% | Required |
| Shell completions | -- | bash + zsh + fish | Needed |
| Operation modes | -- | >= 3 (interactive, print, JSON) | Needed |
| Slash commands | -- | >= 15 (minimum viable set) | Needed |
| Session resume | -- | `--continue` + `--resume <id>` | Needed |
| Session titles | -- | Auto-generated by LLM | Needed |
| Cost tracking | -- | Per-session USD display | Needed |
| Plan/Build modes | -- | Tab-key toggle | Needed |
| .env protection | -- | Auto-exclude from context | Needed |
| Non-interactive mode | -- | `-p` / `--print` | Needed |

### Terminal-Bench Targets

| Metric | Current Best | Aspirational |
|--------|-------------|-------------|
| Terminal-Bench 2.0 | 68.54% (Opus 4.7) | >70% |
| Terminal-Bench Hard | 60.6% (GPT-5.5) | >65% |
| LongCLI-Bench | <20% (all agents) | >30% (significant improvement) |

---

## Part 9: Relevance for Theo Code

### What Theo Code Has (from cli-agent-ux-research.md)

The existing research covers:
- Rendering engine comparison (React/Ink vs Ratatui vs prompt-toolkit vs crossterm)
- Complete slash command inventories for Claude Code, Codex CLI, Aider, Copilot CLI
- Keyboard shortcut mapping across tools
- Color systems and design tokens
- Permission UIs and confirmation patterns
- Status line and context visualization
- Recommended crossterm + simple markdown renderer as starting point

### What Theo Code Needs to Reach 4.0+

| Priority | Gap | Approach | Complexity | Evidence |
|----------|-----|----------|------------|----------|
| **P0** | No CLI exists | Implement CLI with clap (Rust) -- 17 subcommands | High | Industry convergence on subcommand structure |
| **P0** | No TUI | Implement with Ratatui (Rust-native) | High | Matches Rust codebase, no Node dependency |
| **P0** | No slash commands | Implement dual-path input dispatch (OpenDev pattern) | Medium | 9 categories, REPL handler for commands, agent loop for NL |
| **P1** | No Plan/Build modes | Implement dual-mode agent (opencode pattern) | Medium | Tab-key toggle, read-only vs full-access tools |
| **P1** | No session management | Implement session persistence with SQLite | Medium | Auto-titles, resume, cost tracking |
| **P1** | No shell completions | Generate via clap's built-in completion support | Low | bash/zsh/fish required |
| **P2** | No non-interactive mode | Add `-p`/`--print` and `--json` flags | Low | Required for scripting and CI/CD |
| **P2** | No .env protection | Add sensitive file detection and auto-exclusion | Low | Security requirement |
| **P2** | No cost tracking | Track API costs per session, display in status line | Medium | Per-session and cumulative |
| **P3** | No session branching | Implement `/branch` and `/fork` | Medium | pi-mono and Claude Code patterns |
| **P3** | No multi-agent TUI | Support for running/monitoring parallel agents | High | amux pattern for parallel agent display |

### Architecture Recommendation

```
theo (CLI binary, clap-based)
├── Subcommands
│   ├── run (default)       # Start interactive TUI session
│   ├── print / -p          # Non-interactive, stdout
│   ├── config              # Configuration management
│   ├── auth                # API key management
│   ├── doctor              # Installation diagnostics
│   ├── mcp                 # MCP server management
│   ├── completion          # Shell completions (bash/zsh/fish)
│   ├── resume              # Resume session by ID
│   ├── init                # Initialize project config
│   ├── version             # Version info
│   └── help                # Help text
│
├── TUI (Ratatui)
│   ├── InputRouter         # Dual-path: "/" -> CommandHandler, NL -> AgentLoop
│   ├── CommandHandler      # 9 categories of slash commands
│   ├── StreamingRenderer   # Token-by-token markdown rendering
│   ├── DiffViewer          # Unified diff display for file changes
│   ├── StatusLine          # Model, tokens, cost, context %, git branch
│   ├── PermissionDialog    # Confirmation for destructive actions
│   └── ModeIndicator       # Plan vs Build mode display
│
├── Session Manager
│   ├── SessionStore        # SQLite persistence
│   ├── TitleGenerator      # LLM-generated session titles
│   ├── CostTracker         # Per-session cost accumulation
│   ├── SessionIndex        # Self-healing index with auto-rebuild
│   └── SessionBrancher     # Fork/branch session state
│
└── Shell Integration
    ├── Completions          # clap_complete for bash/zsh/fish
    ├── EnvProtection        # .env file detection and exclusion
    └── AliasSupport         # Documented alias patterns
```

### Minimum Viable CLI (P0 scope)

```
Phase 1: Basic CLI
  - clap binary with run/print/config/version/help subcommands
  - All subcommands support --help
  - Shell completions for bash and zsh
  - Startup time <500ms
  
Phase 2: TUI
  - Ratatui-based interactive mode
  - Dual-path input dispatch
  - 15 slash commands (session + model + context + files + help)
  - Streaming markdown with syntax highlighting
  - Status line (model, tokens, context %)
  
Phase 3: Session Management
  - SQLite session persistence
  - --continue and --resume flags
  - Auto-generated session titles
  - Cost tracking per session
  - Plan/Build mode toggle
  
Phase 4: Polish
  - .env protection
  - Non-interactive JSON output
  - Session branching/forking
  - Fish completions
  - Doctor subcommand for diagnostics
```

---

## Sources

- [Terminal-Bench 2.0 (ICLR 2026)](https://arxiv.org/abs/2601.11868)
- [Terminal-Bench 2.0 Leaderboard (vals.ai)](https://www.vals.ai/benchmarks/terminal-bench-2)
- [Terminal-Bench 2.0 (BenchLM)](https://benchlm.ai/benchmarks/terminalBench2)
- [Terminal-Bench Hard (Artificial Analysis)](https://artificialanalysis.ai/evaluations/terminalbench-hard)
- [Terminal-Bench GitHub](https://github.com/laude-institute/terminal-bench)
- [LongCLI-Bench (arXiv:2602.14337)](https://arxiv.org/abs/2602.14337)
- [LongCLI-Bench GitHub](https://github.com/finyorko/longcli-bench)
- [OpenDev -- Building AI Coding Agents for the Terminal (arXiv:2603.05344)](https://arxiv.org/html/2603.05344v2)
- [OpenDev GitHub](https://github.com/opendev-to/opendev)
- [opencode Documentation](https://opencode.ai/docs/)
- [opencode TUI Documentation](https://opencode.ai/docs/tui/)
- [opencode CLI Documentation](https://opencode.ai/docs/cli/)
- [opencode GitHub](https://github.com/opencode-ai/opencode)
- [opencode: Why Terminal Agents Won 2026 (DEV Community)](https://dev.to/ji_ai/opencode-hit-140k-stars-why-terminal-agents-won-2026-aci)
- [Awesome CLI Coding Agents (GitHub)](https://github.com/bradAGI/awesome-cli-coding-agents)
- [LangChain -- Evaluating Deep Agents CLI on Terminal-Bench 2.0](https://blog.langchain.com/evaluating-deepagents-cli-on-terminal-bench-2-0/)
- [SlopCodeBench: Degradation Over Long-Horizon Tasks (arXiv:2603.24755)](https://arxiv.org/html/2603.24755v1)
- [Dive into Claude Code (arXiv:2604.14228)](https://arxiv.org/abs/2604.14228)
- [OpenDev Architecture (co-r-e.com)](https://co-r-e.com/method/opendev-terminal-coding-agent)
- [OpenCode Blog (openreplay)](https://blog.openreplay.com/opencode-ai-coding-agent/)
- [opencode Agents Documentation](https://opencode.ai/docs/agents/)
- [Google ADK -- Context Compression](https://google.github.io/adk-docs/context/compaction/)
- [Context Compaction in Agent Frameworks (DEV Community)](https://dev.to/crabtalk/context-compaction-in-agent-frameworks-4ckk)
- [ZenML -- OpenDev Terminal-Native AI Coding Agent](https://www.zenml.io/llmops-database/terminal-native-ai-coding-agent-with-multi-model-architecture-and-adaptive-context-management)
