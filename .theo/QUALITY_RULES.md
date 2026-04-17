# Quality Rules

Mechanical rules enforced by structural tests and the evaluation harness.
Each rule is verifiable, not subjective. When a rule is violated, the action is clear.

## R1: No `unwrap()` in Production Code

**Rule**: Production code (`crates/*/src/`, `apps/*/src/`) must minimize `.unwrap()` calls.
**Verify**: `grep -r '\.unwrap()' crates/*/src/ apps/*/src/ --include="*.rs" | grep -v test | wc -l`
**Target**: ≤300 (current: 1308)
**Fix**: Use `?` operator, `match`, `.unwrap_or()`, `.unwrap_or_default()`, or `.expect("reason")`.
**Exception**: Test code and one-time initialization where panic is correct behavior.

## R2: No `#[allow(dead_code)]`

**Rule**: Do not suppress dead code warnings. Either use the code or delete it.
**Verify**: `grep -r '#\[allow(dead_code)\]' crates/*/src/ apps/*/src/ --include="*.rs" | wc -l`
**Target**: 0 (current: 12)
**Fix**: Remove the attribute. If the code is used, make the usage visible. If not, delete it.

## R3: Clippy Clean

**Rule**: `cargo clippy --workspace --exclude theo-code-desktop` must produce 0 warnings.
**Verify**: Run clippy, count warnings.
**Target**: 0 warnings (current: 551)
**Fix**: Address each clippy suggestion. Do NOT add `#[allow(clippy::...)]` to suppress.
**Exception**: Only if clippy is demonstrably wrong (rare). Document with a comment.

## R4: Zero Cargo Warnings

**Rule**: `cargo test --workspace --exclude theo-code-desktop --no-run` must produce 0 warnings.
**Verify**: Count `^warning:` lines in build output (minus summary lines).
**Target**: 0 (current: 59)
**Fix**: Remove unused imports, unused variables, unused mut, dead code.

## R5: All Crates Compile

**Rule**: Every crate in the workspace must compile successfully (including test targets).
**Verify**: `cargo test -p <crate> --no-run` for each crate.
**Target**: 13/13

## R6: All Tests Pass

**Rule**: Zero test failures across the workspace.
**Verify**: `cargo test --workspace --exclude theo-code-desktop`
**Target**: 0 failures
**Fix**: Fix the test or fix the code. Never delete a failing test.

## R7: Boundary Tests Pass

**Rule**: Architectural boundary tests in `boundary_test.rs` must all pass.
**Verify**: `cargo test -p theo-governance --test boundary_test`
**Enforces**:
- `theo-domain` has no internal dependencies
- Apps don't import engine crates directly
- Apps only use allowed internal dependencies

## R8: Structural Hygiene Tests Pass

**Rule**: Code quality tests in `structural_hygiene.rs` must all pass.
**Verify**: `cargo test -p theo-governance --test structural_hygiene`
**Enforces**:
- No `println!` in library code
- No source files exceeding 1500 lines
- No `std::process::exit` outside main
- Doc comments on public types

## R9: Documentation Artifacts Present

**Rule**: The following files must exist and be >500 bytes:
1. `clippy.toml` — Clippy configuration
2. `.theo/AGENTS.md` — Agent navigation map
3. `.theo/QUALITY_RULES.md` — This file
4. `.theo/QUALITY_SCORE.md` — Per-crate health dashboard
5. `crates/theo-governance/tests/structural_hygiene.rs` — With 10+ `#[test]` functions

## R10: Leaf-First Changes

**Rule**: When modifying code, prefer leaf crates to minimize rebuild cascading.
**Order**: theo-domain → governance/api-contracts → parser → graph → retrieval → tooling/llm/auth → runtime → application → cli
**Reason**: Changes to `theo-domain` rebuild the entire workspace (~3 min).
