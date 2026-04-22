---
type: insight
topic: "depth over breadth validation"
confidence: 0.88
impact: high
---

# Insight: "Depth > Breadth" Is Validated With a Ceiling

**Key finding:** Production data from Databricks and Mem0 (2025-2026) validates quality-filtered memory over comprehensive memory for agent systems, but with diminishing returns at scale where breadth with good retrieval eventually wins.

**Evidence:**
- Databricks memory scaling experiments: LLM-judge-filtered memories produced better agent performance than unfiltered ingestion. Quote: "more memory does not automatically make an agent better: low-quality traces can teach the wrong lessons, and retrieval gets harder as the store grows" [databricks.com/blog/memory-scaling-ai-agents].
- Mem0 (arXiv:2504.19413): selective pipeline achieves 91% lower p95 latency (1.44s vs. 17.12s) with only 6% accuracy loss vs. full-context on LoCoMo benchmark.
- However, Databricks also found that MORE filtered memory continued to improve, surpassing expert baselines by ~5% in the limit. The ceiling is breadth-with-quality, not depth-alone.
- LinkedIn's Cognitive Memory Agent (InfoQ, April 2026) emphasizes distinguishing "meaningful insights" from "routine chatter" as the core design challenge.

**Implication for Theo:**
1. The 7-gate filtering (depth) is correct for MVP and aligns with production evidence.
2. The architecture should support scaling the store (breadth) while maintaining gates at write time. The plan already does this -- gates at write, budget at read.
3. The future risk is retrieval degradation as the store grows past ~1000 lessons. Plan for retrieval quality monitoring (frecency, gap #5 in the implementation doc) before this becomes a problem.
4. The per-source-type thresholds (code: 0.35, wiki: 0.50, reflection: 0.60) are an implicit "depth per type" calibration that no other system publishes. Document and publish the calibration methodology.
