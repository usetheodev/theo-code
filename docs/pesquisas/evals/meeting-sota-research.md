---
type: report
question: "What would make Theo Code's planning system GENUINELY SOTA, not just better than markdown?"
generated_at: 2026-04-26T12:30:00-03:00
confidence: 0.82
sources_used: 18
meeting_id: 20260426-122956
---

# Report: SOTA Planning System for AI Coding Assistants

## Executive Summary

The competitive landscape for AI coding assistant planning is remarkably primitive -- every major tool (Claude Code, Cursor, Codex CLI, Aider) uses unstructured or semi-structured markdown with no validation, no execution semantics, and no learning. The proposed JSON canonical format with serde/schemars is a necessary foundation but insufficient alone. To reach genuine SOTA, Theo needs three innovations that no competitor has: (1) plan verification before execution using lightweight formal methods, (2) adaptive replanning with DAG mutation during execution, and (3) a plan corpus that improves future planning through pattern extraction. Academic research (GoalAct, ChatHTN, ALAS) confirms these are tractable problems with proven approaches.

## Analysis

### Finding 1: The Competitive Bar Is Extremely Low

Every competitor uses markdown files or in-memory state with no schema, no validation, and no execution semantics:

| Tool | Planning Approach | Validation | Adaptive | Learns |
|---|---|---|---|---|
| Claude Code | `.md` files | None | No | No |
| Cursor | `.md` + checkboxes | None | No | No |
| Codex CLI | `PLANS.md` sections | None | No | No |
| Aider | No persistent plans | N/A | No | No |
| SWE-agent | In-memory only | None | No | No |
| Manus | 3 markdown files | grep-based | Partial (PDCA) | No |
| SWE-AF | Runtime DAG | Partial | Yes (experimental) | No |

**Evidence:** Manus's approach (the most sophisticated competitor) uses grep-based checkbox checking on markdown files [[1]]. SWE-AF's DAG mutation is experimental and not persistent [[2]]. No tool has plan quality scoring or learning from past plans.

**Implication for Theo:** JSON canonical with schemars validation alone already surpasses every competitor. But "better than bad" is not SOTA.

### Finding 2: Academic SOTA Points to Three Key Capabilities

Recent research identifies three capabilities that separate toy planning from robust planning:

**2a. Plan Verification Before Execution**

ChatHTN (Munoz-Avila et al., May 2025) demonstrates that LLM-generated plans can be verified using symbolic methods with soundness guarantees [[3]]. The "Bridging LLM Planning & Formal Methods" paper converts plans to Kripke structures and LTL (Linear Temporal Logic), achieving F1=96.3% in plan correctness classification [[4]]. VeriPlan (CHI 2025) brings formal verification to end-user planning [[5]].

**Practical translation for Theo:** We don't need full LTL model checking. We need:
- Dependency DAG validation (no cycles, no missing prerequisites)
- Resource conflict detection (two tasks editing the same file simultaneously)
- Completeness checking (does the plan cover all stated goals?)
- Estimated token/cost budget before execution

**2b. Adaptive Replanning During Execution**

GoalAct achieves +12.22% success rate over static plans by continuously updating global plans during execution [[6]]. ALAS demonstrates that naive replanning (rewriting the whole plan on failure) is counterproductive -- targeted compensation is better [[7]]. The key insight: plans should mutate at the subtask level, not wholesale.

**Practical translation for Theo:**
- Task-level status tracking (not just checkboxes -- blocked/running/failed/skipped states)
- Failure triggers targeted replan of the failed subtask's branch, not the entire plan
- "Plan drift" detection: when execution diverges >N% from original plan, flag for human review

**2c. Learning from Past Plans**

No competitor or academic system we found implements plan-to-plan learning for coding agents. This is a genuine gap and innovation opportunity. The closest is Artemis's evolutionary mutation [[8]], which treats agent configs as evolvable organisms, and MAPLE (Multi-Agent Adaptive Planning with Long-Term Memory) [[9]].

**Practical translation for Theo:**
- Store completed plans with outcome metadata (success/failure, duration, drift score)
- Extract reusable "plan patterns" (e.g., "refactoring a god-object" always follows: extract trait -> create module -> move impl -> update imports -> test)
- Feed historical plan patterns into the planning prompt as few-shot examples

### Finding 3: Plan-as-Code vs Plan-as-Data

The question "should plans be executable?" has a nuanced answer from the literature:

- **GitHub Actions model** (plan-as-code): Plans are literally executable definitions. High reliability, low flexibility. Not suitable for AI agents because LLM outputs are inherently approximate.
- **HTN model** (plan-as-decomposable-structure): Plans are hierarchical task decompositions that can be verified and rewritten. This is the right model for AI coding agents [[10]].
- **Manus PDCA model** (plan-as-checklist): Plans are progress trackers. Simple, works, but no verification or adaptation.

**Recommendation:** Theo should use the HTN-inspired model: plans are typed, hierarchical, decomposable data structures (JSON) that the runtime interprets. Not literally executable (that's brittle), but with enough structure for verification and mutation.

### Finding 4: Multi-Agent Plan Coordination

OpenAI Agents SDK uses explicit handoffs; Anthropic uses agents-as-tools [[11]]. Neither has a shared plan artifact that sub-agents read/write.

**Innovation opportunity:** A shared plan where:
- The orchestrator agent creates the plan
- Sub-agents claim and execute individual tasks
- Task completion updates are visible to all agents
- Conflicts (two agents modifying same file) are detected before execution

This maps directly to Theo's existing sub-agent architecture in `theo-agent-runtime`.

### Finding 5: Plan Observability

Amazon's AgentCore Evaluations [[12]] and the CLEAR framework [[13]] define measurable plan quality dimensions: planning score, execution efficiency, cost, latency. Braintrust and Galileo offer agent observability platforms [[14]].

**Practical metrics for Theo:**
- Plan completion rate (tasks completed / tasks planned)
- Plan drift score (how much the plan changed during execution)
- Task estimation accuracy (predicted vs actual token cost per task)
- Failure locality (which plan patterns fail most often?)

### Finding 6: What Looks SOTA But Is Not Worth It

**Full LTL model checking:** Converting coding plans to formal logic is academically interesting but the overhead for coding tasks (which are inherently underspecified) is not justified. Lightweight DAG validation gives 80% of the benefit at 5% of the cost.

**Evolutionary plan mutation (Artemis-style):** Requires a large corpus of plan executions to converge. Premature for Theo's stage.

**Plan-as-executable-code:** Brittle. LLM-generated code plans will have bugs that cascade. Data structures with runtime interpretation is safer.

**Graph-of-Thought planning:** Adds complexity without clear benefit over hierarchical decomposition for coding tasks. ToT/GoT shine in reasoning tasks, not multi-step execution plans.

## SOTA Recommendations (Ranked by Impact / Feasibility)

### Tier 1: Foundation (must-have, high feasibility)

1. **JSON canonical format with schemars** -- typed, validated, versionable plans. Already proposed. Do it.
2. **Hierarchical task decomposition** -- tasks contain subtasks, not flat lists. HTN-inspired.
3. **Task state machine** -- `pending -> running -> done | failed | blocked | skipped`. Not checkboxes.
4. **Dependency DAG** -- explicit `depends_on` fields. Validate no cycles, no missing deps.
5. **Plan tools** -- `plan_create`, `plan_update_task`, `plan_query` exposed to the agent. Manus-inspired.

### Tier 2: Differentiation (high impact, moderate feasibility)

6. **Pre-execution verification** -- cycle detection, resource conflicts, completeness check. Lightweight formal methods.
7. **Adaptive subtask replanning** -- on failure, replan the failed branch, not the whole plan. GoalAct-inspired.
8. **Plan observability metrics** -- completion rate, drift score, cost tracking. Feed into observability engine.
9. **Multi-agent plan sharing** -- sub-agents claim tasks, orchestrator sees global state. Unique to Theo.

### Tier 3: Innovation (genuine SOTA, lower feasibility)

10. **Plan pattern corpus** -- store completed plans with outcomes. Extract reusable patterns.
11. **Plan quality scoring** -- predict success probability before execution based on historical data.
12. **Plan-to-plan learning** -- few-shot plan generation from successful historical plans.

## Gaps

- **No empirical data yet** on how often Theo's plans fail and why. We need instrumentation before designing learning systems (consistent with "measure before schema" principle from memory).
- **Plan granularity** is undefined: is a "task" a single tool call? A file edit? A feature? HTN decomposition helps but we need to decide the leaf-level granularity.
- **Human-in-the-loop** interaction with plans is unspecified. When does the user see the plan? Can they edit it? What happens when they do?
- **Plan persistence scope**: per-session? per-project? Manus uses per-session. A plan corpus needs per-project persistence.

## Risks

| Risk | Mitigation |
|---|---|
| Over-engineering: building Tier 3 before Tier 1 is solid | Strict sequencing: T1 -> T2 -> T3. No skipping. |
| Plan overhead: plans that cost more tokens than they save | Budget ceiling: plan creation should be <5% of total task tokens |
| Schema rigidity: JSON schema that can't evolve | Use schemars with `#[serde(default)]` and explicit version field |
| Adaptive replanning loops: agent rewrites plan indefinitely | Max replan count per task (e.g., 3). Hard stop. |
| YAGNI: building plan learning before we have plan data | Instrument first. Collect 100+ completed plans before building patterns. |

## Sources

1. [Planning with Files -- Manus-style persistent markdown planner](https://github.com/OthmanAdi/planning-with-files)
2. [SWE-AF: runtime DAG mutation](https://arxiv.org/html/2601.12560v1) -- referenced in agentic AI survey
3. [ChatHTN: Interleaving Approximate (LLM) and Symbolic HTN Planning](https://www.aimodels.fyi/papers/arxiv/chathtn-interleaving-approximate-llm-symbolic-htn-planning)
4. [Bridging LLM Planning Agents and Formal Methods: Plan Verification](https://arxiv.org/html/2510.03469v1)
5. [VeriPlan: Formal Verification + LLMs for End-User Planning](https://icaps25.icaps-conference.org/files/HPlan/HPlanProceedings-2025.pdf) -- CHI 2025
6. [GoalAct: Global Planning and Hierarchical Execution](https://arxiv.org/abs/2504.16563)
7. [ALAS: Adaptive LLM Agent Scheduler](https://arxiv.org/pdf/2505.12501)
8. [Artemis: Evolving Excellence via Automated Optimization of LLM Agents](https://arxiv.org/html/2512.09108v1)
9. [AGI-Edgerunners/LLM-Agents-Papers -- curated list including MAPLE](https://github.com/AGI-Edgerunners/LLM-Agents-Papers)
10. [Towards a General Framework for HTN Modeling with LLMs](https://www.arxiv.org/pdf/2511.18165)
11. [AI Agents in Production: Frameworks, Protocols, and What Works in 2026](https://47billion.com/blog/ai-agents-in-production-frameworks-protocols-and-what-actually-works-in-2026/)
12. [Amazon Bedrock AgentCore Evaluations](https://aws.amazon.com/blogs/machine-learning/build-reliable-ai-agents-with-amazon-bedrock-agentcore-evaluations/)
13. [CLEAR Framework: Multi-Dimensional Evaluation of Enterprise Agentic AI](https://arxiv.org/html/2511.14136v1)
14. [Best AI Agent Observability Tools 2026 -- Braintrust](https://www.braintrust.dev/articles/best-ai-agent-observability-tools-2026)
15. [Agentic AI: Comprehensive Survey of Architectures](https://link.springer.com/article/10.1007/s10462-025-11422-4)
16. [TAPE: Task-Adaptive Planning and Execution](https://www.sciencedirect.com/science/article/abs/pii/S0957417425030398)
17. [From Mind to Machine: The Rise of Manus AI](https://arxiv.org/html/2505.02024v1)
18. [AI Agent Architecture: Systems That Think, Plan, and Act](https://earezki.com/ai-news/2026-03-25-ai-agent-architecture-building-systems-that-think-plan-and-act/)
