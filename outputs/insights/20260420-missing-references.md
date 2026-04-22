---
type: insight
topic: "missing academic references for memory plan"
confidence: 0.90
impact: medium
---

# Insight: Three Missing References Should Inform Memory Design

**Key finding:** Three papers published in March-April 2026 are directly relevant to Theo's memory plan but are not cited in either the SOTA research or the implementation doc. They do not require architectural changes but offer concrete refinements.

**Evidence:**

1. **MemArchitect** (arXiv:2603.18330, Arizona State University, March 2026) -- introduces a governance layer that decouples memory lifecycle from model weights via "Triage & Bid" economy. Four policy domains: Lifecycle & Hygiene, Consistency & Truth, Provenance & Trust, Efficiency & Safety. Directly relevant to Theo's meta-memory layer and to the contradiction gate (gate 6) where MemArchitect's conflict resolution is more principled than Jaccard similarity.

2. **Knowledge Objects** (arXiv:2603.17781, Zahn & Chana, March 2026) -- hash-addressed immutable tuples with O(1) retrieval. Benchmarks show 252x lower cost vs. in-context memory and 78.9% vs. 31.6% on multi-hop reasoning. Relevant to the lesson promotion lifecycle: a Confirmed lesson could become a KO for constant-time lookup instead of going through retrieval every time.

3. **CodeTracer** (arXiv:2604.11641, April 2026) -- traceable agent states, identifies "evidence-to-action gaps." Validates the hypothesis engine concept from a different angle: the problem is not just tracking confidence, but tracking whether retrieved memory actually influenced behavior.

**Implication for Theo:**
- Add these three papers to `outputs/agent-memory-sota.md` references section.
- MemArchitect's declarative policy model could evolve the imperative `MemoryLifecycleEnforcer::tick` into a more extensible system (post-MVP).
- Knowledge Objects pattern could be the graduation target for lessons that pass quarantine: hash-addressed, O(1), immutable.
- CodeTracer's "evidence-to-action gap" metric could feed into the `usefulness` score: a memory that is retrieved but never influences output has low causal usefulness (gap #3 from 4.7/5 evaluation).
