---
type: insight
topic: "core vs deferred tool split"
confidence: 0.78
impact: high
---

# Insight: The optimal core set is 8-10 tools; planning and git tools are the clearest deferral targets

**Key finding:** Industry consensus is 3-5 core tools (Anthropic docs), but coding agents need more because file operations are fundamentally frequent. Claude Code keeps 19 unconditional tools; Theo should keep 8-10. The 6 planning tools and 4 git tools are mode-specific and their schemas are among the largest -- deferring them saves the most tokens per tool.

**Evidence:**
- Anthropic recommends "3-5 most frequently used tools as non-deferred" [Source: tool-search-tool docs]
- Claude Code: 19 unconditional, 35 conditional on feature flags [Source: arxiv 2604.14228]
- OpenDev: metadata-only index at startup, full content at invocation -- 40% to <5% startup context cost [Source: ZenML/OpenDev]
- Planning tool schemas in Theo average 200-400 tokens each (plan_create alone has 6+ parameters with descriptions). Six planning tools = ~1,200-2,400 tokens of deferrable schema per turn.

**Implication for Theo:**
Confidence is 0.78 (not 0.95) because the split should be validated with telemetry (P0 recommendation in report). However, the planning tools are almost certainly correct for deferral: they are only used in `--mode plan`, and their schemas are expensive. Git tools are the second-clearest target: used only during commit workflows. The unvalidated risk is that some users may call `git_status` early in most sessions, which telemetry will reveal.
