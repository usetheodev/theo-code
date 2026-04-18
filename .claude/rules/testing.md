---
paths:
  - "crates/**/*.rs"
  - "apps/**/*.rs"
  - "apps/theo-ui/**/*.{ts,tsx}"
---

# Testing Rules

## Every logic change needs a test
- Business logic: unit test required.
- Bug fix: regression test BEFORE the fix (write the failing test first).
- Integration boundary (DB, API, LLM): integration test.

## Test Quality
- One behavior per test. If the name has "and", split it.
- Descriptive names: `test_rrf_fusion_ranks_exact_match_first`, not `test_fusion_1`.
- Arrange-Act-Assert pattern. No exceptions.
- Tests must be deterministic. Flaky test = P0 bug.
- Tests must be independent. No shared mutable state between tests.

## What to Test
- Retrieval pipeline: query → ranked results (ground truth validated)
- Code graph: parse → symbols → edges → queries
- Agent state machine: transitions, guard conditions, error states
- Tool execution: input validation, sandbox boundaries
- Wiki generation: code → markdown pages → links

## What NOT to Test
- Trivial getters/setters
- Framework-generated code
- Third-party library internals
- CSS layout (unless business requirement)

## Running Tests
```bash
cargo test                        # All workspace tests
cargo test -p theo-engine-graph   # Specific crate
cd apps/theo-ui && npm test       # Frontend tests
```
