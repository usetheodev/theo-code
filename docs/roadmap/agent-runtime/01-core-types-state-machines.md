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
| `src/identifiers.rs` | `TaskId`, `CallId`, `RunId`, `EventId` (newtypes) | ~100 |
| `src/task.rs` | `TaskState`, `Task`, `AgentType`, `Artifact` | ~250 |
| `src/tool_call.rs` | `ToolCallState`, `ToolCallRecord`, `ToolResultRecord` | ~180 |
| `src/agent_run.rs` | `RunState`, `AgentRun` | ~200 |

### Modificados

| Arquivo | Mudança |
|---------|---------|
| `src/lib.rs` | Adicionar `pub mod` para os 4 novos módulos |
| `src/error.rs` | Adicionar `TransitionError` com campos `from: String` e `to: String` |

## Tipos Definidos

### identifiers.rs

```rust
pub struct TaskId(String);    // Display, Serialize, Deserialize, Clone, Eq, Hash
pub struct CallId(String);
pub struct RunId(String);
pub struct EventId(String);
```

Todos com `::new(impl Into<String>)` e `::generate()` (timestamp + random via `std::time` + random bytes).

**Contrato de IDs**:
- `::new()` faz `assert!(!id.is_empty(), "identifier must not be empty")` — panic em debug, invariant violation.
- `::generate()` usa `format!("{:013x}_{:016x}", timestamp_millis, random_u64)` — garantia de unicidade por timestamp + entropia.
- Nenhuma dependência externa (sem `uuid`). Segue padrão de `SessionId` em `session.rs`.
- `SessionId` e `MessageId` permanecem em `session.rs` — os novos IDs ficam em `identifiers.rs`. São domínios distintos (sessão LLM vs lifecycle de agente).

### task.rs — Task State Machine

**Transições válidas explícitas:**

| From | To (válidos) |
|------|-------------|
| Pending | Ready, Cancelled |
| Ready | Running, Cancelled |
| Running | WaitingTool, WaitingInput, Blocked, Completed, Failed, Cancelled |
| WaitingTool | Running, Failed, Cancelled |
| WaitingInput | Running, Failed, Cancelled |
| Blocked | Running, Failed, Cancelled |
| Completed | (nenhuma — terminal) |
| Failed | (nenhuma — terminal) |
| Cancelled | (nenhuma — terminal) |

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

pub enum AgentType { Coder, Reviewer, Planner, Custom(String) }

pub struct Artifact { pub name: String, pub path: String, pub artifact_type: String }

pub struct Task {
    pub task_id: TaskId,
    pub session_id: SessionId,  // Importado de crate::session, NÃO redefinido
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

**Transições válidas explícitas:**

| From | To (válidos) |
|------|-------------|
| Queued | Dispatched, Cancelled |
| Dispatched | Running, Cancelled |
| Running | Succeeded, Failed, Timeout, Cancelled |
| Succeeded | (terminal) |
| Failed | (terminal) |
| Timeout | (terminal) |
| Cancelled | (terminal) |

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

/// Registro histórico de resultado de execução.
/// Distinto de ToolResult<T> em error.rs (type alias para Result<T, ToolError>).
pub struct ToolResultRecord {
    pub call_id: CallId,
    pub output: String,
    pub status: ToolCallState,  // Succeeded | Failed | Timeout apenas
    pub error: Option<String>,
    pub duration_ms: u64,
}
```

### agent_run.rs — Agent Run State Machine

**Transições válidas explícitas:**

| From | To (válidos) |
|------|-------------|
| Initialized | Planning, Aborted |
| Planning | Executing, Aborted |
| Executing | Evaluating, Aborted |
| Evaluating | Converged, Replanning, Waiting, Aborted |
| Replanning | Planning, Aborted |
| Waiting | Planning, Aborted |
| Converged | (terminal) |
| Aborted | (terminal) |

**Trigger de saída de `Waiting`**: evento externo tipado (ex: user input recebido, recurso liberado).
O orquestrador (Fase 05) é responsável por monitorar o trigger e chamar `transition(Waiting, Planning)`.
A state machine apenas valida que a transição é permitida — não monitora triggers.

**Circuit breaker de replanning**: a state machine aceita o ciclo `Evaluating → Replanning → Planning` indefinidamente. O limite de ciclos **NÃO** é responsabilidade da state machine — é responsabilidade do orquestrador via `AgentRun.max_iterations` (enforced na Fase 07 via BudgetEnforcer). Design consciente: tipo expressa transições válidas, orquestrador expressa limites operacionais.

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
pub trait StateMachine: Copy + PartialEq + std::fmt::Debug {
    fn can_transition_to(&self, target: Self) -> bool;
    fn is_terminal(&self) -> bool;
}

/// Transição atômica: muta estado APENAS se válida.
/// Em caso de Err, estado original preservado intacto.
#[must_use]
pub fn transition<S: StateMachine>(
    current: &mut S,
    target: S,
) -> Result<(), TransitionError> {
    if current.can_transition_to(target) {
        *current = target;
        Ok(())
    } else {
        Err(TransitionError::InvalidTransition {
            from: format!("{:?}", current),
            to: format!("{:?}", target),
        })
    }
}
```

### TransitionError (em error.rs)

```rust
#[derive(Debug, Clone, thiserror::Error)]
pub enum TransitionError {
    #[error("invalid transition from {from} to {to}")]
    InvalidTransition { from: String, to: String },
}
```

## Tech Debt Documentado

- `ToolContext.call_id: String` (em tool.rs) deve migrar para `CallId` em fase futura
- `SessionId::new("")` aceita vazio — inconsistente com `TaskId::new("")` (panic)
- `Phase` enum em theo-agent-runtime será `#[deprecated]` na Fase 05

## Testes Requeridos (~100)

### Estratégia: Tabelas de Transição O(N²)

```rust
#[test]
fn task_state_transition_table_exhaustive() {
    let valid = [(Pending, Ready), (Pending, Cancelled), /*...*/];
    let all = [Pending, Ready, Running, WaitingTool, /*...*/];
    for from in &all {
        for to in &all {
            let expected = valid.contains(&(*from, *to));
            assert_eq!(from.can_transition_to(*to), expected,
                "{:?} → {:?}: expected {}", from, to, expected);
        }
    }
}
```

### TaskState (~30) | ToolCallState (~25) | RunState (~25) | IDs (~20) | Props (4)

Detalhamento completo por seção no meeting-minutes.

## Definition of Done

| # | Critério | Verificação |
|---|----------|-------------|
| 1 | `cargo test -p theo-domain` passa com mínimo 100 novos testes | `cargo test` |
| 2 | `can_transition_to` com match arms exaustivos (sem wildcards) | Code review |
| 3 | Serde roundtrip com `assert_eq!` por variante | Testes |
| 4 | `cargo check --workspace` compila limpo | `cargo check` |
| 5 | Zero async, zero IO, zero deps além de serde/thiserror | Auditoria |
| 6 | `transition()` atômico: estado inalterado em `Err` | Propriedade P4 |
| 7 | Tabelas O(N²) para cada state machine | 3 testes |
| 8 | IDs vazios rejeitados com panic | `#[should_panic]` |
| 9 | `SessionId` de `crate::session`, não redefinido | Code review |
| 10 | `TransitionError` com `from`/`to` | Teste |
