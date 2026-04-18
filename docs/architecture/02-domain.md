# 02 — Domain Core (`theo-domain`)

Pure types, traits, and error enums. The gravitational center of the system — every other crate imports from here. Contains zero internal crate dependencies (external: `serde`, `serde_json`, `thiserror`, `async-trait`, `tokio`).

## Module Map

```
theo-domain/src/
├── lib.rs              # StateMachine trait + transition() free fn
├── agent_run.rs        # RunState state machine + AgentRun aggregate
├── budget.rs           # Budget limits + cost tracking
├── capability.rs       # CapabilitySet (tool/path gating)
├── code_intel.rs       # CodeIntelProvider trait (DIP)
├── episode.rs          # EpisodeSummary + Hypothesis tracking
├── error.rs            # OpenCodeError, ToolError, TransitionError
├── event.rs            # DomainEvent + EventType (21 variants)
├── evolution.rs        # AttemptRecord, Reflection types
├── graph_context.rs    # GraphContextProvider trait (DIP)
├── identifiers.rs      # TaskId, CallId, RunId, EventId newtypes
├── permission.rs       # PermissionRule evaluation
├── priority.rs         # Priority enum (Low..Critical)
├── retry_policy.rs     # RetryPolicy + CorrectionStrategy
├── sandbox.rs          # SandboxConfig, policies, violations
├── session.rs          # SessionId, MessageId newtypes
├── task.rs             # TaskState state machine + Task aggregate
├── tokens.rs           # estimate_tokens() heuristic
├── tool.rs             # Tool trait + ToolContext + ToolSchema
├── tool_call.rs        # ToolCallState state machine + records
├── truncate.rs         # Output truncation (2000 lines / 50KB)
├── wiki_backend.rs     # WikiBackend trait (DIP)
└── working_set.rs      # WorkingSet — agent's hot context scope
```

## Core Traits

### `StateMachine` (lib.rs)
Generic contract for all finite state machines. Implemented by `RunState`, `TaskState`, `ToolCallState`.

```rust
pub trait StateMachine: Copy + PartialEq {
    fn can_transition_to(&self, target: Self) -> bool;
    fn is_terminal(&self) -> bool;
}

pub fn transition<S: StateMachine>(current: &mut S, target: S) -> Result<(), TransitionError>;
```

### `Tool` (tool.rs)
The contract every tool must satisfy. Async trait with DIP — implementations live in `theo-tooling`.

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn id(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> ToolSchema { ToolSchema::new() }
    fn category(&self) -> ToolCategory { ToolCategory::Utility }
    async fn execute(&self, args: Value, ctx: &ToolContext, perms: &PermissionCollector) -> ToolResult<ToolOutput>;
    fn prepare_arguments(&self, args: Value) -> Value { args }
}
```

### `GraphContextProvider` (graph_context.rs)
Async trait for code intelligence. Concrete implementation: `GraphContextService` in `theo-application`.

```rust
#[async_trait]
pub trait GraphContextProvider: Send + Sync {
    async fn initialize(&self, project_dir: &Path) -> Result<(), GraphContextError>;
    async fn query_context(&self, query: &str, budget_tokens: usize) -> Result<GraphContextResult, GraphContextError>;
    fn is_ready(&self) -> bool;
}
```

### `WikiBackend` (wiki_backend.rs)
Async trait for wiki operations. Concrete implementation in `theo-application`.

### `CodeIntelProvider` (code_intel.rs)
Strategy pattern for symbol resolution. Two planned implementations: SCIP (exact) and Tree-Sitter (approximate).

## State Machines

### RunState (agent_run.rs)
```
Initialized → Planning → Executing → Evaluating ─→ Converged (terminal)
                  ↑          │            │
                  │          │            ├─→ Aborted (terminal)
                  │          │            │
                  └──────────┴── Replanning
                  └──────────┴── Waiting
```

### TaskState (task.rs)
```
Pending → Ready → Running → Completed (terminal)
                    │    → Failed (terminal)
                    │    → Cancelled (terminal)
                    ├──→ WaitingTool → Running
                    ├──→ WaitingInput → Running
                    └──→ Blocked → Ready
```

### ToolCallState (tool_call.rs)
```
Queued → Dispatched → Running → Succeeded (terminal)
                        │    → Failed (terminal)
                        │    → Timeout (terminal)
     (any non-terminal) → Cancelled (terminal)
```

## Event System (event.rs)

21 event types grouped by concern:

| Group | Events |
|---|---|
| Task lifecycle | `TaskCreated`, `TaskStateChanged` |
| Tool lifecycle | `ToolCallQueued`, `ToolCallDispatched`, `ToolCallCompleted`, `ToolCallProgress` |
| Run lifecycle | `RunInitialized`, `RunStateChanged` |
| Operational | `LlmCallStart`, `LlmCallEnd`, `BudgetExceeded`, `Error` |
| Streaming | `ReasoningDelta`, `ContentDelta` |
| Context | `ContextOverflowRecovery`, `TodoUpdated` |
| Cognitive | `HypothesisFormed`, `HypothesisInvalidated`, `DecisionMade`, `ConstraintLearned` |
| Sensors | `SensorExecuted` |

Cognitive events have **payload validation contracts** — e.g. `HypothesisFormed` must carry `hypothesis` + `rationale` fields. Enforced by `validate_cognitive_event()`.

`DomainEvent` supports **causal tracking** via `supersedes_event_id: Option<EventId>`.

## Budget & Cost (budget.rs)

```rust
pub struct Budget {
    pub max_time_secs: u64,        // default: 3600 (1h)
    pub max_tokens: u64,           // default: 1_000_000
    pub max_iterations: usize,     // default: 200
    pub max_tool_calls: usize,     // default: 500
}
```

Built-in pricing table for `gpt-4o`, `gpt-4.1`, `claude-sonnet-4`, `claude-opus` with `CostBreakdown::calculate()` supporting cache read/write.

## Sandbox Policy (sandbox.rs)

Fail-closed by default. Four sub-policies:

| Policy | Controls | Default |
|---|---|---|
| `FilesystemPolicy` | Read/write path allow/deny (glob) | Project dir only |
| `NetworkPolicy` | Network access, DNS, domain allow/deny | Network disabled |
| `ProcessPolicy` | Max processes (64), memory (512MB), CPU (120s), file size (100MB) | Restrictive |
| `AuditPolicy` | Log commands, violations, network | All enabled |

Hardcoded deny lists: `/etc/shadow`, `~/.ssh/id_*`, `~/.aws/credentials`, etc.

## Episode & Hypothesis System (episode.rs)

`EpisodeSummary` compacts a window of `DomainEvent`s into a machine-readable record. Generated deterministically from events (no LLM needed). Contains:
- `MachineEpisodeSummary`: objective, key_actions, outcome, successful_steps, failed_attempts, learned_constraints, files_touched
- `Hypothesis` tracking: active/stale/superseded with confidence scores
- Three-tier memory lifecycle: `Active` → `Cooling` (usefulness-gated, threshold 0.3) → `Archived`
- TTL policies: `RunScoped`, `TimeScoped { seconds }`, `Permanent`

## Token Estimation (tokens.rs)

Single source of truth: `estimate_tokens(text) = max(chars/4, words * 1.3)`. All token counting in the system flows through this heuristic.
