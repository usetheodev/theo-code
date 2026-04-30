---
paths:
  - "crates/**/*.rs"
  - "apps/**/*.rs"
  - "apps/theo-ui/**/*.{ts,tsx}"
---

# Testing Rules

# Core Rule

- Every non-trivial logic change needs a test or an explicit reason why a meaningful automated test is not feasible.
- Bug fixes should add a regression test before or alongside the fix; write the failing test first when practical.
- Keep the repo's workflow grounded in narrow, fast feedback, not blanket workspace runs by default.

## What Kind of Test
- Business logic: unit test required.
- Parser/graph/retrieval changes: add coverage near the affected engine crate.
- Runtime/tooling changes: test the behavior at the narrowest boundary that proves the invariant.
- Integration boundary (DB, API, LLM): integration test.

## Test Quality
- One behavior per test. If the name has "and", split it.
- Descriptive names: `test_rrf_fusion_ranks_exact_match_first`, not `test_fusion_1`.
- Arrange-Act-Assert pattern. No exceptions.
- Tests must be deterministic. Flaky test = P0 bug.
- Tests must be independent. No shared mutable state between tests.

## Research-Aligned Focus Areas
- Retrieval pipeline: query → ranked/contextualized results
- Code graph and parsing: parse → symbols → edges → clustering/queries
- Agent runtime: transitions, guard conditions, compaction, subagent/delegation behavior
- Tool execution: input validation, permission/sandbox boundaries, truncation/error surfaces
- Memory and wiki flows: persistence, ingestion, query, lint/enrichment boundaries
- Observability and routing: emitted events, metrics, cost/routing decisions when touched

## What NOT to Test
- Trivial getters/setters
- Framework-generated code
- Third-party library internals
- CSS layout (unless business requirement)

## Running Tests
```bash
cargo test -p <crate>
cargo test -p <crate> <test_name>
cargo clippy -p <crate> --all-targets -- -D warnings
cd apps/theo-ui && npm test
```

## Completion

- Before finishing, run the narrowest relevant tests plus the affected crate's clippy target.
- Run `make check-arch` whenever a change can affect crate boundaries.
