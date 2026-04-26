# Eval baseline — SOTA Tier 1 + Tier 2 plan §T12.1

**Created:** 2026-04-26
**Branch baseline:** `develop` at `37cb3b2` (the commit BEFORE the
SOTA Tier 1+2 wave landed). After-state is whatever HEAD reports
when the bench job runs.

## Why a baseline

`docs/plans/sota-tier1-tier2-plan.md` Global DoD requires SWE-Bench-
Verified or terminal-bench reduced to move ≥10 points above the
baseline. This file documents the comparison anchor:

- **commit:** `37cb3b2`
- **artifact:** to be populated by the first manual run of
  `apps/theo-benchmark/runner/ab_test.py --tasks 10 --bin <baseline-theo>`
  with paid-model API keys.
- **storage:** `apps/theo-benchmark/reports/baseline-37cb3b2.json`
  once captured.

## How the gate works (when baseline JSON exists)

1. `.github/workflows/eval.yml` builds HEAD's theo binary.
2. Runs `ab_test.py` against the same task set used for baseline.
3. Compares solved/failed/cost vs baseline.
4. PR comment posts the delta. (Future: HARD gate failing PR if
   delta is < +0 points.)

## Currently NOT enforced

The eval workflow is INFORMATIONAL until:

- `THEO_GROQ_API_KEY` (smoke job) and/or
- `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` (bench-full job)

are configured in repository Secrets, AND the baseline JSON is
captured.

Maintainer steps to activate the gate:

1. **Manually run** `ab_test.py` against `37cb3b2` with paid keys.
2. **Commit** the resulting JSON to
   `apps/theo-benchmark/reports/baseline-37cb3b2.json`.
3. **Set secrets** in repository settings:
   - `THEO_GROQ_API_KEY`
   - `ANTHROPIC_API_KEY` and/or `OPENAI_API_KEY`
4. **Optional**: add the `bench` label to PRs that should run the full
   bench (without it, only the smoke job runs to keep CI fast and free).

## Costs

- Smoke: < $0.01/PR (Groq free tier).
- Full bench: ~$1–5/PR with Claude Sonnet on 10 reduced tasks.
- Nightly full bench (push to main/develop): ~$5–25/run.

Budget cap maintainers: gate the `bench-full` job further by
restricting the `bench` label to maintainers only, or move the full
bench to a scheduled `cron` workflow that runs once per day.
