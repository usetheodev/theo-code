---
name: review
description: Review code changes or crates for quality, architecture, and compliance. Saves structured reports to docs/reviews/. Use before commits, PRs, or for deep crate audits.
user-invocable: true
context: fork
agent: code-reviewer
argument-hint: "[staged|branch|file|crate-name|deep crate-name]"
---

Review code in the Theo Code workspace. Two modes: **diff review** (pre-commit/PR) and **deep review** (full crate audit).

## Mode Selection

| Argument | Mode | What it does |
|---|---|---|
| `staged` or no args | Diff review | Reviews `git diff --cached` |
| `branch` | Diff review | Reviews all commits on current branch vs main |
| `file path/to/file.rs` | Diff review | Reviews specific file changes |
| `deep {crate-name}` | Deep review | Full crate audit with domain-by-domain analysis |
| `{crate-name}` | Deep review | Same as `deep {crate-name}` when crate name matches workspace crate |

## Phase 1 — Gather Context

Collect all information BEFORE making any judgement.

For **diff review**:

```!
git diff --cached --stat
git diff --cached
git log --oneline -5
```

For **branch review**:

```!
git log --oneline main..HEAD
git diff main...HEAD --stat
git diff main...HEAD
```

For **deep review**:

```!
cargo test -p {crate} 2>&1 | tail -20
cargo clippy -p {crate} --lib --tests 2>&1
```

Then read every source file in the crate (`crates/{crate}/src/**/*.rs`), every test file, and the crate's `Cargo.toml`.

## Phase 2 — Compliance Checks (Automated)

Run these checks and record PASS/FAIL for each. These are NON-NEGOTIABLE.

### 2.1 TDD Compliance
- Every new/changed function has a corresponding test
- Tests use Arrange-Act-Assert pattern
- No code change without test change = automatic FLAG
- **Code without tests = REJECT. No exceptions.**

### 2.2 Architecture Boundary Compliance
Verify against the dependency rules in CLAUDE.md:
```
theo-domain         → (nothing)
theo-engine-*       → theo-domain
theo-governance     → theo-domain
theo-infra-*        → theo-domain
theo-tooling        → theo-domain
theo-agent-runtime  → theo-domain, theo-governance
theo-api-contracts  → theo-domain
theo-application    → all crates above
apps/*              → theo-application, theo-api-contracts
```
- Check `Cargo.toml` for illegal `path =` or workspace deps
- Flag any circular dependency
- Flag any app importing engine/infra crates directly

### 2.3 Error Handling
- No `unwrap()` in production paths (tests are OK)
- No `catch-all` error swallowing (`_ => {}`, `catch (Exception e) {}`)
- Errors are typed with `thiserror`, not string-based
- Every error path has context (file, line, operation that failed)

### 2.4 Clippy & Warnings
```!
cargo clippy -p {crate} --lib --tests -- -D warnings 2>&1
```
- Zero clippy warnings = PASS
- Any warning = FAIL with details

### 2.5 Code Quality Metrics
- No god-files (> 500 LOC per file)
- No god-functions (> 30 LOC per function)
- Cyclomatic complexity <= 10 per function
- No `#[allow(unused)]` or `#[allow(dead_code)]` without comment explaining why

### 2.6 Security
- No hardcoded secrets, tokens, or credentials
- No `unsafe` blocks without `// SAFETY:` comment
- Sandbox is mandatory for bash tool execution
- Input validation at system boundaries

## Phase 3 — Human Review Dimensions

Evaluate each dimension on a 1-5 scale with evidence. These require judgement.

| Dimension | What to evaluate |
|---|---|
| **Correctness** | Does the code do what it claims? Edge cases handled? |
| **Design** | SRP respected? Abstractions justified? KISS/YAGNI applied? |
| **Readability** | Clear names? Self-documenting? Comments only where non-obvious? |
| **Testability** | Easy to test in isolation? DIP at boundaries? |
| **Performance** | O(n) where expected? No unnecessary allocations in hot paths? |
| **Consistency** | Follows existing patterns in the codebase? |

## Phase 4 — Findings Classification

Every finding MUST be classified:

| Severity | Meaning | Action |
|---|---|---|
| **BLOCKER** | Bug, security vuln, data loss risk, test missing for business logic | Must fix before merge |
| **CRITICAL** | Architecture violation, error swallowing, god-file, no error context | Must fix before merge |
| **WARNING** | Code smell, minor DRY violation, naming inconsistency | Fix recommended, not blocking |
| **INFO** | Suggestion, style preference, potential future improvement | Optional, for awareness |

## Phase 5 — Output

### Diff Review Output

Print the review directly to the conversation. Format:

```markdown
# Code Review — {branch/file/staged}

**Date:** {date}
**Reviewer:** Claude Code
**Scope:** {files changed count} files, {lines added}+/{lines removed}-

## Compliance Checks

| Check | Status | Notes |
|---|---|---|
| TDD | PASS/FAIL | details |
| Architecture | PASS/FAIL | details |
| Error Handling | PASS/FAIL | details |
| Clippy | PASS/FAIL | details |
| Code Quality | PASS/FAIL | details |
| Security | PASS/FAIL | details |

## Findings

### BLOCKER (N)
- **[B1]** `file:line` — description. **Fix:** suggested fix.

### CRITICAL (N)
- **[C1]** `file:line` — description. **Fix:** suggested fix.

### WARNING (N)
- **[W1]** `file:line` — description.

### INFO (N)
- **[I1]** `file:line` — description.

## Quality Scores

| Dimension | Score | Evidence |
|---|---|---|
| Correctness | N/5 | ... |
| Design | N/5 | ... |
| Readability | N/5 | ... |
| Testability | N/5 | ... |
| Performance | N/5 | ... |
| Consistency | N/5 | ... |

## Verdict

**{APPROVE / REQUEST_CHANGES / REJECT}**

{One sentence summary of the overall assessment.}
```

**Verdict rules:**
- Any BLOCKER → REJECT
- Any CRITICAL → REQUEST_CHANGES
- All PASS + no BLOCKER/CRITICAL → APPROVE

### Deep Review Output

Save to `docs/reviews/{crate-name}/REVIEW.md`. Format:

```markdown
# {crate-name} — Revisao

> **Contexto**: {one-line description of what the crate does}
>
> **Dependencias permitidas** ({ADR ref}): {list of allowed deps}
>
> **Status global**: deep-review concluido em {date}. {test count} tests passando, {failures} falhas. `cargo clippy -p {crate} --lib --tests` {silent/N warnings}.

## Compliance Checks

| Check | Status | Notes |
|---|---|---|
| TDD | PASS/FAIL | {test count}, {coverage estimate} |
| Architecture | PASS/FAIL | {deps verified against rules} |
| Error Handling | PASS/FAIL | {unwrap count in prod, error types} |
| Clippy | PASS/FAIL | {warning count} |
| Code Quality | PASS/FAIL | {max LOC file, max LOC fn, complexity} |
| Security | PASS/FAIL | {unsafe blocks, secrets, sandbox} |

## Dominios

| # | Nome | Descricao | LOC | Tests | Status |
|---|------|-----------|-----|-------|--------|
| 1 | `module_name` | description | N | N | Revisado/Pendente |

## Notas de Deep-Review por Dominio

> Auditoria orientada a: (1) responsabilidade unica, (2) dependencias, (3) cobertura de testes, (4) hygiene (LOC <= 500, zero clippy warnings, zero unwrap() em prod).

### N. domain_name (N LOC)

{Paragraph describing what the module does, key patterns found, test coverage, and any findings.}

**Findings:**
- [SEVERITY] description — file:line

---

## Findings Summary

| Severity | Count | Fixed | Remaining |
|---|---|---|---|
| BLOCKER | N | N | N |
| CRITICAL | N | N | N |
| WARNING | N | N | N |
| INFO | N | N | N |

## Quality Scores

| Dimension | Score | Evidence |
|---|---|---|
| Correctness | N/5 | ... |
| Design | N/5 | ... |
| Readability | N/5 | ... |
| Testability | N/5 | ... |
| Performance | N/5 | ... |
| Consistency | N/5 | ... |

**Overall: N/5**
```

If any BLOCKER or CRITICAL findings exist, also generate `docs/reviews/{crate-name}/REMEDIATION_PLAN.md`:

```markdown
# Plano de Remediacao — `{crate-name}`

> Derivado de `docs/reviews/{crate-name}/REVIEW.md`. Cada item e um PR ou grupo de PRs executavel.

## Convencoes

| Campo | Significado |
|---|---|
| **ID** | Identificador estavel (ex.: `T1.2`) |
| **Severidade** | BLOCKER / CRITICAL |
| **Bloqueia** | Tarefas que dependem desta |
| **AC** | Criterio(s) de aceitacao verificavel(is) |

## Tasks

### T{N}.{M} — {Title}

- **Severidade:** BLOCKER/CRITICAL
- **Arquivo:** `path/to/file.rs`
- **Descricao:** {what to fix and why}
- **AC:** {verifiable acceptance criteria}
- **Bloqueia:** {downstream task IDs or "nenhuma"}
```

## Review Best Practices (Internalized)

The reviewer MUST follow these principles:

1. **Review the code, not the author.** Focus on technical merit. Be specific and constructive.

2. **Every comment must be actionable.** "This is wrong" is not a review comment. "This swallows the error at line 42 — propagate it with `?` or handle with explicit match" is.

3. **Distinguish taste from correctness.** Style preferences are INFO. Bugs and violations are BLOCKER/CRITICAL. Don't block a PR over naming unless it's genuinely misleading.

4. **Check what's NOT there.** Missing error handling, missing tests, missing validation, missing edge cases — absence is harder to spot than presence.

5. **Read the tests first.** Tests document intent. If the tests are wrong, the implementation is wrong regardless of how clean the code looks.

6. **Verify claims.** If a comment says "this is safe because X", verify X. If a test name says "test_handles_empty_input", read it and confirm it actually tests empty input.

7. **Check the boundaries.** Most bugs live at module boundaries: serialization/deserialization, API contracts, state transitions, error propagation between layers.

8. **One pass per dimension.** Don't try to catch everything in one read. Read once for correctness, once for design, once for security. Each pass has a different lens.

9. **Praise good code.** If you find a particularly clean abstraction, well-written test, or clever-but-readable solution, say so. Reviews that only criticize train people to write defensive code, not good code.

10. **"funciona" ≠ "esta correto".** Code that passes tests today may have snapshot semantics issues, missing typed contracts, or evaluation gaps that surface in production.
