# Cross-Validation: Theo Code × Harness Engineering SOTA

> Data: 2026-04-20
> Baseado em: Pesquisa sistemática com 88 papers, 25 shortlisted, 10 deep-analyzed
> Referências: ReAct, SWE-agent, MemGPT, AutoHarness, CMV, OpenHands, MPAC, ANX, MemCoder, AutoGen
> Paper completo: `research-output/final.md`

---

## Premissa Central

> **Agent = Model + Harness.** O harness é o diferenciador primário de performance.

Evidência: TerminalBench 2.0 demonstra que Opus 4.6 em harnesses customizados supera Opus 4.6 no Claude Code. AutoHarness [@lou2026] prova que Gemini-2.5-Flash **com** harness supera Gemini-2.5-Pro **sem** harness (56.3% vs 38.2% win rate) [MEASURED]. A implicação: investir em harness engineering tem ROI maior que trocar de modelo.

---

## Taxonomia: 14 Categorias em 4 Grupos

```
┌─────────────────────────────────────────────────────────────────────┐
│ CORE LOOP           │ TOOL LAYER          │ ECOSYSTEM               │
│ 1. Agent Loop       │ 4. Tool Design      │ 7. Skills & MCP         │
│ 2. Planning         │ 5. Bash/Code Exec   │ 8. Permissions & Auth   │
│ 3. Context Delivery │ 6. Sandbox/Security │ 9. Memory & State       │
├─────────────────────┴─────────────────────┴─────────────────────────┤
│ QUALITY                                                             │
│ 10. Observability  11. Verification/Evals  12. HITL                 │
│ 13. Debugging/DX   14. Long Horizon                                 │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 1. AGENT LOOP

### SOTA (2026)
- **ReAct** [@yao2022]: Thought→Action→Observation. +12pp ALFWorld, +9.9pp WebShop vs action-only [MEASURED]
- **LATS** [@zhou2023]: MCTS over agent trajectories. Backtracking when path fails.
- **State Machine formalization**: Production systems (OpenHands, Theo Code) usam FSM formal ao invés de while loops.
- **AutoHarness** [@lou2026]: Critic/Refiner loop com Thompson sampling tree search. Converge em ~14.5 iterações [MEASURED].

### Padrões de Referência
1. FSM com transições validadas + eventos publicados por transição
2. Middleware chain interceptora (pre-tool, post-tool, pre-llm, post-llm, on-error)
3. Budget enforcement a cada iteração (tokens, time, iterations, tool_calls)
4. Interpreter persistence entre turns (variáveis de shell, estado de sessão)

### Theo Code — Status

| Aspecto | Status | Módulo | Evidência |
|---|---|---|---|
| ReAct loop formal | ✅ **IMPLEMENTED** | `run_engine.rs` | 6 estados: Planning→Executing→Evaluating→Converged/Replanning/Aborted |
| Budget enforcement | ✅ **IMPLEMENTED** | `BudgetEnforcer` | 4 constraints checados a cada iteração (line 533) |
| Event publishing | ✅ **IMPLEMENTED** | `event_bus.rs` | Cada transição publica DomainEvent |
| Middleware interceptora | ❌ **MISSING** | — | EventBus é pub/sub observacional, não interceptor |
| Interpreter persistence | ❌ **MISSING** | — | Nenhum estado de shell persiste entre turns |
| Backtracking (LATS-style) | ❌ **MISSING** | — | Sem exploração de caminhos alternativos |

**Score: 60%** — Core sólido mas sem middleware chain e sem backtracking.

**Recomendação SOTA:** Implementar `HarnessMiddleware` trait com `fn pre_tool(&mut self, call: &ToolCall) -> ControlFlow<Deny, Allow>`. Inspirado no Claude SDK 6-hook model.

---

## 2. PLANNING & TASK DECOMPOSITION

### SOTA (2026)
- **Plan-and-Execute** separation com artefatos em filesystem
- **LATS** [@zhou2023]: MCTS + environment feedback como value function
- **CodeTree** [@li2024]: Tree search guiada por agente para code gen
- **Plan Reuse** [@li2025]: 93% reuse rate, 93.12% latency reduction via intent classification [MEASURED]
- **AgentGen** [@hu2024]: Synthetic task generation para melhorar planning abilities

### Theo Code — Status

| Aspecto | Status | Módulo | Evidência |
|---|---|---|---|
| Plan-and-Execute separation | ✅ **IMPLEMENTED** | `AgentMode::Plan` + `roadmap.rs` | Escreve `.theo/plans/*.md`, executa step-by-step |
| Sub-agent spawning | ✅ **IMPLEMENTED** | `subagent/mod.rs` | 4 roles, max_depth=1, capability isolation |
| Parallel sub-agents | ✅ **IMPLEMENTED** | `subagent_parallel` | `tokio::spawn` + `JoinSet` |
| Task lifecycle FSM | ✅ **IMPLEMENTED** | `task_manager.rs` | Created→InProgress→Completed/Failed/Cancelled |
| Automatic decomposition | ❌ **MISSING** | — | Usuário deve triggerar Plan mode manualmente |
| Tree search / backtracking | ❌ **MISSING** | — | Sem LATS, sem exploração de alternativas |
| Plan reuse / caching | ❌ **MISSING** | — | Cada task planejada from scratch |

**Score: 60%** — Fundação robusta mas sem auto-decomposição nem tree search.

**Recomendação SOTA:** (1) Auto-detect tasks complexas e triggerar Plan mode sem intervenção. (2) Implementar plan caching baseado em intent similarity (93% reuse possível).

---

## 3. CONTEXT DELIVERY & COMPACTION

### SOTA (2026)
- **CMV** [@santoni2026]: DAG com snapshot/branch/trim. 20% mean, 86% max, 39% mixed tool-use [MEASURED on 76 coding sessions]
- **LLMLingua**: 20x prompt compression com perda mínima
- **EVOR** [@zhang2024]: Retrieval que evolui adaptando-se ao contexto de geração
- **Progressive Disclosure**: should_defer pattern (Anthropic Principle 12)
- **Prompt Caching**: Break-even em 10 turns vs plain context [MEASURED — CMV paper]

### Theo Code — Status

| Aspecto | Status | Módulo | Evidência |
|---|---|---|---|
| Token budget management | ✅ **IMPLEMENTED** | `budget.rs` | 4 constraints + BudgetAllocation 5 slots (15/25/40/15/5%) |
| Multi-stage compaction | ✅ **IMPLEMENTED** | `compaction_stages.rs` | 5 níveis: None(70%)→Warning→Mask(80%)→Prune(85%)→Aggressive(90%) |
| Protected tools | ✅ **IMPLEMENTED** | `compaction_stages.rs` | read_file, graph_context, skill nunca pruned |
| Progressive disclosure | ✅ **IMPLEMENTED** | `Tool::should_defer()` | Deferred tools via tool_search |
| Tool output truncation | ✅ **IMPLEMENTED** | `truncation_rule()` | Head/Tail/HeadTail per tool |
| RAG pipeline (GRAPHCTX) | ✅ **IMPLEMENTED** | `theo-engine-retrieval` | BM25 + RRF 3-ranker + cross-encoder. MRR=0.914 [MEASURED] |
| Greedy knapsack packing | ✅ **IMPLEMENTED** | `assembly.rs` | Fill budget by relevance score |
| LLM-powered summarization | ❌ **MISSING** | `compaction_summary.rs` | Template only, Compact stage deferred |
| Prompt caching API | 🟡 **PARTIAL** | `budget.rs` | Campos existem, não wired à API Anthropic |
| DAG-based state (CMV) | ❌ **MISSING** | — | Sem snapshot/branch/trim |
| Autonomous compression tool | ❌ **MISSING** | — | Agente não controla sua própria compaction |

**Score: 75%** — Um dos mais fortes. GRAPHCTX é state-of-the-art (MRR=0.914). Gaps em LLM summarization e CMV.

**Recomendação SOTA:** (1) Wire prompt caching (ganho imediato, low effort). (2) Implementar LLM summarization no Compact stage. (3) Estudar CMV para session tree branching.

---

## 4. TOOL DESIGN

### SOTA (2026)
- **SWE-agent ACI** [@yang2024]: Custom tools > raw bash. 12.5% SWE-bench [MEASURED]. ACI design principles: concise feedback, immediate errors, windowed view, guardrails.
- **AutoHarness** [@lou2026]: `is_legal(action, state) -> bool` + `propose_legal_action(state)`. 100% legal action rate [MEASURED].
- **Mind the GAP** [@2026]: Text safety ≠ tool-call safety. Separate mechanisms needed.
- **Input examples**: 72% → 90% accuracy improvement (Anthropic studies)
- **Risk annotations** (MCP spec): readOnlyHint, destructiveHint, idempotentHint, openWorldHint

### Theo Code — Status

| Aspecto | Status | Módulo | Evidência |
|---|---|---|---|
| JSON Schema + to_json_schema() | ✅ **IMPLEMENTED** | `theo-domain/tool.rs` | OA-compatible generation |
| Input examples | ✅ **IMPLEMENTED** | `ToolSchema::input_examples` | 72%→90% accuracy boost |
| Categories | ✅ **IMPLEMENTED** | `ToolCategory` enum | FileOps, Search, Execution, Web, Orchestration, Utility |
| Deferred discovery | ✅ **IMPLEMENTED** | `should_defer()` + `search_hint()` | Progressive disclosure via tool_search |
| Truncation rules | ✅ **IMPLEMENTED** | `truncation_rule()` | Head/Tail/HeadTail + llm_suffix coaching |
| LLM suffix coaching | ✅ **IMPLEMENTED** | `ToolOutput::llm_suffix` | Hidden retry hints post-truncation |
| Risk annotations | ❌ **MISSING** | — | Sem readOnlyHint, destructiveHint, idempotentHint |
| Action validation (AutoHarness) | ❌ **MISSING** | — | Sem is_legal() check pre-execution |
| Constrained output | ❌ **MISSING** | — | Sem schema enforcement no output do modelo |
| Tool state persistence | ❌ **MISSING** | — | Tools stateless entre calls |

**Score: 65%** — Schema e discovery são SOTA. Falta risk annotations e action validation.

**Recomendação SOTA:** (1) Add `risk_profile()` ao Tool trait: `RiskProfile { read_only: bool, destructive: bool, idempotent: bool, network: bool }`. (2) Wire ao CapabilityGate para auto-escalation de ferramentas destrutivas.

---

## 5. BASH & CODE EXECUTION

### SOTA (2026)
- SWE-agent: Docker per instance, bash + ACI tools combinados
- OpenHands: Runtime/Sandbox como camada separada
- Isolation cascade: Landlock → bwrap → noop (Linux)
- Toxic sequence detection + environment sanitization

### Theo Code — Status

| Aspecto | Status | Módulo | Evidência |
|---|---|---|---|
| Bash tool (sh -c + piped stdio) | ✅ **IMPLEMENTED** | `bash.rs` (727 lines) | Streaming para TUI |
| Landlock executor | ✅ **IMPLEMENTED** | `sandbox/executor.rs` | v4+ ruleset, async-signal-safe pre_exec |
| Command validation | ✅ **IMPLEMENTED** | `command_validator.rs` | Lexical analysis para padrões perigosos |
| Environment sanitization | ✅ **IMPLEMENTED** | `env_sanitizer.rs` | Whitelist + strip AWS_*, GITHUB_TOKEN, OPENAI_API_KEY |
| Resource limits | ✅ **IMPLEMENTED** | `rlimits.rs` | 64 procs, 512MB RAM, 120s CPU, 100MB file |
| Network isolation | ✅ **IMPLEMENTED** | `network.rs` | unshare(NEWUSER\|NEWNET) |
| Toxic sequences | ✅ **IMPLEMENTED** | `sequence_analyzer.rs` | 6 patterns: payload_drop, exfil, force_push, ssh_key_exfil, env_exfil, reverse_shell |
| bwrap integration | 🟡 **PARTIAL** | `bwrap.rs` (447 lines) | Código existe, não no cascade |

**Score: 95%** — O módulo mais completo do sistema. Supera a maioria dos harnesses open-source.

---

## 6. SANDBOX & SECURITY

### SOTA (2026)
- OpenHands: Docker container per instance
- Filesystem isolation + network isolation + process isolation + audit
- Sensitive file patterns hardcoded
- Mind the GAP: tool-call safety requires separate enforcement from text safety

### Theo Code — Status

| Aspecto | Status | Módulo | Evidência |
|---|---|---|---|
| Filesystem isolation (Landlock) | ✅ **IMPLEMENTED** | Landlock v4+ | ALWAYS_DENIED_READ/WRITE hardcoded |
| Network isolation | ✅ **IMPLEMENTED** | unshare namespaces | allow_network=false default |
| Process isolation | ✅ **IMPLEMENTED** | rlimits | CPU, memory, file, nproc |
| Audit trail | ✅ **IMPLEMENTED** | `sandbox_audit.rs` | JSONL persistente (~/.config/theo/audit/) |
| Sensitive file detection | ✅ **IMPLEMENTED** | `sandbox.rs` | .env, credentials.json, secrets.yaml, id_rsa |
| Container isolation | ❌ **MISSING** | — | Sem Docker-level sandbox |
| macOS sandbox | 🟡 **PARTIAL** | `macos.rs` | Stub only |
| Tool-call safety (separate from text) | 🟡 **PARTIAL** | `CapabilityGate` | Enforced mas sem risk-based escalation |

**Score: 85%** — Excelente no Linux. Container isolation seria o next step para parity com OpenHands.

---

## 7. SKILLS & MCP

### SOTA (2026)
- **MCP** (Anthropic): Open protocol, stdio/SSE transport, resource discovery
- **ANX** [@xu2026]: 3EX architecture. 47-66% token reduction, 58% exec time reduction vs MCP [MEASURED on form-filling]
- **MPAC** [@qian2026]: 5-layer protocol, 21 message types, Lamport clocks. 95% overhead reduction, 4.8x speedup [MEASURED]
- **A2A** (Google): Agent-to-Agent with Agent Card discovery
- **AG-UI**: Event-driven agent-to-frontend
- **Code wrapping MCP**: 98.7% token reduction

### Theo Code — Status

| Aspecto | Status | Módulo | Evidência |
|---|---|---|---|
| Skills system | ✅ **IMPLEMENTED** | `skill/mod.rs` | Catalog, invocação via tool, permissions |
| MCP auth (OAuth PKCE) | ✅ **IMPLEMENTED** | `theo-infra-auth/mcp.rs` | Token storage, PKCE, redirect URI |
| MCP transport (stdio/SSE) | ❌ **MISSING** | — | Nenhum transport layer |
| MCP resource discovery | ❌ **MISSING** | — | Sem list_resources/read_resource |
| MCP tool registration | ❌ **MISSING** | �� | Sem adapter MCP→theo_domain::Tool |
| Skill versioning | ❌ **MISSING** | — | Sem SKILL.md v1/v2 |
| A2A Protocol | ❌ **MISSING** | — | Sem agent-to-agent protocol |
| AG-UI Protocol | ❌ **MISSING** | — | Desktop usa EventBus custom |
| SaaS integration (Composio-style) | ❌ **MISSING** | — | Sem wrapper para APIs externas |

**Score: 20%** — **GAP CRÍTICO.** Só auth existe. Ecossistema fechado.

**Recomendação SOTA:** Implementar MCP client (stdio transport + tool adapter) como prioridade. O protocol é open e as vantagens são ecossistêmicas. ANX e MPAC são complementos futuros.

---

## 8. PERMISSIONS & AUTHORIZATION

### SOTA (2026)
- **Claude SDK 5-layer**: hooks → deny rules → permission mode → allow rules → canUseTool
- **Microsoft Authorization Fabric**: PEP/PDP → ALLOW/DENY/REQUIRE_APPROVAL/MASK
- **IETF draft**: SPIFFE + OAuth Token Exchange + DPoP para agent auth
- **MPAC Governance Layer**: Policy enforcement + audit trail integrado no protocolo

### Theo Code — Status

| Aspecto | Status | Módulo | Evidência |
|---|---|---|---|
| Capability-based access | ✅ **IMPLEMENTED** | `capability.rs` + `capability_gate.rs` | deny > allow > category |
| Per-role capability sets | ✅ **IMPLEMENTED** | `CapabilitySet` | unrestricted(), read_only() por sub-agent role |
| Permission pattern matching | ✅ **IMPLEMENTED** | `permission.rs` | Wildcard/glob, Allow/Ask/Deny |
| Multi-layer evaluation | 🟡 **PARTIAL** | CapabilityGate only | Não há hooks→deny→mode→allow→canUseTool completo |
| REQUIRE_APPROVAL decision | ❌ **MISSING** | — | Gate bloqueia ou permite, nunca pergunta |
| Deny-and-continue | ❌ **MISSING** | — | Denial = error terminal |
| Risk-based escalation | ❌ **MISSING** | — | Sem threshold que escala para humano |
| Centralized PEP/PDP | ❌ **MISSING** | — | Sem policy decision point |

**Score: 45%** — Enforcement existe mas é binário. Falta o espectro ALLOW→REQUIRE_APPROVAL→DENY.

**Recomendação SOTA:** Add `PermissionDecision::RequireApproval { reason, timeout }` ao CapabilityGate. Wire risk annotations das tools ao gate: tool destructiva + operação fora de allowed_paths → RequireApproval.

---

## 9. MEMORY & STATE

### SOTA (2026)
- **MemGPT** [@packer2023]: 3-tier (main/archival/recall). +60.4pp deep memory retrieval [MEASURED]
- **MemCoder** [@deng2026]: +9.4% resolved rate SWE-bench via structured memory [MEASURED]
- **CMV** [@santoni2026]: DAG state management. 39% reduction em mixed tool-use [MEASURED]
- **AriGraph** [@anokhin2024]: Knowledge graph + episodic memory híbrido
- **Voyager** [@wang2023]: Skill library como procedural memory

### Theo Code — Status

| Aspecto | Status | Módulo | Evidência |
|---|---|---|---|
| MemoryProvider trait | ✅ **IMPLEMENTED** | `theo-domain/memory.rs` | prefetch, sync_turn, on_pre_compress, on_session_end |
| Memory fencing (XML) | ✅ **IMPLEMENTED** | `memory.rs` | `<memory-context>` com system-note |
| Lifecycle tiers | ✅ **IMPLEMENTED** | `episode.rs` | Active→Cooling→Archived |
| Memory kinds | ✅ **IMPLEMENTED** | `episode.rs` | Ephemeral/Episodic/Reusable/Canonical |
| Episode summaries | ✅ **IMPLEMENTED** | `episode.rs` | MachineEpisodeSummary + human + files |
| Cross-session bootstrap | ✅ **IMPLEMENTED** | `session_bootstrap.rs` | .theo/progress.json |
| Usefulness tracking | 🟡 **PARTIAL** | `context_metrics.rs` | Computed mas não fed back ao assembler |
| **Concrete backend** | ❌ **MISSING** | — | **Zero implementações do MemoryProvider** |
| Decay/eviction enforcement | ❌ **MISSING** | — | Tiers definidos, transições não enforced |
| Usefulness → assembler loop | ❌ **MISSING** | — | Métricas coletadas sem feedback |
| Facts as first-class objects | ❌ **MISSING** | — | Sem hash-addressed knowledge tuples |
| Intent-to-code mining (MemCoder) | ❌ **MISSING** | — | Sem extração de padrões do git history |

**Score: 50%** — **Design elegante, implementação zero.** Maior distância entre arquitetura e realidade.

**Recomendação SOTA:** (1) Implementar `FileSystemMemoryProvider` com 3-tier hierarchy. (2) Lifecycle enforcer que decai Episodic→Cooling→Archived baseado em staleness + usefulness. (3) Wiring de usefulness_score → assembler budget allocation. Inspirar-se no MemCoder para intent mining do git history.

---

## 10. OBSERVABILITY & TRACING

### SOTA (2026)
- **OpenLLMetry**: OTEL spans sem modificar business logic
- **Langfuse**: Self-hostable, prompt versioning, trace replay
- **Decision Trace Schema** [@2026]: Governance evidence em real-time
- **Arize Phoenix**: Step-by-step execution graphs
- **Pydantic Logfire**: Traces queryáveis via SQL (agentes podem query própria observability)

### Theo Code — Status

| Aspecto | Status | Módulo | Evidência |
|---|---|---|---|
| Domain event bus | ✅ **IMPLEMENTED** | `event_bus.rs` | Pub/sub, bounded 10K, panic-safe |
| Structured JSONL logging | ✅ **IMPLEMENTED** | `observability.rs` | StructuredLogListener |
| Tool call tracing | ✅ **IMPLEMENTED** | `tool_call_manager.rs` | CallId + state transitions + timing |
| Session tree | ✅ **IMPLEMENTED** | `session_tree.rs` | Append-only JSONL com branching |
| Routing decision tracking | ✅ **IMPLEMENTED** | `routing/metrics.rs` | routing_reason por turn |
| Cost tracking (fields) | 🟡 **PARTIAL** | `budget.rs` ModelCost | Campos existem, sem aggregation |
| OpenTelemetry export | ❌ **MISSING** | — | Sem OTEL spans, sem distributed tracing |
| Trace UI/visualization | ❌ **MISSING** | — | Sem Phoenix/Langfuse equivalent |
| Prompt versioning | ❌ **MISSING** | — | System prompts não versionados |
| Self-queryable traces | ❌ **MISSING** | — | Agente não pode query própria observability |

**Score: 60%** — Boa observabilidade interna, invisível para ferramentas externas.

**Recomendação SOTA:** (1) OTEL exporter no EventBus (cada DomainEvent → OTEL span). (2) Cost aggregation por sessão/dia. (3) Prompt versioning via hash do system prompt.

---

## 11. VERIFICATION & EVALS

### SOTA (2026)
- **SWE-bench**: Standard benchmark (12.5% com SWE-agent → 50%+ com harnesses modernos)
- **AutoHarness** [@lou2026]: Automatic constraint synthesis. 100% legal actions [MEASURED]
- **AgentAssay**: Behavioral fingerprinting. 86% regression detection [MEASURED]
- **Who Tests the Testers** [@2026]: Meta-eval dos benchmarks existentes
- **4-dimension eval**: Outcome / Process / Style / Efficiency
- **JSONL trace capture**: Deterministic replay de prompts + responses + tool calls

### Theo Code — Status

| Aspecto | Status | Módulo | Evidência |
|---|---|---|---|
| TDD culture | ✅ **IMPLEMENTED** | Workspace-wide | 530+ testes, RED-GREEN-REFACTOR |
| Sandbox audit trail | ✅ **IMPLEMENTED** | `sandbox_audit.rs` | JSONL persistente com violations |
| Snapshot integrity | ✅ **IMPLEMENTED** | `snapshot.rs` | SHA checksum + schema_version |
| Risk alerts | ✅ **IMPLEMENTED** | `alerts.rs` | Community impact, untested mods |
| Self-verification (done gates) | 🟡 **PARTIAL** | `convergence.rs` | cargo test gate, sem verificação semântica |
| Benchmark pipeline | 🟡 **PARTIAL** | `apps/theo-benchmark` | Isolado, não integrado ao CI |
| **Eval framework** | ❌ **MISSING** | — | **Zero eval sistêmico** |
| LLM-as-judge | ❌ **MISSING** | — | Sem auto-avaliação de qualidade |
| Regression detection | ❌ **MISSING** | — | Sem behavioral fingerprinting |
| JSONL trace capture | ❌ **MISSING** | — | Events são logged mas sem replay framework |
| 4-dimension scoring | ❌ **MISSING** | — | Sem outcome/process/style/efficiency |

**Score: 40%** — **GAP #1 DO SISTEMA.** Sem eval, nenhuma melhoria é mensurável.

**Recomendação SOTA:** Criar `theo-eval` crate com: (1) JSONL trace capture (prompt+response+tools). (2) Eval dimensions (outcome: tests pass?, process: tool calls reasonable?, style: code quality, efficiency: tokens/time). (3) LLM-as-judge para scoring semântico. (4) Regression detector via fingerprinting de traces.

---

## 12. HUMAN-IN-THE-LOOP

### SOTA (2026)
- **AWS HITL 4 Patterns**: Hook System / Tool Context / Step Functions / MCP Elicitation
- **LangGraph**: Interrupt + breakpoint + approve com persistent state
- **HITL Lifelong Code Gen** [@2025]: Human-in-the-loop para aprendizado contínuo
- **Adaptive Confidence Gating** [@2026]: Confidence-based routing para review humano
- **Claude SDK**: approve-with-changes pattern + deny-and-continue recovery

### Theo Code — Status

| Aspecto | Status | Módulo | Evidência |
|---|---|---|---|
| Interrupt flag | ✅ **IMPLEMENTED** | `pilot.rs` AtomicBool | Stop the loop |
| Circuit breaker | ✅ **IMPLEMENTED** | `PilotConfig` | no_progress, same_error, rate limit |
| Interactive approval gates | ❌ **MISSING** | — | **Sem pause-ask-resume** |
| Approve-with-changes | ❌ **MISSING** | — | Sem "aceito mas mude X" |
| Confidence-based escalation | ❌ **MISSING** | — | Sem threshold → humano |
| Stateful pause/resume | ❌ **MISSING** | — | Snapshot existe mas sem trigger interativo |
| Deny-and-continue | ❌ **MISSING** | — | Denial = error, não "try alternative" |

**Score: 25%** — **GAP CRÍTICO.** Agente roda ou para. Nunca pergunta. Blocker para trust em produção.

**Recomendação SOTA:** (1) `ApprovalGate` no CapabilityGate: quando tool é destrutiva + fora de scope → pause + emit approval_request event. (2) CLI poll loop aguarda input. (3) Approve/Deny/ApproveWithChanges como respostas. (4) Timeout → deny (fail-safe).

---

## 13. DEBUGGING & DX

### SOTA (2026)
- **AgentTrace**: Causal graph root-cause em 0.12s (69x faster que LLM-based) [MEASURED]
- **TraceCoder**: Multi-agent debugging + Historical Lesson Learning
- **AgentDebug**: Taxonomy (memory/reflection/planning/action/system failures)
- **AgentPrism**: React components → OTEL traces → interactive viz

### Theo Code — Status

| Aspecto | Status | Módulo | Evidência |
|---|---|---|---|
| Failure classification | ✅ **IMPLEMENTED** | `reflector.rs` | NoProgressLoop, RepeatedSameError |
| Correction engine | ✅ **IMPLEMENTED** | `correction.rs` | RetryLocal→Replan→Subtask→AgentSwap |
| Error taxonomy | ✅ **IMPLEMENTED** | `error.rs` | Domain + LLM errors com routing hints |
| Failure learning | ✅ **IMPLEMENTED** | `context_metrics.rs` | Ring buffer(50), constraint synthesis ≥3 recorrências |
| Session replay | ❌ **MISSING** | — | Session tree append-only, sem replay tool |
| Trace visualization | ❌ **MISSING** | — | Sem interactive viz (AgentPrism-style) |
| Root-cause analysis | ❌ **MISSING** | — | Sem causal graph (AgentTrace-style) |

**Score: 55%** — Auto-correção forte (correction engine é sofisticado). DX para humanos é fraco.

**Recomendação SOTA:** (1) `theo replay <session_id>` command que reproduz a session tree step-by-step. (2) No desktop app: timeline visualization de tool calls + outcomes. (3) Failure taxonomy alinhada com AgentDebug (5 categorias).

---

## 14. LONG HORIZON EXECUTION

### SOTA (2026)
- **Ralph Loop**: Reinject prompt em contexto limpo + filesystem state
- **CMV** [@santoni2026]: Snapshot/branch/trim + DAG persistence
- **Pilot patterns**: Circuit breaker + git-based progress + rate limiting
- **Cross-session bootstrap**: progress.json injetado no boot

### Theo Code — Status

| Aspecto | Status | Módulo | Evidência |
|---|---|---|---|
| Pilot autonomous loop | ✅ **IMPLEMENTED** | `pilot.rs` | Circuit breaker, rate limit, exit conditions |
| Session persistence | ✅ **IMPLEMENTED** | `session_tree.rs` + `snapshot.rs` | JSONL tree + checksummed snapshots |
| Cross-session bootstrap | ✅ **IMPLEMENTED** | `session_bootstrap.rs` | .theo/progress.json no boot |
| Episode summaries | ✅ **IMPLEMENTED** | `episode.rs` | MachineEpisodeSummary structured |
| Git-based progress | ✅ **IMPLEMENTED** | `pilot.rs` | circuit_breaker_no_progress |
| Ralph Loop (reinject) | 🟡 **PARTIAL** | `pilot.rs` | Loop existe mas reinjeção não é explícita |
| Auto-resume from crash | ❌ **MISSING** | — | Snapshot + checksum existe, resume não é auto |
| Thought checkpoints | ❌ **MISSING** | — | Sem token accounting explícito por checkpoint |

**Score: 75%** — Forte. Pilot + session persistence + progress tracking funcionam.

**Recomendação SOTA:** (1) Auto-resume: ao iniciar, check se existe snapshot válido e oferecer continuação. (2) Thought checkpoints: marcar "decision points" explícitos para token accounting.

---

## MATRIZ RESUMO FINAL

| # | Categoria | Score | Trend vs SOTA |
|---|---|---|---|
| 1 | Agent Loop | **60%** | 🟡 Core sólido, falta middleware |
| 2 | Planning & Decomposition | **60%** | 🟡 Manual, sem auto-decomposition |
| 3 | Context & Compaction | **75%** | 🟢 GRAPHCTX é diferenciador |
| 4 | Tool Design | **65%** | 🟡 Schema SOTA, falta risk annotations |
| 5 | Bash & Code Exec | **95%** | 🟢 **Best-in-class** |
| 6 | Sandbox & Security | **85%** | 🟢 Forte no Linux |
| 7 | Skills & MCP | **20%** | 🔴 **GAP CRÍTICO** |
| 8 | Permissions & Auth | **45%** | 🟡 Binário, falta REQUIRE_APPROVAL |
| 9 | Memory & State | **50%** | 🔴 **Design sem implementação** |
| 10 | Observability & Tracing | **60%** | 🟡 Interno ok, externo missing |
| 11 | Verification & Evals | **40%** | 🔴 **GAP #1 do sistema** |
| 12 | Human-in-the-Loop | **25%** | 🔴 **Blocker para produção** |
| 13 | Debugging & DX | **55%** | 🟡 Auto-fix bom, DX humano fraco |
| 14 | Long Horizon | **75%** | 🟢 Pilot + persistence |

**Score Global: 58%** (média ponderada simples)

---

## ROADMAP PRIORITIZADO (baseado em evidência)

### P0 — Sem isso, nada mais importa

| # | Gap | Impacto Evidenciado | Módulo | Esforço |
|---|---|---|---|---|
| 1 | **theo-eval** | "Sem eval, nenhuma melhoria é mensurável" — AutoHarness prova que constraint validation é o mecanismo #1 | Novo crate `theo-eval` | Alto |
| 2 | **MemoryProvider concreto** | MemCoder: +9.4% SWE-bench. MemGPT: +60.4pp retrieval. Memory é o diferenciador medido. | `theo-agent-runtime` | Médio |

### P1 — Habilita o ecossistema

| # | Gap | Impacto Evidenciado | Módulo | Esforço |
|---|---|---|---|---|
| 3 | **MCP Client** | Ecossistema convergindo. ANX: 47-66% token reduction. Sem MCP = ecossistema fechado. | Novo: `theo-mcp-client` | Médio |
| 4 | **HITL Approval Gates** | Confidence gating + approve-with-changes. Blocker para trust. | `theo-agent-runtime` + `theo-governance` | Médio |
| 5 | **Risk Annotations** | Mind the GAP: text safety ≠ tool safety. Tool declarations habilitam auto-escalation. | `theo-domain/tool.rs` | Baixo |

### P2 — Qualidade e DX

| # | Gap | Impacto Evidenciado | Módulo | Esforço |
|---|---|---|---|---|
| 6 | **OTEL Exporter** | OpenLLMetry pattern. EventBus→OTEL spans. Habilita Langfuse/Phoenix. | `theo-agent-runtime` | Baixo |
| 7 | **LLM Summarization** | CMV: 39% reduction em mixed sessions. Compact stage é o último nível. | `compaction_stages.rs` | Médio |
| 8 | **Prompt Caching** | Break-even em 10 turns [MEASURED]. Wire à API Anthropic. | `theo-infra-llm` | Baixo |

### P3 — Diferenciação competitiva

| # | Gap | Impacto Evidenciado | Módulo | Esforço |
|---|---|---|---|---|
| 9 | **Middleware Chain** | Claude SDK 6-hook model. Interceptor > observer. | `theo-agent-runtime` | Médio |
| 10 | **Plan Reuse/Caching** | 93% reuse rate, 93.12% latency reduction [MEASURED] | `theo-agent-runtime` | Médio |
| 11 | **Session Replay** | AgentTrace: 69x faster root-cause. DX para debugging. | `theo-agent-runtime` | Médio |
| 12 | **Auto-decomposition** | Agents devem auto-decompor sem usuário triggerar Plan mode | `run_engine.rs` | Alto |

---

## FORÇAS COMPETITIVAS (onde Theo Code está à frente do mercado)

1. **Bash & Sandbox (95%)** — Landlock + rlimits + toxic sequences + audit. Mais completo que OpenHands (Docker-only) e SWE-agent (sem rlimits explícitos).

2. **GRAPHCTX Retrieval (MRR=0.914)** — BM25 + RRF 3-ranker + cross-encoder + community detection. Supera RAG genérico usado pela maioria dos concorrentes.

3. **State Machine Formal** — RunState com 6 estados validados + eventos publicados. Mais robusto que ReAct loops ad-hoc (Claude Code, Codex).

4. **Correction Engine** — RetryLocal→Replan→Subtask→AgentSwap escalation ladder. Poucos harnesses implementam auto-correção multi-nível.

5. **Sub-agent System** — Role-based isolation com capability gates + max_depth=1. Previne explosão recursiva que afeta AutoGen e CrewAI.

6. **Context Compaction (5-stage)** — Mais granular que compaction simples (Claude Code usa 1 nível). Protected tools garantem que contexto crítico nunca é perdido.

---

## REFERÊNCIAS-CHAVE

| Paper | Contribuição para este documento |
|---|---|
| ReAct [Yao 2022] | Define o agent loop canônico |
| SWE-agent [Yang 2024] | Prova que ACI > raw bash (+2.5x resolve rate) |
| MemGPT [Packer 2023] | 3-tier memory hierarchy (+60.4pp retrieval) |
| AutoHarness [Lou 2026] | Harness < model beating harness > model (Flash > Pro) |
| CMV [Santoni 2026] | DAG state management (20-86% token reduction) |
| MemCoder [Deng 2026] | Structured memory para code agents (+9.4% SWE-bench) |
| OpenHands [Wang 2024] | Reference architecture (Runtime/EventStream/Controller) |
| MPAC [Qian 2026] | Multi-principal protocol (95% overhead reduction) |
| ANX [Xu 2026] | Protocol-first critique of MCP (47-66% token reduction) |
| Mind the GAP [2026] | Text safety ≠ tool safety (separate enforcement needed) |
