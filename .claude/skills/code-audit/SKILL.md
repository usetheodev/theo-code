---
name: code-audit
description: Run code-audit techniques against Theo Code — cyclomatic complexity, coverage + mutation, module size, dependencies, SCA, unit tests, integration tests, pentest (SAST). Pass a technique name or `all`.
user-invocable: true
argument-hint: "[complexity|size|arch|unwrap|panic|unsafe|secrets|io-tests|changelog|sca|all]"
---

Run static code-audit techniques on the Theo Code workspace. Each technique maps to an existing `make check-*` gate script in `scripts/`.

## Arguments

| Arg          | Make target              | Technique                                      |
|--------------|--------------------------|------------------------------------------------|
| `arch`       | `make check-arch`        | T1.5 — Crate dependency direction              |
| `unwrap`     | `make check-unwrap`      | T2.5 — Production `.unwrap()`/`.expect()`      |
| `panic`      | `make check-panic`       | T2.6 — Production `panic!/todo!/unimplemented!`|
| `unsafe`     | `make check-unsafe`      | T2.9 — `unsafe` blocks without `// SAFETY:`    |
| `size`       | `make check-sizes`       | T4.6 — File LOC limits (800 Rust, 400 TS)      |
| `complexity` | `make check-complexity`  | Function LOC ceiling (clippy::too_many_lines)   |
| `io-tests`   | `make check-io-tests`    | T5.2 — Misclassified I/O tests in `src/`       |
| `secrets`    | `make check-secrets`     | T6.2 — Secret scan (grep fallback)             |
| `changelog`  | `make check-changelog`   | T6.5 — CHANGELOG.md updated                    |
| `sca`        | cargo-audit + cargo-deny | CVEs, licenses, outdated packages              |
| `lint`       | `make lint`              | Clippy with warnings-as-errors                 |
| `all`        | all gates in sequence    | Full audit                                     |
| *(no arg)*   | same as `all`            | Full audit                                     |

## How to run

1. Parse `$ARGUMENTS`. Default to `all` if empty.
2. Validate against the table above. If invalid, list valid options and stop.
3. For single technique: run the corresponding `make` target or command.
4. For `all`: run each gate in sequence. Collect all output.
5. After all gates finish, produce a consolidated report.

## Execution for `all`

Run in this order:

```bash
# 1. Build check
cargo build --workspace --exclude theo-code-desktop 2>&1 | tail -5

# 2. Lint
cargo clippy --workspace --all-targets --no-deps -- -D warnings 2>&1 | tail -10

# 3. Architecture
bash scripts/check-arch-contract.sh --report

# 4. Unwrap/expect
bash scripts/check-unwrap.sh --report

# 5. Panic/todo/unimplemented
bash scripts/check-panic.sh --report

# 6. Unsafe SAFETY comments
bash scripts/check-unsafe.sh --report

# 7. File sizes
bash scripts/check-sizes.sh --report

# 8. Function complexity
bash scripts/check-complexity.sh --report

# 9. Inline I/O tests
bash scripts/check-inline-io-tests.sh --report

# 10. Secrets
bash scripts/check-secrets.sh --report

# 11. Changelog
bash scripts/check-changelog.sh

# 12. SCA (if tools available)
command -v cargo-audit >/dev/null && cargo audit || echo "cargo-audit not installed"
command -v cargo-deny >/dev/null && cargo deny check || echo "cargo-deny not installed"
```

## Consolidated Report

After running all gates, produce:

```
THEO CODE — CODE AUDIT REPORT
=============================
Date:   <YYYY-MM-DD HH:MM>
Commit: <git rev-parse --short HEAD>

TECHNIQUE              VERDICT   DETAILS
---------              -------   -------
Build                  PASS|FAIL
Lint (clippy)          PASS|FAIL  N warnings
Architecture (T1.5)   PASS|FAIL  N violations in M crates
Unwrap/expect (T2.5)  PASS|FAIL  N violations, M allowlisted
Panic/todo (T2.6)     PASS|FAIL  N violations, M allowlisted
Unsafe SAFETY (T2.9)  PASS|FAIL  N missing comments
File sizes (T4.6)     PASS|FAIL  N over limit, M allowlisted
Complexity             PASS|FAIL  N crates over ceiling
I/O tests (T5.2)      PASS|FAIL  N misclassified
Secrets (T6.2)        PASS|FAIL  N potential secrets
Changelog (T6.5)      PASS|FAIL
SCA                   PASS|FAIL|SKIP  N vulnerabilities

OVERALL: PASS | FAIL
(FAIL if ANY technique is FAIL)
```

## Rules

- Read-only. This skill never edits files. If fixes are needed, tell the user what to fix.
- If a required tool (cargo-audit, cargo-deny) is missing, report SKIP with install instructions.
- For `all`, expect 2-5 minutes depending on workspace state.
- If the user passes an unknown argument, show the table above and stop.
