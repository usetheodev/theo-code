# Evolution Research — Smart Model Routing for Code Agents

**Prompt source:** `outputs/smart-model-routing-plan.md` (6 phases R0-R5, 40 acceptance criteria)
**Underlying research:** `outputs/smart-model-routing.md` (3245 words, 7 sections, audited 5 reference repos)
**Date:** 2026-04-20
**Baseline:** 75.150 (L1=99.8, L2=50.5)

## 1. Starting context (from the prior deep-research)

theo-code today has **zero routing code** (grep-verified). `AgentConfig.model: String` at `config.rs:252` is the only selection — one model for the entire session. The prior research mapped the 2026 SOTA and 5 reference-repo patterns and recommended a 5-phase incremental build:

- R1: domain trait surface in `theo-domain` (zero-dep)
- R2: rule-based classifier in `theo-infra-llm` (ported from hermes-agent)
- R3: wire into `RunEngine` at the `ChatRequest` build site
- R4: extend to compaction + subagent phases + TOML config
- R5: fallback cascade on errors (overflow / 429 / timeout)

## 2. Reference patterns (already extracted in full in `outputs/smart-model-routing.md`)

Three patterns drive the implementation, each tied to a reference file:

| Pattern | Source | Role in plan |
|---|---|---|
| Slot-based model config | `referencias/opendev/crates/opendev-models/src/config/agent.rs:22-66` | Shape of `.theo/config.toml` `[routing.slots.*]` blocks (R4) |
| One-line cascade (`override ?? default`) | `referencias/Archon/packages/providers/src/claude/provider.ts:562` | The single call-site router invocation (R3) |
| Rule-based classifier w/ complex-keywords | `referencias/hermes-agent/agent/smart_model_routing.py:62-107` | The R2 rules; keywords **paraphrased** (AGPL) |

## 3. Plan adaptations for evolution-loop scope

The plan's R0 specifies files under `apps/theo-benchmark/` — **which is out of scope** for the evolution loop (`CANNOT modify: apps/theo-benchmark/`). Adapting:

- **R0 fixture location:** `.theo/fixtures/routing/*.json` (explicitly allowed path)
- **R0 runner:** cargo integration test in `crates/theo-infra-llm/tests/routing_metrics.rs` that loads the fixture and reports `{avg_cost_per_task, task_success_rate, p50_turn_latency}` as JSON on stdout
- **R0 CLI:** deferred; tests invoked via `cargo test -p theo-infra-llm --test routing_metrics`

All other phases (R1-R5) land inside allowed paths (`crates/*/src/`, `crates/*/tests/`, `.theo/`).

## 4. Execution order (locked)

```
R0 (fixture + metrics harness)  ──▶  R1 (domain trait)  ──▶  R2 (rules)
                                                               ↓
R5 (fallback)  ◀──  R4 (compaction + subagent + TOML)  ◀──  R3 (wire)
```

Linear dependency chain; each phase satisfies the global DoD (see `.theo/evolution_criteria.md`) before the next begins.

## 5. Acceptance criteria snapshot

40 AC tests total across 6 phases (R0=4, R1=6, R2=8, R3=6, R4=8, R5=8). Each AC is a named test; the completion promise "TODAS TASKS, E DODS CONCLUIDOS E VALIDADOS" is satisfied only when every AC passes and every global DoD gate is green.

Full AC list: `outputs/smart-model-routing-plan.md` §2 (per-phase tables).

## 6. What "done" looks like (per the plan's §0)

| Metric | Baseline | Target |
|---|---|---|
| `avg_cost_per_task` | NullRouter equivalent (today) | ≥ 30% lower on mixed fixture |
| `task_success_rate` | today | parity (never regress) |
| `p50_turn_latency` | today | ≤ +5% |
| Workspace tests | 2724 | ≥ 2724 |
| Harness score | 75.150 | ≥ 75.150 |

Cost/latency targets are **ratios, not absolutes** (per §4 of the plan) — environment-dependent numbers are out of scope.
