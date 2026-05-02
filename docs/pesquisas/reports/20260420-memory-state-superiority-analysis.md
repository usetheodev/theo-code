---
type: report
question: "How does Theo's memory & state plan compare to state-of-the-art? What is novel, what is derivative, and what is missing?"
generated_at: 2026-04-20T22:30:00Z
confidence: 0.82
sources_used: 31
---

# Report: Memory & State Superiority Analysis

## Executive Summary

Theo's memory plan is **architecturally well-positioned** relative to the April 2026 SOTA. The 7-gate lesson filtering, retrieval budget packing, and Karpathy wiki compiler are **novel in combination** but individually draw from known patterns. Two features -- hypothesis tracking with Laplace smoothing and episode-based cross-session recall via Tantivy BM25 -- have **no direct precedent** in published coding agents and are candidates for a position paper. The plan's "depth > breadth" philosophy is validated by recent Databricks and Mem0 production data [S1, S2]. Three academic references are missing that should inform the design: MemArchitect (governance policies) [S3], Knowledge Objects (hash-addressed facts) [S4], and CodeTracer (hypothesis failure diagnosis) [S5]. Overall: the plan does not need architectural changes, but should absorb three refinements from these references.

---

## Feature-by-Feature Analysis

### 1. 7-Gate Lesson Filtering

**Position:** Derivative-with-novel-composition. No single system implements all 7 gates.

**Analysis:** The 7 gates (upper confidence bound, lower confidence bound, evidence count, empty content, semantic dedup, contradiction scan via Jaccard, quarantine window) are individually well-known. Confidence bounding appears in MemoryBank's forgetting curve [S6]. Evidence-count gating appears in MemCoder's "experience self-internalization" mechanism [S7]. Semantic dedup is standard in Mem0 [S2]. The **novel aspect** is the specific composition: the upper confidence bound (reject >= 0.95 as "hallucinating certainty") combined with the quarantine-before-promotion lifecycle has no direct analogue. Knowledge Objects [S4] achieve something similar via hash-addressed immutable tuples with O(1) retrieval, but without a quarantine stage -- they are "correct by construction" rather than "correct by observation over time." The Jaccard-based contradiction gate (gate 6) is a pragmatic stand-in; MemArchitect's "Consistency & Truth" domain [S3] uses a more principled "Triage & Bid" economy for conflict resolution. The plan's RM4-followup to replace Jaccard with embeddings+NLI is correct.

**Risks:**
- Jaccard at 0.70 threshold may miss semantic contradictions with different surface forms (e.g., "always run tests first" vs. "skip tests for hotfixes").
- 7-day quarantine is arbitrary; no empirical calibration cited.
- No published ablation exists showing which gates matter most.

**Recommendations:**
- Absorb the "Triage & Bid" pattern from MemArchitect [S3] for gate 6 (contradiction) when NLI is added.
- Run an ablation on the 7 gates against the theo-benchmark harness before publishing claims. This is publishable data.
- Consider Knowledge Objects [S4] for the quarantine-to-confirmed transition: a promoted lesson becomes a hash-addressed KO with O(1) lookup.

---

### 2. Hypothesis Tracking with Laplace Smoothing

**Position:** Genuinely novel for coding agents. No published system uses this.

**Analysis:** Extensive search found zero coding agents that use Laplace smoothing for hypothesis confidence tracking. CodeTracer [S5] is the closest work -- it tracks hypothesis failure in coding agent trajectories and identifies "evidence-to-action gaps" where agents retrieve relevant information but fail to act on it. However, CodeTracer is a **post-hoc diagnostic framework**, not a runtime confidence tracker. Laplace smoothing (additive smoothing, adding pseudocounts to avoid zero probabilities) is well-established in Bayesian classification [S8] but has not been applied to agent memory hypothesis management. The context manager evaluation [S9] identifies "hypothesis engine -- confidence scoring, competition, auto-pruning" as gap #1 for reaching 5/5. The Laplace approach would close this gap by providing a principled prior that decays toward uniform as evidence accumulates, avoiding both premature certainty and permanent uncertainty.

**Risks:**
- No empirical validation exists. The approach is theoretically sound but unproven in practice.
- The alpha parameter of Laplace smoothing needs calibration -- too high and hypotheses never converge; too low and they lock in early.
- Interaction with the 7-gate confidence bounds (0.60-0.95) needs careful analysis: does Laplace-smoothed confidence ever naturally reach those bounds?

**Recommendations:**
- This is **publishable material**. Design a controlled experiment: Laplace-tracked hypotheses vs. raw frequency counting vs. no hypothesis tracking, measured on the theo-benchmark suite.
- Start with alpha=1 (standard Laplace) and calibrate empirically.
- Wire into the existing `MemoryLesson` confidence field -- the hypothesis tracker feeds the lesson's confidence, which then passes through the 7 gates.

---

### 3. Memory Lifecycle Decay (Active -> Cooling -> Archived)

**Position:** MemGPT-inspired but materially different.

**Analysis:** MemGPT [S10] established the tiered memory model (core/archival/recall) but does **not** implement time-based decay between tiers. MemGPT's tiers are static: the agent explicitly moves items via tool calls. Theo's `MemoryLifecycleEnforcer::tick` with age_secs + usefulness + hit_count signals is closer to MemoryBank's Ebbinghaus forgetting curve [S6], but adds two innovations: (a) the `min_hits_to_stay_warm` shield that prevents useful-but-old items from decaying, and (b) the "never promotes backwards" invariant that makes decay unidirectional. MemArchitect [S3] independently arrived at a similar governance-layer approach, decoupling lifecycle management from model weights. The key difference: MemArchitect's policies are declarative rules; Theo's are imperative tick-based transitions. Both are valid; MemArchitect's is more extensible. MemOS [S11] adds "time-to-live or frequency-based decay" but does not publish concrete thresholds or transition logic.

**Risks:**
- The default thresholds (active_max_age: 2h, cooling_max_age: 7d) are not empirically calibrated for coding sessions. A developer may return to a topic after 3 hours -- should that episode have decayed?
- The "never promotes backwards" invariant means a valuable lesson that decayed to Archived cannot be revived by a hit. This may be too aggressive.

**Recommendations:**
- Add a `revive_on_hit` policy that allows Archived items to re-enter Cooling (not Active) on a retrieval hit. This is what MemoryBank does implicitly via its forgetting curve strengthening on access.
- Calibrate thresholds against real session data. The 2h active window seems short for coding; consider 4h or parameterize per-project.
- Cite MemArchitect [S3] as concurrent work with complementary design (declarative vs. imperative policies).

---

### 4. Frozen Snapshot Pattern

**Position:** Common pattern. Hermes and Claude Code both implement it.

**Analysis:** The frozen snapshot (inject memory at session start, mid-session writes update disk but not the system prompt) is a **well-established pattern** used by both Hermes [S12] and Claude Code [S13]. Hermes documents it explicitly: "The system prompt injection is captured once at session start and never changes mid-session. This is intentional -- it preserves the LLM's prefix cache for performance" [S12]. Claude Code's system prompt describes git status as "a snapshot in time, and will not update during the conversation" [S13]. The leaked source code revealed a KAIROS feature flag for background memory consolidation while idle [S13]. Theo's `BuiltinMemoryProvider` implements the same pattern (RM3a-AC-6: "Frozen snapshot preserva estado durante prefetch").

**Risks:**
- This is table stakes, not a differentiator.
- The tradeoff (stale context vs. cache performance) is well-understood but rarely quantified. No published data on how much prefix-cache savings justify stale memory.

**Recommendations:**
- No architectural change needed. The pattern is correct.
- Consider a "refresh on explicit request" mechanism -- if the user says "re-read your memory," update the snapshot mid-session. Hermes does not offer this; it could be a small UX win.

---

### 5. Retrieval Budget Packing (15% of context for memory)

**Position:** Aligned with Anthropic's context engineering best practices. Budget percentage is conservative.

**Analysis:** The emerging consensus in 2026 is that context windows are budgets, not buffers [S14]. Production agent systems allocate 30-40% to knowledge context, 20-30% to history, and 10-15% to buffer [S14]. Theo's 15% allocation for memory falls within the "buffer reserve" tier, which is conservative. Mem0 achieves 91% reduction in p95 latency vs. full-context by using selective retrieval [S2]. The `pack_within_budget` greedy packer (score-descending) in Theo matches the standard approach. The per-source-type thresholds (code: 0.35, wiki: 0.50, reflection: 0.60) are a **novel calibration** -- no other system publishes differentiated thresholds by memory source type.

**Risks:**
- 15% may be too low for projects with rich memory. If a user has 500 lessons and the budget only fits 20, the agent may miss critical context.
- The greedy packer is optimal only for items of roughly equal size. A single large wiki page could crowd out many small lessons.

**Recommendations:**
- Add a diversity constraint to the packer: at least 1 item from each source_type if available, before greedy fill. This prevents any single type from monopolizing the budget.
- Consider making the 15% dynamic based on task type: exploration tasks (user asking questions) get higher memory budget; execution tasks (apply this patch) get lower.
- The per-source-type thresholds are publishable as an engineering contribution. Document the calibration methodology.

---

### 6. Episode-Based Cross-Session Recall

**Position:** Hybrid approach (Tantivy BM25) is differentiated vs. both FTS5 (Hermes) and embedding-only (Mem0).

**Analysis:** Three approaches exist in the wild:

| System | Backend | Strengths | Weaknesses |
|---|---|---|---|
| Hermes [S12] | SQLite + FTS5 | Zero dependencies, fast exact match | No semantic similarity |
| Mem0 [S2] | Vector DB + BM25 hybrid | Best semantic coverage | Cloud dependency, latency |
| Engram [S15] | SQLite + FTS5 via MCP | Single binary, universal | Same as Hermes |
| **Theo** | Tantivy BM25 + namespace filter | Local, fast, typed by source | No dense embeddings yet |

Theo's choice of Tantivy over SQLite FTS5 is architecturally sound: Tantivy provides a Rust-native full-text engine with lower FFI overhead than SQLite bindings, plus native support for schema-typed fields (the `source_type` filter). The `source_type` namespace filter is a **differentiator** -- neither Hermes nor Engram filter by memory type at query time. Mem0 has metadata filtering but it operates at the vector DB level, not the full-text level.

The gap vs. Mem0 is dense embeddings. The plan defers this (gap #7: "Embeddings + NLI for contradiction polarity"). For cross-session recall, BM25 alone may miss paraphrased queries (user asks "how do we deploy" but the lesson says "production release process").

**Risks:**
- BM25-only recall will miss semantic similarity, especially for cross-session queries where terminology shifts over time.
- The Tantivy tokenizer (`memory_simple`: whitespace + lowercase, no stemmer) is very basic. A stemmer would improve recall for English queries.
- No frecency signal in retrieval ranking yet (gap #5 in the implementation doc).

**Recommendations:**
- Phase 1 (current): ship BM25-only. It covers exact match and keyword overlap.
- Phase 2: add Jina Code v2 embeddings (already used in `theo-engine-retrieval` for file search) as a second signal. Use RRF to combine BM25 + dense, mirroring the existing 3-ranker architecture.
- Add a stemmer to the Tantivy tokenizer. English stemming is a single-line config change.

---

## Questions Answered

### Q1: Has any feature been published/implemented in another agent?

| Feature | Prior Art? | Novelty Level |
|---|---|---|
| 7-gate lesson filtering | Individual gates exist in Mem0, MemoryBank, MemCoder. The 7-gate composition is novel. | **Derivative-novel** |
| Hypothesis tracking + Laplace | No precedent found in any coding agent. | **Novel** |
| Lifecycle decay (3-tier) | MemGPT tiers exist. Time-based decay exists in MemoryBank. The specific signal combination is novel. | **Derivative-novel** |
| Frozen snapshot | Hermes, Claude Code both do this. | **Common pattern** |
| Retrieval budget packing | Anthropic best practices. Per-source-type thresholds are novel. | **Standard + novel calibration** |
| Episode cross-session (Tantivy) | Hermes uses FTS5, Mem0 uses vector. Tantivy + namespace filter is novel. | **Derivative-novel** |

### Q2: Which features are genuinely publishable?

Two candidates for a position paper or systems paper:

1. **Hypothesis tracking with Laplace smoothing for agent memory confidence.** No prior art. Needs empirical validation on the benchmark harness.

2. **7-gate lesson filtering with ablation study.** The composition is novel; an ablation showing which gates contribute most to memory quality would be a solid contribution to the agent memory literature.

A third candidate if combined with benchmark data:

3. **Per-source-type retrieval thresholds for heterogeneous memory stores.** The calibration (code: 0.35, wiki: 0.50, reflection: 0.60) is a practical engineering contribution.

### Q3: Missing academic references?

Three references that should inform the design:

1. **MemArchitect** (arXiv:2603.18330) [S3] -- Policy-driven memory governance layer. Directly relevant to meta-memory policies. The "Triage & Bid" economy for context window competition is a more principled version of Theo's greedy packer.

2. **Knowledge Objects** (arXiv:2603.17781) [S4] -- Hash-addressed immutable fact tuples. Relevant to the lesson promotion lifecycle: once a lesson passes all 7 gates and exits quarantine, it could become a KO with O(1) retrieval (252x cheaper than in-context according to their benchmarks).

3. **CodeTracer** (arXiv:2604.11641) [S5] -- Traceable agent states and hypothesis failure diagnosis. Relevant to the hypothesis engine gap. Their finding of "evidence-to-action gaps" validates the need for a system that tracks whether retrieved memory actually influenced agent behavior.

Additionally recommended:
- **SSGM Framework** (arXiv:2603.11768) -- Stability and Safety Governed Memory. Risk taxonomy for evolving memory that should inform the security model.
- **"When to Forget"** (arXiv:2604.12007) -- Memory governance primitive for forgetting decisions. Directly relevant to the decay policy.

### Q4: Is "depth > breadth" validated by literature?

**Yes, with nuance.**

Databricks' memory scaling experiments [S1] showed that filtering by quality (LLM judge) produced better agents than ingesting everything. Their key finding: "more memory does not automatically make an agent better: low-quality traces can teach the wrong lessons, and retrieval gets harder as the store grows." This directly validates Theo's 7-gate filtering approach.

Mem0 [S2] demonstrated a 91% reduction in p95 latency with only a 6-percentage-point accuracy loss vs. full-context. The tradeoff is strongly in favor of selective memory for production systems.

However, Databricks also found that in the limit, more filtered memory continued to improve scores, surpassing expert-curated baselines by ~5%. This suggests that "depth > breadth" is correct **up to a point**, after which you want both depth AND breadth with good retrieval.

The implication for Theo: the 7-gate filtering is the right approach for the MVP. But the system should be designed to eventually scale the store (breadth) while maintaining quality gates (depth). The plan already does this -- gates filter at write time, retrieval filters at read time.

---

## Gaps in the Plan

1. **No explicit governance layer.** MemArchitect [S3] makes the case that lifecycle policies should be declarative and decoupled from model weights. Theo's policies are embedded in `MemoryLifecycleEnforcer::tick`. Consider extracting policies into a declarative config.

2. **No knowledge graph overlay.** Zep/Graphiti [S16] and Hindsight [S17] use entity resolution and temporal knowledge graphs. The plan explicitly defers this (non-goal: "Graphiti temporal KG integration"), which is correct for MVP, but the schema should leave room for entity IDs in lessons.

3. **No privacy/GDPR compliance mechanism.** MemArchitect explicitly addresses "Right-to-be-Forgotten" compliance. Theo's `.gitignore` + filesystem privacy is necessary but not sufficient for regulated environments. RM8+ territory.

4. **No evaluation benchmark for memory quality.** LoCoMo [S2] and DMR [S16] are the standard benchmarks. Theo should define its own benchmark or adapt one for coding-agent memory.

5. **No multi-agent memory isolation.** Gap #5 from the context manager evaluation [S9]. When sub-agents run, they need isolated WorkingSets but potentially shared LTM. Not addressed in the plan.

---

## Sources

- [S1] [Memory Scaling for AI Agents -- Databricks Blog](https://www.databricks.com/blog/memory-scaling-ai-agents)
- [S2] [Mem0: Building Production-Ready AI Agents with Scalable Long-Term Memory (arXiv:2504.19413)](https://arxiv.org/abs/2504.19413)
- [S3] [MemArchitect: A Policy Driven Memory Governance Layer (arXiv:2603.18330)](https://arxiv.org/abs/2603.18330)
- [S4] [Facts as First-Class Objects: Knowledge Objects for Persistent LLM Memory (arXiv:2603.17781)](https://arxiv.org/abs/2603.17781)
- [S5] [CodeTracer: Towards Traceable Agent States (arXiv:2604.11641)](https://arxiv.org/abs/2604.11641)
- [S6] [MemoryBank: Enhancing LLMs with Long-Term Memory (arXiv:2305.10250)](https://arxiv.org/abs/2305.10250)
- [S7] [MemCoder: Your Code Agent Can Grow Alongside You with Structured Memory (arXiv:2603.13258)](https://arxiv.org/abs/2603.13258)
- [S8] [Additive Smoothing -- Wikipedia](https://en.wikipedia.org/wiki/Additive_smoothing)
- [S9] Context Manager Evaluation 4.7/5 (internal: `.claude/projects/memory/context_manager_evaluation_47.md`)
- [S10] [MemGPT: Towards LLMs as Operating Systems (arXiv:2310.08560)](https://arxiv.org/abs/2310.08560)
- [S11] [MemOS: An Operating System for Memory-Augmented Generation (arXiv:2505.22101)](https://arxiv.org/pdf/2505.22101)
- [S12] Hermes Agent -- Persistent Memory docs (internal: `referencias/hermes-agent/website/docs/user-guide/features/memory.md`)
- [S13] [How Claude Code Builds a System Prompt -- Drew Breunig](https://www.dbreunig.com/2026/04/04/how-claude-code-builds-a-system-prompt.html)
- [S14] [Context Engineering for AI Agents: Token Economics -- Maxim](https://www.getmaxim.ai/articles/context-engineering-for-ai-agents-production-optimization-strategies/)
- [S15] [The Agent Memory Race of 2026 -- OSS Insight](https://ossinsight.io/blog/agent-memory-race-2026)
- [S16] [Zep: A Temporal Knowledge Graph Architecture for Agent Memory (arXiv:2501.13956)](https://arxiv.org/abs/2501.13956)
- [S17] Hindsight Memory Provider (internal: `referencias/hermes-agent/plugins/memory/hindsight/README.md`)
- [S18] [Governing Evolving Memory in LLM Agents: SSGM Framework (arXiv:2603.11768)](https://arxiv.org/abs/2603.11768)
- [S19] [When to Forget: A Memory Governance Primitive (arXiv:2604.12007)](https://arxiv.org/abs/2604.12007)
- [S20] [Claude Code Source Leak Analysis -- MindStudio](https://www.mindstudio.ai/blog/claude-code-source-leak-memory-architecture)
- [S21] Memory System implementation doc (internal: `docs/current/memory-system.md`)
- [S22] ADR-008 theo-infra-memory (internal: `docs/adr/008-theo-infra-memory.md`)
- [S23] Agent Memory Plan (internal: `outputs/agent-memory-plan.md`)
- [S24] Agent Memory SOTA Research (internal: `outputs/agent-memory-sota.md`)
- [S25] [Cognitive Architectures for Language Agents -- CoALA (arXiv:2309.02427)](https://arxiv.org/abs/2309.02427)
- [S26] [State of AI Agent Memory 2026 -- Mem0 Blog](https://mem0.ai/blog/state-of-ai-agent-memory-2026)
- [S27] [Hermes Agent Memory System Tutorial](https://www.ququ123.top/en/2026/04/hermes-memory-system-skill/)
- [S28] [Beyond the Context Window: Cost-Performance Analysis (arXiv:2603.04814)](https://arxiv.org/html/2603.04814v1)
- [S29] [Piebald-AI: Claude Code System Prompts](https://github.com/Piebald-AI/claude-code-system-prompts)
- [S30] [The Hidden Costs of Context: Managing Token Budgets -- TianPan.co](https://tianpan.co/blog/2025-11-11-managing-token-budgets-production-llm-systems)
- [S31] [LinkedIn Cognitive Memory Agent -- InfoQ](https://www.infoq.com/news/2026/04/linkedin-cognitive-memory-agent/)
