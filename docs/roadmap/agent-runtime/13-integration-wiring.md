# Fase 13 — Integração Real (Wiring)

## Contexto

As Fases 01-12 construíram 24 componentes isolados com 310 testes passando.
Os componentes **não estão conectados**:
- `AgentLoop::run` ainda usa o for-loop antigo
- `RunEngine.execute()` chama `tool_bridge` diretamente
- Retry hardcoded (sleep 2s, 1 retry)
- Sem budget enforcement, capabilities, metrics, snapshots

Esta fase conecta tudo em 5 sub-fases incrementais.

## Grafo de Dependências

```
Sub A: RunEngine → ToolCallManager (substitui tool_bridge direto)
    ↓
Sub B: RunEngine + BudgetEnforcer + RetryExecutor
    ↓
Sub C: ToolCallManager + CapabilityGate + RunEngine + MetricsCollector
    ↓
Sub D: RunEngine + ConvergenceEvaluator + Snapshots
    ↓
Sub E: AgentLoop::run delega para RunEngine (facade final)
```

---

## Sub-fase A — RunEngine → ToolCallManager

### Objetivo
RunEngine usa ToolCallManager em vez de `tool_bridge::execute_tool_call` direto.

### Arquivo Modificado
- `crates/theo-agent-runtime/src/run_engine.rs`

### Mudanças
1. Substituir chamada direta a `tool_bridge::execute_tool_call` por:
   - `self.tool_call_manager.enqueue(task_id, tool_name, input)` → CallId
   - `self.tool_call_manager.dispatch_and_execute(call_id, registry, ctx)` → ToolResultRecord
2. Construir `Message::tool_result` a partir do `ToolResultRecord`
3. Preservar lógica de AgentState update (record_read, record_search, record_edit_attempt)

### Testes
- `run_engine_uses_tool_call_manager` — tool_call_manager.calls_for_task tem records após run
- `tool_calls_tracked_with_call_id` — eventos ToolCallQueued/Completed publicados

### Definition of Done

| # | Critério | Verificação |
|---|----------|-------------|
| 1 | Zero chamadas diretas a `tool_bridge::execute_tool_call` no run_engine.rs | `grep "tool_bridge::execute_tool_call" run_engine.rs` retorna vazio |
| 2 | ToolCallManager.calls_for_task retorna records após run | Teste unitário |
| 3 | Eventos ToolCallQueued + ToolCallCompleted publicados para cada tool | CapturingListener |
| 4 | Testes existentes do run_engine passam | `cargo test -p theo-agent-runtime` |
| 5 | `cargo check --workspace` compila limpo | Compilação |

---

## Sub-fase B — BudgetEnforcer + RetryExecutor

### Objetivo
Substituir retry hardcoded por RetryExecutor. Adicionar BudgetEnforcer ao loop.

### Arquivo Modificado
- `crates/theo-agent-runtime/src/run_engine.rs`

### Mudanças
1. Adicionar `budget_enforcer: BudgetEnforcer` ao struct
2. No construtor, criar BudgetEnforcer com `Budget::default()` + event_bus
3. Início de cada iteração:
   ```rust
   self.budget_enforcer.record_iteration();
   if let Err(violation) = self.budget_enforcer.check() {
       self.transition_run(RunState::Aborted);
       return AgentResult { success: false, summary: format!("Budget: {}", violation), ... };
   }
   ```
4. Após LLM call: `self.budget_enforcer.record_tokens(tokens)`
5. Após tool call: `self.budget_enforcer.record_tool_call()`
6. Substituir retry hardcoded por:
   ```rust
   let policy = RetryPolicy::default_llm();
   let response = RetryExecutor::with_retry(&policy, "llm_call", &self.event_bus, f, is_retryable).await;
   ```
7. Remover check manual `iteration > max_iterations`

### Testes
- `budget_enforcer_aborts_on_iterations_exceeded`
- `retry_executor_used_for_llm_calls` — eventos de retry publicados
- `budget_records_tokens_after_llm_call`

### Definition of Done

| # | Critério | Verificação |
|---|----------|-------------|
| 1 | Zero `tokio::time::sleep(Duration::from_secs(2))` no run_engine.rs | grep retorna vazio |
| 2 | BudgetEnforcer.check() chamado antes de cada iteração | Code review |
| 3 | record_tokens() chamado após cada LLM response | Code review |
| 4 | record_tool_call() chamado após cada tool execution | Code review |
| 5 | Eventos de retry publicados via RetryExecutor | CapturingListener |
| 6 | `cargo test -p theo-agent-runtime` passa | Testes |

---

## Sub-fase C — CapabilityGate + MetricsCollector

### Objetivo
ToolCallManager checa capabilities antes de dispatch. RunEngine coleta metrics.

### Arquivos Modificados
- `crates/theo-agent-runtime/src/tool_call_manager.rs`
- `crates/theo-agent-runtime/src/run_engine.rs`

### Mudanças tool_call_manager.rs
1. Adicionar `capability_gate: Option<Arc<CapabilityGate>>` ao struct
2. Em `dispatch_and_execute()`, antes de executar:
   ```rust
   if let Some(gate) = &self.capability_gate {
       gate.check_tool(&record.tool_name, tool_category)?;
   }
   ```
3. Método builder: `with_capability_gate(gate: Arc<CapabilityGate>) -> Self`

### Mudanças run_engine.rs
1. Adicionar `metrics: Arc<MetricsCollector>` ao struct
2. Após LLM call: `self.metrics.record_llm_call(duration_ms, tokens)`
3. Após tool call: `self.metrics.record_tool_call(name, duration_ms, success)`
4. No final: `self.metrics.record_run_complete(converged)`
5. Expor: `pub fn metrics(&self) -> RuntimeMetrics`

### Testes
- `capability_gate_blocks_denied_tool`
- `metrics_collected_during_run`
- `metrics_records_convergence`

### Definition of Done

| # | Critério | Verificação |
|---|----------|-------------|
| 1 | ToolCallManager rejeita tools negadas pelo CapabilityGate | Teste unitário |
| 2 | MetricsCollector.record_llm_call chamado no RunEngine | Code review |
| 3 | `engine.metrics()` retorna snapshot com dados reais | Teste unitário |
| 4 | Testes existentes passam | `cargo test` |
| 5 | `cargo check --workspace` compila limpo | Compilação |

---

## Sub-fase D — ConvergenceEvaluator + Snapshots

### Objetivo
Usar ConvergenceEvaluator no evaluate step. Salvar snapshots a cada iteração.

### Arquivo Modificado
- `crates/theo-agent-runtime/src/run_engine.rs`

### Mudanças
1. Adicionar `convergence: ConvergenceEvaluator` ao struct
2. No evaluate step (quando "done" é chamado), substituir `has_real_changes()` por:
   ```rust
   let context = ConvergenceContext {
       has_git_changes: check_git_changes(&self.project_dir).await,
       edits_succeeded: self.agent_state.edits_succeeded,
       done_requested: true,
       iteration: self.run.iteration,
       max_iterations: self.run.max_iterations,
   };
   if self.convergence.evaluate(&context) { /* converged */ }
   ```
3. Adicionar `snapshot_store: Option<Arc<dyn SnapshotStore>>` ao struct
4. Final de cada iteração, salvar snapshot se store presente

### Testes
- `convergence_evaluator_used_in_evaluate`
- `snapshot_saved_each_iteration` — FileSnapshotStore em tempdir
- `pending_criteria_reported_when_not_converged`

### Definition of Done

| # | Critério | Verificação |
|---|----------|-------------|
| 1 | Zero chamadas diretas a `has_real_changes()` no RunEngine | grep retorna vazio |
| 2 | RunSnapshot salvo a cada iteração (quando store presente) | Teste com tempdir |
| 3 | Snapshot validatable via validate_checksum() | Teste unitário |
| 4 | Pending criteria no feedback ao agente | Teste unitário |
| 5 | `cargo test -p theo-agent-runtime` passa | Testes |

---

## Sub-fase E — AgentLoop::run como Facade

### Objetivo
AgentLoop::run delega 100% para RunEngine. Loop antigo removido.

### Arquivo Modificado
- `crates/theo-agent-runtime/src/agent_loop.rs`

### Mudanças
1. AgentLoop::new mantém mesma assinatura (`Arc<dyn EventSink>`)
2. `run()` cria componentes internamente e delega para RunEngine
3. Criar `EventSinkBridge` (EventListener → EventSink mapping)
4. Remover for-loop antigo
5. Manter `phase_nudge` e `has_real_changes` como funções livres (backward compat testes)

### Testes
- `agent_loop_run_delegates_to_engine`
- `agent_loop_backward_compat_signature`
- `event_sink_bridge_forwards_events`
- `cli_binary_compiles` — `cargo check -p theo-code`

### Definition of Done

| # | Critério | Verificação |
|---|----------|-------------|
| 1 | AgentLoop::run NÃO contém for-loop | Code review |
| 2 | AgentLoop::new mantém assinatura (Arc<dyn EventSink>) | Compilação |
| 3 | CLI binary compila sem mudanças | `cargo check -p theo-code` |
| 4 | Desktop app compila sem mudanças | `cargo check -p theo-code-desktop` |
| 5 | Todos os testes passam | `cargo test -p theo-agent-runtime` |
| 6 | `cargo check --workspace` compila limpo | Compilação |

---

## Verificação Final

Após as 5 sub-fases:

```bash
cargo test -p theo-agent-runtime    # > 310 testes, 0 falhas
cargo check --workspace              # compila limpo
```

### Checklist dos 8 Invariantes

| # | Invariante | Enforced por | Status Pré | Status Pós |
|---|-----------|-------------|-----------|-----------|
| 1 | Task tem task_id/session_id/state/created_at | TaskManager | Isolado | Via RunEngine |
| 2 | Tool Call tem call_id único | ToolCallManager | Isolado | Via RunEngine (Sub A) |
| 3 | Tool Result referencia call_id | ToolCallManager | Isolado | Via RunEngine (Sub A) |
| 4 | Completed não volta para Running | TaskState | Funciona | Funciona |
| 5 | Transição gera Event | TaskManager+RunEngine | Parcial | Completo (Sub A-E) |
| 6 | Execução tem run_id | AgentRunEngine | Funciona | Funciona |
| 7 | Resume de snapshot consistente | RunSnapshot | Isolado | Via RunEngine (Sub D) |
| 8 | Sem execução sem budget | BudgetEnforcer | Isolado | Via RunEngine (Sub B) |
