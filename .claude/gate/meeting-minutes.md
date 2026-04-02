# Meeting — 2026-04-01 (Fase 03: Task Lifecycle)

## Proposta
TaskManager em theo-agent-runtime — gerenciador de ciclo de vida de Tasks.
Wraps transitions + EventBus publish. Enforça Invariantes 1, 4 e 5.
Correções QA incorporadas: create_task emite TaskCreated, testes expandidos para ~16,
payload com from/to verificado, thread-safe via Mutex.

## Participantes
- **governance** — Principal Engineer (APPROVE)
- **qa** — QA Staff Engineer (validated=false → correções incorporadas)

## Análises

### Governance (APPROVE)
- TaskManager correto em theo-agent-runtime (orquestrador, não domain)
- Arc<EventBus> injetado (não owned) — SRP correto
- HashMap<TaskId, Task> adequado, TaskId tem Hash+Eq
- API de 5 métodos minimal e YAGNI-compliant
- Task::transition() no domain faz validação, manager não duplica

### QA (validated=false → corrigido)
- 12 testes insuficientes → expandido para ~16
- create_task deve emitir TaskCreated (Invariant 5 incompleto)
- Running→Failed e Running→Cancelled ausentes do plano
- active_tasks deve excluir os 3 terminais explicitamente
- Payload do evento deve conter from/to
- Thread safety recomendada

## Conflitos
1. Invariant 5 incompleto sem TaskCreated — incorporado
2. Terminais parcialmente cobertos — expandido para 3
3. Payload sem verificação — adicionado teste de from/to

## Veredito
**APPROVED** (com correções incorporadas)

## Escopo Aprovado
- `crates/theo-agent-runtime/src/task_manager.rs` (novo)
- `crates/theo-agent-runtime/src/lib.rs` (modificado — add pub mod)

## Condições
1. create_task emite DomainEvent::TaskCreated
2. transition emite DomainEvent::TaskStateChanged com payload {from, to}
3. Mínimo 16 testes
4. active_tasks exclui Completed, Failed E Cancelled
5. Running→Failed e Running→Cancelled testados
6. TaskManager thread-safe (Mutex<HashMap>)
7. cargo check --workspace compila limpo
