# Fase 03 — Task Lifecycle

## Objetivo

Implementar a entidade Task no runtime com lifecycle management completo,
enforcing Invariantes 1 e 4.

## Invariantes Endereçados

- **Invariante 1**: Toda Task possui `task_id`, `session_id`, `state`, `created_at`
- **Invariante 4**: Nenhuma Task pode voltar de `completed` para `running`

## Dependências

- Fase 01 (Task, TaskState, TaskId)
- Fase 02 (EventBus)

## Arquivos

### Novos

| Arquivo | Crate | Conteúdo | Linhas Est. |
|---------|-------|----------|-------------|
| `src/task_manager.rs` | theo-agent-runtime | `TaskManager` | ~200 |

### Modificados

| Arquivo | Mudança |
|---------|---------|
| `theo-agent-runtime/src/lib.rs` | Adicionar `pub mod task_manager` |

## Tipos Definidos

```rust
pub struct TaskManager {
    tasks: HashMap<TaskId, Task>,
    event_bus: Arc<EventBus>,
}

impl TaskManager {
    pub fn create_task(
        &mut self,
        session_id: SessionId,
        agent_type: AgentType,
        objective: String,
    ) -> TaskId;
    // Invariante 1: sempre seta task_id, session_id, state=Pending, created_at

    pub fn transition(
        &mut self,
        task_id: &TaskId,
        target: TaskState,
    ) -> Result<(), TransitionError>;
    // Invariante 4: completed→running rejeitado

    pub fn get(&self, task_id: &TaskId) -> Option<&Task>;
    pub fn tasks_by_session(&self, session_id: &SessionId) -> Vec<&Task>;
    pub fn active_tasks(&self) -> Vec<&Task>;
}
```

## Testes Requeridos (~12)

- `create_task` retorna TaskId e task tem todos os campos obrigatórios (Invariante 1)
- Happy path completo: Pending → Ready → Running → Completed
- Completed → Running retorna `Err(TransitionError)` (Invariante 4)
- Failed → Running retorna `Err`
- Cancelled → qualquer estado retorna `Err`
- Cada transição emite `DomainEvent::TaskStateChanged`
- `tasks_by_session` filtra corretamente
- `active_tasks` exclui terminais
- Task não encontrada retorna `None`
- Múltiplas tasks na mesma sessão
- Transição de estado atualiza `updated_at`
- `completed_at` preenchido quando chega em `Completed`

## Definition of Done

| # | Critério | Verificação |
|---|----------|-------------|
| 1 | `create_task` garante Invariante 1 (task_id, session_id, state, created_at) | Teste unitário |
| 2 | `transition` rejeita violações do Invariante 4 | Teste unitário |
| 3 | Toda transição publica no EventBus | Teste de integração |
| 4 | 12+ testes passando | `cargo test -p theo-agent-runtime` |
