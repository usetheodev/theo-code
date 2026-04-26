# Plano: Resume Runtime Wiring — fechar gaps #3 e #10

> **Versão 1.0** — continuação direta de `sota-gaps-followup-plan.md`. Fecha os
> 2 gaps que ficaram com data layer pronto mas runtime path incompleto.
> Após esta entrega, sistema é SOTA puro na camada de orchestration sem
> deferred items.

## Contexto

Após `sota-gaps-followup-plan.md` (10 gaps fechados, validados E2E real OAuth),
ficaram 2 gaps onde o data layer foi entregue mas o runtime não consulta:

| Gap | Data layer | Runtime | Bug observável |
|---|---|---|---|
| **#3 Resume tool replay** | `ResumeContext.executed_tool_calls: BTreeSet<String>` + `should_skip_tool_call(id)` | ❌ AgentLoop dispatch ignora | Side-effects (write/bash) re-executam ao resumir |
| **#10 Worktree restore** | `WorktreeStrategy {None/Reuse/Recreate}` em `ResumeContext` | ❌ Resumer ignora o enum | Resume cria nova worktree em vez de reusar existente |

Ambos têm cobertura unit (8 + 6 tests respectivamente) **da reconstrução de
estado** — mas a EXECUÇÃO do resume não consulta esse estado.

**Objetivo:** zero gaps reais no runtime. Cada fase atômica (1 PR), TDD,
plan-named tests, validação E2E real (não só unit) via cenário de crash
controlado.

**Estratégia:** 3 fases sequenciais (Fases 30-32):

| Track | Fase | Entrega | Fecha gap |
|---|---|---|---|
| **P — Tool Replay** | 30 | `ResumeContext.executed_tool_results` map + AgentLoop replay-mode dispatch | #3 |
| **Q — Worktree Override** | 31 | `WorktreeOverride` enum + `spawn_with_spec` honra override + Resumer passa | #10 |
| **R — E2E Crash Test** | 32 | Test que mata sub-agent mid-tool-call, resume, valida ambos comportamentos | #3 + #10 combinados |

Fases 30 e 31 podem rodar paralelas; Fase 32 depende de ambas.

---

## Decisões de arquitetura

### D1: Replay mode é via injeção opcional, não modo global

`AgentLoop::with_resume_context(ctx)` é builder. Quando `Some`, dispatch
checa antes de invocar tool. Quando `None`, comportamento atual inalterado
(zero risco de regressão para o path normal).

### D2: Tool result cache é construído no resume, não no spawn

`reconstruct_executed_tool_results(events) -> BTreeMap<call_id, Message>`
é construído **no momento do resume** a partir do event log persistido.
Não há novo armazenamento — usa o mesmo log que já temos.

### D3: Worktree override é um enum explícito, não Option<PathBuf>

`WorktreeOverride` enum: `None / Reuse(PathBuf) / Recreate { base_branch }`.
Casa 1:1 com `WorktreeStrategy` mas vive em `subagent::mod` para que
`spawn_with_spec` possa receber sem importar `subagent::resume`.

### D4: Cenário de crash usa CancellationTree, não kill -9

O test E2E mata o sub-agent via `CancellationTree::cancel_all()` no meio
de um tool call (via hook). Isso é determinístico, reproduzível em CI,
e simula o cenário real de "operador apertou Ctrl+C" ou "parent abortou".

### D5: Backward compat absoluta para AgentLoop

Existing 1000+ AgentLoop tests passam sem mudança. Novo behavior é
**opt-in via builder**. Default behavior é dispatch normal.

---

## Fases

### Fase 30 — Resume tool replay (Track P, gap #3)

**Objetivo:** quando AgentLoop está em replay mode (resume) e o LLM emite
um tool_call cujo `call_id` já existe no event log de execução anterior,
NÃO dispatchar a tool — usar o resultado persistido.

**Arquitetura:**

```rust
// theo-agent-runtime/src/subagent/resume.rs (extender)

#[derive(Debug)]
pub struct ResumeContext {
    pub spec: AgentSpec,
    pub start_iteration: usize,
    pub history: Vec<Message>,
    pub prior_tokens_used: u64,
    pub checkpoint_before: Option<String>,
    pub executed_tool_calls: BTreeSet<String>,
    /// Phase 30 (NOVO): map call_id → Message::tool_result reconstructed
    /// from the event log. AgentLoop consults this BEFORE dispatching to
    /// short-circuit tools whose result is already known.
    pub executed_tool_results: BTreeMap<String, Message>,
    pub worktree_strategy: WorktreeStrategy,
}

pub fn reconstruct_executed_tool_results(
    events: &[SubagentEvent],
) -> BTreeMap<String, Message> {
    let mut out = BTreeMap::new();
    for e in events {
        if e.event_type == "tool_result"
            && let (Some(call_id), Some(name), Some(content)) = (
                e.payload.get("call_id").and_then(|v| v.as_str()),
                e.payload.get("name").and_then(|v| v.as_str()),
                e.payload.get("content").and_then(|v| v.as_str()),
            )
        {
            out.insert(
                call_id.to_string(),
                Message::tool_result(call_id, name, content),
            );
        }
    }
    out
}
```

```rust
// theo-agent-runtime/src/agent_loop.rs (extender)

pub struct AgentLoop {
    // ... existing fields
    /// Phase 30: when Some, dispatch consults this BEFORE invoking each
    /// tool. If `should_skip_tool_call(call_id)` is true, the cached
    /// result from `executed_tool_results` is pushed in lieu of dispatch.
    resume_context: Option<Arc<crate::subagent::resume::ResumeContext>>,
}

impl AgentLoop {
    pub fn with_resume_context(
        mut self,
        ctx: Arc<crate::subagent::resume::ResumeContext>,
    ) -> Self {
        self.resume_context = Some(ctx);
        self
    }
}
```

```rust
// theo-agent-runtime/src/run_engine.rs handle dispatch loop (modificar)

for call in tool_calls {
    // Phase 30 (sota-gaps-followup-followup): replay short-circuit.
    if let Some(ref ctx) = self.config.resume_context
        && ctx.should_skip_tool_call(&call.id)
    {
        if let Some(cached) = ctx.executed_tool_results.get(&call.id) {
            messages.push(cached.clone());
            self.event_bus.publish(DomainEvent::new(
                EventType::ToolCallCompleted,
                self.run.run_id.as_str(),
                serde_json::json!({
                    "tool_name": &call.function.name,
                    "call_id": &call.id,
                    "replayed": true,
                    "status": "Succeeded",
                }),
            ));
            continue;
        }
    }
    // ... existing dispatch path
}
```

**TDD Sequence:**
```
RED:
  reconstruct_executed_tool_results_returns_map_of_call_id_to_message
  reconstruct_executed_tool_results_skips_unknown_event_types
  reconstruct_executed_tool_results_handles_missing_payload_fields_gracefully
  build_context_populates_executed_tool_results
  agent_loop_with_resume_context_skips_replayed_call_id
  agent_loop_with_resume_context_dispatches_unknown_call_id
  agent_loop_without_resume_context_dispatches_normally (regression guard)
  resume_does_not_re_execute_tool_with_completed_event_in_log
  resume_executes_new_tool_when_call_id_absent_from_log

GREEN:
  - Estender ResumeContext (executed_tool_results)
  - reconstruct_executed_tool_results() helper
  - AgentLoop::with_resume_context builder
  - Resume context propagated to AgentRunEngine via AgentConfig
  - Dispatch loop checks resume_context before tool execution
  - ToolCallCompleted event payload gains "replayed": bool

INTEGRATION:
  - Test: spawn_with_spec receives ResumeContext from Resumer
  - Test: ToolCallDispatched event NOT emitted for replayed calls
    (only ToolCallCompleted with replayed=true)
```

**Verify:**
```bash
cargo test -p theo-agent-runtime -- subagent::resume::tests::idempotency
cargo test -p theo-agent-runtime -- agent_loop::tests::with_resume_context
cargo test -p theo-agent-runtime -- run_engine::tests::dispatch_replays
```

**Risco mitigado (D5):** novos testes escritos para garantir que
`AgentLoop::run()` sem `resume_context` produz resultado IDÊNTICO à versão
pre-fase-30.

---

### Fase 31 — Worktree override (Track Q, gap #10)

**Objetivo:** quando Resumer chama `spawn_with_spec` para um spec isolado,
deve passar a path da worktree antiga (Reuse) ou pedir recriação explícita
(Recreate). `spawn_with_spec` honra esse override em vez de criar uma
worktree nova.

**Arquitetura:**

```rust
// theo-agent-runtime/src/subagent/mod.rs (extender)

/// Phase 31 (sota-gaps-followup-followup) — gap #10.
/// Override that the Resumer passes to `spawn_with_spec_with_override`
/// to control worktree behavior on resume:
///   - `None` — default behavior (create new from spec.isolation)
///   - `Reuse(path)` — use the provided existing worktree path
///   - `Recreate { base_branch }` — create new from this base branch
///     (overrides spec.isolation_base_branch if it differs)
#[derive(Debug, Clone)]
pub enum WorktreeOverride {
    None,
    Reuse(std::path::PathBuf),
    Recreate { base_branch: String },
}

impl SubAgentManager {
    /// Phase 31: variant of `spawn_with_spec` that respects an explicit
    /// worktree decision. The default `spawn_with_spec` is now a thin
    /// wrapper over this with `WorktreeOverride::None`.
    pub fn spawn_with_spec_with_override(
        &self,
        spec: &AgentSpec,
        objective: &str,
        context: Option<Vec<Message>>,
        worktree_override: WorktreeOverride,
    ) -> Pin<Box<dyn Future<Output = AgentResult> + Send + '_>> {
        // ... reuses existing spawn_with_spec body but the worktree
        // resolution branch becomes:
        let worktree_handle = match (
            &self.worktree_provider,
            spec.isolation.as_deref(),
            &worktree_override,
        ) {
            (_, _, WorktreeOverride::Reuse(path)) => {
                Some(WorktreeHandle::existing(path.clone()))
            }
            (Some(provider), Some("worktree"), WorktreeOverride::Recreate { base_branch }) => {
                provider.create(&spec.name, base_branch).ok()
            }
            (Some(provider), Some("worktree"), WorktreeOverride::None) => {
                let base = spec.isolation_base_branch.clone()
                    .unwrap_or_else(|| "main".to_string());
                provider.create(&spec.name, &base).ok()
            }
            _ => None,
        };
        // ... rest unchanged
    }
}
```

```rust
// theo-isolation/src/lib.rs (extender WorktreeHandle)

impl WorktreeHandle {
    /// Phase 31: construct a handle pointing to an existing worktree
    /// path. Used by `spawn_with_spec_with_override(Reuse(path))` so the
    /// cleanup logic still runs (CleanupPolicy::OnSuccess).
    pub fn existing(path: PathBuf) -> Self {
        Self { path, owned: false }
    }
}
```

```rust
// theo-agent-runtime/src/subagent/resume.rs (modificar Resumer)

impl<'a> Resumer<'a> {
    pub async fn resume_with_objective(
        &self,
        run_id: &str,
        objective_override: Option<&str>,
    ) -> Result<AgentResult, ResumeError> {
        let ctx = self.build_context(run_id)?;
        let history_msgs = ctx.history.clone();
        let objective = objective_override
            .map(String::from)
            .unwrap_or_else(|| format!(
                "[resumed iter {}] {}",
                ctx.start_iteration, ctx.spec.description
            ));
        // Phase 31: convert WorktreeStrategy → WorktreeOverride
        let wt_override = match &ctx.worktree_strategy {
            WorktreeStrategy::None => WorktreeOverride::None,
            WorktreeStrategy::Reuse(p) => WorktreeOverride::Reuse(p.clone()),
            WorktreeStrategy::Recreate { base_branch } => {
                WorktreeOverride::Recreate { base_branch: base_branch.clone() }
            }
        };
        let result = self
            .manager
            .spawn_with_spec_with_override(
                &ctx.spec,
                &objective,
                Some(history_msgs),
                wt_override,
            )
            .await;
        Ok(result)
    }
}
```

**TDD Sequence:**
```
RED:
  worktree_override_default_creates_new_per_spec
  worktree_override_reuse_uses_existing_path_without_create
  worktree_override_recreate_creates_with_explicit_base_branch
  worktree_handle_existing_marks_as_not_owned (no cleanup on drop)
  spawn_with_spec_with_override_none_matches_legacy_behavior (regression)
  spawn_with_spec_with_override_reuse_skips_provider_create
  spawn_with_spec_with_override_recreate_calls_provider_create_with_base
  resume_propagates_reuse_strategy_to_spawn
  resume_propagates_recreate_strategy_to_spawn
  resume_propagates_none_strategy_when_spec_not_isolated

GREEN:
  - Add WorktreeOverride enum
  - WorktreeHandle::existing constructor
  - spawn_with_spec_with_override(spec, objective, ctx, override)
  - spawn_with_spec becomes alias of *_with_override(.., None)
  - Resumer::resume_with_objective converts strategy → override

INTEGRATION:
  - Test that creates worktree manually, kills, resume reuses same path
  - Test that creates worktree, deletes path, resume creates fresh
```

**Verify:**
```bash
cargo test -p theo-agent-runtime -- subagent::tests::worktree_override
cargo test -p theo-agent-runtime -- subagent::resume::tests::worktree
cargo test -p theo-isolation -- worktree_handle_existing
```

---

### Fase 32 — E2E crash + resume integration test (Track R)

**Objetivo:** smoke test único que exercita ambos os comportamentos
juntos em um cenário realista. Valida que a chain
`spawn → tool_call → kill → persisted state → resume → replay + worktree
reuse → completion` funciona end-to-end.

**Cenário:**

```rust
// theo-agent-runtime/tests/resume_e2e.rs (NOVO)

#[tokio::test]
async fn resume_e2e_replays_completed_tools_and_reuses_worktree() {
    // 1. Setup: project_dir + spec with isolation=worktree
    // 2. Inject CancellationTree shared between manager + a "killer hook"
    // 3. spawn_with_spec_text starts a sub-agent that is configured to:
    //    - tool 1: a stub tool that emits ToolCallCompleted with call_id="c1"
    //    - tool 2: triggers cancellation_tree.cancel_all() via PostToolUse hook
    //    - tool 3: would-be dispatched but never reached (cancelled)
    // 4. Verify: persisted SubagentRun.status == Cancelled, event log has c1
    //    but not c3
    // 5. Construct Resumer + ResumeContext
    // 6. Verify ResumeContext.executed_tool_calls == {c1}
    // 7. Verify ResumeContext.worktree_strategy == Reuse(orig_path)
    //    (because OnSuccess cleanup didn't run for cancelled run)
    // 8. resume_with_objective() runs
    // 9. Sub-agent re-runs but:
    //    - When LLM emits tool_call(c1), AgentLoop short-circuits → no
    //      re-dispatch (assert via event count: 0 ToolCallDispatched for c1)
    //    - When LLM emits tool_call(c3), AgentLoop dispatches normally
    //    - Worktree path is the SAME as original (assert via path equality)
}

#[tokio::test]
async fn resume_e2e_recreates_worktree_when_original_was_cleaned() {
    // 1. Setup spec isolation=worktree, agent that completes successfully
    // 2. Spawn → success → CleanupPolicy::OnSuccess removes worktree
    // 3. Manually mark SubagentRun as Running (simulating "user wants resume
    //    even though it completed" — atypical but data-layer-ready)
    // 4. Resume
    // 5. ResumeContext.worktree_strategy == Recreate{base_branch}
    //    (path doesn't exist anymore)
    // 6. spawn_with_spec_with_override(Recreate) creates a NEW worktree
    // 7. Assert: path different from original; both gitdirs exist
}
```

**TDD Sequence:**
```
RED (file doesn't exist):
  cargo test -p theo-agent-runtime --test resume_e2e
  → no tests found

GREEN:
  Implement resume_e2e.rs with shared CancellationTree pattern
  Mock a Tool that triggers cancellation
  Implement assertions via captured DomainEvents

INTEGRATION:
  Run on real project_dir with real git init
  No external services (all in-process)
```

**Verify:**
```bash
cargo test -p theo-agent-runtime --test resume_e2e
```

---

## Riscos e mitigações

| Risco | Mitigação |
|---|---|
| AgentLoop dispatch é hot path com 1000+ tests | D5: replay_context é Option default None. Existing tests não alteram. Adicionar regression test explícito. |
| spawn_with_spec já tem 30+ call sites | Fase 31 mantém API atual como wrapper de _with_override(.., None). Zero breaking change. |
| WorktreeProvider::create pode falhar silenciosamente | Resumer::resume_with_objective propaga falha como AgentResult.success=false com summary explícita |
| Replay short-circuit pode emitir eventos errados | ToolCallCompleted ganha campo `"replayed": true` para diferenciar — dashboard/observability filtra |
| Cancellation no teste E2E pode ser race-y | Use `tokio::time::pause()` + deterministic mock dispatcher |
| Reuse(path) com worktree corrompida | Resumer detecta `git status` falhando, retorna ResumeError::CorruptedWorktree {path} |

---

## Verificação final agregada

```bash
# Track P — Tool Replay
cargo test -p theo-agent-runtime -- subagent::resume::tests::idempotency
cargo test -p theo-agent-runtime -- agent_loop::tests::with_resume_context
cargo test -p theo-agent-runtime -- run_engine::tests::dispatch_replays

# Track Q — Worktree Override
cargo test -p theo-agent-runtime -- subagent::tests::worktree_override
cargo test -p theo-agent-runtime -- subagent::resume::tests::worktree
cargo test -p theo-isolation -- worktree_handle_existing

# Track R — E2E
cargo test -p theo-agent-runtime --test resume_e2e

# Regression sweep
cargo test -p theo-agent-runtime --lib
cargo test -p theo-agent-runtime --tests
```

---

## Cronograma

```
Sprint único:
  Fase 30 → Fase 31 (paralelo OK)  ~3-4h cada
  Fase 32                          ~2h
  Regression sweep + commit + push ~30min

Total: 6-8h de trabalho concentrado
```

---

## Compromisso de cobertura final

Após este plano: **0 gaps reais NO RUNTIME**. Sistema SOTA puro na camada
de orchestration sem deferred items (excluídos deliberados ainda valem:
A2A protocol, file locking, learned router).

| Gap | Status pós-plano |
|---|---|
| #3 Resume tool replay | ✓ Fase 30 — replay short-circuit ativo no dispatch |
| #10 Resume worktree restore | ✓ Fase 31 — Resumer propaga estratégia ao spawn |

Plus E2E test (Fase 32) garante que ambos funcionam **juntos** com
cenário de crash determinístico.

---

## Trabalho fora deste plano

Confirmados como épicos separados, **NÃO** parte deste escopo:
- A2A protocol (sub-agents spawning sub-agents) — depth limit removal
- MCP HTTP transport — só stdio hoje
- File locking — worktree resolve maioria
- Learned router (fine-tuned model) — heurística cobre 90%
- IDE plugin de profundidade Cursor — produto/UX, não runtime
- Autocomplete inline — produto/UX

---

## Referências

- `docs/plans/sota-gaps-plan.md` v1 — fundação multi-agent
- `docs/plans/sota-gaps-followup-plan.md` v1 — fechou 10 gaps E2E real
- `crates/theo-agent-runtime/src/subagent/resume.rs` — data layer atual
- `crates/theo-agent-runtime/src/subagent/mod.rs::spawn_with_spec` — call site
- TDD: RED → GREEN → REFACTOR (sem exceções)
