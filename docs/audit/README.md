# Audit — Index

This directory tracks the remediation of the 2026-04-23 code audit. All
artifacts referenced in `remediation-plan.md` live here or are linked from here.

## Files

- `remediation-plan.md` — the executable plan with tasks, acceptance criteria,
  and Definitions of Done.
- `tooling.md` — how to install every CLI the audit depends on.
- `licensing.md` — *(pending T3.2)* `deny.toml` policy and license review.
- `quality-gates.md` — *(pending T5.1)* coverage/mutation targets.

## Commands

```bash
make audit              # run all 8 techniques (sub-targets tolerate missing tools)
make check-arch         # T1.5 architectural boundary gate
make audit-tools-check  # which audit tools are missing
make audit-tools        # idempotent installer
```

## Current audit status

See the consolidated table in `remediation-plan.md` under **Rastreabilidade**.
Live progress lives in `../../.theo/audit-remediation-progress.md`.
