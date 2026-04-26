---
type: insight
topic: "sub-agent orchestration pattern convergence"
confidence: 0.90
impact: high
---

# Insight: Orchestrator-Worker is the Dominant Production Pattern

**Key finding:** Every production AI coding assistant in 2026 uses the orchestrator-worker pattern (hub-and-spoke) as its primary multi-agent architecture. The router pattern alone handles an estimated 60% of real-world use cases.

**Evidence:** Claude Code, Codex CLI, Amazon Q, Cursor, Devin, Jules, and Theo Code all implement variations of one lead agent dispatching to specialized workers. The only divergence is in isolation granularity (context window vs. VM vs. worktree) and communication direction (return-only vs. shared state). No production coding agent uses pure peer-to-peer as its primary pattern. Academic frameworks (AutoGen GroupChat, Swarm) support peer patterns, but production systems converge on hierarchy.

**Implication for Theo:** Theo's `SubAgentManager` with role-based workers is architecturally aligned with the industry consensus. Do not invest in peer-to-peer or swarm patterns for the core product. Instead, invest in the quality of the orchestration: model routing per role (Priority 1), file locking (Priority 2), and structured result schemas (Priority 3). These are the differentiators within the dominant pattern.
