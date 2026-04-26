---
type: insight
topic: "infrastructure dominates AI logic in agent systems"
confidence: 0.95
impact: high
---

# Insight: 98.4% Infrastructure, 1.6% AI — The Real Engineering is Around the Loop

**Key finding:** In Claude Code's 512K-line codebase, only 1.6% is AI decision logic. The agent loop itself is a simple while-loop. The real complexity and differentiation lives in the deterministic infrastructure: permission systems, context compaction, tool routing, safety layers, and recovery logic.

**Evidence:** The arXiv paper "Dive into Claude Code" (2604.14228) systematically analyzed Claude Code v2.1.88 and found 7 safety layers, 5 compaction stages, 54 tools, 27 hook events, 4 extension mechanisms, and 7 permission modes. The paper identifies five human values (human decision authority, safety, reliable execution, capability amplification, contextual adaptability) that motivate thirteen design principles, traced to specific implementation choices. The industry maxim for 2026: "2025 was the year of agents. 2026 is the year of harnesses."

**Implication for Theo:** Theo's competitive advantage will not come from a better agent loop — the loop is trivial. It will come from better infrastructure: context compaction (Theo currently has none), permission granularity (Theo has CapabilityGate but no compaction), tool routing efficiency (Theo has basic routing), and safety layers (Theo has basic sandbox). Prioritize building the infrastructure around the loop, not optimizing the loop itself. This is consistent with GRAPHCTX as a differentiator — it's infrastructure that provides better context, not a smarter loop.
