# Fase 11 — Observability (Metrics, Tracing, Structured Logs)

## Objetivo

Adicionar métricas estruturadas, tracing spans e output de log para observabilidade
em produção.

## Dependências

- Fase 02 (EventBus, EventListener)

## Arquivos

### Novos

| Arquivo | Crate | Conteúdo | Linhas Est. |
|---------|-------|----------|-------------|
| `src/metrics.rs` | theo-agent-runtime | `RuntimeMetrics`, `MetricsCollector` | ~120 |
| `src/observability.rs` | theo-agent-runtime | `TracingEventListener`, `StructuredLogListener` | ~180 |

### Modificados

| Arquivo | Mudança |
|---------|---------|
| `Cargo.toml` | Adicionar `tracing` como feature opcional |
| `src/run_engine.rs` | Integrar MetricsCollector |
| `src/lib.rs` | Adicionar `pub mod metrics, observability` |

## Tipos Definidos

### metrics.rs

```rust
#[derive(Debug, Clone, Default)]
pub struct RuntimeMetrics {
    // Contadores
    pub total_runs: u64,
    pub total_tasks: u64,
    pub total_tool_calls: u64,
    pub total_llm_calls: u64,
    pub total_tokens_used: u64,
    pub total_retries: u64,
    pub total_dlq_entries: u64,

    // Médias
    pub avg_iteration_ms: f64,
    pub avg_tool_call_ms: f64,
    pub avg_llm_call_ms: f64,

    // Taxas
    pub tool_success_rate: f64,
    pub convergence_rate: f64,
    pub resume_success_rate: f64,
}

pub struct MetricsCollector {
    metrics: Arc<RwLock<RuntimeMetrics>>,
}

impl MetricsCollector {
    pub fn new() -> Self;
    pub fn record_llm_call(&self, duration_ms: u64, tokens: u64);
    pub fn record_tool_call(&self, tool_name: &str, duration_ms: u64, success: bool);
    pub fn record_retry(&self);
    pub fn record_dlq_entry(&self);
    pub fn record_run_complete(&self, converged: bool);
    pub fn snapshot(&self) -> RuntimeMetrics;
}
```

### observability.rs

```rust
pub struct TracingEventListener;

impl EventListener for TracingEventListener {
    async fn on_event(&self, event: &DomainEvent) {
        // tracing::info_span! / tracing::event!
    }
}

pub struct StructuredLogListener {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl StructuredLogListener {
    pub fn new(writer: Box<dyn Write + Send>) -> Self;
    pub fn stdout() -> Self;
    pub fn file(path: &Path) -> io::Result<Self>;
}

impl EventListener for StructuredLogListener {
    async fn on_event(&self, event: &DomainEvent) {
        // JSON line por evento
    }
}
```

## Métricas Obrigatórias (da Spec)

| Métrica | Onde coletada |
|---------|---------------|
| Tempo por execução | RunEngine |
| Sucesso por tool | ToolCallManager |
| Retries | RetryExecutor |
| Custo (tokens) | BudgetEnforcer |
| Loops por task | RunEngine |
| Taxa de convergência | RunEngine |
| Falhas por tipo | DLQ + ToolCallManager |
| Resume success rate | Persistence |

## Tracing Hierarchy

```
session → task → agent_run → tool_call
```

Cada nível gera spans e eventos rastreáveis por ID.

## Testes Requeridos (~10)

- `MetricsCollector` acumula corretamente com múltiplas chamadas
- `snapshot()` retorna estado consistente
- `StructuredLogListener` escreve JSON lines válidos
- `TracingEventListener` não panics em nenhum tipo de evento
- Success rate calculado corretamente (0/0 = 0.0, não NaN)
- File listener escreve em arquivo (tempfile)
- Stdout listener não panics
- Metrics após reset zeradas
- Concurrent recording (thread-safe via RwLock)
- JSON line format parseable

## Definition of Done

| # | Critério | Verificação |
|---|----------|-------------|
| 1 | MetricsCollector integrado no RunEngine; atualizado a cada LLM e tool call | Code review |
| 2 | StructuredLogListener escreve um JSON line por evento | Teste unitário |
| 3 | `tracing` feature é opcional (`cfg(feature = "tracing")`) | `cargo check` sem feature |
| 4 | 10+ testes passando | `cargo test` |
