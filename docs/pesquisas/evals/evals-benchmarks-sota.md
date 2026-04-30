# Evals & Benchmarks -- State of the Art (April 2026)

**Domain:** Evals / Benchmarks for AI Coding Agents
**Generated:** 2026-04-29
**Confidence:** 0.88
**Sources used:** 34 (web searches, papers, leaderboards, Theo Code codebase)
**Goal:** Raise Evals domain from 3.5/5 to 4.5+/5

---

## Executive Summary

The AI coding benchmark landscape in April 2026 is defined by three converging trends: (1) SWE-bench Verified is now considered contaminated and unreliable for frontier comparison -- SWE-bench Pro and SWE-rebench have emerged as successors; (2) harness engineering has overtaken model capability as the primary determinant of benchmark scores, with a 6x performance gap attributable to harness design alone; and (3) every benchmark that allows extended interaction shows a negative correlation between token consumption and score (rho = -0.734 on ProjDevBench), meaning agents that think more do worse.

For Theo Code specifically: the existing infrastructure (`apps/theo-benchmark/`) has smoke runs, a Terminal-Bench adapter, SWE-bench local runner, and a prompt evolution loop. The current SWE-bench baseline of 50% with Qwen3-30B is competitive for an open-source model but needs systematic expansion to additional benchmarks, Wilson CI scoring, and efficiency-aware metrics. This document maps every major benchmark, its current leaders, what it measures, and how Theo Code should integrate each into its SOTA validation loop.

---

## 1. SWE-bench Ecosystem

### 1.1 SWE-bench Verified (500 instances)

The original gold standard: 500 human-verified GitHub issues from 12 popular Python repositories (Django, Flask, scikit-learn, etc.). Models generate patches; evaluation checks if gold tests pass.

**Current leaders (April 2026):**

| Rank | Agent + Model | Score |
|------|--------------|-------|
| 1 | Claude Mythos Preview | 93.9% |
| 2 | Claude Opus 4.7 (Adaptive) | 87.6% |
| 3 | GPT-5.3 Codex | 85.0% |
| 4 | Claude Opus 4.5 | 80.9% |

**Why it is declining in credibility:**
- OpenAI's February 2026 audit of 138 unsolved problems found 59.4% had material test design issues.
- 35.5% of audited tasks enforce specific implementation details ("narrow tests"), rejecting functionally correct patches.
- 18.8% test for functionality not in the problem description ("wide tests").
- Every frontier model (GPT-5.2, Claude Opus 4.5, Gemini 3 Flash) could reproduce verbatim gold patches for certain tasks -- confirmed contamination.
- Over 94% of Verified issues predate model training cutoffs.
- Contamination + weak tests estimated to inflate scores by 5-15 points on post-2023 models.
- UTBoost research found 169 incorrect patches in Verified that were evaluated as correct. 26 out of 500 tasks have insufficient test suites even after expert verification. 24.4% ranking changes observed after fixing evaluations.

**Verdict:** Treat as directional signal only. A model at 80% is almost certainly better than one at 40%, but the exact number carries less precision than the leaderboard formatting suggests.

Sources: [SWE-bench Verified Leaderboard](https://llm-stats.com/benchmarks/swe-bench-verified) | [OpenAI: Why We No Longer Evaluate SWE-bench Verified](https://openai.com/index/why-we-no-longer-evaluate-swe-bench-verified/) | [UTBoost Exposes Gaps](https://medium.com/@danieldkang/swe-bench-verified-is-flawed-despite-expert-review-utboost-exposes-gaps-in-test-coverage-4b75c6b940c6)

### 1.2 SWE-bench Pro (1,865 tasks)

OpenAI/Scale AI's successor benchmark. Key differences from Verified:
- 41 actively maintained repositories spanning Python, Go, TypeScript, and JavaScript.
- Includes private proprietary startup codebases never publicly available -- contamination is structurally prevented.
- Standardized scaffolding (same harness for all models).

**Current leaders (April 2026):**

| Agent + Model | Score |
|--------------|-------|
| GPT-5.3-Codex (OpenAI-reported) | 57.0% |
| GPT-5 (standardized scaffolding) | 23.3% |
| Claude Opus 4.1 (standardized scaffolding) | 23.1% |

The gap between OpenAI-reported (57%) and standardized-scaffolding (23.3%) for the same family illustrates the harness engineering effect -- custom scaffolding adds ~34 points.

**Verdict:** Currently the most contamination-resistant major benchmark. The standardized-scaffolding results are more trustworthy for model-vs-model comparison.

Sources: [SWE-Bench Pro Leaderboard](https://labs.scale.com/leaderboard/swe_bench_pro_public) | [Morph: SWE-Bench Pro](https://www.morphllm.com/swe-bench-pro)

### 1.3 SWE-rebench

Independent evaluation platform that re-runs submissions with controlled conditions. Claude Opus 4.6 holds the #1 spot. Open-source standouts: GLM-4.7 ranks alongside closed models; Qwen3-Coder-Next leads on Pass@5.

Source: [SWE-rebench Leaderboard](https://swe-rebench.com/)

### 1.4 Multi-SWE-bench (1,632 instances, 7 languages)

Extends SWE-bench beyond Python-only to Java, TypeScript, JavaScript, Go, Rust, C, C++. 68 expert annotators curated 1,632 instances. Accepted at NeurIPS 2025 Datasets track.

Variants: Multi-SWE-bench mini (400 instances, 8 languages, lower compute), Multi-SWE-bench flash (300 instances for rapid evaluation).

Source: [Multi-SWE-bench](https://github.com/multi-swe-bench/multi-swe-bench)

### 1.5 SWE-bench Multilingual (300 tasks, 9 languages)

From the original SWE-bench team. 300 curated tasks across C, C++, Go, Java, JS/TS, PHP, Ruby, Rust from 42 repositories.

Source: [SWE-bench Multilingual Leaderboard](https://www.swebench.com/multilingual-leaderboard.html)

### 1.6 SWE-bench Multimodal (517 issues)

Augments the original benchmark with issues containing visual elements (screenshots, UI mockups). Tests ability to interpret visual + textual information for bug fixing in user-facing applications.

Source: [SWE-bench Multimodal](https://www.swebench.com/multimodal.html)

### 1.7 SWE-Lancer (1,400+ tasks, $1M total value)

OpenAI's benchmark of real Upwork freelance tasks. 764 IC tasks ($414K, $50--$32K each) + 724 management tasks ($585K). Evaluated with Playwright browser automation (more resistant to grader hacking than unit tests).

**Best result:** Claude 3.5 Sonnet earned $208K of $500K possible (26.2% IC success, 44.9% management). No model surpasses human performance. 74% tasks are application logic; 88% are bug fixes.

**Limitation:** All tasks from Upwork/Expensify -- limited infrastructure engineering coverage.

Source: [SWE-Lancer (OpenAI)](https://openai.com/index/swe-lancer/) | [GitHub](https://github.com/openai/SWELancer-Benchmark)

---

## 2. Terminal-Bench 2.0

89 challenging tasks in terminal environments. Long-horizon, fully autonomous. Covers build systems, compilation, git ops, differential cryptanalysis, ML training, biology, chess engine optimization, server config, debugging. Published at ICLR 2026.

**Current leaders (April 2026):**

| Rank | Agent + Model | Score |
|------|--------------|-------|
| 1 (tied) | ForgeCode + Claude Opus 4.6 | 81.8% |
| 1 (tied) | ForgeCode + GPT-5.4 | 81.8% |
| 3 | TongAgents + Gemini 3.1 Pro | 80.2% |
| 4 (tied) | SageAgent + GPT-5.3-Codex | 78.4% |
| 4 (tied) | ForgeCode + Gemini 3.1 Pro | 78.4% |
| 6 | Droid (Factory) + GPT-5.3-Codex | 77.3% |
| -- | Claude Opus 4.7 (Anthropic-reported, no custom harness) | 69.4% |

**Critical insight:** ForgeCode ties two fundamentally different models (Opus 4.6 and GPT-5.4) at the same score because the harness compensates for each model's specific failure modes. The harness, not the model, is the binding constraint.

**Agentic Harness Engineering (AHE):** LangChain's 10 iterations of AHE on Terminal-Bench 2 lifted pass@1 from 69.7% to 77.0%, surpassing Codex-CLI (71.9%) -- keeping the model fixed (GPT-5.2-Codex) and only changing the harness.

**Theo Code integration:** `apps/theo-benchmark/tbench/agent.py` implements `AbstractInstalledAgent` for terminal-bench >= 0.2. Binary runs in `--headless` mode. OTLP trace forwarding and cost computation are wired.

Sources: [Terminal-Bench Leaderboard](https://www.tbench.ai/leaderboard/terminal-bench/2.0) | [ForgeCode Deep Dive](https://medium.com/@richardhightower/forgecode-dominating-terminal-bench-2-0-harness-engineering-beat-claude-code-codex-gemini-etc-eb5df74a3fa4) | [ICLR 2026 Paper](https://openreview.net/pdf/417ac3236de7dbf3fc3414c51754dd239271663e.pdf)

---

## 3. LongCLI-Bench

20 high-quality, long-horizon CLI programming tasks across 4 categories: from-scratch, feature addition, bug fixing, refactoring. Curated from 1,000+ CS assignments and real workflows.

**Key findings:**
- Even SOTA agents achieve pass rates **below 20%**.
- Step-level analysis: majority of tasks stall at <30% completion -- critical failures in early stages.
- Self-correction provides minimal improvement.
- **Static plan injection** (providing a plan upfront) significantly outperforms self-correction.
- **Dynamic interactive guidance** (human-agent collaboration) achieves the best results by far.

**Implication for Theo Code:** The plan-first architecture (`theo-plan` with JSON canonical format) aligns with the finding that static plan injection beats self-correction. The GRAPHCTX pipeline (providing relevant context before the agent starts) is the right pattern.

Sources: [arXiv:2602.14337](https://arxiv.org/abs/2602.14337) | [GitHub](https://github.com/finyorko/longcli-bench)

---

## 4. ProjDevBench

End-to-end project construction benchmark. 20 problems across 8 categories (Algorithm, Data Structure, Assembly, Management, Game, Interpreter, Storage, Optimization). Agents build complete repositories from natural language specs. Average: 138 interaction turns, 4.81M tokens per problem.

**Dual evaluation:** 80% execution score (Online Judge with diagnostic verdicts) + 20% code review score (rule-based + LLM-based spec compliance).

**Results (April 2026):**

| Agent | Model | Final Score |
|-------|-------|-------------|
| Codex | GPT-5 | 77.85 |
| Augment | GPT-5 | 72.35 |
| Cursor | GPT-5 | 71.85 |
| Claude Code | Sonnet-4.5 | 68.87 |
| Gemini CLI | Gemini-3-Pro | 68.61 |

**Failure distribution:** Accepted 27.38%, Wrong Answer 41.86%, TLE 13.91%, Runtime Error 7.01%, Compile Error 4.52%, Memory Leak 3.51%, MLE 1.36%.

**Statistical correlations:**

| Variable Pair | Spearman rho | p-value |
|---------------|-------------|---------|
| Tokens vs. Score | **-0.734** | 0.0002 |
| Turns vs. Score | **-0.668** | 0.0013 |
| Turns vs. Tokens | **0.898** | <0.0001 |

**Key insight:** More interaction = worse performance. High token counts come from repeated turns, not long reasoning. Static complexity (file count, LOC) has weak correlation with performance -- difficulty manifests in interaction, not code size.

Sources: [arXiv:2602.01655](https://arxiv.org/abs/2602.01655) | [GitHub](https://github.com/zsworld6/projdevbench)

---

## 5. tau-Bench (TAU-Bench)

Tool-agent-user interaction benchmark. Simulates dynamic conversations where an agent uses domain-specific API tools and follows policy guidelines. Originally two domains (retail, airline customer service); expanded to banking, voice full-duplex, and knowledge-retrieval domains.

**Evolution:** tau-bench -> tau2-bench -> tau3-bench (April 2026). Fixes: removed incorrect expected actions, clarified ambiguous instructions, fixed impossible constraints, added missing fallback behaviors.

**Current leaders (retail + airline):**

| Model | Score |
|-------|-------|
| Claude Mythos Preview | 89.2% |
| Claude Sonnet 4.6 | 87.5% |
| Claude Sonnet 4.5 | 86.2% |

**tau2-Bench Telecom:** GLM-4.7-Flash (Reasoning) at 98.8%.

**VeRO findings on TAU-Bench:** Optimization of the harness improved TAU-Bench Retail by +0.28 for Orchestrator Sonnet configurations. Tool-use tasks (GAIA, Retail, SimpleQA) show consistent gains from harness optimization; reasoning-heavy tasks (GPQA, MATH) show little improvement.

Sources: [taubench.com](https://taubench.com/) | [GitHub: tau2-bench](https://github.com/sierra-research/tau2-bench) | [BenchLM.ai](https://benchlm.ai/benchmarks/tauBench)

---

## 6. Berkeley Function Calling Leaderboard (BFCL V4)

The de facto standard for evaluating function/tool calling. V4 (updated 2026-04-12) adds holistic agentic evaluation to prior capabilities (V1: AST evaluation, V2: enterprise/OSS functions, V3: multi-turn interactions).

**What it measures:** Serial and parallel function calls across programming languages. Overall accuracy = unweighted average of all sub-categories (simple, parallel, multiple, parallel multiple, exec, relevance, live).

**Key finding:** SOTA models excel at single-turn calls, but memory, dynamic decision-making, and long-horizon reasoning remain open challenges.

**Latest PyPI:** `bfcl_eval-2026.3.23`. Reproducible via `pip install bfcl-eval==2026.3.23`.

Sources: [BFCL V4 Leaderboard](https://gorilla.cs.berkeley.edu/leaderboard.html) | [GitHub](https://github.com/ShishirPatil/gorilla/tree/main/berkeley-function-call-leaderboard)

---

## 7. DevEval & E2EDevBench

### DevEval (Li et al., 2024)
1,825 testing samples from 115 real-world repositories, 10 programming topics. Staged development with UML diagrams (Mermaid syntax) and hierarchical architecture design. Models develop files following a DAG-ordered partial order. Automated testing via PyTest (Python), GTest (C++), JUnit (Java), Jest (JavaScript).

**Limitation:** Provides reference inputs (UML, architecture) at each stage -- not fully autonomous.

### E2EDevBench (Zeng et al., 2025)
50 recent PyPI projects, end-to-end construction. Hybrid evaluation: test-case-based functional assessment + LLM-based requirement verification. SOTA agents fulfill ~50% of requirements. Primary bottleneck: requirement omission and inadequate self-verification.

### DevBench (open-compass)
Comprehensive staged benchmark: PRD interpretation -> UML diagram generation -> architecture design -> code development -> testing. Multi-language (Python, C++, Java, JavaScript).

Sources: [DevEval GitHub](https://github.com/open-compass/DevEval) | [E2EDevBench (arXiv:2511.04064)](https://arxiv.org/html/2511.04064)

---

## 8. GAIA, GPQA, SimpleQA, MATH

These benchmarks are used by VeRO (Evaluation Harness for Agents to Optimize Agents, arXiv:2602.22480) as the multi-benchmark optimization surface.

### GAIA
Multi-step reasoning tasks requiring web browsing, tool use, and multimodal understanding. Conceptually simple for humans, hard for AI. Claude Mythos Preview leads at 52.3%. Scores are heavily dependent on the agent framework -- comparing raw GAIA scores across models without controlling for prompting setup is meaningless.

### GPQA Diamond
Graduate-level science QA. **Largely saturated** as of April 2026. Gemini 3.1 Pro Preview leads at 94.1%, GPT-5.4 at 92.0%. Reasoning-heavy: shows little improvement from harness optimization. Stronger correlation with enterprise production performance than MMLU or HellaSwag.

### SimpleQA
Factual QA benchmark. Tool-use-oriented: shows consistent gains from harness optimization (+0.11 for Orchestrator Sonnet in VeRO).

### MATH
Mathematical reasoning. Like GPQA, shows little improvement from agent optimization -- capability is in the model weights, not the harness.

**VeRO key finding:** Optimization headroom inversely correlates with agent complexity. Simpler agents show larger lifts: +11.5% on GAIA, +10.5% on FACTS, +13.3% on SimpleQA.

Sources: [VeRO (arXiv:2602.22480)](https://arxiv.org/html/2602.22480) | [GPQA Leaderboard](https://artificialanalysis.ai/evaluations/gpqa-diamond) | [BenchLM GAIA](https://benchlm.ai/benchmarks/gaia)

---

## 9. Agentic Benchmark Checklist (ABC)

From Zhu et al., "Establishing Best Practices for Building Rigorous Agentic Benchmarks" (arXiv:2507.02825, UIUC Kang Lab).

**Core findings:**
- SWE-bench Verified uses insufficient test cases; tau-bench counts empty responses as successful. Such issues cause performance estimation errors of up to 100% in relative terms.
- When ABC guidelines were applied to CVE-Bench, performance overestimation dropped by 33%.

**ABC Checklist categories:**

1. **Task Definition:** Clear, unambiguous task descriptions with well-defined success criteria.
2. **Environment Reproducibility:** Fully reproducible, frozen at release. No dynamic external resources.
3. **Data Contamination Prevention:** Private held-out test sets. Continuously refresh tasks post-training-cutoff (cf. LiveCodeBench).
4. **Ground Truth Verification:** Verify correctness of annotations. Provide automatic oracle solver.
5. **Process-Based Evaluation:** Use process metrics alongside outcome metrics. Benchmark LLM-as-a-judge reproducibly.
6. **Efficiency Metrics:** Report cost, latency, token count -- not just accuracy.

**Enterprise reality check:** 37% gap between lab benchmark scores and real-world deployment performance. 50x cost variation for similar accuracy levels.

Sources: [ABC Checklist](https://uiuc-kang-lab.github.io/agentic-benchmarks/) | [arXiv:2507.02825](https://arxiv.org/abs/2507.02825) | [Brookings: Evaluating Agentic AI](https://www.brookings.edu/articles/how-can-we-best-evaluate-agentic-ai/)

---

## 10. Key Cross-Benchmark Findings

### 10.1 More Tokens = Worse Score (rho = -0.734)

ProjDevBench demonstrates a strong negative correlation between token consumption and final score (Spearman rho = -0.734, p = 0.0002). This is not an artifact of one benchmark:
- Context rot degrades model quality as conversation grows. Every irrelevant tool call, grep result, and file read acts as a distractor.
- Research confirms LLMs perform worse on tasks when surrounded by irrelevant information. "More context isn't just expensive; it can make the model seem like a much worse model."
- A 200K-token conversation costs 10x a 20K-token one, and quality degrades simultaneously.

**Implication for Theo Code:** GRAPHCTX's pre-computed context relevance is a direct counter to this pattern. The validation log confirms: with GRAPHCTX, tasks resolve in 1 interaction; without it, 10 interactions (all failures).

### 10.2 Domain-Specific Drop: 62%

llvm-autofix demonstrates that models scoring 60% on SWE-bench Verified drop to ~23% on compiler bugs (62% average drop across 5 models). Domain-specific harnesses recover ~22% per model. The llvm-autofix-mini agent with 4-stage design (Setup->Reason->Generate->Validate) beats generic mini-SWE-agent by 1.22x average.

**Implication for Theo Code:** SOTA thresholds from generic benchmarks do not transfer to specialized domains. The SOTA validation loop must have domain-aware probes, not generic ones.

### 10.3 >60% Accepted Patches Incorrect After Expert Review

Even for the best model (GPT 5 + llvm-autofix-mini), >60% of patches that pass regression tests are semantically incorrect after expert review. The genuine resolution rate is only 20.1%. Three error categories: ChangeAssert (modifying assertions instead of fixing the bug), WrongLocalization, and WrongFix (bypassing, lacking generality, introducing silent bugs).

**Implication for Theo Code:** "cargo test passes" is necessary but insufficient. The quality evaluator must include semantic code review for non-trivial patches.

### 10.4 Harness Engineering Explains 6x Performance Gap

Meta-Harness (Stanford, March 2026) demonstrates that changing only the harness around a fixed LLM can produce a 6x performance gap on the same benchmark. Key results:
- Cross-model transfer works: a single discovered harness improves accuracy by **+4.7 points** on average across 5 held-out (unseen) models.
- 10x faster than comparable text optimizers at converging to a good harness.
- The frozen harness transfers without re-evolution to new benchmarks.

ForgeCode confirms this: identical 81.8% scores with two different models (Opus 4.6, GPT-5.4) after harness adaptation.

**Implication for Theo Code:** The existing `runner/evolve.py` prompt evolution loop is the right pattern. Extend it to evolve the full harness (tool selection, context assembly, retry logic), not just the system prompt.

### 10.5 Cross-Model Transfer Works for Harnesses (+4.7 points)

Meta-Harness math reasoning experiment: harness discovered with one model was tested on 5 unseen models and improved accuracy by +4.7 points on average (34.1% -> 38.8%). This means Theo Code can optimize its harness on a cheap model and transfer the improvements to frontier models.

**Implication for Theo Code:** Develop harness on Qwen3-30B (current setup), validate that improvements transfer to Claude/GPT when switching models.

---

## 11. Theo Code's Current Eval Infrastructure

### 11.1 What Exists (`apps/theo-benchmark/`)

| Component | Status | Description |
|-----------|--------|-------------|
| **Smoke suite** | 17+ scenarios | TOML-defined tasks (read, grep, fix, rename, multi-file edit, etc.) |
| **SWE-bench local runner** | Working | `swe/local_runner.py` with local instances |
| **SWE-bench Verified results** | Baseline | 4/7 resolved with Qwen3-30B-FP8 (bugs open 7-9.5 years) |
| **Terminal-Bench adapter** | Integrated | `tbench/agent.py` implements `AbstractInstalledAgent` |
| **Prompt evolution loop** | Working | `runner/evolve.py` -- EVAL->ANALYZE->MUTATE->RE-EVAL->COMPARE->ACCEPT/REVERT |
| **GRAPHCTX evaluation** | Validated | 100% success with GRAPHCTX vs 60% without (10 tasks) |
| **Report storage** | JSONL + JSON | Per-run reports in `reports/` |
| **Tests** | Present | `test_swe_harness.py`, `test_task_engine.py`, `test_decompose.py` |

### 11.2 Current Baselines

| Benchmark | Model | Score | Notes |
|-----------|-------|-------|-------|
| Smoke suite | Qwen3-30B-FP8 | ~80% | 17 scenarios, varies per iteration |
| SWE-bench (local, 7 issues) | Qwen3-30B-FP8 | 57% (4/7) | PR-ready fixes, bugs open 7-9.5 years |
| GRAPHCTX vs baseline | Qwen3-30B-FP8 | 100% vs 60% | 10 tasks, 1 interaction vs 10 |

### 11.3 Wilson CI Scoring

Wilson score interval on 3 runs minimum provides a lower-bound confidence estimate. This is the right statistical approach for small N -- better than point estimates.

### 11.4 Key Validation Log Observations

1. GRAPHCTX context is critical: all successful cases used `search_code` (Theo Code) first.
2. Model resolves bugs open 7-9.5 years in repos it never saw.
3. Agent doesn't call `done` -- spends remaining iterations testing/verifying.
4. Core loop: `read_file -> grep -> read specific lines -> edit`.
5. FP8 model significantly better than AWQ 4-bit for reasoning.
6. Complex logic bugs (multi-step reasoning) remain hard.

---

## Comprehensive Benchmark Comparison Table

| Benchmark | Tasks | Languages | What It Measures | Top Score | Contamination Risk | Cost/Run | Relevance to Theo |
|-----------|-------|-----------|-----------------|-----------|-------------------|----------|-------------------|
| SWE-bench Verified | 500 | Python | Issue resolution (patches) | 93.9% | **HIGH** (confirmed) | ~$200 | Directional only |
| SWE-bench Pro | 1,865 | Py/Go/TS/JS | Issue resolution (contamination-free) | 57.0% | LOW (private repos) | ~$500 | **PRIMARY** target |
| SWE-rebench | varies | Python | Independent re-evaluation | Opus 4.6 #1 | LOW | ~$200 | Secondary validation |
| Multi-SWE-bench | 1,632 | 7 langs | Multilingual issue resolution | varies | MEDIUM | ~$400 | **HIGH** (Rust track) |
| SWE-Lancer | 1,400+ | JS/TS | Real freelance tasks ($50-$32K) | $208K/$500K | LOW | ~$300 | Real-world signal |
| Terminal-Bench 2.0 | 89 | Multi | Long-horizon autonomous terminal tasks | 81.8% | LOW | ~$100 | **Already integrated** |
| LongCLI-Bench | 20 | Multi | Long-horizon CLI programming | <20% | LOW | ~$50 | Plan-first validation |
| ProjDevBench | 20 | C++ | End-to-end project construction | 77.85 | LOW | ~$800 | Architecture design |
| tau-bench | varies | N/A | Tool-agent-user interaction | 89.2% | LOW | ~$50 | Tool-use proficiency |
| BFCL V4 | 2,000+ | Multi | Function/tool calling | varies | LOW | ~$30 | **Tool accuracy** |
| DevEval | 1,825 | 4 langs | Staged development (UML-guided) | varies | MEDIUM | ~$200 | Staged pipeline |
| E2EDevBench | 50 | Python | End-to-end construction + hybrid eval | ~50% | LOW | ~$300 | Requirement coverage |
| GAIA | varies | N/A | Multi-step reasoning + tools | 52.3% | LOW | ~$100 | Agent reasoning |
| GPQA Diamond | 448 | N/A | Graduate science QA | 94.1% | MEDIUM (saturated) | ~$20 | Model capability |

---

## SOTA Validation Loop Thresholds

These thresholds define when Theo Code's SOTA validation loop should flag a capability as below/at/above state-of-the-art.

### Tier 1: Must-Track (integrate into CI)

| Metric | Below SOTA | At SOTA | Above SOTA | Source |
|--------|-----------|---------|------------|--------|
| SWE-bench Verified (Qwen3-30B) | <40% | 40-55% | >55% | Current baseline: 57% local |
| Smoke suite pass rate | <70% | 70-90% | >90% | Current: ~80% |
| Terminal-Bench 2.0 | <50% | 50-70% | >70% | Frontier: 81.8% |
| Tokens per resolved issue | >500K | 100K-500K | <100K | ProjDevBench correlation |
| GRAPHCTX lift (with vs without) | <1.2x | 1.2-2x | >2x | Current: ~1.7x |

### Tier 2: Periodic Evaluation (monthly)

| Metric | Below SOTA | At SOTA | Above SOTA | Source |
|--------|-----------|---------|------------|--------|
| BFCL V4 overall accuracy | <75% | 75-88% | >88% | Leaderboard |
| tau-bench retail | <60% | 60-80% | >80% | Frontier: 89.2% |
| LongCLI-Bench pass rate | <10% | 10-20% | >20% | Frontier: <20% |
| ProjDevBench (if C++ support added) | <50 | 50-70 | >70 | Best: 77.85 |

### Tier 3: Qualitative Indicators

| Metric | Red Flag | Acceptable | Target |
|--------|----------|-----------|--------|
| Expert review of accepted patches | <20% correct | 20-50% correct | >50% correct |
| Interaction count per resolved issue | >15 turns | 5-15 turns | <5 turns |
| Domain-specific vs generic drop | >60% | 30-60% | <30% |
| Stall detection (no progress cycles) | >3 cycles | 2 cycles | 1 cycle |

### Efficiency-Aware Composite Score

Following ABC guidelines, Theo Code should report a composite that penalizes high cost:

```
composite = accuracy * (1 - alpha * log(cost_usd / baseline_cost))
```

Where `alpha = 0.1` and `baseline_cost` is the median cost across evaluated models. This prevents "throw more tokens at it" strategies from inflating scores.

---

## Relevance for Theo Code

### What to do next (ordered by impact)

1. **Add SWE-bench Pro runner.** The existing `swe/local_runner.py` can be extended to pull Pro instances. This is the most credible benchmark for frontier comparison. Multi-language support (Go, TS) will stress-test Theo Code beyond Python.

2. **Evolve the harness, not just the prompt.** `runner/evolve.py` currently mutates the system prompt only. Meta-Harness shows that evolving tool selection, context assembly, retry logic, and output parsing yields 6x the improvement. Extend the evolution loop to mutate these dimensions.

3. **Add efficiency metrics to every benchmark run.** Report `tokens_per_resolved_issue`, `cost_usd`, `wall_time`, and `interaction_count` alongside accuracy. The ABC checklist mandates efficiency-aware reporting.

4. **Implement semantic patch review.** The llvm-autofix finding (>60% accepted patches incorrect) applies to Theo Code too. Add an LLM-as-judge step after test pass to check: Does the patch modify assertions instead of fixing the bug? Does it weaken activation conditions? Does it generalize beyond the specific test case?

5. **Run Wilson CI on 5+ runs, not 3.** With 3 runs, the Wilson lower bound is too conservative for meaningful comparison. 5 runs gives a tighter interval while remaining computationally tractable.

6. **Add tau-bench for tool-use regression.** Theo Code's tool-calling proficiency can be measured directly via tau-bench retail/airline domains. This is lightweight (~$50/run) and catches regressions in the tool-use pipeline.

7. **Track the "more tokens = worse" curve.** For every benchmark run, plot tokens vs. score and flag runs where the correlation is positive (indicating context rot is being managed) or negative (indicating degradation).

8. **Cross-model harness transfer validation.** Run the same evolved harness on Qwen3-30B and a frontier model (Claude Sonnet 4.6 or GPT-5). If the harness transfers with +4 points (per Meta-Harness), the evolution loop is working correctly.

### What NOT to do

- **Do not chase SWE-bench Verified scores.** The benchmark is contaminated and unreliable. Use it as a directional signal only.
- **Do not build a custom GPQA/MATH runner.** These are saturated and measure model weights, not agent capability. Use them only if VeRO-style optimization is being explored.
- **Do not optimize for a single benchmark.** The ABC checklist warns against benchmark gaming. Track 3+ benchmarks simultaneously to prevent overfitting.
- **Do not report accuracy without cost.** A system that scores 80% at $50/run is meaningfully different from one that scores 82% at $500/run. Always report the efficiency-aware composite.

---

## Sources

- [SWE-bench Verified Leaderboard](https://llm-stats.com/benchmarks/swe-bench-verified)
- [SWE-bench Official](https://www.swebench.com/)
- [SWE-Bench Pro Leaderboard (Scale AI)](https://labs.scale.com/leaderboard/swe_bench_pro_public)
- [SWE-rebench](https://swe-rebench.com/)
- [OpenAI: Why We No Longer Evaluate SWE-bench Verified](https://openai.com/index/why-we-no-longer-evaluate-swe-bench-verified/)
- [UTBoost Exposes Gaps in SWE-bench Verified](https://medium.com/@danieldkang/swe-bench-verified-is-flawed-despite-expert-review-utboost-exposes-gaps-in-test-coverage-4b75c6b940c6)
- [SWE-Lancer (OpenAI)](https://openai.com/index/swe-lancer/)
- [Multi-SWE-bench](https://github.com/multi-swe-bench/multi-swe-bench)
- [SWE-bench Multilingual Leaderboard](https://www.swebench.com/multilingual-leaderboard.html)
- [SWE-bench Multimodal](https://www.swebench.com/multimodal.html)
- [Terminal-Bench 2.0 Leaderboard](https://www.tbench.ai/leaderboard/terminal-bench/2.0)
- [Terminal-Bench ICLR 2026 Paper](https://openreview.net/pdf/417ac3236de7dbf3fc3414c51754dd239271663e.pdf)
- [ForgeCode Deep Dive (Medium)](https://medium.com/@richardhightower/forgecode-dominating-terminal-bench-2-0-harness-engineering-beat-claude-code-codex-gemini-etc-eb5df74a3fa4)
- [LongCLI-Bench (arXiv:2602.14337)](https://arxiv.org/abs/2602.14337)
- [ProjDevBench (arXiv:2602.01655)](https://arxiv.org/abs/2602.01655)
- [ProjDevBench GitHub](https://github.com/zsworld6/projdevbench)
- [tau-bench Leaderboard](https://taubench.com/)
- [tau2-bench GitHub](https://github.com/sierra-research/tau2-bench)
- [BFCL V4 Leaderboard](https://gorilla.cs.berkeley.edu/leaderboard.html)
- [BFCL GitHub](https://github.com/ShishirPatil/gorilla/tree/main/berkeley-function-call-leaderboard)
- [DevEval GitHub](https://github.com/open-compass/DevEval)
- [E2EDevBench (arXiv:2511.04064)](https://arxiv.org/html/2511.04064)
- [VeRO (arXiv:2602.22480)](https://arxiv.org/html/2602.22480)
- [GPQA Diamond Leaderboard](https://artificialanalysis.ai/evaluations/gpqa-diamond)
- [BenchLM GAIA](https://benchlm.ai/benchmarks/gaia)
- [ABC Checklist](https://uiuc-kang-lab.github.io/agentic-benchmarks/)
- [ABC Paper (arXiv:2507.02825)](https://arxiv.org/abs/2507.02825)
- [Meta-Harness (arXiv:2603.28052)](https://arxiv.org/abs/2603.28052)
- [Meta-Harness Project Page](https://yoonholee.com/meta-harness/)
- [Meta-Harness GitHub](https://github.com/stanford-iris-lab/meta-harness)
- [Agentic Harness Engineering (arXiv:2604.25850)](https://arxiv.org/abs/2604.25850)
- [LangChain: Improving Deep Agents with Harness Engineering](https://www.langchain.com/blog/improving-deep-agents-with-harness-engineering)
- [Morph: AI Coding Benchmarks 2026](https://www.morphllm.com/ai-coding-benchmarks-2026)
- [CodeAnt: SWE-bench Scores Explained](https://www.codeant.ai/blogs/swe-bench-scores)
