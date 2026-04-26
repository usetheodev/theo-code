---
type: insight
topic: "model routing for sub-agent cost optimization"
confidence: 0.92
impact: high
---

# Insight: Dual-Model Routing is a Proven Cost-Quality Multiplier

**Key finding:** Separating planning (expensive model) from execution (cheap model) across sub-agent roles delivers SOTA benchmark results at significantly lower cost. Anthropic's multi-agent research system with Opus lead + Sonnet workers outperforms single-agent Opus by 90.2%.

**Evidence:** Aider's Architect/Editor pattern (reasoning model describes solution, coding model generates edits) produces state-of-the-art benchmark results. Anthropic's internal research system uses Claude Opus 4 as lead and Claude Sonnet 4 as subagent workers. Multi-agent systems consume ~15x more tokens than standard chat, making model routing essential for cost control. Plan-and-Execute architectures achieve up to 92% task completion with 3.6x speedup over sequential ReAct.

**Implication for Theo:** Theo already has `SubAgentRoleId` mapped to routing slots in `theo-domain/src/routing.rs` and role-based routing in `theo-infra-llm/src/routing/`. The infrastructure exists to route Explorer and Reviewer roles to cheaper/faster models while keeping Implementer on the primary model. This is the highest-ROI improvement available: it reduces cost per sub-agent interaction while potentially improving quality through specialization.
