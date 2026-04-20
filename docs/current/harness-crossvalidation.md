# Cross-Validation: Theo Code × Harness Engineering

> Data: 2026-04-20
> Referências: [awesome-harness-engineering](https://github.com/ai-boost/awesome-harness-engineering), MindStudio article (Stripe/Shopify/Airbnb), LangChain "Anatomy of an Agent Harness"

## Metodologia

Cada categoria do awesome-harness-engineering é mapeada contra módulos existentes no Theo Code. Status usa 3 níveis:

- **IMPLEMENTED** — funcional e testado
- **PARTIAL** — infraestrutura existe mas incompleta
- **MISSING** — não existe no sistema

---

## 1. AGENT LOOP

| Aspecto | Status | Módulo Theo Code | Detalhes |
|---|---|---|---|
| ReAct Loop (Think→Act→Observe) | **IMPLEMENTED** | `theo-agent-runtime/run_engine.rs` | State machine: Planning→Executing→Evaluating→Converged/Replanning/Aborted |
| State Machine formal | **IMPLEMENTED** | `theo-domain` (RunState) + `run_engine.rs` | 6 estados, transições validadas, eventos publicados |
| Extended Thinking / Reasoning depth | **PARTIAL** | `theo-infra-llm/routing` | `reasoning_effort` no RoutingChoice, mas sem `budget_tokens` explícito |
| Middleware/Hooks composáveis | **PARTIAL** | `event_bus.rs` + pilot hooks | EventBus pub/sub existe; hooks são listeners, não middleware chain composável |
| Interpreter persistence | **MISSING** | — | Não há persistência de estado de interpretador entre turns (ex: variáveis de shell) |

**Gap principal:** Não há hook middleware chain no estilo "6 composable hooks for cross-cutting concerns" (pre-tool, post-tool, pre-llm, post-llm, on-error, on-complete). O EventBus é observacional, não interceptor.

---

## 2. PLANNING & TASK DECOMPOSITION

| Aspecto | Status | Módulo Theo Code | Detalhes |
|---|---|---|---|
| Plan-and-Execute separation | **IMPLEMENTED** | `AgentMode::Plan` + `roadmap.rs` | Modo Plan escreve `.theo/plans/*.md`, modo Agent executa |
| Task lifecycle management | **IMPLEMENTED** | `task_manager.rs` + `theo-domain/task.rs` | FSM: Created→InProgress→Completed/Failed/Cancelled |
| Sub-agent spawning | **IMPLEMENTED** | `subagent/mod.rs` | 4 roles (Explorer/Implementer/Verifier/Reviewer), max_depth=1 |
| Parallel sub-agents | **IMPLEMENTED** | `subagent_parallel` meta-tool | `tokio::spawn` + `JoinSet` |
| Automatic task decomposition | **MISSING** | — | Agente não auto-decompõe; depende do usuário ou Plan mode |
| LATS / tree search over trajectories | **MISSING** | — | Sem backtracking ou exploration de alternativas |
| Initializer→Executor handoff | **PARTIAL** | `pilot.rs` + `session_bootstrap.rs` | Pilot reinicia loops, mas não há handoff formal entre agentes especializados |

**Gap principal:** O agente não decompõe automaticamente tasks complexas em sub-tasks. O Plan mode produz o plano, mas a execução sequencial das tasks do roadmap é manual.

---

## 3. CONTEXT DELIVERY & COMPACTION

| Aspecto | Status | Módulo Theo Code | Detalhes |
|---|---|---|---|
| Token budget management | **IMPLEMENTED** | `theo-domain/budget.rs` + `theo-engine-retrieval/budget.rs` | 4 constraints (time, tokens, iterations, tool_calls), BudgetAllocation com 5 slots |
| Multi-stage compaction | **IMPLEMENTED** | `compaction_stages.rs` | 5 níveis: None→Warning→Mask→Prune→Aggressive (70%→90% thresholds) |
| LLM-powered summarization | **MISSING** | `compaction_summary.rs` (template only) | Template existe, mas Compact stage está deferred |
| Prompt caching | **PARTIAL** | `theo-domain/budget.rs` | Campos `prompt_cache_tokens`, `cache_read_tokens` no ModelCost, mas não wired à API |
| Progressive disclosure (Skills) | **IMPLEMENTED** | `Tool::should_defer()` + `tool_search` | Deferred tools escondidos, surfaced sob demanda (Anthropic principle 12) |
| Tool call offloading | **IMPLEMENTED** | `truncation_rule()` per tool | Head/Tail/HeadTail truncation antes de append ao contexto |
| Retrieval-augmented context | **IMPLEMENTED** | `theo-engine-retrieval/pipeline.rs` + `assembly.rs` | BM25 + RRF 3-ranker + cross-encoder reranking + greedy knapsack packing |
| Autonomous context compression | **MISSING** | — | Agente não controla compaction via tool dedicada |

**Gap principal:** LLM-powered summarization (o nível mais avançado de compaction) está deferred. Prompt caching da Anthropic não está wired. O agente não tem agency sobre sua própria compaction.

---

## 4. TOOL DESIGN

| Aspecto | Status | Módulo Theo Code | Detalhes |
|---|---|---|---|
| Schema + JSON Schema generation | **IMPLEMENTED** | `theo-domain/tool.rs` ToolSchema | `to_json_schema()` OA-compatible, input_examples (72%→90% accuracy) |
| Categories | **IMPLEMENTED** | `ToolCategory` enum | FileOps, Search, Execution, Web, Orchestration, Utility |
| Deferred discovery | **IMPLEMENTED** | `should_defer()` + `search_hint()` | Progressive disclosure via `tool_search` meta-tool |
| Truncation rules | **IMPLEMENTED** | `truncation_rule()` per tool | Head/Tail/HeadTail + `llm_suffix` coaching |
| Input examples | **IMPLEMENTED** | `ToolSchema::input_examples` | Melhoria de accuracy documentada |
| Risk annotations (readOnlyHint, destructiveHint) | **MISSING** | — | Sem annotations de risco no schema. Categories existem mas não indicam mutabilidade |
| Constrained output (outlines/instructor) | **MISSING** | — | Sem constrained decoding ou schema enforcement no output |
| Tool state persistence | **MISSING** | — | Tools são stateless entre calls |

**Gap principal:** Tool risk annotations (MCP spec: `readOnlyHint`, `destructiveHint`, `idempotentHint`, `openWorldHint`) não existem. O sistema de permissões usa CapabilitySet (allow/deny por nome), mas as tools não declaram seu perfil de risco intrinsecamente.

---

## 5. BASH & CODE EXECUTION

| Aspecto | Status | Módulo Theo Code | Detalhes |
|---|---|---|---|
| Bash tool | **IMPLEMENTED** | `theo-tooling/bash.rs` (727 lines) | `sh -c` com piped stdio, streaming para TUI |
| Sandbox cascade | **IMPLEMENTED** | `sandbox/executor.rs` | Landlock→Noop (bwrap code existe mas não wired) |
| Command validation | **IMPLEMENTED** | `sandbox/command_validator.rs` | Lexical analysis para padrões perigosos |
| Environment sanitization | **IMPLEMENTED** | `sandbox/env_sanitizer.rs` | Whitelist + strip de AWS_*, GITHUB_TOKEN, OPENAI_API_KEY |
| Resource limits | **IMPLEMENTED** | `sandbox/rlimits.rs` | 64 procs, 512MB RAM, 120s CPU, 100MB file |
| Network isolation | **IMPLEMENTED** | `sandbox/network.rs` | unshare(NEWUSER\|NEWNET) |
| Toxic sequence detection | **IMPLEMENTED** | `theo-governance/sequence_analyzer.rs` | 6 patterns: payload_drop, exfil, force_push, ssh_key_exfil, env_exfil, reverse_shell |

**Status: FORTE.** Um dos módulos mais completos. Gap menor: bwrap não está no cascade ativo.

---

## 6. SANDBOX & SECURITY

| Aspecto | Status | Módulo Theo Code | Detalhes |
|---|---|---|---|
| Filesystem isolation | **IMPLEMENTED** | Landlock v4+ ruleset | ALWAYS_DENIED_READ/WRITE hardcoded (.ssh, .gnupg, .aws, etc.) |
| Network isolation | **IMPLEMENTED** | unshare namespaces | allow_network=false default, DNS independente |
| Process isolation | **IMPLEMENTED** | rlimits | CPU, memory, file size, nproc |
| Audit trail | **IMPLEMENTED** | `theo-governance/sandbox_audit.rs` | JSONL persistente (~/.config/theo/audit/YYYYMMDD.jsonl) |
| Sensitive file detection | **IMPLEMENTED** | `sandbox.rs` patterns | .env, credentials.json, secrets.yaml, id_rsa, id_ed25519 |
| bwrap (bubblewrap) | **PARTIAL** | `sandbox/bwrap.rs` (447 lines) | Código existe, não wired ao cascade |
| macOS sandbox | **PARTIAL** | `sandbox/macos.rs` (137 lines) | Stub |
| Container-level isolation | **MISSING** | — | Sem Docker/container-based sandbox |

**Status: FORTE** no Linux. Gaps em cross-platform (macOS) e container isolation.

---

## 7. SKILLS & MCP

| Aspecto | Status | Módulo Theo Code | Detalhes |
|---|---|---|---|
| Skills system | **IMPLEMENTED** | `theo-tooling/skill/mod.rs` | Catalog de skills, invocação via tool, permissions tracked |
| Skill versioning | **MISSING** | — | Sem versionamento de skills (SKILL.md v1, v2...) |
| MCP auth (OAuth PKCE) | **IMPLEMENTED** | `theo-infra-auth/mcp.rs` | Token storage, PKCE challenge, redirect URI |
| MCP resource discovery | **MISSING** | — | Sem `list_resources()` / `read_resource()` |
| MCP tool registration | **MISSING** | — | Sem adapter para wrappear MCP tools como `theo_domain::Tool` |
| MCP transport (SSE/stdio) | **MISSING** | — | Sem transport layer |
| A2A Protocol | **MISSING** | — | Sem agent-to-agent protocol |
| AG-UI Protocol | **MISSING** | — | Sem agent-to-frontend event protocol (desktop usa EventBus custom) |
| Composio-style SaaS integration | **MISSING** | — | Sem wrapper para APIs externas como actions |

**Gap principal:** MCP é o maior gap. Só auth skeleton existe. Toda a cadeia de discovery→registration→transport→execution está ausente. Isso limita a extensibilidade por terceiros.

---

## 8. PERMISSIONS & AUTHORIZATION

| Aspecto | Status | Módulo Theo Code | Detalhes |
|---|---|---|---|
| Capability-based access | **IMPLEMENTED** | `theo-domain/capability.rs` + `capability_gate.rs` | deny > allow > category check, enforced pre-dispatch |
| Per-role capability sets | **IMPLEMENTED** | `CapabilitySet::unrestricted()`, `read_only()` | Sub-agents recebem sets restritos por role |
| Permission rules (pattern matching) | **IMPLEMENTED** | `theo-domain/permission.rs` | Wildcard/glob matching, Allow/Ask/Deny actions |
| Multi-layer evaluation | **PARTIAL** | CapabilityGate only | Não há 5-layer (hooks→deny→mode→allow→canUseTool) como Claude SDK |
| Interactive approval flow | **MISSING** | — | Sem pause-and-ask antes de operações destrutivas |
| Deny-and-continue recovery | **MISSING** | — | Denial = error, não "try alternative" |
| Risk-based escalation | **MISSING** | — | Sem threshold de risco que escala para humano |
| Authorization Fabric (PEP/PDP) | **MISSING** | — | Sem endpoint centralizado de decisão de autorização |

**Gap principal:** Sem human approval flow. O CapabilityGate bloqueia ou permite, mas nunca pergunta. Não há "REQUIRE_APPROVAL" como no Microsoft Authorization Fabric.

---

## 9. MEMORY & STATE

| Aspecto | Status | Módulo Theo Code | Detalhes |
|---|---|---|---|
| MemoryProvider trait | **IMPLEMENTED** | `theo-domain/memory.rs` | prefetch, sync_turn, on_pre_compress, on_session_end |
| Memory fencing (XML tags) | **IMPLEMENTED** | `memory.rs` | `<memory-context>` com system-note |
| Memory lifecycle tiers | **IMPLEMENTED** | `theo-domain/episode.rs` | Active→Cooling→Archived, Ephemeral/Episodic/Reusable/Canonical |
| Episode summaries | **IMPLEMENTED** | `episode.rs` | MachineEpisodeSummary + human_summary + affected_files |
| Cross-session bootstrap | **IMPLEMENTED** | `session_bootstrap.rs` | `.theo/progress.json` injetado como system context |
| Concrete memory backend | **MISSING** | — | **MemoryProvider tem zero implementações concretas** |
| Memory decay/eviction | **MISSING** | — | Tiers definidos mas lifecycle transitions não enforced |
| Usefulness-based retrieval | **PARTIAL** | `context_metrics.rs` | Usefulness computado mas não fed back ao assembler |
| Facts as first-class objects | **MISSING** | — | Sem hash-addressed knowledge objects |

**Gap principal:** O trait MemoryProvider é elegante mas vazio — nenhuma implementação concreta. Memory é o módulo com maior distância entre design e implementação.

---

## 10. OBSERVABILITY & TRACING

| Aspecto | Status | Módulo Theo Code | Detalhes |
|---|---|---|---|
| Domain event bus | **IMPLEMENTED** | `event_bus.rs` | Pub/sub, bounded log (10K), panic-safe dispatch |
| Structured JSONL logging | **IMPLEMENTED** | `observability.rs` | StructuredLogListener, stdout ou file |
| Tool call tracing | **IMPLEMENTED** | `tool_call_manager.rs` | CallId, state transitions, timing, output truncado |
| Session tree (append-only) | **IMPLEMENTED** | `session_tree.rs` | JSONL com branching, compaction entries, model changes |
| Routing decision tracking | **IMPLEMENTED** | `theo-infra-llm/routing/metrics.rs` | routing_reason por turn |
| OpenTelemetry | **MISSING** | — | **Sem OTEL spans, sem distributed tracing** |
| Trace UI / visualization | **MISSING** | — | Sem Arize Phoenix / Langfuse equivalent |
| Cost tracking dashboard | **PARTIAL** | `theo-domain/budget.rs` ModelCost | Campos existem, sem aggregation ou dashboard |
| Prompt versioning | **MISSING** | — | System prompts não versionados |

**Gap principal:** Sem OpenTelemetry. O sistema tem boa observabilidade interna (event bus + JSONL), mas não exporta para ferramentas standard. Impossível correlacionar com infra externa.

---

## 11. VERIFICATION & EVALS

| Aspecto | Status | Módulo Theo Code | Detalhes |
|---|---|---|---|
| Sandbox audit trail | **IMPLEMENTED** | `sandbox_audit.rs` | JSONL persistente com violations |
| Risk alerts | **IMPLEMENTED** | `theo-governance/alerts.rs` | Community impact, untested mods |
| Snapshot integrity | **IMPLEMENTED** | `snapshot.rs` | SHA checksum, schema_version |
| Test framework (TDD) | **IMPLEMENTED** | Workspace-wide | Unit + integration, strong TDD culture |
| Output validation / eval harness | **MISSING** | — | **Sem eval framework** |
| LLM-as-judge | **MISSING** | — | Sem auto-avaliação de qualidade do output |
| Regression detection | **MISSING** | — | Sem AgentAssay-style behavioral fingerprinting |
| Benchmark pipeline | **PARTIAL** | `apps/theo-benchmark` | Existe mas isolado, não integrado ao CI |
| Self-verification loops | **PARTIAL** | `convergence.rs` (done tool gates) | Cargo test como gate, mas sem verificação semântica |

**Gap principal:** **Eval é o maior gap arquitetural.** Sem framework para medir qualidade do agente sistematicamente. Shopify trata eval como QA — nós não temos isso.

---

## 12. HUMAN-IN-THE-LOOP

| Aspecto | Status | Módulo Theo Code | Detalhes |
|---|---|---|---|
| Interrupt flag | **IMPLEMENTED** | `pilot.rs` AtomicBool | Pode parar o loop |
| Circuit breaker | **IMPLEMENTED** | `PilotConfig` | no_progress, same_error, rate limit |
| Interactive approval gates | **MISSING** | — | **Sem pause-ask-resume para operações sensíveis** |
| Approve-with-changes | **MISSING** | — | Sem "aceito mas mude X" |
| Review UI | **MISSING** | — | Sem interface de review para outputs |
| Confidence-based escalation | **MISSING** | — | Sem threshold de confiança que escala para humano |
| Stateful pause/resume | **MISSING** | — | Snapshot existe mas sem trigger de pause interativo |

**Gap principal:** Zero HITL beyond interrupt. O agente roda ou para — nunca pergunta. Isso é blocker para confiança em produção.

---

## 13. DEBUGGING & DX

| Aspecto | Status | Módulo Theo Code | Detalhes |
|---|---|---|---|
| Failure classification | **IMPLEMENTED** | `reflector.rs` | NoProgressLoop, RepeatedSameError |
| Correction engine | **IMPLEMENTED** | `correction.rs` | RetryLocal→Replan→Subtask→AgentSwap escalation |
| Error taxonomy | **IMPLEMENTED** | `theo-domain/error.rs` + `theo-infra-llm/error.rs` | Domain errors + LLM errors com routing hints |
| Failure learning | **IMPLEMENTED** | `context_metrics.rs` | Ring buffer (50), constraint synthesis em ≥3 recorrências |
| Session replay / time-travel | **MISSING** | — | Session tree é append-only mas sem replay tool |
| Trace visualization | **MISSING** | — | Sem AgentPrism-style interactive viz |
| Root-cause analysis tool | **MISSING** | — | Sem AgentTrace-style causal graph |

**Gap principal:** DX tooling para debugging humano é fraco. O agente se auto-corrige bem, mas o desenvolvedor não tem ferramentas para inspecionar o que aconteceu.

---

## 14. LONG HORIZON EXECUTION

| Aspecto | Status | Módulo Theo Code | Detalhes |
|---|---|---|---|
| Pilot / autonomous loop | **IMPLEMENTED** | `pilot.rs` | Circuit breaker, rate limiting, exit conditions |
| Session persistence | **IMPLEMENTED** | `session_tree.rs` + `snapshot.rs` | JSONL tree + checksummed snapshots |
| Cross-session bootstrap | **IMPLEMENTED** | `session_bootstrap.rs` | `.theo/progress.json` injetado no boot |
| Episode summaries | **IMPLEMENTED** | `episode.rs` | Structured summaries para continuação |
| Ralph Loop (reinject in clean context) | **PARTIAL** | `pilot.rs` | Loop existe mas sem "reinject original prompt em contexto limpo" explícito |
| Auto-resume from crash | **MISSING** | — | Snapshot + checksum existe mas resume não é auto-triggered |
| Thought checkpoints | **MISSING** | — | Sem checkpoints explícitos de raciocínio para token accounting |
| Git-based progress tracking | **IMPLEMENTED** | `pilot.rs` circuit_breaker_no_progress | Detecta N loops sem mudanças git |

---

## MATRIZ RESUMO

| Categoria | Implementado | Parcial | Ausente | Score |
|---|---|---|---|---|
| **Agent Loop** | 2 | 2 | 1 | 🟡 70% |
| **Planning & Decomposition** | 3 | 1 | 3 | 🟡 55% |
| **Context & Compaction** | 5 | 1 | 2 | 🟢 75% |
| **Tool Design** | 5 | 0 | 3 | 🟡 65% |
| **Bash & Code Exec** | 7 | 0 | 0 | 🟢 100% |
| **Sandbox & Security** | 6 | 2 | 1 | 🟢 85% |
| **Skills & MCP** | 2 | 0 | 7 | 🔴 20% |
| **Permissions & Auth** | 3 | 1 | 4 | 🟡 45% |
| **Memory & State** | 5 | 1 | 3 | 🟡 60% |
| **Observability & Tracing** | 5 | 1 | 3 | 🟡 65% |
| **Verification & Evals** | 4 | 2 | 3 | 🟡 55% |
| **Human-in-the-Loop** | 2 | 0 | 5 | 🔴 25% |
| **Debugging & DX** | 4 | 0 | 3 | 🟡 55% |
| **Long Horizon** | 5 | 1 | 2 | 🟢 75% |

---

## TOP 5 GAPS CRÍTICOS (por impacto)

| # | Gap | Impacto | Módulo Afetado |
|---|---|---|---|
| **1** | **Eval Framework inexistente** | Sem forma de medir se o agente está melhorando ou regredindo. Impossível iterar com confiança. | Novo: `theo-eval` |
| **2** | **MCP não funcional** | Sem extensibilidade por terceiros. Skills são hardcoded. Ecossistema fechado. | `theo-tooling` + novo: MCP client |
| **3** | **Human-in-the-Loop ausente** | Agente roda ou para. Nunca pergunta. Blocker para trust em produção. | `theo-agent-runtime` + `theo-governance` |
| **4** | **MemoryProvider sem implementação** | Design elegante com zero backend. Agente não lembra nada entre sessões de verdade. | `theo-agent-runtime` (concrete impl) |
| **5** | **OpenTelemetry ausente** | Observabilidade interna ok, mas invisível para ferramentas standard. Não integrável com infra externa. | Novo: OTEL exporter no `event_bus` |

---

## MÓDULOS QUE NÃO EXISTEM (candidatos a criação)

| Módulo Proposto | Responsabilidade | Referências |
|---|---|---|
| `theo-eval` | Eval framework: outcome/process/style/efficiency dimensions, JSONL traces, LLM-as-judge, regression detection | promptfoo, AgentAssay, OpenAI Eval Framework |
| `theo-mcp-client` | MCP transport (stdio/SSE), resource discovery, tool registration adapter | Model Context Protocol spec, Composio |
| `theo-otel` | OpenTelemetry exporter: spans from EventBus, tool call traces, LLM call instrumentation | OpenLLMetry, Arize Phoenix |
| HITL module (in `theo-agent-runtime`) | Approval gates, pause/resume, confidence-based escalation, approve-with-changes | AWS HITL Patterns, LangGraph interrupts |
| Memory backend (in `theo-agent-runtime`) | Concrete MemoryProvider: filesystem-backed, 3-tier (core/archival/recall), decay enforcement | Letta/MemGPT, mem0 |

---

## FORÇAS DO SISTEMA (onde Theo Code está à frente)

1. **Bash & Sandbox** (100%) — Landlock + rlimits + toxic sequence detection + audit trail. Mais completo que a maioria dos harnesses open-source.
2. **GRAPHCTX retrieval** — BM25 + RRF 3-ranker + cross-encoder + community detection é state-of-the-art para code retrieval. MRR=0.914.
3. **State machine formal** — RunState com transições validadas e eventos publicados. Mais robusto que ReAct loops ad-hoc.
4. **Sub-agent system** — Role-based isolation com capability gates. Max_depth=1 previne explosão recursiva.
5. **Correction engine** — RetryLocal→Replan→Subtask→AgentSwap é um escalation ladder sofisticado que poucos harnesses implementam.
