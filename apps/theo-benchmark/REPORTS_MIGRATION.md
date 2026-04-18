# Reports Migration — Historical Results Invalidation

## Date: 2026-04-16

## Bug P0: Temperature Never Propagated

**Impact:** ALL benchmark reports generated before this fix used `temperature=0.1`
(the hardcoded default in `config.rs:307`), regardless of what was configured.

**Root cause:** `_headless.py` set `THEO_TEMPERATURE=0.0` in the environment, but
the Rust binary (`cmd_headless()` in `main.rs`) never called
`ProjectConfig::with_env_overrides()` — the env var was silently ignored.

**Fix:** Added `--temperature` CLI flag (highest precedence), env var fallback via
`ProjectConfig::with_env_overrides().apply_to()`, and headless JSON now includes
`environment.temperature_actual` for auditability.

## Bug P1: Oracle Mode Was Default

**Impact:** All SWE-bench reports used oracle mode by default, passing `FAIL_TO_PASS`
test names to the agent. This is data leakage — results are inflated and not
comparable with published baselines (SWE-Agent, OpenHands, Agentless).

**Fix:** Default flipped to non-oracle. `--oracle` flag is now opt-in with warning.

## Affected Reports

All reports in `reports/` with schema `theo.smoke.v1` or `theo.swe.v1`/`v2` generated
before commit that includes this file are **invalidated** for:

- **Publication:** Cannot be cited as evidence of system performance
- **Comparison:** Cannot be compared against other agents' published numbers
- **Ablation:** Cannot be used as baselines for ablation studies

## How to Identify Valid Reports

Valid reports (post-fix) will have:
- Schema `theo.headless.v2` with `environment.temperature_actual` field
- `oracle_mode: false` for SWE-bench (or explicitly `true` with documented disclosure)
- `environment.theo_version` matching a post-fix binary

## Re-running

To generate valid baselines:

```bash
# Smoke (deterministic)
python runner/smoke.py --temperature 0.0

# SWE-bench (non-oracle, deterministic, official grading)
python swe/adapter.py --dataset lite --temperature 0.0 --grade
```
