# ADR-002: Reject Ratatui — Use Crossterm Alone

- **Status**: Accepted
- **Date**: 2026-04-11
- **Deciders**: Meeting 20260411-103954
- **Context**: CLI Professionalization Plan, Fase 1

## Context

The Rust TUI ecosystem has two dominant approaches for terminal apps:

1. **Crossterm alone** — low-level terminal control, append-only output, streaming-friendly
2. **Ratatui + Crossterm backend** — immediate-mode full-screen TUI with double-buffered rendering

The theo-cli is a streaming AI coding agent (like Claude Code, Aider). Users scroll back through session history, pipe output to files, and expect terminal-standard behavior (Ctrl+C, Ctrl+L).

## Decision

**Use crossterm alone. Do not adopt Ratatui.**

## Rationale

### Ratatui's model is wrong for streaming agents

Ratatui maintains two buffers (current + previous) and diffs them on every `terminal.draw()` call, writing only changed cells. This is optimal for apps like `gitui` or `lazygit` where the entire screen is redrawn 30+ times per second.

For theo-cli, this is wrong because:

| Concern | Ratatui | Crossterm alone |
|---|---|---|
| Append-only stream | ❌ Redraws whole screen | ✅ Just emit lines |
| Scrollback history | ❌ Lost on exit (alternate screen) | ✅ Native terminal scrollback |
| Piped output (`theo \| tee`) | ❌ Breaks | ✅ Works (with TTY detection) |
| LLM token streaming | ❌ Frame-based, not char-based | ✅ Direct write per chunk |
| Ctrl+L / Ctrl+C | ❌ Custom handling | ✅ OS-native |

### Real-world reference

- **Claude Code**: uses ink (React for CLI), append-only streaming
- **Aider**: plain stdout with colors, no TUI framework
- **bat / delta**: crossterm-based, no Ratatui
- **gitui / lazygit**: Ratatui/tview — but these are **not streaming agents**

### Ratatui would add

- ~500KB to binary
- Additional dependency tree (layout, widgets, buffer diffing)
- Cognitive overhead for contributors who expect TUI paradigms
- Breaks user muscle memory (alternate screen, no scrollback)

## Alternatives Considered

### Alternative 1: Hybrid (Ratatui for status bar, crossterm for content)
- **Rejected**: Status bar can be rendered via crossterm cursor save/restore or simply redrawn on newlines. Adding Ratatui just for this is overkill.

### Alternative 2: Ratatui in alternate screen for interactive commands
- **Rejected**: Would create inconsistent UX (some commands fullscreen, others not). `dialoguer` handles interactive prompts cleanly without Ratatui.

## Consequences

### Positive
- Smaller binary
- Native terminal behavior preserved
- Streaming-first architecture stays simple
- No "frame rate" concerns
- Output pipeable to other tools

### Negative
- No split-pane layouts (if ever needed)
- No built-in widgets (tables done via `comfy-table`)
- Custom status line implementation needed

### When to Revisit

Reopen this ADR if theo-cli evolves to require:
- Persistent split panes (code view + chat)
- Full-screen modal interactions
- Vim-style modal editing with multiple viewports
- Real-time dashboards with multiple concurrent updates

At that point, a **new binary** (`theo-tui`) would be more appropriate than retrofitting the streaming CLI.

## References

- Plan: `docs/roadmap/cli-professionalization.md`
- Research: `outputs/reports/rust-terminal-ecosystem.md` §2
- Meeting: `.claude/meetings/20260411-103954-cli-professionalization.md`
- Related: ADR-001 (Streaming Markdown)
