# Quality Score Dashboard

Per-crate health metrics. Updated after each autoloop phase completion.

**Last updated**: 2026-04-17 (baseline)
**Overall score**: 53.830 (L1=94.1, L2=13.6)

## Crate Health Matrix

| Crate | Compiles | Tests | Unwraps | Grade |
|---|---|---|---|---|
| theo-domain | Yes | 251 | 112 | B |
| theo-engine-parser | Yes | 468 | 176 | B |
| theo-agent-runtime | Yes | 359 | 296 | C |
| theo-engine-retrieval | Yes | 274 | 141 | B |
| theo-infra-llm | Yes | 156 | 88 | B |
| theo-tooling | Yes | 144 | 257 | C |
| theo-engine-graph | Yes | 103 | 36 | B |
| theo-infra-auth | Yes | 87 | 78 | B |
| theo-application | Yes | 70 | 96 | C |
| theo-governance | Yes | 64 | 6 | A |
| theo-api-contracts | Yes | 0 | 0 | D |
| theo-cli | Yes | — | — | C |
| theo-marklive | Yes | 4 | 15 | C |

### Grades
- **A**: Compiles, 50+ tests, <10 unwraps
- **B**: Compiles, 50+ tests, <200 unwraps
- **C**: Compiles, <50 tests OR >200 unwraps
- **D**: Compiles but 0 tests or compile errors in test target

## Workspace Metrics

| Metric | Value | Target | Status |
|---|---|---|---|
| Compiling crates | 13/13 | 13/13 | OK |
| Tests passed | 2561 | 2561+ | OK |
| Tests failed | 0 | 0 | OK |
| Cargo warnings | 59 | 0 | Needs work |
| Clippy warnings | 551 | 0 | Needs work |
| Unwrap count | 1308 | ≤300 | Needs work |
| Structural tests | 0 | 30 | Missing |
| Boundary tests | 5 | 15 | Needs work |
| Doc artifacts | 0/5 | 5/5 | Missing |
| Dead code attrs | 12 | 0 | Needs work |

## Score Breakdown

```
Layer 1 (94.1/100):
  Compile:  40.0 / 40  (13/13 crates)
  Tests:    40.0 / 40  (2561/2561 passed)
  Count:     10.0 / 10  (2572 tests, cap 2500)
  Warnings:  4.1 / 10  (59 warnings, penalty)

Layer 2 (13.6/100):
  Clippy:    14.9 / 20  (551 warnings)
  Unwrap:     2.6 / 20  (1308 unwraps)
  Structural: 0.0 / 15  (0 tests)
  Docs:       0.0 / 15  (0/5 artifacts)
  Dead code:  6.0 / 15  (12 attrs)
  Boundary:   5.0 / 15  (5 tests)
```

## Priority Actions

1. **Fix cargo warnings** → +5.9 pts L1
2. **Create 5 doc artifacts** → +15.0 pts L2
3. **Add structural tests** → up to +15.0 pts L2
4. **Expand boundary tests** → +10.0 pts L2
5. **Fix clippy warnings** → up to +5.1 pts L2
6. **Remove unwraps** → up to +17.4 pts L2
7. **Remove dead_code attrs** → +3.0 pts L2
