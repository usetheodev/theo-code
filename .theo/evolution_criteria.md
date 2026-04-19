# SOTA Criteria — Tool Design Revision (Anthropic Best Practices)

**Version:** 1.0 (tool-design revision)
**Date:** 2026-04-19
**Prompt:** Revisar tools baseados em Anthropic best practices
**Based on:** https://www.anthropic.com/engineering/writing-tools-for-agents + opendev-tools-core + fff-mcp

## Rubric (each dimension 0-3, CONVERGED at avg >= 2.5)

### 1. Pattern Fidelity (opendev-tools-core + fff-mcp)
- **3** — `llm_suffix`, `truncation_rule`, `format_validation_error`, `should_defer` all land with opendev-traceable semantics; citations in code comments
- **2** — 3/4 patterns land with fidelity
- **1** — 1-2 patterns land, partial fidelity
- **0** — patterns invoked by name only, no mechanism

### 2. Architectural Fit (theo-domain boundaries)
- **3** — new types live in `theo-domain`; `theo-tooling` and `theo-agent-runtime` consume them; zero circular deps; no unwrap; thiserror-typed errors
- **2** — one boundary friction (e.g., shared helper in tooling instead of domain) but no violation
- **1** — cross-crate type duplicated to avoid import
- **0** — violates `theo-domain -> nothing` or adds unwrap

### 3. Completeness
- **3** — 5+ tools updated end-to-end (llm_suffix wired, truncation enforced, rich descriptions, validation overrides); ToolOutput serializes the new field
- **2** — 3-4 tools fully wired
- **1** — trait surface changes but zero tool adopts the new APIs
- **0** — scaffolding only

### 4. Testability
- **3** — unit tests for each new trait method (default + override), integration test showing llm_suffix reaches agent-runtime serializer, truncation-boundary property test
- **2** — unit tests present, missing integration
- **1** — smoke test only
- **0** — no tests or tests only cover the defaults

### 5. Simplicity
- **3** — each change <= 200 LOC, no premature abstractions, descriptions < 800 chars, no over-parameterization
- **2** — one change crosses 200 LOC but justified
- **1** — abstraction for hypothetical future tools
- **0** — refactor sprawl, helper modules that hide behavior

## Guardrails

- Hygiene floor: `theo-evaluate.sh` score must not regress
- Each change <= 200 LOC; decompose if larger
- `theo-domain` stays dependency-free
- No new workspace members, no new external crates
- TDD enforced: every behavior change starts with a failing test
- Tool descriptions limited to 800 chars (token budget)
- Truncation defaults: bash=8000 tail, read=15000 head, grep=4000 tail, webfetch=10000 head

## Done Definition

- `cargo test -p theo-domain -p theo-tooling -p theo-agent-runtime` passes
- `cargo clippy --workspace -- -D warnings` passes
- At least 5 tools use `llm_suffix` in their error paths
- At least 3 tools declare a non-None `truncation_rule`
- Top 5 tools (read, grep, glob, bash, edit) have decision-tree descriptions with NOT-usage rules
- SOTA rubric average >= 2.5

## Mapping to 12 Anthropic Principles

| Principle | Addressed by | Cycle |
|---|---|---|
| 1 Strategic selection | P3 (descriptions) | cycle 1 |
| 2 Consolidation | — (kept out; no tool merges in this pass) | — |
| 3 Distinct purposes | P3 (NOT-usage rules) | cycle 1 |
| 4 Namespacing | — (git_*, task_*, http_* already prefixed) | — |
| 5 Unambiguous params | P4 (format_validation_error) | cycle 2 |
| 6 Response format control | — (deferred; revisit post-convergence) | — |
| 7 Semantic identifiers | — (tools return human-readable paths already) | — |
| 8 Actionable errors | P1 (llm_suffix) + P4 | cycles 1-2 |
| 9 Pagination defaults | — (read has offset/limit; keep) | — |
| 10 Truncate with guidance | P1 suffix + P2 rule | cycle 1-2 |
| 11 Onboarding descriptions | P3 | cycle 1 |
| 12 Minimize context | P2 truncation + P5 should_defer | cycles 2-3 |

Principles 2, 4, 6, 7, 9 are considered already satisfied or explicitly out-of-scope for this pass.
