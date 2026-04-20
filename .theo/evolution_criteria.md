# SOTA Criteria — Smart Model Routing (6-phase plan)

**Version:** 4.0 (model-routing cycle)
**Date:** 2026-04-20
**Plan:** `outputs/smart-model-routing-plan.md`
**Research:** `outputs/smart-model-routing.md`

## Completion promise decoder

The user set `completion_promise = "TODAS TASKS, E DODS CONCLUIDOS E VALIDADOS"`. Decoded:

- **TODAS TASKS** = all 6 phases (R0 through R5) committed and green.
- **E DODS** = every global DoD gate (10 items, §1 of the plan) plus every per-phase DoD extra must pass.
- **CONCLUIDOS E VALIDADOS** = each of the 40 acceptance-criteria tests exists, has a named `#[test]` function, and passes on `cargo test --workspace`.

The `<promise>` is only emitted when all three clauses are true. Partial convergence (e.g. "R1-R3 done, R4 pending") does NOT satisfy the promise and must return to IMPLEMENT.

## Rubric (5 dimensions, score 0-3; average >= 2.5 converges a single cycle)

Each individual implement cycle (one phase of the plan) is scored against the rubric separately. The loop only converges at Phase 5 once every plan phase has converged.

### 1. Pattern Fidelity
- **3** — The landed code traceably follows a reference pattern cited in `outputs/smart-model-routing.md`. In-code comment names the source (e.g. "ref: hermes-agent/.../smart_model_routing.py:62-107").
- **2** — Reference pattern applied with small idiomatic adjustments for Rust.
- **1** — Loose inspiration; no explicit source citation.
- **0** — Ad-hoc, no reference.

### 2. Architectural Fit
- **3** — `theo-domain` stays dep-free; consumer crates use new surface through the trait only; no circular imports; no `unwrap()` in production paths; typed errors via `thiserror`.
- **2** — One minor boundary friction (e.g. helper in tooling instead of domain) without violation.
- **1** — Cross-crate duplication to avoid an import.
- **0** — Violates `theo-domain → (nothing)` or adds `unwrap()` in a hot path.

### 3. Completeness (per-phase)
- **3** — All acceptance criteria for that phase pass (counted from the plan's AC tables); per-phase DoD extras land; regression test enforces future invariant.
- **2** — All ACs pass but per-phase DoD extras partial.
- **1** — Only happy-path AC passes; edge cases unaddressed.
- **0** — Scaffolding only; no AC actually passes.

### 4. Testability
- **3** — Every AC is a named `#[test]` with Arrange-Act-Assert structure. Integration test exercises the runtime pipeline. Where applicable, a `proptest` or property guard fires at least 100 cases.
- **2** — Unit tests for the happy path plus at least one failure path.
- **1** — Smoke test only.
- **0** — No tests or tests cover only the defaults.

### 5. Simplicity
- **3** — Phase lands in ≤ 200 LOC, no speculative abstraction, every new trait method is justified by ≥ 2 concrete consumers.
- **2** — One change crosses 200 LOC but decomposition isn't possible without losing atomicity.
- **1** — Speculative extension point with no consumer.
- **0** — Refactor sprawl; the phase rewrites unrelated code.

## Global Definition of Done (inherited from plan §1)

Every commit must satisfy all 10:

1. `cargo test --workspace` exits 0.
2. `cargo check --workspace --tests` emits 0 warnings.
3. `.githooks/pre-commit` + `.githooks/commit-msg` pass without `--no-verify`.
4. No `Co-Authored-By:` or `Generated-with` trailer (enforced by hook).
5. `theo-domain → (nothing)`; no new external deps in that crate.
6. TDD order documented (commit body cites the failing test commit-scope and the implementation).
7. Change ≤ 200 LOC (including tests).
8. Harness score ≥ 75.150.
9. Zero `unwrap()` in production code paths.
10. Plan traceability updated (`.theo/evolution_research.md` or similar).

## Per-phase completeness checkpoints

| Phase | Must land before proceeding |
|---|---|
| R0 | 4 AC tests green; `.theo/fixtures/routing/` has 30 labelled JSON cases; `cargo test -p theo-infra-llm --test routing_metrics` emits JSON report. |
| R1 | 6 AC tests green; `theo-domain::routing` module exists; trait is object-safe; `NullRouter` is behaviour-preserving. |
| R2 | 8 AC tests green; `RuleBasedRouter` in `theo-infra-llm`; paraphrased keyword list with `paraphrased-from:` header; `PricingTable` loads from config. |
| R3 | 6 AC tests green; `RunEngine` routes every turn through `router.route()`; structural-hygiene test enforces single call-site. |
| R4 | 8 AC tests green; compaction uses `RoutingPhase::Compaction`; subagent roles map to slots; TOML parsing works; env override works; CLI flag works. |
| R5 | 8 AC tests green; cascade bounded to 2 hops; `LlmError::FallbackExhausted` variant; property test for "fallback never returns same model". |

## Guardrails specific to this cycle

- **Hermes-agent keyword list is AGPL-3.0.** The R2 keyword list is paraphrased, not verbatim. Header comment must include `paraphrased-from: referencias/hermes-agent/agent/smart_model_routing.py (AGPL-3.0; list re-derived from scratch)`.
- **R0 fixture runner is a cargo test, not a theo-benchmark binary** — adaptation documented in `.theo/evolution_research.md` §3.
- **No new workspace crate.** Routing lives in `theo-domain` + `theo-infra-llm` + `theo-agent-runtime`; no new member in root `Cargo.toml`.
- **Model IDs use tier aliases**, not hard-coded vendor names in `rules.rs`. The `PricingTable` resolves aliases to concrete IDs via config.

## Convergence gate (Phase 5 final check)

Before emitting `<promise>TODAS TASKS, E DODS CONCLUIDOS E VALIDADOS</promise>`:

- [ ] 6 `evolution:` commits land (one per R0-R5 phase).
- [ ] 40 AC tests exist and pass.
- [ ] `cargo test --workspace` green.
- [ ] `cargo check --workspace --tests` 0 warnings.
- [ ] Harness score ≥ 75.150.
- [ ] Every commit message free of `Co-Authored-By:`.
- [ ] `outputs/smart-model-routing-plan.md §0` success metrics snapshotted in final `.theo/evolution_assessment.md`.

Any unchecked item → return to IMPLEMENT.
