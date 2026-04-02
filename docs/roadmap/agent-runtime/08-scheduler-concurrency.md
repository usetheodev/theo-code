# Fase 08 — Scheduler & Concurrency Control

## Objetivo

Adicionar um task scheduler que suporta prioridade, fairness e limites de concorrência
para cenários multi-task.

## Dependências

- Fase 03 (TaskManager)
- Fase 05 (RunEngine)

## Arquivos

### Novos

| Arquivo | Crate | Conteúdo | Linhas Est. |
|---------|-------|----------|-------------|
| `src/priority.rs` | theo-domain | `Priority` enum | ~40 |
| `src/scheduler.rs` | theo-agent-runtime | `Scheduler`, `SchedulerConfig` | ~250 |

### Modificados

| Arquivo | Mudança |
|---------|---------|
| `theo-domain/src/lib.rs` | Adicionar `pub mod priority` |
| `theo-agent-runtime/src/lib.rs` | Adicionar `pub mod scheduler` |

## Tipos Definidos

### theo-domain/src/priority.rs

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}
```

### theo-agent-runtime/src/scheduler.rs

```rust
pub struct SchedulerConfig {
    pub max_concurrent_runs: usize,        // default: 1
    pub max_concurrent_tool_calls: usize,  // default: 1
    pub fairness_window_secs: u64,         // round-robin window
}

pub struct Scheduler {
    config: SchedulerConfig,
    queue: BinaryHeap<ScheduledTask>,
    active: HashMap<RunId, JoinHandle<AgentResult>>,
    semaphore: Arc<Semaphore>,
    event_bus: Arc<EventBus>,
}

struct ScheduledTask {
    task_id: TaskId,
    priority: Priority,
    enqueued_at: Instant,
}

impl Scheduler {
    pub fn new(config: SchedulerConfig, event_bus: Arc<EventBus>) -> Self;
    pub async fn submit(&mut self, task_id: TaskId, priority: Priority);
    pub async fn run_next(&mut self) -> Option<AgentResult>;
    pub fn active_count(&self) -> usize;
    pub fn queue_depth(&self) -> usize;
    pub async fn cancel(&mut self, task_id: &TaskId) -> bool;
    pub async fn drain(&mut self);
}
```

## Controles do Scheduler

| Controle | Mecanismo |
|----------|-----------|
| Concorrência | tokio `Semaphore` |
| Prioridade | `BinaryHeap` com `Ord` |
| Starvation avoidance | `enqueued_at` como tiebreaker (FIFO para mesma prioridade) |
| Fairness entre sessões | Round-robin window |
| Limite de execução paralela | `max_concurrent_runs` |

## Testes Requeridos (~12)

- Single task roda imediatamente
- Limite de concorrência respeitado (2 tasks, max=1 → sequencial)
- Prioridade: Critical roda antes de Low
- Fairness: mesma prioridade → FIFO
- Cancel remove da fila
- Cancel aborta run ativo
- `active_count` correto
- `queue_depth` correto
- `drain` espera todas as runs ativas terminarem
- Submit com fila cheia (backpressure)
- Scheduler vazio retorna `None`
- Múltiplas sessões com fairness

## Definition of Done

| # | Critério | Verificação |
|---|----------|-------------|
| 1 | `Scheduler` respeita `max_concurrent_runs` via tokio `Semaphore` | Teste de concorrência |
| 2 | Tasks de maior prioridade preemptam na fila | Teste unitário |
| 3 | `cancel` transiciona task para `Cancelled` | Teste unitário |
| 4 | 12+ testes passando | `cargo test` |
