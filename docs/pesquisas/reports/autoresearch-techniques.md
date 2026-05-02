---
type: report
question: "What techniques from Karpathy's autoresearch can improve an AI coding agent's benchmark performance?"
generated_at: 2026-04-15T19:00:00-03:00
confidence: 0.88
sources_used: 8
---

# Report: Autoresearch Techniques for AI Coding Agent Improvement

## Executive Summary

Karpathy's autoresearch (March 2026, 72k+ stars) demonstrates a deceptively simple pattern: give an AI agent a single file to modify, a fixed evaluation budget, and a clear metric — then let it run hundreds of experiments autonomously. The core insight is not the ML training itself but the **experiment loop architecture** — a pattern directly transferable to improving Theo's benchmark scores on coding tasks.

## What Autoresearch Is

An autonomous experiment loop where an AI agent modifies `train.py`, runs a 5-minute training experiment, evaluates via `val_bpb`, keeps improvements via git commit, discards failures via `git reset`, and repeats indefinitely. Three files form the contract:

- `prepare.py` — immutable evaluation infrastructure (the "fair judge")
- `train.py` — the agent's sandbox (the only thing it can change)
- `program.md` — human-authored research direction (the "meta-prompt")

Results: 700 experiments over 2 days yielded ~20 additive improvements. 11% efficiency gain on Time-to-GPT-2 leaderboard. [Source 1, 2]

---

## Pattern 1: The Ratchet Loop

**How it works:** Each iteration follows commit-run-evaluate-keep/discard. Git history only moves forward — failed experiments are reverted. `results.tsv` logs every attempt with commit hash, metric, memory, status, and description.

**Key properties:**
- Deterministic evaluation (fixed 5-min budget = comparable results)
- Binary keep/discard decision (did metric improve? yes/no)
- Complete audit trail (TSV + git log)
- Agent reads its own history to decide what to try next

**Applicable to Theo:** Implement an auto-benchmark loop where the agent modifies its own prompts/tool implementations, runs a benchmark suite, and keeps only improvements. The "ratchet" ensures we never regress.

```
loop {
    let baseline = run_benchmark(current_config);
    let candidate = agent.propose_modification(history);
    apply(candidate);
    let result = run_benchmark(candidate_config);
    if result.score > baseline.score {
        commit(candidate, result);
    } else {
        revert(candidate);
    }
    log(candidate, result);
}
```

---

## Pattern 2: Three-File Contract (Separation of Concerns)

**How it works:** Strict separation between what the agent can touch (sandbox), what measures success (immutable evaluator), and what guides strategy (human-authored direction).

**Why it matters:** Prevents the agent from gaming the metric by modifying the evaluator. The human steers via `program.md` without touching code.

**Applicable to Theo:** For self-improvement benchmarks:
- **Immutable:** benchmark harness, test cases, scoring function
- **Agent sandbox:** system prompts, tool implementations, retrieval parameters
- **Human direction:** which benchmarks to optimize, constraints, quality bar

---

## Pattern 3: The program.md Meta-Prompt

**How it works:** A single markdown file carrying three registers simultaneously:
1. **Instructions** — what the agent should search for
2. **Constraints** — what must not change (no new deps, fixed budget)
3. **Stopping criteria** — when to wrap up

**Key directives from Karpathy's program.md:**
- "NEVER STOP. Once the experiment loop has begun, do NOT pause to ask the human."
- "All else being equal, simpler is better. A small improvement that adds ugly complexity is not worth it."
- "Fix typos and re-run, skip ideas that are broken at the root, kill anything past 10 minutes."
- Hardcoded baseline metrics the agent must beat.

**Applicable to Theo:** Structure agent runtime prompts with explicit:
- Autonomy directive (don't stop to ask)
- Simplicity bias (prefer clean solutions)
- Failure recovery rules (retry vs skip vs abort)
- Concrete numeric targets to beat

---

## Pattern 4: Multi-Dimensional Metric Tracking

**How it works:** While `val_bpb` is the primary metric, `results.tsv` also tracks GPU memory, pass/fail status, and experiment descriptions. The scaled version (SkyPilot, 16 GPUs) added cost tracking. [Source 5]

**Extended metrics from the "agentic coding skills" adaptation:** [Source 6]
- Code correctness (primary)
- Token consumption and cost
- Execution time
- Error rates and self-corrections
- Tool call frequency

**Key insight:** "Correctness dominates the score, which is desirable. Only after maximizing quality do time and cost become deciding factors."

**Applicable to Theo:** Track per-benchmark-task:
- Pass/fail (primary)
- Token usage (cost proxy)
- Wall-clock time
- Tool calls count
- Self-correction count (retries)
- Retrieved context relevance score

---

## Pattern 5: Parallel Hypothesis Testing (Scaled Version)

**How it works:** With 16 GPUs, the agent shifted from sequential greedy search to factorial grid testing — e.g., 3 weight decay values x 4 learning rates = 12 parallel experiments per 5-minute wave. This prevented local optima entrapment. [Source 5]

**Results:** 910 experiments in 8 hours, 2.87% improvement, 9x faster than sequential.

**Applicable to Theo:** When optimizing prompts or retrieval parameters, test multiple variants in parallel rather than sequential A/B. Even on a single machine, run N benchmark tasks with different configs simultaneously.

---

## Pattern 6: Emergent Phase Progression

**How it works:** Without explicit programming, the agent's search naturally progressed through phases:
1. Hyperparameter mapping (broad exploration)
2. Architectural discovery (structural changes)
3. Fine-tuning (narrow optimization)
4. Diminishing returns (plateau)

**Applicable to Theo:** Expect and plan for similar phases in self-optimization:
1. Prompt wording tweaks (easy wins)
2. Tool/retrieval architecture changes (medium effort)
3. Agent loop structure modifications (high effort)
4. Plateau — time to change the benchmark or add capabilities

---

## Gaps

- **No strategic backtracking:** The ratchet pattern is greedy — it cannot accept a temporary regression for a larger future gain. Karpathy acknowledged this as the "creativity ceiling."
- **RLHF conservatism:** Karpathy noted the agent felt "cagey and scared" on open-ended problems, cycling through minor variations rather than bold experiments. RLHF-trained models are risk-averse.
- **Single metric limitation:** Binary keep/discard on one metric misses Pareto-optimal tradeoffs (e.g., slightly worse accuracy but 3x faster).
- **No cross-experiment learning:** Each iteration is stateless beyond reading results.tsv. No learned heuristics persist across sessions.

---

## Recommendations for Theo

### Immediate (can implement now)

1. **Build a benchmark ratchet loop** — Script that runs Theo against a task suite, proposes prompt/config changes, evaluates, keeps improvements. Start with SWE-bench-lite or a custom task suite.

2. **Structure agent prompts like program.md** — Add explicit autonomy directives, simplicity bias, failure recovery rules, and numeric targets to the agent runtime system prompt.

3. **Add results.tsv-style logging** — Every benchmark run logs: task, pass/fail, tokens, time, tool_calls, self_corrections. Machine-readable, append-only.

### Medium-term

4. **Parallel config search** — Test retrieval parameters (chunk size, top-k, reranking weights) in parallel across benchmark tasks rather than sequentially.

5. **Immutable evaluator contract** — Ensure benchmark scoring is fully isolated from agent-modifiable code. The agent should never be able to touch the test harness.

6. **Multi-metric Pareto tracking** — Don't just optimize pass rate. Track the correctness-cost-speed Pareto frontier and select configs on the efficient frontier.

### Longer-term

7. **Cross-session learning** — Persist what worked/failed across optimization sessions. Build a "research memory" that informs future experiment selection.

8. **Escape local optima** — Implement occasional random restarts or "bold experiment" mode to break out of incremental optimization plateaus.

---

## Sources

1. [GitHub - karpathy/autoresearch](https://github.com/karpathy/autoresearch) — Primary repository
2. [VentureBeat - Karpathy's autoresearch](https://venturebeat.com/technology/andrej-karpathys-new-open-source-autoresearch-lets-you-run-hundreds-of-ai) — Overview and results
3. [DataCamp - Guide to AutoResearch](https://www.datacamp.com/tutorial/guide-to-autoresearch) — Technical architecture deep dive
4. [Fortune - The Karpathy Loop](https://fortune.com/2026/03/17/andrej-karpathy-loop-autonomous-ai-agents-future/) — 700 experiments, 20 additive improvements
5. [SkyPilot Blog - Scaling Autoresearch](https://blog.skypilot.co/scaling-autoresearch/) — Parallel execution, 910 experiments, 2.87% improvement
6. [Kirill Krainov - Improving Agentic Coding Skills](https://zerocopy.blog/2026/03/25/karpathys-autoresearch-improving-agentic-coding-skills/) — Multi-metric tracking, adaptation for coding agents
7. [The New Stack - 630-line script](https://thenewstack.io/karpathy-autonomous-experiment-loop/) — Technical implementation details
8. [Particula - 100 ML Experiments](https://particula.tech/blog/karpathy-autoresearch-autonomous-ml-experiments) — Overnight experiment results
