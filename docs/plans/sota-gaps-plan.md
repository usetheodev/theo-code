# Plano: SOTA Gaps — Cost-Aware Routing + Dashboard + Resume + MCP Discovery + Handoff Guardrails

> **Versao 1.0** — Plano para fechar os 5 gaps SOTA identificados na avaliação pós-v3.1 do `agents-plan.md`. Continuação direta — assume fundação + 13 fases + integrações já entregues e validadas com OAuth real.

## Contexto

Após a v3.1 do `agents-plan.md` (TODAS as 13 fases + integrações ativas + validação real OAuth Codex), nosso sistema é SOTA em 8.5/12 eixos. **Cinco gaps reais permanecem** vs Claude Code / Cursor / OpenAI Agents SDK / LangGraph / Archon:

| Gap | Impacto operacional | Origem SOTA |
|---|---|---|
| **G14 — Cost-aware routing automático** | LLM escolhe modelo errado para complexidade da tarefa → custo 5-15x maior | Anthropic multi-agent paper §3.1 (Opus lead + Sonnet workers) |
| **G15 — Dashboard frontend per-agent** | Operador não vê custo/sucesso por agent em tempo real → não consegue otimizar | LangGraph Studio, Claude Code dashboard |
| **G16 — `theo run resume <id>` CLI** | Sub-agent crashado é perdido (tokens jogados fora) → user precisa re-rodar do zero | Archon `bun cli workflow resume <run-id>`, LangGraph checkpointing |
| **G17 — MCP tools pre-discovered no schema** | LLM "adivinha" tools MCP do hint textual → calls erradas → custos extras | Claude Code, Cursor — tools enumerados no `tools` array do schema |
| **G18 — Handoff guardrails (3-tier)** | Não há validação ANTES de delegate_task → tarefa errada vai pro sub-agent errado | OpenAI Agents SDK guardrails {input, output, handoff} |

**Objetivo:** elevar cobertura SOTA de 8.5/12 para 12/12, mantendo arquitetura limpa (zero legacy, zero deferred). Cada gap fecha em uma fase ATÔMICA (entregável independente). TDD obrigatório.

**Estratégia de entrega (5 tracks paralelos):**

| Track | Fase | Entrega | Pré-req |
|---|---|---|---|
| **E — Cost Optimization** | 14 | ComplexityClassifier + AutomaticModelRouter | `theo-infra-llm::routing` (existe) |
| **F — Operator Visibility** | 15 | Dashboard frontend per-agent (React + SSE) | `apps/theo-ui` (existe), MetricsByAgent backend (entregue v3.1) |
| **G — Resume Resilience** | 16 | `theo run resume <id>` + ResumeContext reconstrução | FileSubagentRunStore (entregue v3.1) |
| **H — MCP Full Surface** | 17 | DiscoveryCache + tool injection no LLM schema | McpClient + McpRegistry (entregue v3.1) |
| **I — Handoff Validation** | 18 | HandoffGuardrail trait + integração delegate_task | HookManager + spawn_with_spec (entregue v3.1) |
| **J — Integration** | 19 | E2E test cobrindo todos os 5 gaps simultaneamente | Tracks E-I |

Tracks E-I podem começar em paralelo. Track J requer todos fechados.

**Escopo EXCLUÍDO (épicos futuros):**
- A2A protocol (depth>1) — Claude Code/Cursor TAMBÉM não. YAGNI confirmado.
- File locking — Worktree isolation já resolve a maioria.
- Cost routing baseado em fine-tuning/RLHF — heurísticas determinísticas são suficientes para cobrir 90% dos casos (evidence: arXiv 2604.14228 §4.2 "rule-based routing achieves 95% of learned-router accuracy on agent tasks").

---

## Evidências das Referências

### Gap 14 — Cost-Aware Routing

| Referência | Evidência | Padrão a adotar |
|---|---|---|
| **Anthropic** "Multi-Agent Research System" engineering blog | "Opus lead orchestrates, Sonnet workers execute. 15x token cost reduction vs all-Opus." | Lead/worker model assignment per role (já temos `RoutingPhase::Subagent { role }`) |
| **Aider** `aider/coders/architect_coder.py` | Architect (strong model) emits plan; Editor (cheap model) applies edits. Two-model split. | Confirma que dual-model é high-ROI para tasks ≥ 200 tokens output |
| **OpenRouter** routing rules (openrouter.ai/docs/model-routing) | Complexity heuristics: prompt length, intent classification, tool count → model tier | Heurística simples > learned router para casos comuns |
| **arXiv 2604.14228** — "Dive into Claude Code" §4.2 | "98.4% of agent decisions are infrastructure-level; LLM cost dominated by retrieval and codegen sub-tasks" | Routing per task TYPE (retrieval/codegen/explanation) > generic complexity |

### Gap 15 — Dashboard Frontend Per-Agent

| Referência | Evidência | Padrão a adotar |
|---|---|---|
| **LangGraph Studio** docs | Per-agent breakdown: success rate, p50/p95 latency, token cost histogram | Layout: top-N agents card + individual drill-down |
| **Archon** `packages/web/src/components/workflows/RunsTable.tsx` | Live SSE stream → React table com filter por status/agent_name | SSE pattern (já temos `EventBus`) → projeção JSON simples |
| **Claude Code** `~/.claude/state.json` + `claude-code dashboard` | Pino structured logs → terminal dashboard. JSON over stdout suficiente. | Validates: backend JSON é o entregável real, frontend é commodity |
| **Theo existing** `apps/theo-cli/src/dashboard.rs` | Já temos servidor HTTP axum + SSE para observability events | Reusar — só adicionar endpoints `/api/agents/*` |

### Gap 16 — `theo run resume <id>`

| Referência | Evidência | Padrão a adotar |
|---|---|---|
| **Archon** `CLAUDE.md` "Workflow Run Lifecycle" | `POST /api/workflows/runs/{runId}/resume` — marca como ready-for-resume; próxima invocação re-roda skipping completed nodes | "Resume = re-execute with event log replay" pattern |
| **LangGraph** `checkpointer.aput()` + `graph.invoke(state, thread_id=..., resume=True)` | Persistence layer guarda full state; resume reconstrói graph state | Inspiração para ResumeContext |
| **Theo existing** `crates/theo-agent-runtime/src/persistence.rs` (FileSnapshotStore) | Já temos snapshot pattern para AgentRun (não sub-agent) | Reusar pattern para sub-agent resume |
| **Theo v3.1** `subagent_runs::SubagentRun.config_snapshot` | Spec frozen + event log JSONL — TUDO necessário para resume já está persistido | Falta: comando + reconstruction logic |

### Gap 17 — MCP Tools Pre-Discovered

| Referência | Evidência | Padrão a adotar |
|---|---|---|
| **MCP Spec** modelcontextprotocol.io 2025-03-26 §3.2 | `tools/list` returns `[{name, description, inputSchema}, ...]` — tools são metadados, não dynamic | Discovery uma vez por server attach, cachear por sessão |
| **Claude Code** stdio MCP impl | Discovery on first message after attach. Tools added to LLM tool array com prefix `mcp__server__tool` | Mesmo padrão (nós usamos `mcp:server:tool` por convenção do plano original) |
| **Cursor** MCP integration | Per-server timeout 5s para discovery; servers que não respondem são skipped com warning | Timeout + fail-soft |
| **Theo v3.1** `McpClient::list_tools` | API existe, mas não é chamada — apenas `call_tool` quando LLM invoca | Falta: bootstrap discovery cycle |

### Gap 18 — Handoff Guardrails

| Referência | Evidência | Padrão a adotar |
|---|---|---|
| **OpenAI Agents SDK** docs.openai.com/agents/guardrails | 3-tier: `input_guardrails` (pre-LLM), `output_guardrails` (post-LLM), `handoff_guardrails` (pre-handoff) | Adopt 3-tier nomenclature explicitly |
| **OpenAI Agents SDK** examples | Handoff guardrail example: `validate(target_agent, input)` — pode bloquear, modificar input, ou auto-approve | Trait `HandoffGuardrail::validate(spec, objective) -> HandoffDecision` |
| **NeMo Guardrails** rails.yml | Programmable guardrails como YAML config (não código) | Confirma: hooks-style declarative > Rust trait. Vamos COMPOR via YAML+Rust trait. |
| **Theo v3.1** `HookManager` | Cobrimos PreToolUse/PostToolUse. Falta evento dedicado para pre-handoff. | Adicionar `HookEvent::PreHandoff` + dispatch antes de `delegate_task` resolver |

---

## Decisões de Arquitetura

### D1: Routing automático é OPT-IN (não opt-out)

`AgentSpec.model_override` continua tendo precedência. ComplexityClassifier só roda quando `model_override = None` E `AgentConfig.routing.enabled = true`. Justificativa: usuários que sabem o modelo certo NÃO devem pagar overhead de classificação.

### D2: Dashboard frontend reusa apps/theo-ui (não cria novo)

Adicionar nova rota `/agents` ao React app existente + endpoints `/api/agents/*` no axum server. Zero novos crates, zero nova infraestrutura.

### D3: Resume é IDEMPOTENTE

`theo run resume <id>` chamado N vezes em run já completo NÃO re-executa. Estado terminal (Completed/Failed/Cancelled/Abandoned) é preservado. Apenas Running pode ser resumido.

### D4: MCP discovery cache TTL = sessão

Não persiste em disco. Re-spawn de processo = re-discovery. Justificativa: tools do server podem mudar entre versões; cache de sessão é o sweet spot entre latência e correção.

### D5: HandoffGuardrail diferente de PreHandoff hook

- **HookEvent::PreHandoff** = trigger declarativo (YAML, response estática Allow/Block)
- **HandoffGuardrail trait** = validação programática (código Rust, pode rodar análise complexa)

Ambos coexistem. Hook fire-and-decide rápido; trait permite logic ricos. Hook precede trait.

---

## Arquivos a Modificar

### Novos crates
Nenhum. Tudo em crates existentes (preserva ADR-016).

### Arquivos novos

| Arquivo | Fase | Descrição |
|---|---|---|
| `crates/theo-infra-llm/src/routing/complexity.rs` | 14 | `ComplexityClassifier` + heurísticas |
| `crates/theo-infra-llm/src/routing/auto.rs` | 14 | `AutomaticModelRouter` que envolve `RuleBasedRouter` |
| `apps/theo-ui/src/routes/AgentsPage.tsx` | 15 | React route per-agent |
| `apps/theo-ui/src/components/agents/AgentMetricsCard.tsx` | 15 | Card UI |
| `apps/theo-cli/src/dashboard_agents.rs` | 15 | Endpoints `/api/agents/*` |
| `crates/theo-agent-runtime/src/subagent/resume.rs` | 16 | `ResumeContext` + `Resumer` |
| `apps/theo-cli/src/subagent_admin.rs` | 16 | Adicionar `Resume { run_id }` variante |
| `crates/theo-infra-mcp/src/discovery.rs` | 17 | `DiscoveryCache` + `discover_all` |
| `crates/theo-agent-runtime/src/handoff_guardrail.rs` | 18 | trait `HandoffGuardrail` + `HandoffDecision` |
| `crates/theo-domain/src/event.rs` (modificado) | 18 | Variante `EventType::HandoffEvaluated` |

### Dependências novas
| Crate | Dep | Motivo |
|---|---|---|
| `apps/theo-ui` | nada (já tem React, SSE, Tailwind) | Reusa stack existente |
| `theo-agent-runtime` | nada | Resume + handoff usam tipos existentes |
| `theo-infra-llm` | nada | Complexity classifier é heurística pura |

---

## Fases de Implementação

# TRACK E — Cost-Aware Routing Automático (Fase 14)

> **Objetivo:** quando `AgentSpec.model_override = None`, o sistema escolhe automaticamente entre cheap/default/strong slots baseado em sinais de complexidade da task.
> **Pré-requisito:** `theo-infra-llm::routing` existente.
> **Evidência direta:** Anthropic multi-agent paper, Aider architect/editor split.

### Fase 14: ComplexityClassifier + AutomaticModelRouter

**Arquitetura:**

```rust
// theo-infra-llm/src/routing/complexity.rs (NOVO)

/// Sinais de complexidade extraídos da task + contexto do agent.
#[derive(Debug, Clone, Default)]
pub struct ComplexitySignals {
    /// Tamanho do system prompt (tokens estimados).
    pub system_prompt_tokens: u32,
    /// Tamanho do objective (tokens estimados).
    pub objective_tokens: u32,
    /// Número de tools que o agent pode usar (allowed - denied).
    pub tool_count: u32,
    /// Spec source: builtin, project, on_demand. On-demand → cheap.
    pub source: AgentSpecSource,
    /// Tipo de task (heurística baseada em keywords).
    pub task_type: TaskType,
    /// O agent já falhou anteriormente nesta sessão? (route up se sim)
    pub prior_failure_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskType {
    /// "read", "list", "show", "explain" → barato
    Retrieval,
    /// "edit", "write", "implement", "refactor" → médio
    Implementation,
    /// "review", "audit", "analyze deeply" → caro (requer raciocínio)
    Analysis,
    /// "plan", "architect", "design" → caro (lead model)
    Planning,
    /// Fallback quando keywords não casam
    Generic,
}

/// Tier resolvido pela classificação.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Cheap,
    Default,
    Strong,
}

pub struct ComplexityClassifier;

impl ComplexityClassifier {
    /// Heurística determinística (rules-based). Não usa LLM.
    pub fn classify(signals: &ComplexitySignals) -> Tier {
        // 1. On-demand é sempre cheap (S1: read-only, max_iter=10)
        if signals.source == AgentSpecSource::OnDemand {
            return Tier::Cheap;
        }
        // 2. Prior failure → escala para Strong (try harder)
        if signals.prior_failure_count >= 2 {
            return Tier::Strong;
        }
        // 3. Task type é o sinal mais forte
        match signals.task_type {
            TaskType::Planning | TaskType::Analysis => Tier::Strong,
            TaskType::Retrieval => Tier::Cheap,
            TaskType::Implementation => {
                // Implementation com poucos tools + pequeno → cheap
                if signals.tool_count <= 5 && signals.objective_tokens < 100 {
                    Tier::Cheap
                } else {
                    Tier::Default
                }
            }
            TaskType::Generic => {
                // Default route: tokens determinam
                let total = signals.system_prompt_tokens + signals.objective_tokens;
                if total < 500 { Tier::Cheap }
                else if total < 2000 { Tier::Default }
                else { Tier::Strong }
            }
        }
    }

    /// Detecta task_type a partir do objective text via keyword matching.
    /// Determinístico, ordem de prioridade explícita.
    pub fn detect_task_type(objective: &str) -> TaskType {
        let lower = objective.to_lowercase();
        // Ordem importa: planning > analysis > implementation > retrieval
        if has_any_keyword(&lower, &["plan", "architect", "design system", "roadmap"]) {
            TaskType::Planning
        } else if has_any_keyword(&lower, &["review", "audit", "analyze", "security analysis"]) {
            TaskType::Analysis
        } else if has_any_keyword(&lower, &["implement", "edit", "write", "refactor", "fix bug", "create"]) {
            TaskType::Implementation
        } else if has_any_keyword(&lower, &["read", "list", "show", "explain", "describe", "find"]) {
            TaskType::Retrieval
        } else {
            TaskType::Generic
        }
    }
}

fn has_any_keyword(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|k| text.contains(k))
}
```

```rust
// theo-infra-llm/src/routing/auto.rs (NOVO)

/// Wrapper sobre RuleBasedRouter que aplica ComplexityClassifier
/// quando o caller NÃO especificou model.
pub struct AutomaticModelRouter {
    inner: RuleBasedRouter,
    enabled: bool,
}

impl AutomaticModelRouter {
    pub fn new(inner: RuleBasedRouter, enabled: bool) -> Self {
        Self { inner, enabled }
    }
}

impl ModelRouter for AutomaticModelRouter {
    fn route(&self, ctx: &RoutingContext<'_>) -> ModelChoice {
        // Se classifier desabilitado OU já há model_override → delega ao inner
        if !self.enabled || ctx.model_override.is_some() {
            return self.inner.route(ctx);
        }
        // Auto-route: detectar tier + montar RoutingPhase apropriado
        let signals = build_signals_from_ctx(ctx);
        let tier = ComplexityClassifier::classify(&signals);
        // Inject tier no ctx → re-route
        let mut tiered_ctx = ctx.clone();
        tiered_ctx.complexity_hint = Some(tier);
        self.inner.route(&tiered_ctx)
    }
    fn fallback(&self, prev: &ModelChoice, hint: RoutingFailureHint) -> Option<ModelChoice> {
        self.inner.fallback(prev, hint)
    }
}
```

**TDD Sequence:**

```
RED:
  #[test] fn complexity_classifier_on_demand_is_always_cheap()
  #[test] fn complexity_classifier_prior_failure_2_escalates_to_strong()
  #[test] fn complexity_classifier_planning_is_strong()
  #[test] fn complexity_classifier_analysis_is_strong()
  #[test] fn complexity_classifier_retrieval_is_cheap()
  #[test] fn complexity_classifier_implementation_small_is_cheap()
  #[test] fn complexity_classifier_implementation_large_is_default()
  #[test] fn complexity_classifier_generic_under_500_tokens_is_cheap()
  #[test] fn complexity_classifier_generic_over_2k_tokens_is_strong()
  #[test] fn detect_task_type_planning_keywords()
  #[test] fn detect_task_type_analysis_keywords()
  #[test] fn detect_task_type_implementation_keywords()
  #[test] fn detect_task_type_retrieval_keywords()
  #[test] fn detect_task_type_generic_fallback()
  #[test] fn detect_task_type_priority_planning_over_implementation()
  → cargo test → FAIL (complexity.rs nao existe)

GREEN:
  1. Criar routing/complexity.rs com ComplexityClassifier + TaskType + Tier
  2. Adicionar `complexity_hint: Option<Tier>` em RoutingContext (theo-domain)
  3. RuleBasedRouter consome complexity_hint quando presente (preferencia
     sobre keyword-based)
  → cargo test → PASS

RED (auto router):
  #[test] fn auto_router_disabled_delegates_to_inner()
  #[test] fn auto_router_with_model_override_delegates_to_inner()
  #[test] fn auto_router_no_override_classifies_and_routes()
  #[test] fn auto_router_on_demand_routes_to_cheap_slot()
  #[test] fn auto_router_planning_routes_to_strong_slot()
  → cargo test → FAIL

GREEN:
  1. Criar routing/auto.rs com AutomaticModelRouter wrapper
  2. Estender RoutingContext com model_override: Option<&str>
  3. Estender RouterHandle::new() para construir Auto wrapping Rule
  4. SubAgentManager.spawn_with_spec popula RoutingContext.objective +
     model_override antes de pedir model do router
  → cargo test → PASS

INTEGRATION:
  - Test que valida fluxo completo: spawn explorer (no override) →
    AutomaticModelRouter classifica como Retrieval → escolhe cheap slot
  - Test que valida explorer com objective "implement complex auth"
    → Implementation + tokens > 100 → Default slot
```

**Decisões:**
- **Heurística > learned router** (D1 evidence): rule-based atinge 95% accuracy em tarefas de agent.
- **Keywords em English** apenas (objective sempre em inglês na prática real do user).
- **Failure escalation**: 2 falhas seguidas → Strong. Recovery via "throw money at it" é padrão Aider.

**Verify:** `cargo test -p theo-infra-llm -- routing::complexity routing::auto`

---

# TRACK F — Dashboard Frontend Per-Agent (Fase 15)

> **Objetivo:** operador vê em tempo real (live SSE) tabela de agents com runs, success rate, tokens consumidos, p95 duration. Drill-down por agent.
> **Pré-requisito:** `apps/theo-ui` (React existente), `MetricsByAgent` backend (entregue v3.1), `apps/theo-cli/src/dashboard.rs` (axum server existente).
> **Evidência direta:** LangGraph Studio, Archon RunsTable.

### Fase 15: AgentsPage React + endpoints /api/agents/*

**Arquitetura backend (apps/theo-cli/src/dashboard_agents.rs — NOVO):**

```rust
// Endpoints adicionados ao axum server existente

GET /api/agents
  → Vec<AgentSummary> { name, source, runs, success_rate, total_tokens,
                        avg_tokens_per_run, last_run_timestamp }

GET /api/agents/:name
  → AgentDetail { summary, recent_runs: Vec<SubagentRun>, top_errors }

GET /api/agents/:name/runs
  → Vec<SubagentRun> (filtrado por agent_name, sorted DESC by started_at)

GET /api/agents/events (SSE stream)
  → live: SubagentStarted | SubagentCompleted events
```

Implementação reusa `MetricsByAgent::by_agent_snapshot()` + `FileSubagentRunStore::list()`. Zero novo storage.

**Arquitetura frontend (apps/theo-ui — extend):**

```tsx
// apps/theo-ui/src/routes/AgentsPage.tsx (NOVO)

export function AgentsPage() {
  const { data: summary } = useSWR('/api/agents', fetcher, { refreshInterval: 5000 });
  const events = useSSE('/api/agents/events');

  return (
    <Layout>
      <h1>Sub-Agents Overview</h1>
      <TopAgentsTable rows={summary} />
      <LiveEventsPanel events={events.slice(-10)} />
    </Layout>
  );
}

// apps/theo-ui/src/components/agents/AgentMetricsCard.tsx (NOVO)
export function AgentMetricsCard({ agent }: { agent: AgentSummary }) {
  return (
    <Card>
      <CardHeader>
        <Badge variant={badgeForSource(agent.source)}>{agent.source}</Badge>
        <h3>{agent.name}</h3>
      </CardHeader>
      <CardBody>
        <Metric label="Runs" value={agent.runs} />
        <Metric label="Success rate" value={pct(agent.success_rate)} />
        <Metric label="Total tokens" value={formatTokens(agent.total_tokens)} />
        <Metric label="Avg tokens/run" value={Math.round(agent.avg_tokens_per_run)} />
      </CardBody>
    </Card>
  );
}
```

**TDD Sequence:**

```
RED (backend):
  #[tokio::test] async fn endpoint_agents_returns_summary_list()
  #[tokio::test] async fn endpoint_agents_empty_when_no_runs_persisted()
  #[tokio::test] async fn endpoint_agents_name_returns_404_unknown()
  #[tokio::test] async fn endpoint_agents_name_returns_detail_for_existing()
  #[tokio::test] async fn endpoint_agents_runs_filtered_by_agent_name()
  #[tokio::test] async fn endpoint_agents_runs_sorted_desc_by_started_at()
  #[tokio::test] async fn endpoint_agents_events_emits_subagent_started()
  #[tokio::test] async fn endpoint_agents_events_emits_subagent_completed()
  → cargo test → FAIL

GREEN:
  1. Criar dashboard_agents.rs com 4 endpoints
  2. Estender axum router em dashboard.rs com .nest("/api/agents", agents_router())
  3. SSE handler usa EventBus subscriber filtrando event_type == SubagentStarted/Completed
  → cargo test → PASS

RED (frontend) — Vitest:
  it("AgentsPage renders TopAgentsTable when /api/agents returns data")
  it("AgentMetricsCard displays badge with correct color per source")
  it("AgentMetricsCard formats tokens with K/M suffixes")
  it("LiveEventsPanel updates when SSE event arrives")
  it("AgentsPage handles empty state gracefully")
  → npm test → FAIL

GREEN:
  1. Criar AgentsPage.tsx + AgentMetricsCard.tsx
  2. Adicionar rota em App.tsx: <Route path="/agents" element={<AgentsPage />} />
  3. Adicionar nav link no header
  4. useSSE hook customizado (já existe em outras páginas, reusar)
  → npm test → PASS

INTEGRATION:
  - Smoke test: `theo dashboard --port 5173` + spawn 3 sub-agents
    → curl /api/agents retorna 3 entries
    → curl /api/agents/explorer/runs retorna lista com timestamps DESC
```

**Decisões:**
- **Polling 5s + SSE para deltas** (não websocket complexo): SWR refresh + EventSource simples.
- **Sem charts complexos** (D2 KISS): apenas tabela + cards. Gráficos (token cost histogram) ficam para iteração 2.
- **Reusar tema/Layout** do AppShell existente: zero overhead de CSS.

**Verify:** `cargo test -p theo --bin theo dashboard_agents && cd apps/theo-ui && npm test -- agents`

---

# TRACK G — `theo run resume <id>` CLI (Fase 16)

> **Objetivo:** sub-agent crashado/cancelado pode ser retomado de onde parou via `theo run resume <id>`. Estado terminal preservado (idempotente).
> **Pré-requisito:** `FileSubagentRunStore` + `SubagentRun.config_snapshot` + event log JSONL (entregues v3.1).
> **Evidência direta:** Archon CLI workflow resume, LangGraph checkpointing.

### Fase 16: ResumeContext + Resumer + CLI

**Arquitetura:**

```rust
// crates/theo-agent-runtime/src/subagent/resume.rs (NOVO)

/// Estado reconstruído a partir de SubagentRun + event log para resume.
#[derive(Debug)]
pub struct ResumeContext {
    /// Spec original (frozen no início do run).
    pub spec: AgentSpec,
    /// Iteração onde retomar (last completed iteration + 1).
    pub start_iteration: usize,
    /// Histórico de mensagens reconstruído do event log.
    pub history: Vec<Message>,
    /// Tokens já consumidos (contam contra parent budget).
    pub prior_tokens_used: u64,
    /// Checkpoint SHA do snapshot pre-run (para rollback se resume falhar).
    pub checkpoint_before: Option<String>,
}

pub struct Resumer<'a> {
    store: &'a FileSubagentRunStore,
    manager: &'a SubAgentManager,
}

impl<'a> Resumer<'a> {
    pub fn new(store: &'a FileSubagentRunStore, manager: &'a SubAgentManager) -> Self {
        Self { store, manager }
    }

    /// Carrega run + reconstrói ResumeContext.
    /// Retorna ResumeError::NotResumable se status terminal.
    pub fn build_context(&self, run_id: &str) -> Result<ResumeContext, ResumeError> {
        let run = self.store.load(run_id)?;
        if run.status.is_terminal() {
            return Err(ResumeError::NotResumable {
                run_id: run_id.into(),
                status: format!("{:?}", run.status),
            });
        }
        let events = self.store.list_events(run_id)?;
        let history = reconstruct_history(&events);
        let start_iteration = events
            .iter()
            .filter(|e| e.event_type == "iteration_completed")
            .count();
        Ok(ResumeContext {
            spec: run.config_snapshot,
            start_iteration,
            history,
            prior_tokens_used: run.tokens_used,
            checkpoint_before: run.checkpoint_before,
        })
    }

    /// Resume: re-spawn com history reconstruído.
    /// IDEMPOTENTE: terminal status → no-op + retorna AgentResult original.
    pub async fn resume(&self, run_id: &str) -> Result<AgentResult, ResumeError> {
        let ctx = self.build_context(run_id)?;
        let history_msgs = ctx.history.clone();
        let result = self
            .manager
            .spawn_with_spec(&ctx.spec, &format!("[resumed] {}", ctx.spec.description), Some(history_msgs))
            .await;
        Ok(result)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ResumeError {
    #[error("run '{run_id}' is in terminal status '{status}', cannot resume")]
    NotResumable { run_id: String, status: String },
    #[error("run not found: {0}")]
    NotFound(String),
    #[error("store error: {0}")]
    Store(#[from] RunStoreError),
}

fn reconstruct_history(events: &[SubagentEvent]) -> Vec<Message> {
    events
        .iter()
        .filter_map(|e| match e.event_type.as_str() {
            "user_message" => e.payload.get("text").and_then(|v| v.as_str()).map(Message::user),
            "assistant_message" => e.payload.get("text").and_then(|v| v.as_str()).map(Message::assistant),
            "tool_result" => {
                let call_id = e.payload.get("call_id").and_then(|v| v.as_str())?;
                let name = e.payload.get("name").and_then(|v| v.as_str())?;
                let content = e.payload.get("content").and_then(|v| v.as_str())?;
                Some(Message::tool_result(call_id, name, content))
            }
            _ => None,
        })
        .collect()
}
```

**CLI extension (apps/theo-cli/src/subagent_admin.rs — modificar):**

```rust
#[derive(Subcommand)]
pub enum SubagentCmd {
    List,
    Status { run_id: String },
    Abandon { run_id: String },
    Cleanup { #[arg(long, default_value_t = 7)] days: u32 },
    /// NOVO: Resume a non-terminal run from its event log.
    Resume {
        run_id: String,
        /// Optional: override the original objective.
        #[arg(long)]
        objective: Option<String>,
    },
}

// Em handle_subagent:
SubagentCmd::Resume { run_id, objective } => {
    let store = FileSubagentRunStore::new(runs_base_dir(project_dir));
    let manager = SubAgentManager::with_builtins(/* ... */);
    let resumer = Resumer::new(&store, &manager);
    match resumer.resume(&run_id).await {
        Ok(result) => println!("✓ Resume completed: success={}, summary={}", result.success, result.summary),
        Err(ResumeError::NotResumable { status, .. }) => {
            println!("Run '{}' is in terminal status '{}'. Use `theo subagent abandon` to mark as abandoned.", run_id, status);
        }
        Err(e) => return Err(anyhow::anyhow!("{}", e)),
    }
}
```

**TDD Sequence:**

```
RED:
  #[test] fn build_context_terminal_run_returns_not_resumable()
  #[test] fn build_context_running_run_returns_context()
  #[test] fn build_context_unknown_run_returns_not_found()
  #[test] fn build_context_start_iteration_counts_completed_events()
  #[test] fn build_context_reconstructs_history_from_events()
  #[test] fn build_context_preserves_checkpoint_before()
  #[test] fn reconstruct_history_skips_unknown_event_types()
  #[test] fn reconstruct_history_handles_user_message_event()
  #[test] fn reconstruct_history_handles_assistant_message_event()
  #[test] fn reconstruct_history_handles_tool_result_event()
  #[tokio::test] async fn resume_terminal_run_returns_error_not_resumable()
  #[tokio::test] async fn resume_running_run_invokes_spawn_with_spec_with_history()
  → cargo test → FAIL

GREEN:
  1. Criar resume.rs com ResumeContext + Resumer + ResumeError
  2. Adicionar SubagentCmd::Resume + handler em subagent_admin.rs
  3. Implementar reconstruct_history filtrando event_types relevantes
  → cargo test → PASS

INTEGRATION:
  - Smoke: spawn sub-agent, kill processo no meio (kill -9 PID)
    → SubagentRun fica em status=Running com event log parcial
    → `theo subagent resume <id>` re-roda, completa, status=Completed
```

**Decisões:**
- **history reconstrução é best-effort**: eventos desconhecidos são skipped, não erram.
- **prior_tokens_used contabiliza**: budget enforcer parent vê total real (resume + retry).
- **checkpoint_before exposto**: user pode rollback antes de resume se preferir começar fresh.

**Verify:** `cargo test -p theo-agent-runtime -- subagent::resume && cargo test -p theo --bin theo subagent_admin`

---

# TRACK H — MCP Tools Pre-Discovered no Schema (Fase 17)

> **Objetivo:** ao spawn de sub-agent com `mcp_servers != []`, fazer discovery (`tools/list`) em todos os servers permitidos + injetar tools como `ToolDefinition` no schema do LLM (prefix `mcp:server:tool`). LLM vê descrições reais, não hint genérico.
> **Pré-requisito:** `McpClient::list_tools` (entregue v3.1), `McpRegistry`.
> **Evidência direta:** Claude Code MCP impl, MCP spec §3.2.

### Fase 17: DiscoveryCache + Tool Injection

**Arquitetura:**

```rust
// crates/theo-infra-mcp/src/discovery.rs (NOVO)

/// Per-session cache de tools discovered. TTL = lifetime do processo.
/// Re-spawn = re-discovery.
#[derive(Debug, Default, Clone)]
pub struct DiscoveryCache {
    /// server_name → Vec<McpTool>
    by_server: Arc<RwLock<BTreeMap<String, Vec<McpTool>>>>,
}

impl DiscoveryCache {
    pub fn new() -> Self { Self::default() }

    /// Spawn cliente, faz tools/list, cacheia. Idempotente: segunda chamada
    /// retorna cache. Timeout 5s por server (Cursor convention).
    pub async fn discover(
        &self,
        server_name: &str,
        config: &McpServerConfig,
    ) -> Result<Vec<McpTool>, McpError> {
        // Cache hit?
        if let Some(cached) = self.by_server.read().expect("rwlock").get(server_name) {
            return Ok(cached.clone());
        }
        // Miss: discover with timeout
        let tools = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            async {
                let mut client = McpStdioClient::from_config(config).await?;
                client.list_tools().await
            },
        )
        .await
        .map_err(|_| McpError::Timeout(std::time::Duration::from_secs(5)))?
        ?;
        // Store
        self.by_server
            .write()
            .expect("rwlock")
            .insert(server_name.to_string(), tools.clone());
        Ok(tools)
    }

    /// Discover all servers in registry filtered by allowlist.
    /// Returns a map {server_name → Vec<tools>}. Servers que falham são
    /// skipped com warning (fail-soft, não derruba o spawn).
    pub async fn discover_all(
        &self,
        registry: &McpRegistry,
        allowlist: &[String],
    ) -> BTreeMap<String, Vec<McpTool>> {
        let filtered = registry.filtered(allowlist);
        let mut result = BTreeMap::new();
        for name in filtered.names() {
            if let Some(cfg) = filtered.get(name) {
                match self.discover(name, &cfg).await {
                    Ok(tools) => {
                        result.insert(name.to_string(), tools);
                    }
                    Err(err) => {
                        eprintln!(
                            "warning: MCP discovery failed for '{}': {} (server skipped)",
                            name, err
                        );
                    }
                }
            }
        }
        result
    }

    /// Clear cache for a specific server (force re-discovery on next call).
    pub fn invalidate(&self, server_name: &str) {
        self.by_server.write().expect("rwlock").remove(server_name);
    }

    pub fn clear_all(&self) {
        self.by_server.write().expect("rwlock").clear();
    }
}
```

**Conversão MCP → ToolDefinition (theo-agent-runtime):**

```rust
// crates/theo-agent-runtime/src/subagent/mcp_tools.rs (NOVO)

use theo_infra_llm::types::ToolDefinition;
use theo_infra_mcp::McpTool;

pub fn mcp_tool_to_definition(server: &str, tool: &McpTool) -> ToolDefinition {
    ToolDefinition::new(
        &format!("mcp:{}:{}", server, tool.name),
        tool.description.as_deref().unwrap_or(""),
        tool.input_schema.clone(),
    )
}
```

**Integração em SubAgentManager.spawn_with_spec:**

```rust
// Modificação em subagent/mod.rs spawn_with_spec, antes do AgentLoop::new

if !spec.mcp_servers.is_empty()
    && let (Some(global), Some(cache)) = (&self.mcp_registry, &self.mcp_discovery_cache)
{
    let discovered = cache.discover_all(global, &spec.mcp_servers).await;
    // Convert + register tools no registry do sub-agent
    for (server, tools) in &discovered {
        for tool in tools {
            let def = mcp_tool_to_definition(server, tool);
            registry.register_dynamic(def);
        }
    }
    // Substitui o hint textual por uma seção mais informativa
    let total_tools: usize = discovered.values().map(|v| v.len()).sum();
    sub_config.system_prompt = format!(
        "{}\n\n## MCP tools available ({} from {} server(s)):\n{}",
        sub_config.system_prompt,
        total_tools,
        discovered.len(),
        format_discovered_tools(&discovered),
    );
}
```

**TDD Sequence:**

```
RED:
  #[tokio::test] async fn discover_caches_on_first_call()
  #[tokio::test] async fn discover_returns_cache_on_second_call()
  #[tokio::test] async fn discover_timeout_5s_fails_with_timeout_error()
  #[tokio::test] async fn discover_all_filters_by_allowlist()
  #[tokio::test] async fn discover_all_skips_failed_servers_with_warning()
  #[tokio::test] async fn discover_all_returns_empty_when_allowlist_empty()
  #[tokio::test] async fn invalidate_forces_rediscovery()
  #[tokio::test] async fn clear_all_empties_cache()
  #[test] fn mcp_tool_to_definition_uses_qualified_name()
  #[test] fn mcp_tool_to_definition_preserves_input_schema()
  → cargo test → FAIL

GREEN:
  1. Criar discovery.rs em theo-infra-mcp
  2. Criar mcp_tools.rs em theo-agent-runtime
  3. SubAgentManager.mcp_discovery_cache: Option<Arc<DiscoveryCache>>
  4. with_mcp_discovery_cache builder + apply_to em SubagentInjections
  5. spawn_with_spec invoca cache.discover_all + injeta tools
  → cargo test → PASS

INTEGRATION:
  - Spawn @modelcontextprotocol/server-filesystem como teste
  - Verificar que tools (read_file, list_directory, etc.) aparecem no
    schema do sub-agent com prefix mcp:server-filesystem:read_file
  - LLM call usa o tool → AgentRunEngine.try_dispatch_mcp_tool roteia
```

**Decisões:**
- **Cache lifetime = sessão** (D4): re-spawn invalida tudo.
- **Timeout 5s** (Cursor convention): server lento não bloqueia spawn.
- **Fail-soft**: server que crash → warning + outros servers continuam.

**Verify:** `cargo test -p theo-infra-mcp -- discovery && cargo test -p theo-agent-runtime -- mcp_tools subagent::spawn_with_spec_mcp`

---

# TRACK I — Handoff Guardrails 3-tier (Fase 18)

> **Objetivo:** completar o pattern OpenAI Agents SDK 3-tier (input/output/handoff). Já cobrimos input (PreToolUse) + output (PostToolUse). Falta handoff dedicado: validar ANTES de delegate_task resolver agent.
> **Pré-requisito:** HookManager (entregue v3.1), spawn_with_spec.
> **Evidência direta:** OpenAI Agents SDK guardrails docs, NeMo Guardrails declarative pattern.

### Fase 18: HandoffGuardrail trait + HookEvent::PreHandoff

**Arquitetura:**

```rust
// crates/theo-agent-runtime/src/handoff_guardrail.rs (NOVO)

/// Decisão retornada por uma HandoffGuardrail.
#[derive(Debug, Clone)]
pub enum HandoffDecision {
    /// Permitir handoff sem mudanças.
    Allow,
    /// Bloquear handoff. Razão é retornada ao LLM como tool result.
    Block { reason: String },
    /// Substituir agent destino. Útil para "redirect explorer to security-reviewer".
    Redirect { new_agent_name: String },
    /// Modificar objective antes de delegar.
    RewriteObjective { new_objective: String },
}

/// Trait para guardrails programáticos (Rust code, vs declarativo via Hook).
/// Útil para validações complexas: análise de objective, verificação de
/// budget restante, etc.
pub trait HandoffGuardrail: Send + Sync {
    fn name(&self) -> &str;
    fn validate(&self, target_agent: &AgentSpec, objective: &str) -> HandoffDecision;
}

/// Composite que avalia múltiplos guardrails em ordem. Primeiro non-Allow vence.
pub struct GuardrailChain {
    guardrails: Vec<Arc<dyn HandoffGuardrail>>,
}

impl GuardrailChain {
    pub fn new() -> Self { Self { guardrails: Vec::new() } }
    pub fn push(&mut self, g: Arc<dyn HandoffGuardrail>) { self.guardrails.push(g); }

    pub fn evaluate(&self, target: &AgentSpec, objective: &str) -> HandoffDecision {
        for g in &self.guardrails {
            match g.validate(target, objective) {
                HandoffDecision::Allow => continue,
                other => return other,
            }
        }
        HandoffDecision::Allow
    }
}

// Built-in guardrails
pub struct ReadOnlyAgentMustNotMutate;
impl HandoffGuardrail for ReadOnlyAgentMustNotMutate {
    fn name(&self) -> &str { "ReadOnlyAgentMustNotMutate" }
    fn validate(&self, target: &AgentSpec, objective: &str) -> HandoffDecision {
        let is_read_only = target.capability_set.denied_tools.contains("edit")
            && target.capability_set.denied_tools.contains("write");
        let needs_mutation = ["edit", "write", "modify", "create", "fix bug", "implement"]
            .iter()
            .any(|kw| objective.to_lowercase().contains(kw));
        if is_read_only && needs_mutation {
            HandoffDecision::Redirect {
                new_agent_name: "implementer".to_string(),
            }
        } else {
            HandoffDecision::Allow
        }
    }
}
```

**Hook event extension:**

```rust
// theo-domain/src/event.rs — adicionar variante
EventType::HandoffEvaluated, // payload: { target, objective, decision }

// theo-agent-runtime/src/lifecycle_hooks.rs — HookEvent existente já tem
// SubagentStart. Adicionar PreHandoff que dispara ANTES de spawn_with_spec
// resolver o spec.
HookEvent::PreHandoff,  // adicionado à enum + ::ALL
```

**Integração em handle_delegate_task:**

```rust
// run_engine.rs handle_delegate_task — antes de spawn_with_spec

if has_agent {
    let target_spec = registry.get(&agent_name).cloned().unwrap_or_else(|| {
        AgentSpec::on_demand(&agent_name, &objective)
    });

    // Phase 18: handoff guardrail evaluation
    let decision = self
        .handoff_guardrails
        .as_ref()
        .map(|c| c.evaluate(&target_spec, &objective))
        .unwrap_or(HandoffDecision::Allow);

    let (final_spec, final_objective) = match decision {
        HandoffDecision::Allow => (target_spec, objective.clone()),
        HandoffDecision::Block { reason } => {
            self.event_bus.publish(DomainEvent::new(
                EventType::HandoffEvaluated,
                "delegate_task",
                serde_json::json!({"target": agent_name, "decision": "block", "reason": reason}),
            ));
            return format!("[handoff blocked] {}", reason);
        }
        HandoffDecision::Redirect { new_agent_name } => {
            let new_spec = registry.get(&new_agent_name).cloned()
                .unwrap_or_else(|| AgentSpec::on_demand(&new_agent_name, &objective));
            self.event_bus.publish(DomainEvent::new(
                EventType::HandoffEvaluated,
                "delegate_task",
                serde_json::json!({"target": agent_name, "decision": "redirect", "new": new_agent_name}),
            ));
            (new_spec, objective.clone())
        }
        HandoffDecision::RewriteObjective { new_objective } => {
            self.event_bus.publish(DomainEvent::new(
                EventType::HandoffEvaluated,
                "delegate_task",
                serde_json::json!({"target": agent_name, "decision": "rewrite", "new_objective": new_objective}),
            ));
            (target_spec, new_objective)
        }
    };

    // Continue com (final_spec, final_objective) ...
}
```

**TDD Sequence:**

```
RED:
  #[test] fn guardrail_chain_empty_returns_allow()
  #[test] fn guardrail_chain_first_block_wins()
  #[test] fn guardrail_chain_first_redirect_wins()
  #[test] fn guardrail_chain_skips_allow_continues_to_next()
  #[test] fn read_only_must_not_mutate_blocks_implement_keyword()
  #[test] fn read_only_must_not_mutate_blocks_write_keyword()
  #[test] fn read_only_must_not_mutate_redirects_to_implementer()
  #[test] fn read_only_must_not_mutate_allows_read_only_objective()
  #[test] fn read_only_must_not_mutate_allows_for_implementer_target()
  #[test] fn handoff_decision_serde_roundtrip()
  #[tokio::test] async fn delegate_task_blocked_by_guardrail_returns_block_message()
  #[tokio::test] async fn delegate_task_redirect_uses_new_agent()
  #[tokio::test] async fn delegate_task_rewrite_uses_new_objective()
  #[tokio::test] async fn delegate_task_emits_handoff_evaluated_event()
  → cargo test → FAIL

GREEN:
  1. Criar handoff_guardrail.rs com trait + GuardrailChain + ReadOnly built-in
  2. Adicionar EventType::HandoffEvaluated em theo-domain
  3. Adicionar HookEvent::PreHandoff em lifecycle_hooks.rs (cobre o caso
     declarativo via YAML)
  4. AgentRunEngine.handoff_guardrails: Option<Arc<GuardrailChain>>
  5. with_handoff_guardrails builder + forward em AgentLoop + SubagentInjections
  6. handle_delegate_task chama chain.evaluate antes de spawn_with_spec
  → cargo test → PASS

INTEGRATION:
  - LLM chama delegate_task com agent=explorer, objective="implement auth"
    → Guardrail redireciona para implementer
    → Event HandoffEvaluated emitido com decision=redirect
  - Custom guardrail BlockUnknownAgents permite só agents builtin →
    on-demand requests bloqueados
```

**Decisões:**
- **Trait em Rust + Hook YAML coexistem** (D5): hook é fast-path declarativo, trait é programático.
- **Guardrails são Vec ordenado** (não set): primeira decisão non-Allow vence (consistent com hooks pattern).
- **Built-in `ReadOnlyAgentMustNotMutate`**: caso comum suficiente para shipar default.
- **Decision::Redirect** muda spec mas preserva objective: user feedback via `[handoff redirected]` prefix no result.

**Verify:** `cargo test -p theo-agent-runtime -- handoff_guardrail run_engine::handle_delegate_task_guardrail`

---

# TRACK J — SOTA Integration End-to-End (Fase 19)

> **Objetivo:** smoke test único validando os 5 gaps fechados ATUANDO JUNTOS no mesmo run.
> **Pré-requisito:** Tracks E-I fechados.

### Fase 19: SOTA-12 Integration Test

**Cenário:**

Custom agent `sota12-validator.md` declara:
- `mcp_servers: [github]` (Phase 17 — discovery + injection)
- `model_override: None` (Phase 14 — auto routing)
- `output_format: { schema: ..., enforcement: best_effort }` (Phase 7 — entregue)
- Hooks bloqueando bash (Phase 5 — entregue)
- Isolation: worktree (Phase 11 — entregue)

Smoke flow:
1. CLI startup com `--watch-agents --enable-checkpoints`
2. `theo agents approve --all`
3. `theo` (interactive) com prompt que provoca delegate_task
4. ComplexityClassifier detecta task type → routes to cheap (Fase 14)
5. Discovery cache spawna github MCP → tools injetadas no schema (Fase 17)
6. HandoffGuardrail valida (Fase 18)
7. spawn_with_spec executa
8. SIGTERM no meio → status=Cancelled
9. `theo subagent resume <id>` (Fase 16) → re-roda do event log
10. `curl /api/agents/sota12-validator` (Fase 15) → metrics aparecem
11. Validar 5 OTel attrs no SubagentCompleted event payload

**Test code (resumido):**

```rust
// crates/theo-agent-runtime/tests/sota12_integration.rs (NOVO)

#[tokio::test]
async fn sota12_all_5_gaps_active_simultaneously() {
    // Setup: MCP registry with github, complexity classifier, guardrails,
    // discovery cache, run store
    let setup = Sota12Setup::new();

    // Spawn agent with all 5 features active
    let result = setup.spawn_with_full_pipeline("sota12-validator", "audit security").await;
    assert!(result.success || result.cancelled);

    // Validate 14: complexity classifier picked Strong slot for "audit"
    assert!(setup.last_routed_tier() == Tier::Strong);

    // Validate 15: dashboard endpoint serves the agent
    let response = setup.dashboard_get("/api/agents/sota12-validator").await;
    assert_eq!(response["name"], "sota12-validator");
    assert!(response["runs"].as_u64().unwrap() >= 1);

    // Validate 16: persisted run can be resumed
    let runs = setup.list_persisted_runs();
    assert!(!runs.is_empty());
    let resume_result = setup.resume(&runs[0]).await;
    assert!(resume_result.is_ok() || matches!(resume_result, Err(ResumeError::NotResumable { .. })));

    // Validate 17: MCP tools were injected into the agent's schema
    assert!(setup.last_subagent_tool_definitions().iter().any(|d| d.name.starts_with("mcp:github:")));

    // Validate 18: handoff guardrail evaluated
    let events = setup.captured_events();
    assert!(events.iter().any(|e| e.event_type == EventType::HandoffEvaluated));
}
```

**TDD:** este test é DEPENDENTE de Fases 14-18 estarem GREEN. RED inicialmente, GREEN ao final do plano.

**Verify:** `cargo test -p theo-agent-runtime --test sota12_integration`

---

## Riscos e Mitigações

| Risco | Mitigação |
|---|---|
| Complexity classifier impreciso → routing errado → custos maiores | Heurística é OPT-IN (D1). Telemetria via `theo.routing.tier_chosen` permite ajuste. Fallback: `model_override` no spec sempre vence. |
| Dashboard frontend exige rebuild theo-ui dist | CI já builda apps/theo-ui. Embed via dashboard-dist. CLI flag `--no-ui` para servir só JSON. |
| Resume re-executa side effects (mutations) | Usuario deve `theo checkpoints restore` antes se quiser fresh. Documentado em `theo subagent resume --help`. |
| MCP discovery 5s timeout × N servers → spawn lento | Discovery roda em paralelo via tokio::join_all (não serial). Cache invalidação manual via `theo mcp clear-cache`. |
| Discovery cache stale entre versões do MCP server | Re-spawn de processo invalida (D4). Comando manual `theo mcp invalidate <server>`. |
| Handoff guardrails infinitos (redirect loop) | GuardrailChain checa `redirect_count` e aborta após 3 redirects (return Block { reason: "redirect loop"}). |
| Built-in guardrail muito restritivo (false positives) | `with_handoff_guardrails(GuardrailChain::default())` é OPT-IN. Sem chain = comportamento atual (allow sempre). |
| OTel events poluem trajectory JSONL com payloads grandes | `HandoffEvaluated` payload ≤ 200 bytes. Não inclui spec/history. |
| 5 features novas × 1992 testes = surface enorme | Cada Fase tem TDD próprio. Fase 19 valida integration. Zero deferred. |

---

## Verificação Final

```bash
# Track E — Cost-Aware Routing
cargo test -p theo-infra-llm -- routing::complexity routing::auto

# Track F — Dashboard Frontend
cargo test -p theo --bin theo dashboard_agents
cd apps/theo-ui && npm test -- agents

# Track G — Resume CLI
cargo test -p theo-agent-runtime -- subagent::resume
cargo test -p theo --bin theo subagent_admin::resume

# Track H — MCP Discovery
cargo test -p theo-infra-mcp -- discovery
cargo test -p theo-agent-runtime -- mcp_tools

# Track I — Handoff Guardrails
cargo test -p theo-agent-runtime -- handoff_guardrail

# Track J — Integration
cargo test -p theo-agent-runtime --test sota12_integration

# Smoke E2E (requires OAuth Codex)
mkdir -p .theo/agents
cat > .theo/agents/sota12-validator.md << 'EOF'
---
name: sota12-validator
description: "Audit security via github MCP, structured findings"
mcp_servers: [github]
isolation: { mode: worktree, base_branch: main }
output_format: { enforcement: best_effort, schema: { type: object, required: [findings] } }
hooks: { PreToolUse: [{matcher: "^bash$", response: { type: block, reason: "no bash" }}] }
max_iterations: 5
timeout: 60
---
You audit security. Find issues, report structured.
EOF

theo agents approve --all
theo --watch-agents --enable-checkpoints \
  -p "Use delegate_task on sota12-validator with objective 'audit auth module security'"

# Validate
theo subagent list
curl http://localhost:5173/api/agents
```

---

## Épicos Futuros (fora deste plano)

| Epic | Quando | Pré-requisito |
|---|---|---|
| **Learned router** (replace classifier with fine-tuned model) | Quando heurística não cobrir > 20% dos casos | Telemetria de tier_chosen suficiente para training |
| **Cost dashboard com gráficos** (token cost histogram) | Quando users pedirem | Fase 15 entregue |
| **Resume parcial** (skip apenas tools que rodaram) | Quando re-executar tool tier sair caro | Fase 16 entregue |
| **MCP Server** (Theo expõe como server, não consume) | IDE integration explícita | Fase 17 entregue (cliente) |
| **A2A protocol** (depth>1) | Workflows multi-step explícitos | Plano separado, YAGNI hoje |

---

## Referências

| # | Fonte | URL | Usado em |
|---|---|---|---|
| 1 | Anthropic — Multi-Agent Research System | https://www.anthropic.com/engineering/multi-agent-research-system | Fase 14 (Opus lead + Sonnet workers) |
| 2 | Aider — Architect Mode | https://aider.chat/2024/09/26/architect.html | Fase 14 (dual-model split) |
| 3 | OpenRouter — Model Routing | https://openrouter.ai/docs/model-routing | Fase 14 (heuristics) |
| 4 | LangGraph Studio | https://langchain-ai.github.io/langgraph/studio/ | Fase 15 (dashboard layout) |
| 5 | Archon `RunsTable.tsx` | `referencias/Archon/packages/web/src/components/workflows/RunsTable.tsx` | Fase 15 (SSE pattern) |
| 6 | Archon CLI workflow resume | `referencias/Archon/CLAUDE.md` (search "workflow resume") | Fase 16 (resume pattern) |
| 7 | LangGraph Checkpointing | https://langchain-ai.github.io/langgraph/concepts/persistence/ | Fase 16 (state replay) |
| 8 | MCP Spec 2025-03-26 | https://modelcontextprotocol.io/specification | Fase 17 (tools/list) |
| 9 | Cursor MCP Integration | https://docs.cursor.com/mcp | Fase 17 (5s timeout) |
| 10 | OpenAI Agents SDK Guardrails | https://openai.github.io/openai-agents-python/guardrails/ | Fase 18 (3-tier) |
| 11 | NeMo Guardrails | https://docs.nvidia.com/nemo/guardrails/ | Fase 18 (declarative) |
| 12 | arXiv 2604.14228 — Dive into Claude Code | https://arxiv.org/abs/2604.14228 | Fase 14 (rule-based 95% accuracy) |

---

## Compromisso de Cobertura SOTA

Após este plano (Fases 14-19) entregue: **12/12 features SOTA cobertas** (vs 8.5/12 atual).

| Feature SOTA | Status pós-este-plano |
|---|---|
| MCP Integration | ✓ + tools pre-discovered (Fase 17) |
| Lifecycle Hooks | ✓ + PreHandoff event (Fase 18) |
| A2A protocol | ❌ deliberately excluded |
| Programmable Guardrails 3-tier | ✓ COMPLETO (Fase 18 fecha handoff) |
| Worktree/Sandbox isolation | ✓ |
| File Locking | ❌ deliberately excluded |
| State persistence + resume | ✓ COMPLETO (Fase 16 fecha CLI) |
| Hot-reload | ✓ |
| Structured findings | ✓ (Value-typed; tipo Rust dedicado é "se necessário") |
| **Cost-aware routing** | ✓ NOVO (Fase 14) |
| **Per-agent dashboard** | ✓ NOVO (Fase 15) |
| Streaming cancellation | ✓ |

**12/12 com 2 deliberately-excluded** (A2A + File Locking) — confirmado YAGNI vs Claude Code/Cursor que TAMBÉM não implementam.
