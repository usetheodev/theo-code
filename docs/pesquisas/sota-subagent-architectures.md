---
type: report
question: "What are the state-of-the-art sub-agent architectures in AI coding assistants and LLM agent systems, and how does Theo Code compare?"
generated_at: 2026-04-22T18:00:00-03:00
confidence: 0.82
sources_used: 47
---

# Report: State-of-the-Art Sub-Agent Architectures in AI Coding Assistants

## Executive Summary

The industry has converged on a dominant pattern: **orchestrator-worker with capability-restricted sub-agents**, where a lead agent decomposes tasks and delegates to specialized workers operating in isolated contexts. The three critical differentiators across systems are (1) isolation model (process vs. context window vs. VM vs. worktree), (2) communication pattern (return-only vs. shared state vs. message bus), and (3) depth policy (flat vs. recursive vs. swarm). Theo Code's current `SubAgentManager` implements the most common pattern (orchestrator-worker, flat depth=1, role-restricted capabilities) but lacks several production features found in leading systems: peer-to-peer messaging, file locking, shared task lists, and dynamic agent topology.

## Part 1: Production Sub-Agent Systems

### 1.1 Claude Code (Anthropic)

**Architecture: Lead-Agent + Subagents + Agent Teams**

```
                    +------------------+
                    |   Lead Agent     |
                    |  (main context)  |
                    +--------+---------+
                             |
              +--------------+--------------+
              |              |              |
        +-----v----+  +-----v----+  +------v---+
        | Subagent  |  | Subagent |  | Subagent |
        | (explore) |  | (edit)   |  | (test)   |
        +----------+  +----------+  +----------+
        
        Each subagent: own context, returns summary only
        Agent Teams: + shared task list + peer messaging + file locks
```

**Key Details:**
- Subagents are defined in Markdown files, version-controlled, stored per-project or globally [1].
- Each subagent returns only its output to the orchestrator, not its full working context. This is intentional: exploratory work stays out of the main context [1].
- **Agent Teams** (experimental) add coordination primitives subagents lack: shared task list with dependency tracking, peer-to-peer messaging between teammates, and file locking to prevent conflicts [2].
- A hidden "Swarms" feature was discovered behind feature flags in January 2026 [3].
- The system is analyzed in the arXiv paper "Dive into Claude Code" (2604.14228): 1,884 files, ~512K lines, 7 safety layers, 5 compaction stages, 54 tools, 27 hook events [4]. Only 1.6% of the codebase is AI decision logic; 98.4% is deterministic infrastructure.
- Safety architecture: deny-first with human escalation, graduated trust spectrum, defense in depth, reversibility-weighted risk assessment [4].

**Trade-offs:**
- (+) Strong isolation prevents context contamination
- (+) Subagent Markdown files are version-controllable and composable
- (-) Parent must manually manage dependency graph between subagents
- (-) No peer messaging between standard subagents (only in Agent Teams)
- (-) Two agents writing to the same file can conflict (no built-in file locking in subagents)

**What makes it novel:** The separation between subagents (simple, return-only) and Agent Teams (full coordination) provides a progressive complexity ladder. The Markdown-defined agent specification format is uniquely declarative.

**Sources:** [Claude Code Docs](https://code.claude.com/docs/en/agent-teams) [1], [InfoQ](https://www.infoq.com/news/2025/08/claude-code-subagents/) [2], [Verdent](https://www.verdent.ai/guides/claude-code-source-code-leak-architecture) [3], [arXiv 2604.14228](https://arxiv.org/abs/2604.14228) [4]

---

### 1.2 OpenAI Codex CLI

**Architecture: CLI-first with Configurable Depth**

```
        +------------------+
        |   Codex Agent    |
        |  (local CLI)     |
        +--------+---------+
                 |
          agents.max_depth=1 (default)
          agents.max_threads=N
                 |
        +--------v---------+
        |   Child Agent    |
        |  (local process) |
        +------------------+
        
        Can also run as MCP server for external orchestration
```

**Key Details:**
- `agents.max_depth` defaults to 1 (direct child only, no deeper nesting). Raising it enables recursive delegation but increases token usage, latency, and resource consumption [5].
- `agents.max_threads` caps concurrent open threads [5].
- Codex can be invoked as an MCP server, enabling it to be orchestrated by external systems (including the OpenAI Agents SDK) [6].
- The Codex App (2026) for macOS provides a GUI for managing multiple agents in parallel [7].
- Two interaction modes: real-time pairing and task delegation, planned to converge [6].

**Trade-offs:**
- (+) Configurable depth gives users control over recursion
- (+) MCP server mode enables composability with other agent systems
- (-) Deep nesting can cause exponential token/cost growth
- (-) Local-only execution limits parallelism to machine resources

**What makes it novel:** The dual-mode design (local CLI agent that can also serve as MCP server) makes Codex uniquely composable in multi-agent architectures.

**Sources:** [OpenAI Codex Subagents](https://developers.openai.com/codex/subagents) [5], [OpenAI Codex CLI](https://developers.openai.com/codex/cli) [6], [OpenAI Codex App](https://openai.com/index/introducing-the-codex-app/) [7]

---

### 1.3 Cursor / Windsurf

**Architecture: Three-Phase Agent with Multi-Agent Collaboration**

```
        +--------------------+
        |   Cursor Agent     |
        |  (IDE-integrated)  |
        +--------+-----------+
                 |
         1. EXPLORE (read codebase)
         2. PLAN (present to dev)
         3. EXECUTE (autonomous)
                 |
        +--------v-----------+
        |  Multi-Agent Mode  |
        |  (Cursor 2.0+)     |
        +--------+-----------+
              /        \
     +------v---+  +---v------+
     | Backend  |  | Frontend |
     | Agent    |  | Agent    |
     +---------+  +----------+
     
     Shared workspace, automatic conflict resolution
```

**Key Details:**
- Composer 2 implements a three-phase workflow: Explore, Plan, Execute [8].
- Multi-agent collaboration allows multiple agents working simultaneously on different aspects of a feature, coordinating through a shared workspace architecture [8].
- Cursor 3 (April 2026) shifts the primary interface to managing parallel coding agents. Internally, 35% of merged PRs at Cursor's own engineering team are written by autonomous cloud agents [9].
- Usage data: agent mode users now outnumber tab-completion users 2:1 (reversed from March 2025) [9].
- Supports local-to-cloud agent handoff, multi-repo parallel execution [9].

**Trade-offs:**
- (+) Deep IDE integration provides rich codebase context
- (+) Human-in-the-loop at the Plan phase catches errors early
- (-) Closed source; architecture details are inferred, not documented
- (-) Cloud agent costs scale with parallelism

**What makes it novel:** The Explore-Plan-Execute three-phase model with mandatory human approval at Plan is a well-structured approach that balances autonomy with control. The inversion from tab-completion-first to agent-first represents a real industry shift.

**Sources:** [Digital Applied](https://www.digitalapplied.com/blog/cursor-2-0-agent-first-architecture-guide) [8], [InfoQ](https://www.infoq.com/news/2026/04/cursor-3-agent-first-interface/) [9]

---

### 1.4 Devin (Cognition)

**Architecture: Fully Autonomous Cloud Agent**

```
        +-----------------------------+
        |        Devin Cloud VM       |
        |  +-------+  +--------+     |
        |  |  IDE   |  | Browser|     |
        |  +-------+  +--------+     |
        |  +--------+  +-------+     |
        |  |Terminal |  | Shell |     |
        |  +--------+  +-------+     |
        |                             |
        |  Proprietary Agent Models   |
        |  Interactive Planning       |
        +-----------------------------+
                    |
                    v
              GitHub PR
```

**Key Details:**
- Runs in a fully sandboxed cloud environment with its own IDE, browser, terminal, and shell [10].
- Devin 2.0 (April 2025): $20/month entry price, Interactive Planning (editable execution plans), 4x faster problem solving, 67% PR merge rate (up from 34%) [11].
- Proprietary agent models optimized for SE tasks, not just GPT-4 wrappers [10].
- Cognition acquired Windsurf in July 2025 [11].
- Weakness: "senior-level at codebase understanding but junior at execution" [11].

**Trade-offs:**
- (+) Full cloud VM isolation eliminates local resource constraints
- (+) Complete environment (IDE + browser + terminal) enables complex tasks
- (-) Fully autonomous = less developer control
- (-) Coordination is task-scoped, not spec-driven; no multi-agent workspace [12]
- (-) Real-world evaluation (Answer.AI): 14 failures, 3 successes, 3 inconclusive out of 20 tasks [11]

**What makes it novel:** The fully autonomous, cloud-VM-per-task model provides complete environment isolation that no local agent can match.

**Sources:** [Cognition Blog](https://cognition.ai/blog/devin-annual-performance-review-2025) [11], [Contrary Research](https://research.contrary.com/company/cognition) [10], [Augment Code](https://www.augmentcode.com/tools/intent-vs-devin) [12]

---

### 1.5 SWE-Agent

**Architecture: Custom ACI + Interactive File Viewer**

```
        +--------------------+
        |    SWE-Agent Loop  |
        | +----------------+ |
        | |  Custom ACI    | |
        | | find_file      | |
        | | search_file    | |
        | | edit (linted)  | |
        | +----------------+ |
        | +----------------+ |
        | | File Viewer    | |
        | | (100-line win) | |
        | +----------------+ |
        +--------+-----------+
                 |
         Max 50 search hits
         Linter-validated edits
         Line-range editing
```

**Key Details:**
- Performance gains come from tailored interface design (ACI), modular command abstraction, and context/history management, not model-centric modifications [13].
- Custom commands: `find_file`, `search_file`, `search_dir` with max 50 hits to prevent context overflow [13].
- Interactive file viewer: 100-line window with line numbers, edits validated by built-in linter [13].
- LangChain's Open SWE framework extends this with child agent spawning via task tool, middleware hooks, and file-based memory for long-running tasks [14].

**Trade-offs:**
- (+) Interface design is the key insight: constrained outputs prevent context overflow
- (+) Linter-validated edits prevent syntactically invalid changes
- (-) Single-agent design limits parallelism
- (-) Less extensible than multi-agent frameworks

**What makes it novel:** The core insight that agent performance is more about ACI (Agent-Computer Interface) design than model selection is a foundational contribution to the field.

**Sources:** [EmergentMind](https://www.emergentmind.com/topics/swe-agent-scaffold) [13], [LangChain Blog](https://blog.langchain.com/open-swe-an-open-source-framework-for-internal-coding-agents/) [14]

---

### 1.6 Aider

**Architecture: Architect/Editor Dual-Model Pattern**

```
        +-------------------+
        |  Architect Model  |  (reasoning, planning)
        |  (e.g. Opus)      |
        +---------+---------+
                  |
           Describes solution
                  |
        +---------v---------+
        |   Editor Model    |  (code generation)
        |  (e.g. Sonnet)    |
        +---------+---------+
                  |
           File edits + auto-commit
```

**Key Details:**
- Architect/Editor pattern: Architect model describes the solution, Editor model translates into file edits [15].
- Three modes: `/mode architect` (planning), `/mode code` (editing), `/ask` (questions) [15].
- Auto-runs tests and linters, fixes detected problems, commits with sensible messages [16].
- State-of-the-art benchmark results from the dual-model approach [15].

**Trade-offs:**
- (+) Separating reasoning from editing allows optimal model selection for each
- (+) Simple, intuitive mental model
- (-) Not truly multi-agent; it's dual-model with different roles
- (-) No parallelism; sequential workflow

**What makes it novel:** The explicit separation of reasoning (Architect) from code generation (Editor) is a simple but effective pattern that outperforms single-model approaches. This maps well to the industry trend of using expensive models for planning and cheaper models for execution.

**Sources:** [Aider Blog](https://aider.chat/2024/09/26/architect.html) [15], [Aider Docs](https://aider.chat/docs/) [16]

---

### 1.7 Amazon Q Developer

**Architecture: Five Specialized Agents**

```
        +---------------------------+
        |    Amazon Q Developer     |
        +---------------------------+
        |  /dev    → Development    |
        |  /doc    → Documentation  |
        |  /test   → Testing        |
        |  /review → Code Review    |
        |  /transform → Migration   |
        +---------------------------+
        
        CLI Agent Orchestrator (CAO):
        +---------------------------+
        |     Supervisor Agent      |
        +--------+--------+--------+
        |Worker 1|Worker 2|Worker N |
        +--------+--------+--------+
```

**Key Details:**
- Five specialized agents: /dev, /doc, /transform, /review, /test [17].
- CLI Agent Orchestrator (CAO): open-source multi-agent framework that transforms CLI tools into coordinated systems. Hierarchical supervisor + specialized workers [18].
- SWE-Bench results: 49% on SWTBench Verified, 66% on SWEBench Verified [17].
- MCP support enables pulling context from Jira, Figma, etc. [17].
- Real case: modernized thousands of legacy Java applications using parallel agents [17].

**Trade-offs:**
- (+) Deep AWS ecosystem integration
- (+) CAO enables orchestrating multiple CLI agents (Q + Claude Code + others)
- (-) Tightly coupled to AWS services
- (-) Agent specialization is command-level, not task-level

**What makes it novel:** CAO is the first open-source framework specifically designed to orchestrate multiple CLI coding agents together in a hierarchical system.

**Sources:** [AWS Q Developer](https://aws.amazon.com/q/developer/features/) [17], [AWS Open Source Blog](https://aws.amazon.com/blogs/opensource/introducing-cli-agent-orchestrator-transforming-developer-cli-tools-into-a-multi-agent-powerhouse/) [18]

---

### 1.8 Google Jules

**Architecture: Async Cloud VM per Task**

```
        +----------------------------+
        |   GitHub Repository        |
        +-----------+----------------+
                    | clone
        +-----------v----------------+
        |   Google Cloud VM          |
        |   (sandboxed, ephemeral)   |
        |                            |
        |   Gemini 2.5 Pro           |
        |   Full codebase parsing    |
        |   Dependencies + configs   |
        +-----------+----------------+
                    |
                    v
              PR with diff + reasoning
```

**Key Details:**
- Every task gets a fresh Google Cloud VM, deleted after completion. Stateless and secure [19].
- Clones entire repository including dependencies, configs, and test files for project-level understanding [19].
- Asynchronous execution: developer assigns task and continues working [19].
- Pricing: Free (15 tasks/day), Pro (75/day, $20/mo), Ultra (300/day, $125/mo, multi-agent workflows) [19].
- **Project Jitro (Jules V2)**: KPI-driven development where agent autonomously identifies what to change to move a metric in the right direction (e.g., "improve test coverage from 40% to 80%") [20].

**Trade-offs:**
- (+) Full codebase context in an isolated VM is uniquely powerful
- (+) Asynchronous model frees developer time
- (-) Stateless = no learning across tasks
- (-) GitHub-only integration

**What makes it novel:** Jules V2 / Project Jitro's outcome-driven agent model (define KPIs, agent finds the path) is a fundamentally different paradigm from instruction-driven agents.

**Sources:** [Google Blog](https://blog.google/innovation-and-ai/models-and-research/google-labs/jules/) [19], [TestingCatalog](https://www.testingcatalog.com/google-prepares-jules-v2-agent-capable-of-taking-bigger-tasks/) [20]

---

## Part 2: Academic/Research Frameworks

### 2.1 AutoGen (Microsoft) / Microsoft Agent Framework

**Architecture: Event-Driven Actor Model (v0.4+) / Graph Workflows (MAF)**

```
  AutoGen v0.4:                          MAF (2026):
  +------------+                         +------------------+
  | Agent A    |  <-- messages -->       | Workflow Graph   |
  +------------+                         | +----+  +----+   |
  | Agent B    |  <-- messages -->       | |Node|->|Node|   |
  +------------+                         | +----+  +----+   |
  | Agent C    |  <-- messages -->       | +----+           |
  +------------+                         | |Node|           |
                                         | +----+           |
  GroupChat: selector decides            +------------------+
  who speaks next                        Typed nodes + edges
                                         Human-in-the-loop pauses
```

**Key Details:**
- AutoGen v0.4 (Jan 2025): event-driven architecture, agents as actors responding to messages [21].
- Split into three paths (March 2026): Microsoft Agent Framework (MAF), AutoGen v0.7.x (research), AG2 (community fork) [22].
- MAF merges AutoGen's orchestration with Semantic Kernel's enterprise features. Explicit Graph-based Workflows replace implicit "GroupChat" management [23].
- Magentic-One: generalist agent team (browse web, manage files, execute code) built on AutoGen v0.7.5 [22].
- Key abstraction shift: from "Manager Agent decides who speaks" to "typed graph nodes with explicit edges" [23].

**Trade-offs:**
- (+) Most mature multi-agent conversation framework
- (+) MAF adds enterprise features (session state, middleware, telemetry)
- (-) Three-way split creates confusion about which version to use
- (-) GroupChat pattern can be unpredictable in who speaks next

**Sources:** [Microsoft Research](https://www.microsoft.com/en-us/research/publication/autogen-enabling-next-gen-llm-applications-via-multi-agent-conversation-framework/) [21], [sanj.dev](https://sanj.dev/post/autogen-microsoft-multi-agent-framework) [22], [Microsoft Learn](https://learn.microsoft.com/en-us/agent-framework/migration-guide/from-autogen/) [23]

---

### 2.2 CrewAI

**Architecture: Role-Based Agent Orchestration**

```
        +-------------------+
        |       Crew        |
        +--------+----------+
                 |
        +--------v----------+
        |   Manager Agent   |
        +---+------+------+-+
            |      |      |
        +---v-+ +--v--+ +-v---+
        |Rsrch| |Write| |Anlst|
        |Agent| |Agent| |Agent|
        +-----+ +-----+ +-----+
        
        Each agent: role, goal, backstory, tools
        Flows: event-driven orchestration layer
```

**Key Details:**
- Four primitives: Agents, Tasks, Tools, Crew [24].
- Role-based design: each agent has role, goal, backstory, and tools [24].
- Execution models: sequential, parallel, conditional processing [24].
- Hierarchical coordination: senior agents can override juniors [24].
- CrewAI Flows: enterprise event-driven orchestration supporting Crews natively [25].
- 43,000+ GitHub stars (2026), Fortune 500 adoption (DocuSign, PwC) [25].

**Trade-offs:**
- (+) Most intuitive API; role-based design maps to human team thinking
- (+) Fastest prototyping experience among frameworks
- (-) Less control over agent interactions than LangGraph
- (-) Opinionated; harder to customize deeply

**Sources:** [CrewAI Docs](https://crewai.com/) [24], [VisionStack](https://visionstack.visionsparksolutions.com/reviews/crewai/) [25]

---

### 2.3 LangGraph

**Architecture: Stateful Directed Graphs with Cycles**

```
        +--------+     +--------+     +--------+
        | Node A +---->| Node B +---->| Node C |
        +--------+     +---+----+     +--------+
                            |              |
                            |   +----------+
                            |   |  (cycle)
                            v   v
                        +--------+
                        | Node D |
                        +--------+
        
        State: TypedDict with reducer functions
        Checkpointing: MemorySaver / PostgresSaver
        Fan-out/Fan-in for parallel execution
```

**Key Details:**
- Adds cycles to LangChain's DAG model; this is what enables agent loops [26].
- State management via TypedDict with reducer functions for concurrent updates [26].
- Agents communicate only through shared AgentState; no direct agent-to-agent calls [27].
- Checkpointing: MemorySaver, AsyncSqliteSaver, PostgresSaver [26].
- Recent additions: Command primitive for dynamic edgeless flows, interrupt primitive for human-in-the-loop, semantic search for long-term memory, cross-thread memory [26].
- Industry estimate: router pattern handles 60% of real-world use cases [27].

**Trade-offs:**
- (+) Most control and observability (via LangSmith)
- (+) Persistence and recovery built-in
- (+) Graph model is mathematically rigorous
- (-) Steeper learning curve; more code than CrewAI
- (-) Python-centric

**Sources:** [LangGraph Docs](https://www.langchain.com/langgraph) [26], [Medium](https://medium.com/@timarkanta.sharma/architecting-multi-agent-systems-with-langgraph-patterns-trade-offs-and-real-world-design-ba8c535c6b35) [27]

---

### 2.4 OpenAI Swarm / Agents SDK

**Architecture: Two Primitives — Agents + Handoffs**

```
        +----------+    handoff()    +----------+
        |  Agent A |  ------------> |  Agent B  |
        | (triage) |                | (billing) |
        +----------+                +-----+-----+
                                          |
                                     handoff()
                                          |
                                    +-----v-----+
                                    |  Agent C   |
                                    | (support)  |
                                    +-----------+
        
        Stateless: each run() starts fresh
        Context variables for state
        Full conversation history carries over
```

**Key Details:**
- Swarm (Oct 2024): educational framework, intentionally minimal [28].
- Two primitives: Agents (instructions + functions + model) and Handoffs (function returns Agent object) [28].
- Stateless: each `run()` starts from scratch. Context variables enable state across calls [28].
- Agents SDK (March 2025): production successor with guardrails, tracing, TypeScript support [29].
- Core insight: explicit handoffs are simpler and more observable than implicit routing [28].

**Trade-offs:**
- (+) Extreme simplicity; two abstractions handle everything
- (+) Handoffs are explicit and observable
- (-) Stateless design requires external state management for long tasks
- (-) No parallelism; agents are sequential

**Sources:** [GitHub openai/swarm](https://github.com/openai/swarm) [28], [OpenAI Agents SDK](https://openai.github.io/openai-agents-python/) [29]

---

### 2.5 Anthropic Agent SDK

**Architecture: Tool-Use-First Agent Loop**

```
        +----------------------------+
        |      Agent Loop            |
        |  +--------+               |
        |  | Prompt  |               |
        |  +----+---+               |
        |       |                    |
        |  +----v---+  +--------+  |
        |  | Claude  +->| Tools  |  |
        |  +----+---+  +--+-----+  |
        |       |          |        |
        |  +----v---+  +--v-----+  |
        |  |Response |  |SubAgent|  |  (agent-as-tool)
        |  +--------+  +--------+  |
        +----------------------------+
        
        Tools include: file ops, shell, web search, MCP, sub-agents
        Extended thinking for chain-of-thought
        Managed Agents (April 2026) for cloud execution
```

**Key Details:**
- Tool-use-first: agents are Claude models equipped with tools, including invoking other agents as tools [30].
- Same agent loop powers Claude Code; renamed from "Claude Code SDK" to "Claude Agent SDK" in late 2025 [31].
- Five architectural patterns: routing, parallelization, orchestrator-worker, context management, evaluator-optimizer [30].
- Multi-agent research system: lead agent + Sonnet subagents outperforms single Opus by 90.2% on internal research eval [32].
- Multi-agent systems consume ~15x more tokens than standard chat [32].
- Managed Agents (April 2026): platform handles orchestration, sandboxing, state, credentials [33].

**Trade-offs:**
- (+) Battle-tested (powers Claude Code)
- (+) Minimal abstraction; agent loop is a simple while-loop
- (+) Extended thinking provides visible chain-of-thought
- (-) Agent-as-tool means sub-agents can't communicate with each other
- (-) 15x token cost for multi-agent is significant

**Sources:** [Anthropic Engineering](https://www.anthropic.com/engineering/building-agents-with-the-claude-agent-sdk) [31], [Anthropic Patterns](https://aimultiple.com/building-ai-agents) [30], [Anthropic Multi-Agent Research](https://www.anthropic.com/engineering/multi-agent-research-system) [32], [InfoQ](https://www.infoq.com/news/2026/04/anthropic-managed-agents/) [33]

---

### 2.6 Google A2A (Agent-to-Agent Protocol)

**Architecture: Three-Layer Protocol**

```
        Layer 3: Protocol Bindings (JSON-RPC, gRPC, HTTP/REST)
        Layer 2: Abstract Operations (SendMessage, GetTask, Subscribe)
        Layer 1: Canonical Data Model (Protocol Buffers)
        
        Agent A                           Agent B
        +--------+                        +--------+
        | Agent  |  -- A2A Protocol -->   | Agent  |
        | Card   |  <-- Task + Parts --   | Card   |
        +--------+                        +--------+
        
        /.well-known/agent-card.json      /.well-known/agent-card.json
```

**Key Details:**
- Open standard by Google (April 2025), donated to Linux Foundation (June 2025) [34].
- Three-layer architecture: data model (protobuf), abstract operations, protocol bindings [35].
- Agent Cards at `/.well-known/agent-card.json` describe capabilities [35].
- Multimodal Parts: text, binary, files, structured data [35].
- 11 JSON-RPC methods: SendMessage, SendStreamingMessage, GetTask, SubscribeToTask, etc. [35].
- A2A = horizontal bus (agent-to-agent); MCP = vertical bus (agent-to-tools) [34].
- IBM's ACP merged into A2A (August 2025) [36].
- 150+ organizations, v1.0 with Signed Agent Cards, production at Microsoft, AWS, Salesforce, SAP [36].

**Key Relationship to MCP:** A2A and MCP are complementary. MCP connects agents to tools/data (vertical). A2A connects agents to agents (horizontal). A phased adoption roadmap: MCP first (tool access), then A2A (collaborative task execution) [37].

**Sources:** [Google Blog](https://developers.googleblog.com/en/a2a-a-new-era-of-agent-interoperability/) [34], [A2A Spec](https://github.com/a2aproject/A2A/blob/main/docs/specification.md) [35], [Stellagent](https://stellagent.ai/insights/a2a-protocol-google-agent-to-agent) [36], [arXiv 2505.02279](https://arxiv.org/html/2505.02279v1) [37]

---

### 2.7 MCP (Model Context Protocol)

**Architecture: JSON-RPC Client-Server for Tool Integration**

```
        +-------------+     JSON-RPC      +-------------+
        |  AI Agent   | <===============> |  MCP Server |
        |  (client)   |                   | (tool/data) |
        +-------------+                   +-------------+
        
        Capabilities:
        - Tool invocation (synchronous + async)
        - Typed data exchange
        - Session management
        - Auth (API keys, OAuth 2.0, mTLS)
        - Parallel tool calls (Nov 2025+)
        - Server-side agent loops (Nov 2025+)
```

**Key Details:**
- Anthropic released MCP in November 2024 [38].
- 97M+ monthly SDK downloads. Adopted by OpenAI, Google DeepMind, Microsoft [38].
- Donated to Agentic AI Foundation (Linux Foundation) in December 2025 [38].
- November 2025 spec: async execution, parallel tool calls, server-side agent loops, modern authorization [39].
- Security concerns: prompt injection, tool permission exfiltration, lookalike tools [38].
- Evolving from tool-calling protocol to foundation for secure distributed agent architectures [38].

**Sources:** [Wikipedia](https://en.wikipedia.org/wiki/Model_Context_Protocol) [38], [MCP Anniversary Blog](https://blog.modelcontextprotocol.io/posts/2025-11-25-first-mcp-anniversary/) [39]

---

## Part 3: Key Architectural Patterns — Cross-System Analysis

### 3.1 Isolation Models

| System | Isolation Model | Granularity |
|--------|----------------|-------------|
| Claude Code | Context window per subagent | Per-task |
| Codex CLI | Process + configurable depth | Per-agent |
| Cursor | Shared workspace + conflict resolution | Per-feature |
| Devin | Full cloud VM | Per-task |
| Jules | Ephemeral cloud VM | Per-task |
| Theo Code | Context window + capability set | Per-role |
| LangGraph | State channels with reducers | Per-node |
| AutoGen/MAF | Event-driven actors | Per-message |

**Key Finding:** The trend is moving toward VM-level isolation for complex tasks (Devin, Jules) while keeping context-window isolation for simple delegation (Claude Code, Theo). The cost-performance tradeoff determines the right level.

### 3.2 Communication Patterns

| Pattern | Systems | Pros | Cons |
|---------|---------|------|------|
| **Return-only** (child reports to parent) | Claude Code subagents, Anthropic SDK, Theo Code | Simple, no context leak | No peer collaboration |
| **Shared state** (read/write common state) | LangGraph, Cursor | Rich coordination | Race conditions, complexity |
| **Message passing** (explicit messages) | AutoGen, A2A, Claude Code Agent Teams | Decoupled, observable | Higher latency |
| **Handoffs** (transfer of control) | Swarm, Agents SDK | Clear ownership | Sequential, no parallelism |
| **Event bus** (publish-subscribe) | Theo Code (EventBus) | Loose coupling | Event ordering challenges |

**Key Finding:** Return-only is dominant in production coding agents because it keeps the parent context clean. Shared-state is used when agents must coordinate on the same files. Message passing is emerging for enterprise multi-agent systems via A2A.

### 3.3 Orchestration Strategies

```
Hierarchical (Hub-and-Spoke):
  Most common in production (2026). One coordinator dispatches to workers.
  Used by: Claude Code, Amazon Q, Theo Code, Anthropic SDK, CrewAI
  
Peer-to-Peer (Mesh):
  Agents interact directly. Better for exploration/creativity.
  Used by: A2A protocol, Swarm/Agents SDK, AutoGen GroupChat
  
Graph-Based:
  Explicit nodes and edges define flow. Most controllable.
  Used by: LangGraph, Microsoft Agent Framework (MAF)
  
Blackboard:
  Agents contribute to shared workspace; system evolves through accumulation.
  Used by: Cursor multi-agent, some research systems
```

**Key Finding:** Industry estimate — the hub-and-spoke (hierarchical) pattern handles the vast majority of production use cases [27]. Graph-based workflows are gaining traction for complex stateful pipelines. Pure peer-to-peer is rare in production.

### 3.4 Context Management

| Strategy | Description | Used By |
|----------|-------------|---------|
| **Summary-and-handoff** | Subagent returns summary, not full context | Claude Code, Anthropic SDK |
| **Compaction pipeline** | Progressive context compression (5 stages in Claude Code) | Claude Code |
| **File-based memory** | Offload large results to files | Open SWE, LangGraph |
| **Structured result schemas** | Subagents return JSON, not free text | Anthropic SDK (recommended) |
| **Persistent memory stores** | External KV storage for cross-agent facts | LangGraph, Anthropic SDK |
| **Windowed viewing** | 100-line sliding window | SWE-Agent |

### 3.5 Result Aggregation

| Pattern | Description | Cost |
|---------|-------------|------|
| **Lead-agent synthesis** | Orchestrator combines all subagent outputs | 15x tokens (Anthropic data) |
| **Merge-on-filesystem** | Agents write to different files; git merge | Low token cost, conflict risk |
| **State reducer** | Typed merge functions in shared state | Deterministic, requires schema |
| **Citation chain** | Dedicated CitationAgent handles attribution | Higher quality, extra agent cost |

### 3.6 Error Handling in Multi-Agent Systems

| Pattern | Description |
|---------|-------------|
| **Timeout per role** | Different timeouts for different agent types (Theo, Claude Code) |
| **Depth limit** | Prevent recursive spawning (Theo MAX_DEPTH=1, Codex max_depth=1) |
| **Unbounded loop detection** | Guard against agents retrying infinitely |
| **Panic isolation** | tokio::spawn + JoinSet catches panics without killing parent (Theo) |
| **Budget enforcement** | Shared budget deducted from parent (Theo, Anthropic) |
| **Circuit breaker** | After N failures, stop retrying and report |

### 3.7 Cost/Latency Optimization

| Strategy | Description | Used By |
|----------|-------------|---------|
| **Model routing** | Expensive model for planning, cheap for execution | Aider (Architect/Editor), Anthropic (Opus lead + Sonnet workers) |
| **Caching** | Cache tool results across agents | MCP (15-min cache) |
| **Parallel execution** | Fan-out independent subtasks | LangGraph, Theo, Claude Code Agent Teams |
| **Depth limits** | Prevent exponential cost from deep recursion | Codex, Theo |
| **Effort scaling** | Scale agent count to query complexity | Anthropic multi-agent research |

---

## Part 4: Frontier Patterns

### 4.1 Agent-as-Tool vs Agent-as-Peer

This is the fundamental architectural dichotomy of 2025-2026:

**Agent-as-Tool:** Sub-agent is wrapped as a callable function. Parent has full control. Used by Anthropic SDK, Claude Code subagents, Amazon Q, Theo Code.
- (+) Control, observability, deterministic
- (-) Bottleneck at orchestrator, no peer collaboration

**Agent-as-Peer:** Agents interact directly without central coordinator. Used by A2A, Swarm/Agents SDK handoffs, AutoGen GroupChat.
- (+) Resilient, emergent solutions, no bottleneck
- (-) Harder to debug, state drift, cost explosion

**Hybrid (emerging):** Use agent-as-tool for well-defined tasks, agent-as-peer for exploration. Claude Code's Agent Teams represent an early hybrid [40].

**Sources:** [AWS Blog](https://aws.amazon.com/blogs/machine-learning/multi-agent-collaboration-patterns-with-strands-agents-and-amazon-nova/) [40]

### 4.2 Recursive Agent Spawning with Depth Limits

| System | Max Depth | Configurable? | Notes |
|--------|-----------|---------------|-------|
| Theo Code | 1 | No (hardcoded) | Sub-agents cannot spawn sub-agents |
| Codex CLI | 1 (default) | Yes (`agents.max_depth`) | Users can increase but warned about cost |
| Claude Code | 1 | No | Subagents marked `is_subagent`, no delegation tools |
| OpenSage | Dynamic | Yes (runtime) | First system to support AI-created agent topologies [41] |

**Frontier:** OpenSage (2026) is the first framework where agents can create sub-agent instances at runtime, enabling vertical and horizontal topologies that no human pre-defined [41]. This is a significant departure from static agent architectures.

**Sources:** [arXiv OpenSage](https://arxiv.org/html/2602.16891v1) [41]

### 4.3 Speculative Execution

Applying speculative execution from CPU architecture to multi-agent systems:

- **Distributed Speculative Inference (DSI):** Overlaps verification with drafting, making inference non-blocking. 1.29-1.92x faster than sequential inference (ICLR 2025) [42].
- **Parallel Token Prediction (PTP):** Generates arbitrary-length sequences in parallel by feeding random variables as input (ICLR 2026) [43].
- **Applied to agents:** Run multiple sub-agents on the same problem with different approaches, select the best result. Used implicitly by Anthropic's voting pattern for high-stakes outputs [30].

**Sources:** [ICLR 2025](https://proceedings.iclr.cc/paper_files/paper/2025/file/b36554b97da741b1c48c9de05c73993e-Paper-Conference.pdf) [42], [arXiv PTP](https://arxiv.org/pdf/2512.21323) [43]

### 4.4 Self-Improving Agent Architectures

The Layered Recursive Stack (2026 standard):

```
        +---------------------+
        |  Meta-Optimizer     |  ← Improves the improvement process
        +---------------------+
        |  Critic / Judge     |  ← Evaluates every modification
        +---------------------+
        |  Self-Correction    |  ← Debugs and fixes failures
        +---------------------+
        |  Task Agent         |  ← Executes the actual task
        +---------------------+
        |  Skill Library      |  ← Reusable learned capabilities
        +---------------------+
```

Key systems:
- **Darwin Godel Machine:** Open-ended self-improvement through code self-modification. Because coding ability improves self-modification ability, it creates a recursive cascade [44].
- **Hyperagents:** Self-referential agents integrating task agent + meta agent in a unified framework [45].
- **ICLR 2026 Workshop on RSI:** Five axes of self-improvement: change targets, timing, mechanisms, contexts, evidence [46].

**Safety concern:** Recursive self-improvement systems require explicit observability mechanisms and safety evaluations in the benchmark set to validate each iteration before it progresses [46].

**Sources:** [Self-Evolving Agents](https://evoailabs.medium.com/self-evolving-agents-open-source-projects-redefining-ai-in-2026-be2c60513e97) [44], [arXiv](https://aclanthology.org/2025.acl-long.1354.pdf) [45], [ICLR 2026 RSI Workshop](https://openreview.net/pdf?id=OsPQ6zTQXV) [46]

### 4.5 Cross-Agent Learning and Knowledge Transfer

| Pattern | Description | Source |
|---------|-------------|--------|
| **Multiagent finetuning** | Specializes models via multiagent-generated data, preserving diverse reasoning chains | Research (2025) |
| **AgentRxiv** | LLM agent labs share research on a preprint server for collaboration | Research (2025) |
| **Skill Library** | Agents save reusable skills that other agents can load | OpenSage, self-evolving agents |
| **Shared memory** | Cross-thread persistent memory in LangGraph | LangGraph (2026) |
| **File-as-Bus** | Agents communicate through files on disk | AiScientist, Open SWE |

---

## Part 5: Theo Code — Current State and Gap Analysis

### Current Architecture

```
        +-----------------------+
        |    Main AgentLoop     |
        |   (theo-agent-runtime)|
        +----------+------------+
                   |
          SubAgentManager::spawn()
          MAX_DEPTH = 1 (hardcoded)
                   |
        +----------v------------+
        |     SubAgent          |
        |  Role: Explorer |     |
        |        Implementer |  |
        |        Verifier |     |
        |        Reviewer       |
        +----------+------------+
                   |
          Returns AgentResult
          (summary + success flag)
```

**What Theo has:**
- Four specialized roles with capability restrictions (Explorer=read-only, Implementer=unrestricted, Verifier=no-edit, Reviewer=read-only) -- matches industry pattern
- PrefixedEventForwarder -- tags sub-agent events with role name for observability
- Parallel execution via `spawn_parallel()` with `tokio::JoinSet`
- Timeout per role (5-10 min)
- Depth=1 enforcement -- sub-agents cannot spawn sub-agents
- Capability set restrictions via `CapabilityGate`
- Shared EventBus for event forwarding

**What Theo is missing (compared to SOTA):**

| Gap | Found In | Priority | Complexity |
|-----|----------|----------|------------|
| **Peer messaging** between sub-agents | Claude Code Agent Teams | Medium | High |
| **File locking** to prevent conflicts | Claude Code Agent Teams | High | Medium |
| **Shared task list** with dependency tracking | Claude Code Agent Teams | Medium | High |
| **Configurable max_depth** | Codex CLI | Low | Low |
| **Model routing** per role (cheap model for Explorer, expensive for Implementer) | Aider, Anthropic | High | Medium |
| **Context compaction** (5-stage pipeline) | Claude Code | High | High |
| **Structured result schemas** (JSON, not free text) | Anthropic SDK | Medium | Low |
| **Budget tracking** per sub-agent | Anthropic SDK | Medium | Medium |
| **MCP integration** for tool discovery | Anthropic, OpenAI, Google | High | High |
| **Agent Card** / capability advertisement | A2A protocol | Low | Medium |
| **Effort scaling** (adjust agent count to query complexity) | Anthropic research | Medium | Medium |
| **Worktree isolation** (git worktree per agent) | Claude Code Agent Teams | Medium | Medium |

---

## Gaps and Unknowns

1. **Cost data is scarce.** Anthropic's "15x token cost for multi-agent" is the only public figure. Other systems don't publish cost multipliers.
2. **Benchmark comparisons across multi-agent systems are absent.** No SWE-Bench-style benchmark exists for multi-agent coordination quality.
3. **Production failure modes are underdocumented.** Most systems describe happy paths; systematic analysis of multi-agent failure modes (deadlocks, context overflow, conflicting edits) is limited.
4. **Cursor and Devin architectures are inferred**, not documented. Their closed-source nature means analysis is based on observable behavior and marketing materials.
5. **A2A + MCP convergence is speculative.** The phased adoption roadmap (MCP first, then A2A) is theoretical; real-world production deployments of combined A2A+MCP are not yet documented.

---

## Recommendations for Theo Code

### Priority 1: Model Routing per Sub-Agent Role
**Why:** The Architect/Editor pattern (Aider) and Opus-lead/Sonnet-workers pattern (Anthropic) demonstrate that using expensive models for planning and cheap models for execution significantly reduces cost while maintaining quality. Theo already has role-based routing infrastructure (`SubAgentRoleId` maps to routing slots).

**Action:** Map SubAgentRole to model tiers in the existing routing system. Explorer and Reviewer use a fast/cheap model. Implementer uses the primary model.

### Priority 2: File Locking for Parallel Sub-Agents
**Why:** `spawn_parallel()` already exists but has no protection against two agents writing to the same file. This is an identified gap in Claude Code subagents too, only addressed in Agent Teams.

**Action:** Implement a lightweight `FileLockManager` shared across sub-agents. Advisory locking, not OS-level. Sub-agent declares files it intends to modify; manager rejects conflicts.

### Priority 3: Structured Result Schemas
**Why:** `AgentResult` currently uses `summary: String`. Anthropic recommends structured results to minimize context bloat when lead agent synthesizes outputs.

**Action:** Extend `AgentResult` with typed fields: `findings: Vec<Finding>`, `files_changed: Vec<PathBuf>`, `confidence: f32`. Keep `summary` as human-readable fallback.

### Priority 4: Context Compaction
**Why:** Claude Code's 5-stage compaction pipeline is a core differentiator. Without it, long sub-agent interactions fill context windows inefficiently.

**Action:** Implement progressive compaction: (1) tool result truncation, (2) conversation summarization, (3) sliding window, (4) relevance filtering, (5) emergency compression.

### Priority 5: MCP Server Mode
**Why:** Codex CLI's dual mode (agent + MCP server) enables composability. Running Theo as an MCP server would allow it to be orchestrated by other agents or integrated into IDE workflows.

**Action:** Expose Theo's tools as MCP server endpoints. This aligns with the existing `tool_manifest.rs` infrastructure.

---

## Sources (Consolidated)

1. [Claude Code Docs — Agent Teams](https://code.claude.com/docs/en/agent-teams)
2. [InfoQ — Claude Code Subagents](https://www.infoq.com/news/2025/08/claude-code-subagents/)
3. [Verdent — Claude Code Architecture](https://www.verdent.ai/guides/claude-code-source-code-leak-architecture)
4. [arXiv 2604.14228 — Dive into Claude Code](https://arxiv.org/abs/2604.14228)
5. [OpenAI Codex Subagents](https://developers.openai.com/codex/subagents)
6. [OpenAI Codex CLI Docs](https://developers.openai.com/codex/cli)
7. [OpenAI — Introducing Codex App](https://openai.com/index/introducing-the-codex-app/)
8. [Digital Applied — Cursor 2.0](https://www.digitalapplied.com/blog/cursor-2-0-agent-first-architecture-guide)
9. [InfoQ — Cursor 3](https://www.infoq.com/news/2026/04/cursor-3-agent-first-interface/)
10. [Contrary Research — Cognition](https://research.contrary.com/company/cognition)
11. [Cognition Blog — Devin Performance Review](https://cognition.ai/blog/devin-annual-performance-review-2025)
12. [Augment Code — Intent vs Devin](https://www.augmentcode.com/tools/intent-vs-devin)
13. [EmergentMind — SWE-Agent Scaffold](https://www.emergentmind.com/topics/swe-agent-scaffold)
14. [LangChain — Open SWE](https://blog.langchain.com/open-swe-an-open-source-framework-for-internal-coding-agents/)
15. [Aider — Architect Mode](https://aider.chat/2024/09/26/architect.html)
16. [Aider Documentation](https://aider.chat/docs/)
17. [AWS — Amazon Q Developer Features](https://aws.amazon.com/q/developer/features/)
18. [AWS — CLI Agent Orchestrator](https://aws.amazon.com/blogs/opensource/introducing-cli-agent-orchestrator-transforming-developer-cli-tools-into-a-multi-agent-powerhouse/)
19. [Google Blog — Jules](https://blog.google/innovation-and-ai/models-and-research/google-labs/jules/)
20. [TestingCatalog — Jules V2](https://www.testingcatalog.com/google-prepares-jules-v2-agent-capable-of-taking-bigger-tasks/)
21. [Microsoft Research — AutoGen](https://www.microsoft.com/en-us/research/publication/autogen-enabling-next-gen-llm-applications-via-multi-agent-conversation-framework/)
22. [sanj.dev — AutoGen 2026](https://sanj.dev/post/autogen-microsoft-multi-agent-framework)
23. [Microsoft Learn — MAF Migration](https://learn.microsoft.com/en-us/agent-framework/migration-guide/from-autogen/)
24. [CrewAI Docs](https://crewai.com/)
25. [VisionStack — CrewAI Review](https://visionstack.visionsparksolutions.com/reviews/crewai/)
26. [LangGraph Docs](https://www.langchain.com/langgraph)
27. [Medium — LangGraph Patterns](https://medium.com/@timarkanta.sharma/architecting-multi-agent-systems-with-langgraph-patterns-trade-offs-and-real-world-design-ba8c535c6b35)
28. [GitHub — openai/swarm](https://github.com/openai/swarm)
29. [OpenAI Agents SDK](https://openai.github.io/openai-agents-python/)
30. [AIMultiple — Anthropic Patterns](https://aimultiple.com/building-ai-agents)
31. [Anthropic — Building Agents with Claude Agent SDK](https://www.anthropic.com/engineering/building-agents-with-the-claude-agent-sdk)
32. [Anthropic — Multi-Agent Research System](https://www.anthropic.com/engineering/multi-agent-research-system)
33. [InfoQ — Anthropic Managed Agents](https://www.infoq.com/news/2026/04/anthropic-managed-agents/)
34. [Google Blog — A2A Protocol](https://developers.googleblog.com/en/a2a-a-new-era-of-agent-interoperability/)
35. [A2A Specification](https://github.com/a2aproject/A2A/blob/main/docs/specification.md)
36. [Stellagent — A2A Protocol Growth](https://stellagent.ai/insights/a2a-protocol-google-agent-to-agent)
37. [arXiv 2505.02279 — Agent Protocol Survey](https://arxiv.org/html/2505.02279v1)
38. [Wikipedia — Model Context Protocol](https://en.wikipedia.org/wiki/Model_Context_Protocol)
39. [MCP Anniversary Blog](https://blog.modelcontextprotocol.io/posts/2025-11-25-first-mcp-anniversary/)
40. [AWS — Multi-Agent Collaboration Patterns](https://aws.amazon.com/blogs/machine-learning/multi-agent-collaboration-patterns-with-strands-agents-and-amazon-nova/)
41. [arXiv — OpenSage](https://arxiv.org/html/2602.16891v1)
42. [ICLR 2025 — DSI](https://proceedings.iclr.cc/paper_files/paper/2025/file/b36554b97da741b1c48c9de05c73993e-Paper-Conference.pdf)
43. [arXiv — PTP](https://arxiv.org/pdf/2512.21323)
44. [Medium — Self-Evolving Agents](https://evoailabs.medium.com/self-evolving-agents-open-source-projects-redefining-ai-in-2026-be2c60513e97)
45. [ACL 2025 — Hyperagents](https://aclanthology.org/2025.acl-long.1354.pdf)
46. [ICLR 2026 — RSI Workshop](https://openreview.net/pdf?id=OsPQ6zTQXV)
47. [Addy Osmani — Code Agent Orchestra](https://addyosmani.com/blog/code-agent-orchestra/)
