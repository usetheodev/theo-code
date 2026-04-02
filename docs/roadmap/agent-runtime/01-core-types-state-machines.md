# Fase 01 — Core Types & State Machines

## Objetivo

Introduzir as 3 state machines e contratos de dados como tipos puros em `theo-domain`.
Zero runtime logic, zero async, zero IO.

## Invariantes Endereçados

- **Invariante 1**: Toda Task possui `task_id`, `session_id`, `state`, `created_at`
- **Invariante 4**: Nenhuma Task pode voltar de `completed` para `running`

## Arquivos

### Novos (theo-domain)

| Arquivo | Conteúdo | Linhas Est. |
|---------|----------|-------------|
| `src/identifiers.rs` | `TaskId`, `CallId`, `RunId`, `EventId` (newtypes) | ~80 |
| `src/task.rs` | `TaskState`, `Task`, `AgentType`, `Artifact` | ~200 |
| `src/tool_call.rs` | `ToolCallState`, `ToolCallRecord`, `ToolResult` | ~150 |
| `src/agent_run.rs` | `RunState`, `AgentRun` | ~170 |

### Modificados

| Arquivo | Mudança |
|---------|---------|
| `src/lib.rs` | Adicionar `pub mod` para os 4 novos módulos |
| `src/error.rs` | Adicionar `TransitionError` |

## Tipos Definidos

### identifiers.rs

```rust
pub struct TaskId(String);    // Display, Serialize, Deserialize, Clone, Eq, Hash
pub struct CallId(String);
pub struct RunId(String);
pub struct EventId(String);
```

Todos com `::new(impl Into<String>)` e `::generate()` (timestamp + random).

### task.rs — Task State Machine

```
pending → ready → running →
    ├── waiting_tool
    ├── waiting_input
    ├── blocked
    ↓
completed | failed | cancelled
```

```rust
pub enum TaskState {
    Pending, Ready, Running,
    WaitingTool, WaitingInput, Blocked,
    Completed, Failed, Cancelled,
}

impl TaskState {
    pub fn can_transition_to(&self, target: TaskState) -> bool;
    pub fn is_terminal(&self) -> bool;
}

pub struct Task {
    pub task_id: TaskId,
    pub session_id: SessionId,
    pub state: TaskState,
    pub agent_type: AgentType,
    pub objective: String,
    pub artifacts: Vec<Artifact>,
    pub created_at: u64,
    pub updated_at: u64,
    pub completed_at: Option<u64>,
}
```

### tool_call.rs — Tool Call State Machine

```
queued → dispatched → running →
    ├── succeeded
    ├── failed
    ├── timeout
    └── cancelled
```

```rust
pub enum ToolCallState {
    Queued, Dispatched, Running,
    Succeeded, Failed, Timeout, Cancelled,
}

pub struct ToolCallRecord {
    pub call_id: CallId,
    pub task_id: TaskId,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub state: ToolCallState,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub completed_at: Option<u64>,
}

pub struct ToolResultRecord {
    pub call_id: CallId,
    pub output: String,
    pub status: ToolCallState,
    pub error: Option<String>,
    pub duration_ms: u64,
}
```

### agent_run.rs — Agent Run State Machine

```
initialized → planning → executing → evaluating →
    ├── converged
    ├── replanning
    ├── waiting
    └── aborted
```

```rust
pub enum RunState {
    Initialized, Planning, Executing, Evaluating,
    Converged, Replanning, Waiting, Aborted,
}

pub struct AgentRun {
    pub run_id: RunId,
    pub task_id: TaskId,
    pub state: RunState,
    pub iteration: usize,
    pub max_iterations: usize,
    pub created_at: u64,
    pub updated_at: u64,
}
```

### Transition Validation

```rust
pub fn transition<S: StateMachine>(current: &mut S, target: S) -> Result<(), TransitionError>
```

Cada state machine implementa `can_transition_to` com match arms explícitos (sem wildcards).

## Testes Requeridos (~45)

### Por State Machine (x3, ~15 cada)
- Todas as transições válidas retornam `Ok`
- Todas as transições inválidas retornam `Err(TransitionError)`
- Estados terminais rejeitam todas as transições
- Serde JSON roundtrip para cada variante
- `Display` para cada variante

### Invariante 4 (dedicado)
- `TaskState::Completed.can_transition_to(TaskState::Running)` == `false`

### Identificadores
- `TaskId::generate()` produz IDs únicos (1000 gerações sem colisão)
- `Display` e `Serialize` roundtrip

## Dependências

Nenhuma — esta é a fundação.

## Definition of Done

| # | Critério | Verificação |
|---|----------|-------------|
| 1 | `cargo test -p theo-domain` passa com todos os novos testes (mínimo 45) | `cargo test -p theo-domain` |
| 2 | Todos os `can_transition_to` têm match arms exaustivos (sem wildcards) | Code review |
| 3 | Cada struct deriva `Serialize, Deserialize` e passa roundtrip test | Testes unitários |
| 4 | `cargo check --workspace` compila limpo | `cargo check --workspace` |
| 5 | Zero `async`, zero IO, zero dependência externa além de serde/thiserror | Auditoria de imports |
