# Semgrep Rules (T6.1)

Custom SAST ruleset in `.semgrep/theo.yaml`. Semgrep is installed via
`make audit-tools` (`pipx install semgrep`).

## Rules

| ID | Severity | Summary |
| --- | --- | --- |
| `theo.log-no-token-interpolation` | ERROR | Blocks `log::*!` / `tracing::*!` / `println!` / `eprintln!` from interpolating a variable whose name matches `token`, `password`, `api_key`, `secret`, `bearer`, `session_key`, `private_key`. |
| `theo.log-no-inline-secret-field` | ERROR | Same as above, but catches `info!("token={token}")` rewrite form that the macro-matcher cannot see. |
| `theo.sandbox-no-unwrap-on-create-executor` | WARNING | Forbids `create_executor(…).unwrap()` — undermines the T2.2 fallback path. |
| `theo.shell-no-format-in-arg` | WARNING | Flags `Command::new("sh").arg("-c").arg(format!(...))` — command-injection heuristic. |

## Running locally

```bash
semgrep --config .semgrep/theo.yaml --error --error --severity=ERROR --severity=WARNING crates/ apps/
```

Only ERROR-severity matches fail CI; WARNINGS surface for review but do
not block merges.

## CI wiring

`make audit` already calls `semgrep --error --config p/rust --config p/ci`
when the binary is present. T6.1 additionally requires the CI job to
load this repository-local ruleset:

```yaml
- name: Semgrep
  run: semgrep --error --config .semgrep/theo.yaml crates apps
```

## Allowlisting

Prefer rewrites over suppression:

- Wrap the secret in `secrecy::SecretString` or a newtype whose
  `Display` returns `"<redacted>"`.
- Restructure the log call so the secret is never in scope.

If a suppression is truly unavoidable, add a `// nosemgrep: <rule-id>`
comment and reviewers will scrutinise during code review.

## Baseline

Initial run against `crates/` + `apps/`: **0 violations** (per the
2026-04-23 audit; grep-based pre-check also found zero token-field
interpolations).
