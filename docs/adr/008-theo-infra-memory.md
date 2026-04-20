# ADR-008 — theo-infra-memory as a dedicated workspace crate

**Status:** Accepted (2026-04-20)
**Context cycle:** `outputs/agent-memory-plan.md`
**Team review:** `.claude/meetings/20260420-134446-agent-memory-sota.md` (decision #1-2, concern from arch-validator)

## Context

The memory plan introduces a coordinator (`MemoryEngine`), multiple providers (`BuiltinMemoryProvider`, `RetrievalBackedMemory`, `WikiMemoryProvider`), a 7-gate `LessonStore`, a Karpathy-style wiki compiler, and shared utilities (`atomic_write`, `fs_util`). These are infrastructure concerns — file I/O, hashing, retrieval-engine adapters, LLM compiler orchestration — symmetric to what `theo-infra-llm` and `theo-infra-auth` already provide for their domains.

`CLAUDE.md` today documents 11 workspace crates. Adding `theo-infra-memory` makes the count 12. The `arch-validator` review in the meeting flagged this as a decision that needs explicit justification (concern captured in decision #2).

## Decision

Accept `theo-infra-memory` as the 12th workspace crate, located at `crates/theo-infra-memory/`. Also accept `theo-test-memory-fixtures` as a test-only crate (excluded from release builds) when RM2/RM5 need deterministic mocks.

Dependency direction (inviolable):

```
theo-domain         → (nothing)
theo-infra-memory   → theo-domain only
                      (+ theo-engine-retrieval behind feature `memory-retrieval`, default off)
theo-application    → theo-infra-memory  (for MemoryEngine composition)
theo-agent-runtime  → theo-domain, theo-governance  (unchanged — wires via trait)
```

`theo-infra-memory` may opt into `theo-engine-retrieval` through a gated feature so the retrieval integration stays optional for offline / minimal builds.

## Rationale

1. **Symmetry with existing infra crates.** `theo-infra-llm` adapts LLM providers; `theo-infra-auth` adapts OAuth/device-flow services. Memory adapts persistence + compilation — the same layer of the dependency graph.
2. **Keeps `theo-domain` pure.** All I/O (file writes, SHA-256 hashing, Tantivy adapters, tokio sync primitives) lives in the new crate. Domain continues to depend on nothing.
3. **Feature gating works cleanly.** A new crate declares `[features]` naturally. Collapsing memory into `theo-application` would force the application crate to grow feature flags for retrieval, compiler backends, and mock LLM — bloating the composition layer.
4. **Explicit boundary for security-sensitive code.** The prompt-injection scan port from `hermes-agent` needs to live somewhere callable from tooling AND engine. A dedicated crate is the natural home; stuffing it into `theo-tooling` would couple it to sandbox primitives.
5. **Test-only fixtures crate is orthogonal.** `theo-test-memory-fixtures` is `publish = false`, excluded from release builds, and only referenced from `[dev-dependencies]`. It does not count against "no new workspace members in production" norms.

## Consequences

- `Cargo.toml` root `[workspace] members` grows from 14 to 16 entries (11 prod crates → 12 prod + 4 apps + 1 test fixture crate).
- `CLAUDE.md` count of "11 crates" bumps to 12.
- New imports allowed:
  - `theo-infra-memory` may import `theo-domain`.
  - `theo-infra-memory` may import `theo-engine-retrieval` ONLY behind the `memory-retrieval` feature flag.
  - Apps must NOT import `theo-infra-memory` directly — they go through `theo-application::memory::MemoryEngine`.
- Tests in `theo-infra-memory` may use `theo-test-memory-fixtures` in `[dev-dependencies]`.

## Alternatives considered

- **Collocate under `theo-application::memory`.** Rejected: application layer becomes a feature-flag dumping ground and loses clarity; memory impl mixes with non-memory use cases.
- **Collocate under `theo-tooling`.** Rejected: tooling is the tool runtime (apply_patch, bash, etc). Memory is a separate concern orthogonal to tool execution.
- **Keep everything in `theo-domain`.** Rejected outright: violates the `theo-domain → nothing` invariant (domain would need `tokio`, `sha2`, etc.).

## Enforcement

- `arch-validator` sub-agent includes a dependency-direction check for the new crate.
- `cargo check -p theo-infra-memory` must succeed without the `memory-retrieval` feature (proves optionality).
- CI (pre-commit hook) runs the standard clippy+test gates on the new crate.

## References

- `outputs/agent-memory-plan.md` §1 (Global DoD point 5), §2 (RM-pre-4), §3 (phase files)
- `.claude/meetings/20260420-134446-agent-memory-sota.md` decisions #1, #2
- `CLAUDE.md` workspace layout (to be updated)
- `.claude/rules/architecture.md` dependency direction table
