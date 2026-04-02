# Fase 02 — Event System

## Objetivo

Substituir o sistema de eventos atual (`AgentEvent` + `EventSink`) por um sistema spec-compliant
com persistência em memória e notificação de listeners. Domain define os tipos; runtime implementa o bus.

## Invariante Endereçado

- **Invariante 5**: Toda transição de estado gera um Event persistido

## Dependência

- Fase 01 (identifiers, tipos base)

## Arquivos

### Novos

| Arquivo | Crate | Conteúdo | Linhas Est. |
|---------|-------|----------|-------------|
| `src/event.rs` | theo-domain | `DomainEvent`, `EventType` | ~120 |
| `src/event_bus.rs` | theo-agent-runtime | `EventBus`, `EventListener` trait | ~180 |

### Modificados

| Arquivo | Mudança |
|---------|---------|
| `theo-domain/src/lib.rs` | Adicionar `pub mod event` |
| `theo-agent-runtime/src/events.rs` | Deprecar tipos antigos, re-exportar novos |
| `theo-agent-runtime/src/lib.rs` | Adicionar `pub mod event_bus` |

## Tipos Definidos

### theo-domain/src/event.rs

```rust
pub struct DomainEvent {
    pub event_id: EventId,
    pub event_type: EventType,
    pub entity_id: String,
    pub timestamp: u64,
    pub payload: serde_json::Value,
}

pub enum EventType {
    TaskCreated, TaskStateChanged,
    ToolCallQueued, ToolCallDispatched, ToolCallCompleted,
    RunInitialized, RunStateChanged,
    LlmCallStart, LlmCallEnd,
    BudgetExceeded, Error,
}
```

### theo-agent-runtime/src/event_bus.rs

```rust
#[async_trait]
pub trait EventListener: Send + Sync {
    async fn on_event(&self, event: &DomainEvent);
}

pub struct EventBus {
    listeners: Vec<Arc<dyn EventListener>>,
    log: Mutex<Vec<DomainEvent>>,
}

impl EventBus {
    pub fn new() -> Self;
    pub fn subscribe(&mut self, listener: Arc<dyn EventListener>);
    pub async fn publish(&self, event: DomainEvent);
    pub fn events(&self) -> Vec<DomainEvent>;
    pub fn events_for(&self, entity_id: &str) -> Vec<DomainEvent>;
}
```

Implementações: `PrintEventListener` (substitui `PrintEventSink`), `NullEventListener`.

## Testes Requeridos (~15)

- `DomainEvent` serde roundtrip para cada `EventType`
- `EventBus::publish` appends ao log e notifica listeners
- `EventBus::events_for` filtra corretamente
- `EventBus::events` retorna na ordem de inserção
- Invariante 5 contract test: transição de estado → evento gerado
- `NullEventListener` não panics
- Múltiplos listeners recebem o mesmo evento

## Definition of Done

| # | Critério | Verificação |
|---|----------|-------------|
| 1 | Toda transição de estado produz um `DomainEvent` (integration test) | Teste de contrato |
| 2 | `EventBus::events()` retorna eventos na ordem de inserção | Teste unitário |
| 3 | Antigos `AgentEvent` e `EventSink` preservados com `#[deprecated]` | Compilação sem breaking change |
| 4 | `cargo test -p theo-agent-runtime` passa com 15+ novos testes | `cargo test` |
