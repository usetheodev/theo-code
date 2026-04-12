# ADR-001: Streaming Markdown State Machine

- **Status**: Accepted
- **Date**: 2026-04-11
- **Deciders**: Meeting 20260411-103954
- **Context**: CLI Professionalization Plan, Task T1.4

## Context

LLM responses arrive token-by-token via `ContentDelta` events. To render them as formatted markdown in the terminal without waiting for the full response, we need an **incremental** markdown renderer. No existing crate solves this — all markdown parsers assume complete input.

The challenge is that markdown syntax spans arbitrary character boundaries:

- `**bold**` only becomes bold when the closing `**` arrives
- Code blocks (` ``` `) must be buffered until the closing fence
- Tables need all rows to align columns

If we naively render each chunk, we emit broken escape sequences and the output looks corrupted.

## Decision

Implement a **stateful buffer + state machine** in `apps/theo-cli/src/render/streaming.rs`:

```
States:
  Idle                    -- accumulating plain text
  InlineFormat(kind)      -- inside **bold**, *italic*, `code`
  CodeBlockOpen(lang)     -- between ``` fence
  ListItem(indent)        -- bullet/numbered list
  Blockquote              -- > prefixed
```

**Flushing rules**:

| Input | Action |
|---|---|
| Plain char in Idle | Emit immediately |
| `**` opening | Enter InlineFormat(Bold), buffer |
| `**` closing | Emit buffered text as bold, return to Idle |
| Newline in Idle | Emit newline |
| ` ``` ` opening | Enter CodeBlockOpen, start buffering |
| ` ``` ` closing | Pipe buffer through syntect, emit block, return to Idle |
| Mid-stream reset (e.g. RunStateChanged::Idle) | Flush any pending buffer as plain text |
| Incomplete markdown at stream end | Emit as plain text (no corruption) |

**Idempotency guarantee**: Same input sequence must produce identical output regardless of chunk boundaries.

## Alternatives Considered

### Alternative 1: Buffer everything, render at end
- **Rejected**: Defeats the purpose of streaming. User waits for full response before seeing anything.

### Alternative 2: Use `termimad`
- **Rejected**: Assumes complete input, no streaming API, version coupling with crossterm.

### Alternative 3: Render raw markdown (no parsing)
- **Rejected**: Terminal user sees `**asterisks**` literally, not bold. Unprofessional.

### Alternative 4: Use Ratatui immediate-mode + re-render whole buffer each chunk
- **Rejected**: See ADR-002. Ratatui's redraw model is incompatible with append-only streaming.

## Consequences

### Positive
- Real-time feedback during LLM generation
- Code blocks get syntax highlighting the moment they close
- No corrupted output even if stream is interrupted
- Stateful flush() cleanly resets between turns

### Negative
- Custom implementation = custom maintenance burden
- State machine is the highest-risk component in F1
- Code blocks have inherent latency (must wait for closing fence)
- Property tests are mandatory to prove idempotency

### Risks
- Edge cases in nested fences (` ``` ` inside code block as text)
- Performance: state machine must handle < 1ms per chunk to not stall stream

## Implementation Notes

- Task T1.4a implements the buffer + state machine
- Task T1.4b adds `proptest` property tests + `criterion` benchmark
- Gate between T1.4a and T1.5: property tests green + bench < 1ms/chunk
- Reset state on `RunStateChanged::Idle` event
- Fallback: if parser panics or state is corrupt, emit raw text and log error

## References

- Plan: `docs/roadmap/cli-professionalization.md` (T1.4)
- Research: `outputs/reports/rust-terminal-ecosystem.md` §3.1
- Meeting: `.claude/meetings/20260411-103954-cli-professionalization.md`
- Related: ADR-002 (Reject Ratatui)
