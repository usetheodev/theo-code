---
type: insight
topic: "SOTA validation pipeline — evidence-based thresholds and 16-agent loop assessment"
confidence: 0.85
impact: high
meeting_id: 20260429-143744
generated_at: 2026-04-29T14:37:44-03:00
---

# Insight: Evidence-Based SOTA Thresholds and the 16-Agent Question

## Position: APPROVE with CONCERN

**Key finding:** Research strongly supports harness-focused investment over model upgrades, but evidence explicitly warns against multi-candidate/multi-verifier patterns. A 16-agent loop risks falling into the "more structure = better" trap that Tsinghua's ablation study disproved.

## Evidence-Based Thresholds

| Capability | Threshold | Source | Confidence |
|---|---|---|---|
| Harness orchestration delta | 6x performance from harness alone, same model | Stanford Meta-Harness [1] | 0.95 |
| Self-evolution loop | +4.8 SWE-Bench (only consistently beneficial module) | Tsinghua ablation [1] | 0.90 |
| Representation format | +16.8 pts switching from native code to structured NL | Tsinghua [1] | 0.85 |
| Incremental single-feature execution | Eliminated one-shotting + premature completion | Anthropic long-running harness [2] | 0.90 |
| Context compaction | Required for multi-window; 5-stage pipeline in Claude Code | Anthropic [2], arXiv 2604.14228 [3] | 0.85 |
| Verifiers as separate agents | -0.8 SWE-Bench, -8.4 OS World | Tsinghua ablation [1] | 0.90 |
| Multi-candidate search | -2.4 SWE-Bench, -5.6 OS World | Tsinghua ablation [1] | 0.90 |
| Industry SOTA ceiling | 72.7% SWE-Bench Verified (Claude Code) | Public benchmark [4] | 0.95 |
| Theo current baseline | 50% SWE-Bench with Qwen3-30B | Internal benchmark [5] | 0.95 |

## The 16-Agent Loop Question

**Evidence AGAINST a 16-agent loop:**

1. **Verifiers hurt.** Tsinghua found that adding verification agents *decreases* performance by 0.8-8.4 points across benchmarks [1]. A 16-agent loop with dedicated verifier agents directly contradicts this.

2. **Multi-candidate search hurts.** Running multiple candidates and selecting the best reduced SWE-Bench by 2.4 points [1]. If the 16-agent loop uses parallel candidate generation, this is a known anti-pattern.

3. **~90% of compute goes to child agents.** The harness is an orchestration pattern, not a reasoning pattern [1]. Adding more agents increases orchestration cost without proportional reasoning benefit.

4. **Anthropic's own 15x token cost.** Multi-agent systems consume 15x more tokens than single-agent [3]. 16 agents would amplify this dramatically.

**Evidence FOR structured multi-agent (but NOT 16):**

1. **Orchestrator-worker with 3-4 roles is the production consensus.** Claude Code uses Lead + {Explorer, Implementer, Verifier, Reviewer}. Theo already has this [3].

2. **Aider's Architect/Editor dual-model** achieves SOTA with just 2 agents [6]. Simplicity wins.

3. **The self-evolution loop (single retry-with-gate) is the only module that consistently helps** [1]. This is 1 agent retrying, not 16 agents collaborating.

4. **Anthropic's Planner-Generator-Evaluator** uses 3 agents, not 16. It was 20x more expensive ($200 vs $9) but worked [1].

## Risks

1. **Direct contradiction:** A 16-agent loop with verifiers/multi-candidates violates the two strongest negative findings in the ablation literature.
2. **Cost explosion:** At 15x tokens per multi-agent call, 16 agents could consume 50-100x the single-agent budget with no evidence of proportional quality gain.
3. **Diminishing returns curve:** Every production system that works (Claude Code, Codex, Aider) uses 2-4 specialized agents, not 16. No published system demonstrates benefit from >5 concurrent coding agents on a single task.

## Recommendations

1. **Cap at 4-5 specialized agents** (Orchestrator + Explorer + Implementer + Tester + optional Reviewer). This matches every successful production system.
2. **Invest in self-evolution loop** — single agent retrying with acceptance gate. +4.8 SWE-Bench proven.
3. **Invest in representation quality** — structured NL over code-native harness. +16.8 pts proven.
4. **Invest in incremental execution** — one feature at a time, progress artifacts between context windows. Anthropic's long-running pattern [2].
5. **Do NOT add verifier agents or multi-candidate search** unless ablation on Theo's own benchmark proves benefit. The default assumption from literature is that they hurt.
6. **Measure before scaling agents** — run Theo's current 4-role setup through SWE-Bench first, then add/remove roles with ablation evidence.

## Implication for Theo

The path from 50% to 65%+ SWE-Bench is NOT "more agents." It is: (1) better harness orchestration (representation, incremental execution, progress artifacts), (2) self-evolution retry loop with acceptance gate, and (3) context compaction for multi-window tasks. These three changes alone account for +20-25 points in published literature, which would put Theo in the 70-75% range — matching industry SOTA — without any model upgrade.

A 16-agent loop should be rejected unless the proposer can cite empirical evidence showing benefit beyond 4-5 agents on coding benchmarks. No such evidence exists in the surveyed literature.

## Sources

1. [The Rise of Harness Engineering — Stanford/Tsinghua/DeepMind/LangChain/Anthropic compilation, March 2026](docs/pesquisas/harness-engineering-guide.md)
2. [Effective Harnesses for Long-Running Agents — Anthropic, November 2025](docs/pesquisas/effective-harnesses-for-long-running-agents.md)
3. [SOTA Sub-Agent Architectures — internal research report, 47 sources](docs/pesquisas/sota-subagent-architectures.md)
4. [Claude Code SWE-Bench Verified — public leaderboard](https://swebench.com)
5. [Theo benchmark results — internal](apps/theo-benchmark/reports/)
6. [Aider Architect/Editor pattern](https://aider.chat/2024/09/26/architect.html)
7. [Harness Engineering for Coding Agent Users — Birgitta Bockeler, April 2026](docs/pesquisas/harness-engineering.md)
8. [Harness Engineering: Leveraging Codex — OpenAI, February 2026](docs/pesquisas/harness-engineering-openai.md)
9. [SOTA Planning System — internal research report, 18 sources](docs/pesquisas/meeting-sota-research.md)
