# Fase 04 — Tool Call Lifecycle

## Objetivo

Substituir a execução ad-hoc de tools em `agent_loop.rs` por um `ToolCallManager` que
rastreia cada call através de Queued → Running → Succeeded/Failed/Timeout.

## Invariantes Endereçados

- **Invariante 2**: Toda Tool Call possui `call_id` único e rastreável
- **Invariante 3**: Todo Tool Result referencia um `call_id`

## Dependências

- Fase 01 (ToolCallState, ToolCallRecord, ToolResultRecord, CallId)
- Fase 02 (EventBus)
- Fase 03 (TaskManager — para associar calls a tasks)

## Arquivos

### Novos

| Arquivo | Crate | Conteúdo | Linhas Est. |
|---------|-------|----------|-------------|
| `src/tool_call_manager.rs` | theo-agent-runtime | `ToolCallManager` | ~250 |

### Modificados

| Arquivo | Mudança |
|---------|---------|
| `src/tool_bridge.rs` | Delegar execução pelo manager |
| `src/lib.rs` | Adicionar `pub mod tool_call_manager` |

## Tipos Definidos

```rust
pub struct ToolCallManager {
    calls: HashMap<CallId, ToolCallRecord>,
    results: HashMap<CallId, ToolResultRecord>,
    event_bus: Arc<EventBus>,
}

impl ToolCallManager {
    pub fn enqueue(
        &mut self,
        task_id: TaskId,
        tool_name: String,
        input: serde_json::Value,
    ) -> CallId;
    // Invariante 2: gera call_id único

    pub async fn dispatch_and_execute(
        &mut self,
        call_id: &CallId,
        registry: &ToolRegistry,
        ctx: &ToolContext,
    ) -> ToolResultRecord;
    // Invariante 3: resultado referencia call_id

    pub fn get_record(&self, call_id: &CallId) -> Option<&ToolCallRecord>;
    pub fn get_result(&self, call_id: &CallId) -> Option<&ToolResultRecord>;
    pub fn calls_for_task(&self, task_id: &TaskId) -> Vec<&ToolCallRecord>;
}
```

## Testes Requeridos (~15)

- `enqueue` produz `CallId` único (Invariante 2)
- `ToolResultRecord` sempre referencia seu `CallId` (Invariante 3)
- Progressão de estado: Queued → Dispatched → Running → Succeeded
- Tool call falhando transiciona para `ToolCallState::Failed`
- Timeout detectado e estado correto
- Eventos emitidos para cada transição
- `calls_for_task` filtra por TaskId
- Tool não encontrada retorna resultado com erro
- Output truncado em 8000 chars (mantém comportamento atual)
- Múltiplas calls para a mesma task
- `get_record` para call inexistente retorna `None`
- Cancelled tool call não executa
- Duração registrada corretamente em `duration_ms`
- Antigo `tool_bridge::execute_tool_call` wraps o novo manager
- Serde roundtrip de `ToolCallRecord`

## Definition of Done

| # | Critério | Verificação |
|---|----------|-------------|
| 1 | Toda execução de tool passa pelo `ToolCallManager` | Code review — nenhuma chamada direta |
| 2 | Invariante 2: call_id único gerado internamente | Teste unitário |
| 3 | Invariante 3: resultado carrega call_id | Teste unitário |
| 4 | Antigo `tool_bridge::execute_tool_call` funciona via manager | Backward compat test |
| 5 | 15+ testes passando | `cargo test -p theo-agent-runtime` |
