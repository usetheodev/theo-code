# Architecture Decision Records

Each ADR captures a decision that shaped the code and could confuse a future
reader without context. Add a new ADR when:

1. The reason for a design is **not obvious from the code**.
2. You are rejecting an alternative that seems plausible on paper.
3. The decision crosses a team / bounded-context boundary.
4. A refactor touches a public contract, a workspace crate relationship, or
   a security invariant.

Keep ADRs short (≤ 1 page of substance). File-name pattern:
`ADR-NNN-short-topic.md` (three-digit zero-padded, hyphen-separated topic).
Legacy entries without the `ADR-` prefix are kept for history.

## Index

| ID | Title | Status | Scope |
| --- | --- | --- | --- |
| [ADR-001](ADR-001-streaming-markdown.md) | Streaming markdown state machine | Accepted | theo-cli render |
| [ADR-002](ADR-002-reject-ratatui.md) | Initial decision to reject ratatui (later reversed) | Superseded by ADR-003 | theo-cli TUI |
| [ADR-003](ADR-003-xdg-paths.md) | XDG base-dir paths for persistent state | Accepted | theo-cli config |
| [ADR-004](ADR-004-cli-infra-exception.md) | CLI-infra exception for model/config wiring | Accepted | theo-cli |
| [003](003-tui-ratatui-migration.md) | CLI → TUI ratatui migration (legacy numbering) | Accepted | theo-cli, agent-runtime, tooling |
| [004](004-interactive-approval-gate.md) | Interactive approval gate (legacy numbering) | Accepted | theo-cli, governance |
| [008](008-theo-infra-memory.md) | theo-infra-memory crate introduction | Accepted | theo-infra-memory |
| [ADR-009](ADR-009-agent-observability-engine.md) | Agent observability engine | Accepted | agent-runtime |
| [ADR-010](ADR-010-architecture-contract-interpretation.md) | `allowed_workspace_deps` is an upper bound, not a mandate | Accepted | architecture contract (T1.4) |
| [ADR-011](ADR-011-retrieval-graph-dependency.md) | `theo-engine-retrieval` may depend on `-graph` and `-parser` | Accepted | architecture contract (T1.6) |
| [ADR-012](ADR-012-frontend-major-upgrades.md) | Front-end major upgrade strategy (React 19, Tailwind 4, …) | Accepted | apps/theo-ui (T3.4) |
| [ADR-013](ADR-013-playwright-e2e-deferral.md) | Defer Playwright browser E2E suite | Accepted | apps/theo-ui (T5.7) |
| [ADR-014](ADR-014-prefer-manual-validation-over-garde.md) | Prefer manual `validate()` fns over `garde` (KISS/YAGNI) | Accepted | theo-agent-runtime + all future DTOs (T6.3) |
| [ADR-015](ADR-015-desktop-ipc-thin-shim-tests.md) | Desktop IPC coverage lives in `theo-application`, not `theo-desktop` | Accepted | apps/theo-desktop (T5.6) |
| [ADR-016](ADR-016-agent-runtime-orchestrator-deps.md) | `theo-agent-runtime` may depend on its orchestrated infra crates | Accepted | theo-agent-runtime (T1.1) |
| [ADR-017](ADR-017-inline-io-tests-triage.md) | Inline `#[test]` blocks are acceptable when hermetic (tempfile-isolated) | Accepted | check-inline-io-tests.sh (T5.2) |
| [ADR-018](ADR-018-phase4-refactors-tracked-via-allowlist.md) | Phase-4 refactors tracked via size allowlist + decomposition plan | Accepted | run_engine / tui::run / compact_with_policy (T4.1/T4.2/T4.3) |
| [ADR-019](ADR-019-unwrap-gate-enforced-baseline.md) | Unwrap/expect gate enforced via moving baseline | Accepted | check-unwrap.sh (T2.5) |
| [ADR-020](ADR-020-coverage-baseline-ci-expansion.md) | Coverage baseline expansion happens in CI, not in-session | Accepted | tarpaulin/mutants (T5.1) |

## Writing a new ADR

1. Reserve the next number by running `ls docs/adr/ADR-*.md | tail -1` and adding 1.
2. Copy the structure of an existing short ADR (e.g. ADR-010 or ADR-012).
3. Required sections: **Context**, **Decision**, **Consequences**. Optional:
   *Why not X*, *Risks*, *Guard-rails*.
4. Add a one-line row to the index above.
5. If the ADR changes an `INVIOLABLE` rule in `.claude/rules/*.md`, update that
   rule in the same PR.

## Conventions

- Date format `YYYY-MM-DD` (ISO-8601) in the ADR body.
- `Status:` one of `Proposed | Accepted | Superseded | Rejected`.
- When superseding an earlier ADR, list both `Status: Superseded by ADR-NNN`
  in the old file AND `Supersedes: ADR-MMM` in the new file.
- Prefer Markdown tables for matrices; avoid diagrams unless a diagram is
  the clearest representation.
