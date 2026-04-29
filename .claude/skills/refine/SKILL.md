---
name: refine
description: Run the SOTA refinement cycle (keep/discard pattern). Identifies worst gap, proposes improvement, retests, keeps or discards. Human-gated merge.
user-invocable: true
argument-hint: "[dry-run|apply|status|thresholds]"
---

SOTA Refinement Cycle for Theo Code. Validates existing features against evidence-based thresholds and iteratively improves the worst-performing areas.

## Arguments

| Argument | Behavior |
|---|---|
| No args / `dry-run` | Run cycle in dry-run mode (identify gaps, propose hypotheses, no changes) |
| `apply` | Run cycle with changes enabled (still human-gated before merge) |
| `status` | Show current threshold status (PASS/FAIL for each dod-gate) |
| `thresholds` | Display all thresholds from `docs/sota-thresholds.toml` |

## Pre-requisites

Before running, verify:

```!
# Check thresholds TOML exists
test -f docs/sota-thresholds.toml && echo "OK: thresholds exist" || echo "MISSING: run Phase 2 first"

# Check probe runner exists
test -f apps/theo-benchmark/e2e/probe_runner.py && echo "OK: probe runner exists" || echo "MISSING: run Phase 1 first"

# Check autoloop exists
test -f apps/theo-benchmark/autoloop/cycle.py && echo "OK: autoloop exists" || echo "MISSING: run Phase 3 first"
```

## Process

### For `status` or `thresholds`:

```!
cd apps/theo-benchmark && python e2e/threshold_checker.py
```

### For `dry-run` (default):

```!
cd apps/theo-benchmark && python autoloop/cycle.py
```

### For `apply`:

**WARNING:** This will propose code changes. Each change requires your explicit approval.

```!
cd apps/theo-benchmark && python autoloop/cycle.py --apply
```

## Constraints (Meeting D3 — Inviolable)

1. **Human-gated merge** — every change requires explicit approval
2. **Forbidden paths** — the cycle CANNOT modify:
   - `.claude/rules/*-allowlist.txt`
   - `CLAUDE.md`
   - `Makefile`
   - `scripts/check-*.sh`
3. **Allowed crates** — only `theo-engine-retrieval` and `theo-agent-runtime`
4. **Budget cap** — $10 per cycle run (configurable in `autoloop/config.toml`)
5. **Max iterations** — 5 per run

## Evidence Base

Thresholds are defined in `docs/sota-thresholds.toml` with citations:
- Retrieval floors: MRR>=0.90, Recall@5>=0.92, Recall@10>=0.95, DepCov>=0.96
- Self-evolution loop: +4.8 SWE-Bench (Tsinghua ablation)
- Verifiers HURT: -0.8 to -8.4 (do NOT add verifier agents)
- Industry SOTA: 72.7% SWE-Bench Verified (Claude Code)

## TDD Gate

Any code change proposed by the cycle MUST:
1. Have a failing test written FIRST
2. Pass all existing tests after the change
3. Pass `make check-arch` (no boundary violations)
