---
name: test
description: Run tests for the workspace or specific crate. Use when asked to run tests or validate changes.
user-invocable: true
allowed-tools: Bash(cargo *) Bash(npm *)
argument-hint: "[crate|changed|all]"
---

Run tests for Theo Code.

## Arguments

- No args or `all`: `cargo test` (full workspace)
- `changed`: detect changed crates via `git diff`, test only those
- Crate name: `cargo test -p $ARGUMENTS`
- Test name: `cargo test $ARGUMENTS`
- `ui`: `cd apps/theo-ui && npm test`

## Steps

1. Determine which tests to run based on arguments
2. Run tests, capture output
3. If failures: analyze root cause, show failing test name + error
4. Report: X passed, Y failed, Z skipped
5. For failures: suggest whether it's a code bug or test issue

## For `changed` mode

```bash
git diff --name-only HEAD | grep "^crates/" | sed 's|crates/\([^/]*\)/.*|\1|' | sort -u
```

Then run `cargo test -p <crate>` for each changed crate.

## TDD Compliance Report

After running tests, also report:

1. **New code without tests** — check `git diff` for new functions/methods without corresponding test additions
2. **Test-to-code ratio** — for each changed crate, report lines of test vs lines of code
3. **RED-GREEN evidence** — if test was committed separately before implementation (check git log)

Format:
```
TDD COMPLIANCE:
  ✓ All new functions have tests
  ✗ crate::module::new_function — NO TEST FOUND
  Ratio: 0.45 (test lines / code lines)
```

**A passing test suite with untested new code is NOT a pass.** Flag it.
