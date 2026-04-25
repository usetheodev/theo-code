---
name: code-audit
description: Run code-audit techniques against Theo Code — cyclomatic complexity, coverage + mutation, module size, dependencies, SCA, unit tests, integration tests, pentest (SAST). Pass a technique name or `all`.
user-invocable: true
context: fork
argument-hint: "[complexity|coverage|size|dependency|sca|unit-test|integration|pentest|all]"
---

Run static code-audit techniques on the Theo Code workspace. Each technique is executed by a dedicated read-only agent that reports findings without modifying code.

## Arguments

| Arg          | Agent                      | Technique                                      |
|--------------|----------------------------|------------------------------------------------|
| `lint`       | lint-auditor               | Linting — Rust + TS                            |
| `complexity` | complexity-analyzer        | Cyclomatic complexity (CCN) — Rust + TS        |
| `coverage`   | test-coverage-auditor      | Coverage + mutation testing                    |
| `size`       | module-size-auditor        | File / function / class size limits            |
| `dependency` | dependency-auditor         | Internal dep graph + boundary violations       |
| `sca`        | sca-auditor                | CVEs, licenses, outdated packages              |
| `unit-test`  | unit-test-auditor          | Unit test quality (AAA, determinism, names)    |
| `integration`| integration-test-auditor   | Integration / instrumentation test coverage    |
| `pentest`    | pentest-auditor            | SAST, secrets, OWASP, sandbox audit            |
| `all`        | all 8 agents (in sequence) | Full audit                                     |
| *(no arg)*   | same as `all`              | Full audit                                     |

## How to dispatch

1. Parse `$ARGUMENTS`. Default to `all` if empty.
2. Validate against the table above. If invalid, list valid options and stop.
3. For single-technique: invoke the matching agent via the Agent tool with `subagent_type=<agent-name>`.
4. For `all`: invoke each agent in sequence. Do NOT parallelize — full cargo/npm audits can saturate disk/network. Collect reports.
5. After all agents finish, synthesize a single consolidated report (see below).

## Prompt to pass each agent

When invoking an agent, include:

- The working directory (always Theo Code root).
- Any staged/changed files if the user scoped the request (`git diff --name-only --cached`).
- A reminder: "Read-only. Report findings in the format in your system prompt. Do not edit any file."

Example delegation prompt for `complexity-analyzer`:

> Audit cyclomatic complexity across the Theo Code workspace. Focus on crates/ and apps/theo-ui/src/. Skip tests, target/, node_modules/. Follow the thresholds and report format in your agent definition. Return a single consolidated report.

## Consolidated report (for `all`)

After all 8 agents finish, produce this summary:

```
THEO CODE — CODE AUDIT REPORT
=============================
Date:   <YYYY-MM-DD HH:MM>
Commit: <git rev-parse --short HEAD>

TECHNIQUE              VERDICT   CRITICAL  HIGH  WARN
---------              -------   --------  ----  ----
Cyclomatic complexity  PASS|FAIL  N         M     K
Coverage + Mutation    PASS|FAIL  ...
Module size            ...
Dependency structure   ...
SCA                    ...
Unit test quality      ...
Integration tests      ...
Pentest (SAST)         ...

OVERALL: PASS | WARN | FAIL
(FAIL if ANY technique is FAIL)

TOP-10 BLOCKING ISSUES (across all techniques):
  1. [pentest]    crates/theo-tooling/src/sandbox.rs:L88  silent Noop fallback
  2. [sca]        openssl 0.10.48 RUSTSEC-2023-0044
  3. [dependency] theo-domain imports theo-infra-auth
  ...

FULL REPORTS: see each agent's output above.
```

## Rules

- Never run these on `main` unattended — use only in feature branches or local dev.
- If a required tool (cargo-audit, cargo-mutants, semgrep, gitleaks, madge, stryker) is missing, the agent will report `INSTALL:` instructions. Relay those to the user and continue.
- Read-only. If the user asks for fixes after the audit, switch to normal work mode (the audit skill itself never edits files).
- For `all`, expect total runtime 5-20 minutes depending on tooling and workspace state.
- If the user passes any unknown argument, show the table above and stop. Do not guess.

## Example invocations

```
/code-audit               # Full audit
/code-audit complexity    # Only CCN
/code-audit sca           # Only dependency vulnerabilities
/code-audit pentest       # Only SAST/secrets/sandbox audit
```
