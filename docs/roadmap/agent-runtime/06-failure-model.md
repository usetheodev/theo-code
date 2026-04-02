# Fase 06 — Failure Model (Retry, Backoff, DLQ)

## Objetivo

Adicionar retry estruturado com exponential backoff + jitter e dead-letter queue
para operações que falham permanentemente.

## Dependências

- Fase 01 (tipos base)
- Fase 02 (EventBus)
- Fase 04 (ToolCallManager)

## Arquivos

### Novos

| Arquivo | Crate | Conteúdo | Linhas Est. |
|---------|-------|----------|-------------|
| `src/retry_policy.rs` | theo-domain | `RetryPolicy`, `CorrectionStrategy` | ~60 |
| `src/retry.rs` | theo-agent-runtime | `RetryExecutor` | ~150 |
| `src/dlq.rs` | theo-agent-runtime | `DeadLetterQueue`, `DeadLetter` | ~80 |

### Modificados

| Arquivo | Mudança |
|---------|---------|
| `theo-domain/src/lib.rs` | Adicionar `pub mod retry_policy` |
| `theo-agent-runtime/src/run_engine.rs` | Integrar retry nas chamadas LLM e tools |
| `theo-agent-runtime/src/lib.rs` | Adicionar `pub mod retry, dlq` |

## Tipos Definidos

### theo-domain/src/retry_policy.rs

```rust
pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub jitter: bool,
}

impl RetryPolicy {
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration;
    pub fn default_llm() -> Self;   // 3 retries, 1000ms, 30000ms max, jitter=true
    pub fn default_tool() -> Self;  // 2 retries, 200ms, 5000ms max, jitter=true
}

pub enum CorrectionStrategy {
    RetryLocal,
    Replan,
    Subtask,
    AgentSwap,
}
```

### theo-agent-runtime/src/retry.rs

```rust
pub struct RetryExecutor;

impl RetryExecutor {
    pub async fn with_retry<F, T, E>(
        policy: &RetryPolicy,
        operation_name: &str,
        event_bus: &EventBus,
        f: F,
    ) -> Result<T, E>
    where
        F: Fn() -> Pin<Box<dyn Future<Output = Result<T, E>> + Send>>,
        E: Display;
}
```

### theo-agent-runtime/src/dlq.rs

```rust
pub struct DeadLetter {
    pub call_id: CallId,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub error: String,
    pub attempts: u32,
    pub created_at: u64,
}

pub struct DeadLetterQueue {
    letters: Vec<DeadLetter>,
}

impl DeadLetterQueue {
    pub fn push(&mut self, letter: DeadLetter);
    pub fn drain(&mut self) -> Vec<DeadLetter>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}
```

## Tipos de Falha

| Tipo | Tratamento |
|------|-----------|
| Transitória (network, rate limit) | retry com backoff |
| Determinística (args inválidos) | abort |
| Permissão (denied) | fail imediato |
| Timeout | retry limitado |
| Semântica (output errado) | replan |
| Infra (provider down) | fallback |

## Testes Requeridos (~15)

- Backoff: `attempt=0 → base_delay`, `attempt=3 → base*8 capped at max`
- Jitter dentro de `[0, delay]`
- `RetryExecutor` retries até `max_retries` e depois falha
- `RetryExecutor` retorna sucesso no retry N se operação volta a funcionar
- DLQ aceita dead letters e `drain` esvazia
- DLQ `len()` e `is_empty()` corretos
- Integração: tool call falha 3x → entra na DLQ
- `default_llm()` valores corretos
- `default_tool()` valores corretos
- `CorrectionStrategy` serde roundtrip
- Evento emitido a cada retry
- `delay_for_attempt` nunca excede `max_delay_ms`
- `delay_for_attempt` com jitter produz valores variados
- DeadLetter serde roundtrip
- RetryPolicy com 0 retries executa apenas uma vez

## Definition of Done

| # | Critério | Verificação |
|---|----------|-------------|
| 1 | LLM calls no RunEngine usam `RetryExecutor` | Code review |
| 2 | Tool calls que esgotam retries entram na DLQ | Teste de integração |
| 3 | `delay_for_attempt` nunca excede `max_delay_ms` | Teste unitário |
| 4 | 15+ testes passando | `cargo test` |
