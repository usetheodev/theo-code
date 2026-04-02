# Fase 05 — Agent Run Lifecycle

## Objetivo

Substituir o for-loop simples em `agent_loop.rs` por um `AgentRunEngine` que segue o ciclo
Initialized → Planning → Executing → Evaluating → Converged/Replanning.

## Invariante Endereçado

- **Invariante 6**: Toda execução agentiva possui `run_id`

## Dependências

- Fase 01 (RunState, AgentRun, RunId)
- Fase 02 (EventBus)
- Fase 03 (TaskManager)
- Fase 04 (ToolCallManager)

## Arquivos

### Novos

| Arquivo | Crate | Conteúdo | Linhas Est. |
|---------|-------|----------|-------------|
| `src/run_engine.rs` | theo-agent-runtime | `AgentRunEngine` | ~350 |

### Modificados

| Arquivo | Mudança |
|---------|---------|
| `src/agent_loop.rs` | Reescrito como facade sobre `RunEngine` |
| `src/state.rs` | `Phase` enum marcado `#[deprecated]` |
| `src/lib.rs` | Adicionar `pub mod run_engine` |

## Tipos Definidos

```rust
pub struct AgentRunEngine {
    run: AgentRun,
    task_manager: Arc<Mutex<TaskManager>>,
    tool_call_manager: Arc<Mutex<ToolCallManager>>,
    event_bus: Arc<EventBus>,
    client: LlmClient,
    registry: ToolRegistry,
    config: AgentConfig,
}

impl AgentRunEngine {
    pub fn new(/* params */) -> Self;
    // Invariante 6: gera run_id

    pub async fn execute(
        &mut self,
        task_id: &TaskId,
        project_dir: &Path,
    ) -> AgentResult;

    // Ciclo interno:
    async fn plan(&mut self) -> RunState;
    async fn execute_step(&mut self) -> RunState;
    async fn evaluate(&mut self) -> RunState;
}
```

### AgentLoop como Facade

```rust
impl AgentLoop {
    pub async fn run(&self, task: &str, project_dir: &Path) -> AgentResult {
        let task_id = self.task_manager.lock().create_task(/*...*/);
        let mut engine = AgentRunEngine::new(/*...*/);
        engine.execute(&task_id, project_dir).await
    }
}
```

## Ciclo de Execução

```
while NOT converged AND within_budget:
    1. plan()       → Planning state
    2. execute()    → Executing state (LLM call + tool calls)
    3. evaluate()   → Evaluating state
    4. decide:
       - converged?   → Converged (terminal)
       - need replan?  → Replanning → volta a 1
       - budget out?   → Aborted (terminal)
    5. persist state
```

## Testes Requeridos (~18)

- `new()` gera `run_id` único (Invariante 6)
- Ciclo completo: Initialized → Planning → Executing → Evaluating → Converged
- Ciclo com replan: Evaluating → Replanning → Planning → Executing → Converged
- Max iterations trigger abort
- Cada transição de estado emite evento
- `AgentLoop::run` funciona como antes (backward compat)
- RunState::can_transition_to validado em cada passo
- Estado persistido entre iterações
- Plan step produz mensagem LLM
- Execute step processa tool calls via ToolCallManager
- Evaluate step verifica "done" meta-tool
- Aborted é estado terminal
- Converged é estado terminal
- Waiting state quando aguardando input
- run.iteration incrementado a cada ciclo
- run.updated_at atualizado a cada transição
- Múltiplas runs para a mesma task (retry scenario)
- Old `Phase` enum ainda compila (deprecated, não removido)

## Definition of Done

| # | Critério | Verificação |
|---|----------|-------------|
| 1 | Invariante 6: todo run tem `run_id` único | Teste unitário |
| 2 | Transições validadas via `RunState::can_transition_to` | Code review |
| 3 | `AgentLoop::run` funciona como facade (backward compat) | Teste existente passa |
| 4 | `Phase` enum marcado `#[deprecated]` mas compila | `cargo check` |
| 5 | 18+ testes passando | `cargo test -p theo-agent-runtime` |
