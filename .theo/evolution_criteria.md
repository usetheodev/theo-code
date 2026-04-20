# SOTA Criteria — Tool Calling 2.0 Adoption

**Version:** 3.0 (Tool Calling 2.0 cycle)
**Date:** 2026-04-20
**Baseline:** 588d0b6 score 72.3

## Rubric (each dimension 0-3, CONVERGED at avg >= 2.5)

### 1. Pattern Fidelity
- **3** — input_examples, dynamic filtering, batch_execute all land with Anthropic-traceable semantics
- **2** — 2 of 3 land faithfully
- **1** — 1 lands
- **0** — none

### 2. Architectural Fit
- **3** — new surface in `theo-domain`; `theo-tooling` and `theo-agent-runtime` consume via trait; no new external deps; no unwrap in production code
- **2** — one boundary friction but no violation
- **1** — cross-crate duplication to avoid import
- **0** — violates `theo-domain → nothing` or adds unwrap

### 3. Completeness
- **3** — 3+ tools carry input_examples; webfetch emits filtered output; batch_execute has dispatch + binding semantics + integration test
- **2** — 2 of the 3 above are fully wired
- **1** — trait / schema surface changes but no tool adopts
- **0** — scaffolding only

### 4. Testability
- **3** — per-feature unit tests (schema serialization, HTML reducer pure function, batch variable binding), plus integration tests on the agent-runtime dispatch path
- **2** — unit only
- **1** — smoke only
- **0** — none

### 5. Simplicity
- **3** — each change <= 200 LOC, no premature abstractions, no new deps, no new workspace members
- **2** — one change crosses 200 LOC but justified
- **1** — speculative abstractions
- **0** — refactor sprawl

## Guardrails

- Hygiene floor: `cargo check --workspace --tests` stays at 0 warnings, 0 errors (baseline was already fixed)
- Harness score must stay >= 72.3 (baseline)
- Pre-commit hook passes WITHOUT `--no-verify` on every commit
- Each change <= 200 LOC; decompose larger changes
- TDD: every behavior change starts with a failing test
- `theo-domain` stays dependency-free
- No new external crates; no new workspace members

## Done Definition

- `cargo test --workspace` still passes (2556+ tests)
- `cargo check --workspace --tests` 0 warnings
- At least 3 tools declare a non-empty `input_examples`
- Webfetch reducer strips `<script>`, `<style>`, and at least nav/header/footer on a representative HTML fixture
- `batch_execute` meta-tool: schema present in `registry_to_definitions`, dispatch returns per-step results, integration test covers a 2-step chain with variable binding
- SOTA rubric average >= 2.5
