---
type: report
question: "Is docs/plans/agents-plan.md well-evidenced, complete, and correctly phased?"
generated_at: 2026-04-23T00:00:00Z
confidence: 0.82
sources_used: 19
verdict: APPROVE with CONCERNS
---

# Report: Review of Dynamic Sub-Agent System Plan

## Executive Summary

The plan is solid, well-evidenced, and correctly structured for incremental delivery. The evidence table is defensible and the "what NOT to adopt" column shows genuine engineering judgment (not just adoption enthusiasm). However, three concerns require attention before implementation begins: (1) the plan is missing two significant reference systems that emerged in 2025-2026 (Google ADK and OpenAI Agents SDK), (2) MCP integration in Phase 6 carries higher risk than the plan acknowledges given protocol security gaps, and (3) the markdown+frontmatter format should explicitly acknowledge the emerging multi-format landscape rather than treating it as settled.

**Verdict: APPROVE with 3 CONCERNS.**

---

## Question 1: Is the evidence well-used?

### Assessment: Strong (0.85 confidence)

The evidence table correctly maps each reference to a specific architectural decision, which is above average for planning documents. Specific strengths:

**"What to adopt" column -- well-justified:**

- OpenDev's resolution order (project > global > built-in) is correctly identified as the canonical pattern. The plan maps it to `SubAgentRegistry::with_all()` which mirrors `SubagentManager::with_builtins_and_custom()` [Source 3].
- Hermes' `delegate_task` schema (goal+context+toolsets) is correctly chosen over the current split `subagent`/`subagent_parallel` tools. The current `tool_bridge.rs` (lines 103-170) confirms the split exists and is awkward.
- Anthropic SDK's agent-as-tool pattern validates model routing per role, which Theo already has via `RoutingPhase::Subagent { role }` in `theo-domain/src/routing.rs`.

**"What NOT to adopt" column -- shows engineering discipline:**

- Rejecting OpenDev's mailbox inter-agent communication with "YAGNI -- depth=1 resolve" is correct. The current `subagent/mod.rs` enforces `MAX_DEPTH: usize = 1` (line 144), and the plan preserves this invariant.
- Rejecting Hermes' heartbeat pattern because "we don't have gateway timeout" is valid -- Theo uses tokio timeout (lines 241-258 of current `mod.rs`), which is simpler and sufficient.
- Rejecting Archon's DAG executor as over-engineering is correct for the current scope.

**One weakness:** The Aider evidence ("confirms model routing per role is high-ROI") is used as validation rather than as a distinct architectural input. This is honest but could be omitted to tighten the table.

---

## Question 2: Is there evidence MISSING?

### Assessment: Yes -- two significant gaps (0.88 confidence)

**Gap 1: Google ADK (Agent Development Kit) -- released April 2025**

Google ADK introduces a hierarchical agent tree with LLM-driven delegation and `AgentTool` (wrap an agent as a tool). This is directly relevant to the plan's `delegate_task` design. ADK's state sharing mechanism (conversation history automatically transfers between delegator and delegate) is a pattern the plan should evaluate. ADK 2.0 Alpha (2026) adds graph-based workflows and native A2A protocol support [Source 10, 12].

The plan's `delegate_task` tool is closer to OpenAI Agents SDK's "agent as tool" pattern than to ADK's hierarchical delegation. This is probably the right call for Theo (depth=1 constraint), but the plan should document why ADK's deeper hierarchies were considered and rejected.

**Gap 2: OpenAI Agents SDK -- released March 2025**

OpenAI Agents SDK replaced the experimental Swarm with production-grade handoffs, guardrails, and tracing primitives. The SDK's handoff model (typed tool calls with full context transfer) and three-tier guardrails (running in parallel by default) are relevant references [Source 10, 12]. The guardrails pattern in particular could inform Theo's `CapabilityGate` evolution.

**Gap 3 (minor): AGENTS.md as cross-tool standard**

The plan assumes `.theo/agents/*.md` as the custom agent format. The industry is converging on `AGENTS.md` as a universal agent instruction file supported by Claude Code, Cursor, Copilot, Gemini CLI, Windsurf, Aider, Zed, Warp, and others [Source 8]. While Theo's per-agent markdown files serve a different purpose (agent specs vs. instructions), the plan should acknowledge this standard and explain why per-file specs are needed in addition to any `AGENTS.md` the project may have.

**Recommendation:** Add a row for Google ADK and OpenAI Agents SDK to the evidence table. Document why ADK's multi-level hierarchies and Agents SDK's handoff model were evaluated and not adopted (depth=1 constraint + simplicity preference).

---

## Question 3: Does "98.4% infrastructure" justify 7 phases?

### Assessment: Yes, but the citation needs nuance (0.80 confidence)

The arXiv 2604.14228 finding is real and verified: the paper analyzed Claude Code v2.1.88 (~512K lines TypeScript) and found 1.6% AI decision logic vs. 98.4% deterministic infrastructure across 5 layers, 21 subsystems [Source 14, 15]. The paper was submitted 14 April 2026 by VILA-Lab researchers.

**The justification works because:**

The plan's 7 phases are predominantly infrastructure: domain types, registry, parser, file locking, worktree isolation, MCP transport, cleanup. The only "AI decision" is the on-demand agent creation in Phase 4 (where the LLM decides to create an agent that doesn't exist in the registry). This ratio (~14% AI decision, ~86% infrastructure) actually mirrors the paper's finding.

**But the citation needs nuance:**

The 98.4% figure describes Claude Code's TOTAL codebase (permission systems, TUI, extension mechanisms, OAuth, etc.), not specifically its sub-agent subsystem. Applying a whole-system ratio to justify a sub-system plan is a category error. The plan should cite the finding as directional evidence ("infrastructure is where differentiation lives") rather than as a precise justification.

Also: Claude Code's 512K lines serve millions of users across all use cases. Theo is a much smaller system where each phase's ROI should be evaluated independently. Worktree isolation (Phase 5) and MCP integration (Phase 6) should each justify their own existence, not ride on a generic "infrastructure is important" claim.

**Recommendation:** Keep the citation but reframe it: "This confirms our phasing strategy of building infrastructure (Phases 1-4) before advanced features (Phases 5-7)." Remove it as justification for Phases 5-6 specifically.

---

## Question 4: Is markdown+frontmatter the emerging standard?

### Assessment: It is ONE of the emerging standards, not THE standard (0.75 confidence)

The landscape as of April 2026:

| Format | Used By | Frontmatter? | Notes |
|---|---|---|---|
| `.md` + YAML frontmatter | Claude Code (SKILL.md), VS Code Copilot (.agent.md) | Yes | Agent Skills open standard by Anthropic |
| `AGENTS.md` (single file) | 10+ tools (Cursor, Copilot, Gemini, etc.) | No | Universal instructions, not per-agent specs |
| `.mdc` | Cursor | Yes (custom) | Activation modes, Cursor-only |
| YAML/JSON configs | Google ADK, LangGraph, CrewAI | N/A | Programmatic, not file-based |

**The plan's choice of markdown+frontmatter is well-aligned** with the Claude Code SKILL.md format and the VS Code `.agent.md` format [Source 7, 8, 9]. Anthropic has published Agent Skills as an open standard for cross-platform portability.

**However:** The plan should note that this format is not universal -- programmatic frameworks (ADK, LangGraph) use code-based agent definitions, and the `AGENTS.md` standard (supported by the widest ecosystem) does NOT use per-file frontmatter. Theo's per-file approach is appropriate for the use case (each file = one agent spec with distinct capabilities), but this should be a documented decision, not an unstated assumption.

**Recommendation:** Add an ADR note in the plan explaining: "We use per-file markdown+frontmatter because each agent needs distinct capability sets, model overrides, and tool restrictions. This aligns with Claude Code SKILL.md and VS Code .agent.md rather than the single-file AGENTS.md standard."

---

## Question 5: Is MCP integration (Phase 6) premature?

### Assessment: Partially premature -- split it (0.78 confidence)

**MCP protocol maturity as of April 2026:**

- 97M+ monthly SDK downloads, 6,400+ servers on registries [Source 4, 5]
- Donated to Linux Foundation (AAIF) December 2025
- Streamable HTTP transport is production-ready
- BUT: 30 CVEs cataloged in 60 days across the ecosystem [Source 5]
- Enterprise readiness items (auth, audit trails, SSO, gateways) are pre-RFC [Source 4]
- Official 2026 roadmap lists transport scaling, agent communication, and governance as priorities -- meaning these are NOT solved yet [Source 4]

**Risk assessment:**

| Sub-feature | Maturity | Risk |
|---|---|---|
| MCP Client (consume external servers) | Medium-high | Moderate -- well-trodden path, many implementations exist |
| MCP Server (expose Theo tools) | Medium | Higher -- Theo becomes a dependency for others, protocol may shift |
| MCP security (auth, sandboxing) | Low | High -- 30 CVEs in 60 days, auth story incomplete |

**The plan's own insight (`insight-mcp-a2a-convergence.md`) rates MCP confidence at 0.78** and explicitly says "MCP server should be near-term priority" but also notes "A2A is not urgent." This is internally consistent.

**Recommendation:** Split Phase 6 into two:

- **Phase 6a (MCP Client):** Low risk, high value. Sub-agents consuming external MCP servers for tool discovery. This is the "MCP as tool source" pattern which is mature and well-supported. Ship with Phases 1-5.
- **Phase 6b (MCP Server):** Higher risk, lower urgency. Theo exposing tools via MCP. Defer until the security and auth stories mature (track the 2026 roadmap). This can ship independently without blocking Phases 1-5.

This split also reduces the blast radius: if MCP protocol changes break something, only the client integration is affected in the near term.

---

## Additional Risks Identified

### Risk 1: On-demand agent creation is under-specified

Phase 4 allows the LLM to create agents that don't exist in the registry: "agent = nome nao registrado -> cria AgentSpec::on_demand() com defaults." The plan doesn't specify:
- What default capability set does an on-demand agent get? Unrestricted? Read-only?
- Can the LLM specify tools for an on-demand agent, or only objective+context?
- Is there a limit on how many on-demand agents can be created per session?

If an on-demand agent gets unrestricted capabilities by default, the LLM effectively bypasses the carefully designed CapabilityGate system.

**Recommendation:** On-demand agents MUST default to read-only (`CapabilitySet::read_only()`) unless the user has explicitly configured a less restrictive default. Document this in the plan.

### Risk 2: AgentFinding parsing is fragile

The plan says: "findings e parseado do output do sub-agent quando possivel (pattern matching em linhas com severity markers)." Parsing LLM output with regex for structured data is inherently unreliable. The plan acknowledges this ("Para sub-agents que nao emitem findings estruturados, findings fica vazio") but doesn't specify what happens when parsing partially succeeds (some lines match, some don't).

**Recommendation:** Use a two-phase approach: (1) instruct the sub-agent to emit a JSON block at the end of its output, (2) fall back to regex parsing only if JSON is absent. This is what Hermes' `tool_trace` does -- structured first, text fallback second.

### Risk 3: No migration path for existing subagent/subagent_parallel callers

The plan says the tool API change is not breaking because "Nao ha API publica externa." However, any user who has prompt templates or CLAUDE.md instructions referencing `subagent` or `subagent_parallel` tool names will be affected. The plan should include a deprecation period or alias.

**Recommendation:** In Phase 4, register `subagent` and `subagent_parallel` as aliases for `delegate_task` with a deprecation warning. Remove in Phase 7.

---

## Summary of Recommendations

| # | Recommendation | Priority | Phase affected |
|---|---|---|---|
| 1 | Add Google ADK and OpenAI Agents SDK to evidence table | Medium | Pre-implementation |
| 2 | Reframe 98.4% citation as directional, not precise justification | Low | Documentation |
| 3 | Document why per-file frontmatter over AGENTS.md single-file | Low | Pre-implementation |
| 4 | Split Phase 6 into 6a (client, ship now) and 6b (server, defer) | High | Phase 6 |
| 5 | Default on-demand agents to read-only capabilities | High | Phase 4 |
| 6 | Use JSON-first + regex-fallback for AgentFinding parsing | Medium | Phase 3 |
| 7 | Add tool name aliases for backward compatibility | Medium | Phase 4 |

---

## Sources

1. [arXiv 2604.14228 -- Dive into Claude Code](https://arxiv.org/abs/2604.14228)
2. [VILA-Lab/Dive-into-Claude-Code GitHub](https://github.com/VILA-Lab/Dive-into-Claude-Code)
3. OpenDev subagent source: `crates/opendev-agents/src/subagents/` (local reference)
4. [MCP 2026 Roadmap](https://blog.modelcontextprotocol.io/posts/2026-mcp-roadmap/)
5. [MCP Production Gaps -- The New Stack](https://thenewstack.io/model-context-protocol-roadmap-2026/)
6. [MCP Enterprise Readiness -- WorkOS](https://workos.com/blog/2026-mcp-roadmap-enterprise-readiness)
7. [Claude Code Skills Documentation](https://code.claude.com/docs/en/skills)
8. [AGENTS.md Guide -- Data Science Collective](https://medium.com/data-science-collective/the-complete-guide-to-ai-agent-memory-files-claude-md-agents-md-and-beyond-49ea0df5c5a9)
9. [VS Code Custom Agents](https://code.visualstudio.com/docs/copilot/customization/custom-agents)
10. [Google ADK Multi-Agent Systems](https://google.github.io/adk-docs/agents/multi-agents/)
11. [Agentic Delegation: LangGraph vs OpenAI vs Google ADK](https://www.arcade.dev/blog/agent-handoffs-langgraph-openai-google/)
12. [Claude vs OpenAI vs ADK Comparison -- Composio](https://composio.dev/content/claude-agents-sdk-vs-openai-agents-sdk-vs-google-adk)
13. [State of Agent Engineering -- LangChain](https://www.langchain.com/state-of-agent-engineering)
14. [State of AI Agent Frameworks 2026 -- Fordel Studios](https://fordelstudios.com/research/state-of-ai-agent-frameworks-2026)
15. [PyShine -- Claude Code 98.4% Infrastructure](https://pyshine.com/Dive-into-Claude-Code-Systematic-AI-Coding-Analysis/)
16. [Anthropic -- Agent Skills Open Standard](https://www.anthropic.com/engineering/equipping-agents-for-the-real-world-with-agent-skills)
17. [MCP Security Report -- Zuplo](https://zuplo.com/mcp-report)
18. [Emerging Agent Architecture -- Glean](https://www.glean.com/blog/emerging-agent-stack-2026)
19. Theo codebase: `crates/theo-agent-runtime/src/subagent/mod.rs`, `tool_bridge.rs` (local)
