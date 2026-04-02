# Fase 07 — Budget Enforcement

## Objetivo

Enforçar Invariante 8: nenhuma execução pode rodar sem limite de orçamento.
Rastrear consumo de tempo e tokens.

## Invariante Endereçado

- **Invariante 8**: Nenhuma execução pode rodar sem limite de orçamento (tempo/token)

## Dependências

- Fase 01 (tipos base)
- Fase 02 (EventBus)
- Fase 05 (RunEngine)

## Arquivos

### Novos

| Arquivo | Crate | Conteúdo | Linhas Est. |
|---------|-------|----------|-------------|
| `src/budget.rs` | theo-domain | `Budget`, `BudgetUsage`, `BudgetViolation` | ~80 |
| `src/budget_enforcer.rs` | theo-agent-runtime | `BudgetEnforcer` | ~140 |

### Modificados

| Arquivo | Mudança |
|---------|---------|
| `theo-domain/src/lib.rs` | Adicionar `pub mod budget` |
| `theo-agent-runtime/src/config.rs` | Adicionar campo `Budget` ao `AgentConfig` |
| `theo-agent-runtime/src/run_engine.rs` | Checar budget antes de cada step |
| `theo-agent-runtime/src/lib.rs` | Adicionar `pub mod budget_enforcer` |

## Tipos Definidos

### theo-domain/src/budget.rs

```rust
pub struct Budget {
    pub max_time_secs: u64,         // wall-clock limit (default: 300)
    pub max_tokens: u64,            // total token limit (default: 200_000)
    pub max_iterations: usize,      // iteration limit (default: 30)
    pub max_tool_calls: usize,      // tool call cap (default: 100)
}

pub struct BudgetUsage {
    pub elapsed_secs: u64,
    pub tokens_used: u64,
    pub iterations_used: usize,
    pub tool_calls_used: usize,
}

impl BudgetUsage {
    pub fn exceeds(&self, budget: &Budget) -> Option<BudgetViolation>;
}

pub enum BudgetViolation {
    TimeExceeded,
    TokensExceeded,
    IterationsExceeded,
    ToolCallsExceeded,
}
```

### theo-agent-runtime/src/budget_enforcer.rs

```rust
pub struct BudgetEnforcer {
    budget: Budget,
    usage: BudgetUsage,
    start_time: Instant,
    event_bus: Arc<EventBus>,
}

impl BudgetEnforcer {
    pub fn new(budget: Budget, event_bus: Arc<EventBus>) -> Self;
    pub fn check(&self) -> Result<(), BudgetViolation>;
    pub fn record_tokens(&mut self, tokens: u64);
    pub fn record_iteration(&mut self);
    pub fn record_tool_call(&mut self);
    pub fn usage(&self) -> &BudgetUsage;
    pub fn remaining(&self) -> Budget;
}
```

## Testes Requeridos (~12)

- Budget check passa quando dentro dos limites
- Cada tipo de violação detectado corretamente
- `record_tokens` acumula corretamente
- `record_iteration` incrementa
- `record_tool_call` incrementa
- `remaining()` calcula corretamente
- RunEngine aborta quando budget excedido (integration test com mock LLM)
- Evento `BudgetExceeded` emitido na violação
- Default budget com valores sensatos
- BudgetViolation serde roundtrip
- `exceeds` retorna `None` quando dentro do budget
- Time budget baseado em wall-clock (não CPU)

## Definition of Done

| # | Critério | Verificação |
|---|----------|-------------|
| 1 | RunEngine checa budget antes de cada LLM call e tool call (Invariante 8) | Code review |
| 2 | Violação de budget emite `BudgetExceeded` e aborta o run | Teste de integração |
| 3 | Token usage rastreado do `ChatResponse::usage` | Code review |
| 4 | `AgentConfig` inclui campo `Budget` com default | Teste unitário |
| 5 | 12+ testes passando | `cargo test` |
