# Fase 10 — Persistence & Resume

## Objetivo

Implementar persistência de snapshots para que agent runs possam ser resumidos
de um estado consistente (Invariante 7).

## Invariante Endereçado

- **Invariante 7**: Todo `resume` deve partir de snapshot consistente

## Dependências

- Fases 01-07 (todas as state machines, eventos, budget)

## Arquivos

### Novos

| Arquivo | Crate | Conteúdo | Linhas Est. |
|---------|-------|----------|-------------|
| `src/snapshot.rs` | theo-agent-runtime | `RunSnapshot` | ~200 |
| `src/persistence.rs` | theo-agent-runtime | `SnapshotStore` trait, `FileSnapshotStore` | ~150 |

### Modificados

| Arquivo | Mudança |
|---------|---------|
| `src/run_engine.rs` | Snapshot após cada iteração; método `resume()` |
| `Cargo.toml` | Adicionar `sha2` para checksum |
| `src/lib.rs` | Adicionar `pub mod snapshot, persistence` |

## Tipos Definidos

### snapshot.rs

```rust
pub struct RunSnapshot {
    pub run: AgentRun,
    pub task: Task,
    pub tool_calls: Vec<ToolCallRecord>,
    pub tool_results: Vec<ToolResultRecord>,
    pub events: Vec<DomainEvent>,
    pub budget_usage: BudgetUsage,
    pub messages: Vec<serde_json::Value>,  // LLM conversation history
    pub dlq: Vec<DeadLetter>,
    pub snapshot_at: u64,
    pub checksum: String,  // SHA256
}

impl RunSnapshot {
    pub fn compute_checksum(&self) -> String;
    pub fn validate_checksum(&self) -> bool;
}
```

### persistence.rs

```rust
#[async_trait]
pub trait SnapshotStore: Send + Sync {
    async fn save(&self, run_id: &RunId, snapshot: &RunSnapshot) -> Result<(), PersistenceError>;
    async fn load(&self, run_id: &RunId) -> Result<Option<RunSnapshot>, PersistenceError>;
    async fn list_runs(&self) -> Result<Vec<RunId>, PersistenceError>;
    async fn delete(&self, run_id: &RunId) -> Result<(), PersistenceError>;
}

pub struct FileSnapshotStore {
    base_dir: PathBuf,  // ~/.theo/snapshots/
}

impl SnapshotStore for FileSnapshotStore { ... }

#[derive(Debug, thiserror::Error)]
pub enum PersistenceError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("snapshot not found for run {0}")]
    NotFound(String),
}
```

### Resume no RunEngine

```rust
impl AgentRunEngine {
    pub async fn resume(
        snapshot: RunSnapshot,
        client: LlmClient,
        registry: ToolRegistry,
        config: AgentConfig,
        event_bus: Arc<EventBus>,
    ) -> Result<AgentResult, PersistenceError>;
}
```

## Fluxo de Resume

1. Carregar snapshot do store
2. Validar checksum (Invariante 7: consistência)
3. Reidratar estado (run, task, tool calls, events, messages)
4. Continuar execução do ponto onde parou
5. Manter `run_id` original

## Testes Requeridos (~15)

- Save + load roundtrip produz snapshot idêntico
- Checksum mismatch detectado em corrupção
- Resume de snapshot continua na iteração correta
- Eventos antes do snapshot preservados
- `FileSnapshotStore` cria/lê arquivos nos paths corretos (tempdir)
- `list_runs` retorna todos os runs salvos
- `delete` remove o snapshot
- Load de run inexistente retorna `None`
- Snapshot com DLQ entries preserva DLQ
- Budget usage preservado no resume
- Messages (conversation history) preservados
- Checksum computado deterministicamente
- Resume com task em estado Running continua execução
- Resume com task em estado WaitingTool re-executa tool pendente
- PersistenceError serde roundtrip

## Definition of Done

| # | Critério | Verificação |
|---|----------|-------------|
| 1 | RunEngine salva snapshot após cada iteração | Code review |
| 2 | `resume` restaura todo o estado e continua (Invariante 7) | Teste de integração |
| 3 | Checksum validation na carga (detecta corrupção) | Teste unitário |
| 4 | `FileSnapshotStore` testado com tempdir | Teste de integração |
| 5 | 15+ testes passando | `cargo test` |
