---
name: show-domain
description: X-ray diagnostic of workspace domains. Shows test coverage, complexity, lints, dead code, sizes, and health score per domain module. Use to get a quick health snapshot of any crate or the full workspace.
user-invocable: true
context: fork
argument-hint: "[crate-name|all]"
---

Domain-level X-ray diagnostic for the Theo Code workspace. Produces a structured health report per domain module within a crate, covering 6 dimensions: test coverage, cyclomatic complexity, lint warnings, dead code, module size, and an aggregate health score.

## Arguments

| Argument | Behavior |
|---|---|
| `{crate-name}` | X-ray a single crate (e.g., `theo-domain`, `theo-agent-runtime`) |
| `all` | X-ray every crate in the workspace |
| No args | X-ray every crate in the workspace |

## Process

### Step 1 — Discover Domains

For each target crate, enumerate domain modules. A "domain" is a top-level module directory or file under `crates/{crate}/src/`:

```!
# List domain modules (directories = multi-file modules, .rs files = single-file modules)
ls -1 crates/{crate}/src/ | grep -v 'lib.rs\|mod.rs\|main.rs' | sed 's/\.rs$//' | sort
```

Cross-reference with `docs/reviews/{crate}/REVIEW.md` domain table if it exists (create `docs/reviews/{crate}/` dir if needed).

### Step 2 — Collect Metrics Per Domain

For EACH domain module, collect these 6 dimensions:

#### 2.1 Test Coverage

Count test functions and estimate coverage:

```!
# Count tests in the module (inline #[test] + tests/ files)
grep -rc '#\[test\]' crates/{crate}/src/{domain}/ 2>/dev/null || echo 0
grep -rc '#\[test\]' crates/{crate}/tests/ 2>/dev/null | grep -i {domain} || echo 0
```

If `cargo-tarpaulin` is available and the user requests deep mode:

```!
cargo tarpaulin -p {crate} --out Json 2>/dev/null | python3 -c "
import json, sys
data = json.load(sys.stdin)
# Filter to domain files and compute line coverage
"
```

Otherwise, use heuristic: `test_count / public_fn_count` as proxy.

**Scoring:**
- 5: >= 90% coverage or test/fn ratio >= 0.8
- 4: >= 75% or ratio >= 0.6
- 3: >= 60% or ratio >= 0.4
- 2: >= 40% or ratio >= 0.2
- 1: < 40% or ratio < 0.2

#### 2.2 Cyclomatic Complexity

Count decision points per function (if, match arms, for, while, ?, &&, ||):

```!
# Find functions with high complexity indicators
grep -n 'fn \|if \|match \|for \|while \|\.unwrap_or\|&&\|||' crates/{crate}/src/{domain}*.rs crates/{crate}/src/{domain}/**/*.rs 2>/dev/null
```

Use clippy cognitive complexity lint as primary source:

```!
cargo clippy -p {crate} -- -W clippy::cognitive_complexity 2>&1 | grep {domain}
```

**Scoring:**
- 5: All functions CCN <= 5
- 4: All functions CCN <= 10
- 3: Max CCN <= 15
- 2: Max CCN <= 20
- 1: Any function CCN > 20

#### 2.3 Lint Warnings

```!
cargo clippy -p {crate} --lib --tests 2>&1 | grep -c 'warning\[' | head -1
```

Filter warnings to the specific domain module files.

**Scoring:**
- 5: Zero warnings
- 4: 1-2 warnings
- 3: 3-5 warnings
- 2: 6-10 warnings
- 1: > 10 warnings

#### 2.4 Dead Code

Check for unused items in the domain:

```!
# Compiler dead_code warnings
RUSTFLAGS="-W dead_code" cargo check -p {crate} 2>&1 | grep {domain}

# Unused imports
cargo clippy -p {crate} -- -W unused_imports 2>&1 | grep {domain}

# Functions that are pub but never called from outside the module
grep -n 'pub fn\|pub async fn' crates/{crate}/src/{domain}*.rs crates/{crate}/src/{domain}/**/*.rs 2>/dev/null
```

Cross-reference pub functions with grep across the workspace to find truly unused ones.

**Scoring:**
- 5: Zero dead code
- 4: 1-2 unused items
- 3: 3-5 unused items
- 2: 6-10 unused items
- 1: > 10 unused items or entire unused modules

#### 2.5 Module Size

```!
# Total LOC (excluding blank lines and comments)
find crates/{crate}/src/{domain}* -name '*.rs' 2>/dev/null | xargs wc -l | tail -1
```

Count files, total LOC, max single-file LOC, max function LOC.

**Scoring:**
- 5: Total <= 200 LOC, max file <= 150, max fn <= 20
- 4: Total <= 400 LOC, max file <= 300, max fn <= 30
- 3: Total <= 600 LOC, max file <= 500, max fn <= 40
- 2: Total <= 1000 LOC, max file <= 700, max fn <= 60
- 1: Total > 1000 LOC or max file > 700 or max fn > 60

#### 2.6 Error Handling Quality

Check for error handling anti-patterns:

```!
# unwrap/expect in production code (not tests)
grep -rn '\.unwrap()\|\.expect(' crates/{crate}/src/{domain}*.rs crates/{crate}/src/{domain}/**/*.rs 2>/dev/null | grep -v '#\[test\]\|#\[cfg(test)\]'

# Empty error handling
grep -rn '=> {}\|=> ()\|_ => {}' crates/{crate}/src/{domain}*.rs crates/{crate}/src/{domain}/**/*.rs 2>/dev/null
```

**Scoring:**
- 5: Zero unwraps in prod, all errors typed with thiserror
- 4: 1-2 unwraps with justification comments
- 3: 3-5 unwraps or some string-based errors
- 2: 6-10 unwraps or error swallowing
- 1: > 10 unwraps or catch-all error handling

### Step 3 — Compute Health Score

**Health Score = weighted average of 6 dimensions:**

| Dimension | Weight | Rationale |
|---|---|---|
| Test Coverage | 25% | Tests are the foundation |
| Complexity | 20% | High complexity = high bug risk |
| Lints | 15% | Compiler knows best |
| Dead Code | 10% | Cleanliness signal |
| Module Size | 15% | SRP indicator |
| Error Handling | 15% | Production safety |

**Health Score = (coverage * 0.25 + complexity * 0.20 + lints * 0.15 + dead_code * 0.10 + size * 0.15 + errors * 0.15) / 5.0 * 100**

**Health Tier:**
- 90-100%: EXCELLENT (green)
- 75-89%: GOOD (blue)
- 60-74%: FAIR (yellow)
- 40-59%: POOR (orange)
- 0-39%: CRITICAL (red)

### Step 4 — Output

Print the report directly to the conversation AND save to `docs/reviews/{crate}/DOMAIN_XRAY.md` (create dirs if needed with `mkdir -p docs/reviews/{crate}`).

## Output Template

```markdown
# {crate-name} — Domain X-Ray

**Date:** {date}
**Crate:** {crate-name}
**Total domains:** {count}
**Overall health:** {score}% ({tier})

## Summary

| # | Domain | LOC | Tests | Coverage | Complexity | Lints | Dead Code | Size | Errors | Health | Tier |
|---|--------|-----|-------|----------|------------|-------|-----------|------|--------|--------|------|
| 1 | `module_a` | 120 | 8 | 4/5 | 5/5 | 5/5 | 5/5 | 5/5 | 5/5 | 95% | EXCELLENT |
| 2 | `module_b` | 450 | 3 | 2/5 | 3/5 | 4/5 | 3/5 | 3/5 | 4/5 | 62% | FAIR |
| ... | | | | | | | | | | | |

## Hot Spots (Health < 60%)

Domains that need immediate attention, ordered by health score ascending:

### `module_b` — 62% FAIR

| Dimension | Score | Detail |
|---|---|---|
| Coverage | 2/5 | 3 tests for 15 public functions (ratio 0.2) |
| Complexity | 3/5 | `process_data()` at line 87 has CCN ~18 |
| Lints | 4/5 | 1 warning: unused variable `tmp` at line 102 |
| Dead Code | 3/5 | `old_helper()` at line 45 not called anywhere |
| Size | 3/5 | 450 LOC total, `process_data()` is 55 LOC |
| Errors | 4/5 | 2 unwraps at lines 91, 134 (both in fallible paths) |

**Recommended actions:**
1. Add tests for `validate()`, `transform()`, `persist()` (coverage 2→4)
2. Split `process_data()` into 3 functions (complexity 3→5, size 3→4)
3. Replace unwraps with `?` operator (errors 4→5)

## Domain Details

### 1. `module_a` (120 LOC) — 95% EXCELLENT

**Files:** `src/module_a.rs`
**Public API:** 5 functions, 2 structs, 1 enum
**Tests:** 8 (`#[test]` inline) + 0 (integration)
**Derives:** `Debug, Clone, Serialize, Deserialize` consistent

| Dimension | Score | Detail |
|---|---|---|
| Coverage | 4/5 | 8 tests / 5 pub fns = 1.6 ratio |
| Complexity | 5/5 | Max CCN: 3 (simple branching) |
| Lints | 5/5 | Zero warnings |
| Dead Code | 5/5 | All pub items referenced externally |
| Size | 5/5 | 120 LOC, max fn 15 LOC |
| Errors | 5/5 | Zero unwraps, all Result<T, E> |

### 2. `module_b` (450 LOC) — 62% FAIR

(... detailed breakdown as in Hot Spots ...)

---

## Crate Totals

| Metric | Value |
|---|---|
| Total LOC | {sum} |
| Total test functions | {sum} |
| Domains at EXCELLENT | {count} ({pct}%) |
| Domains at GOOD | {count} ({pct}%) |
| Domains at FAIR | {count} ({pct}%) |
| Domains at POOR | {count} ({pct}%) |
| Domains at CRITICAL | {count} ({pct}%) |
| Top complexity function | `{fn_name}` in `{domain}` (CCN ~{n}) |
| Largest domain | `{domain}` ({n} LOC) |
| Most untested domain | `{domain}` ({n} tests / {n} pub fns) |
| Total dead code items | {count} |
| Total lint warnings | {count} |
| Total unwraps in prod | {count} |
```

## Multi-Crate Output (when `all`)

When scanning all crates, produce a workspace-level summary FIRST, then individual crate reports.

```markdown
# Theo Code — Workspace Domain X-Ray

**Date:** {date}
**Crates:** {count}
**Total domains:** {count}
**Overall health:** {score}% ({tier})

## Crate Health Ranking

| # | Crate | Domains | LOC | Health | Tier | Top Issue |
|---|-------|---------|-----|--------|------|-----------|
| 1 | theo-domain | 37 | 3200 | 92% | EXCELLENT | — |
| 2 | theo-engine-graph | 12 | 2100 | 78% | GOOD | coverage in `graph_builder` |
| 3 | theo-agent-runtime | 57 | 18000 | 65% | FAIR | complexity in `run_engine` |
| ... | | | | | | |

## Workspace Hot Spots (Bottom 5 domains across all crates)

| Domain | Crate | Health | Top Issue |
|--------|-------|--------|-----------|
| `run_engine` | theo-agent-runtime | 42% | CCN > 25, 800 LOC |
| ... | | | |
```

Save workspace summary to `docs/reviews/WORKSPACE_XRAY.md`.

## Notes

- This skill is READ-ONLY. It diagnoses but never fixes.
- Heuristic scoring is used when cargo-tarpaulin is unavailable. Scores are approximate.
- For precise coverage numbers, run `/code-audit coverage` instead.
- For precise complexity numbers, run `/code-audit complexity` instead.
- This skill complements `/review` (which judges quality) and `/code-audit` (which runs specific tools). `/show-domain` gives the bird's-eye-view across all dimensions simultaneously.
