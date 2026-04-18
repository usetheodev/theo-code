# 04 — Agent Runtime (`theo-agent-runtime`)

The brain of the system. Orchestrates LLM calls, tool execution, state machines, context management, convergence detection, sensors, evolution loop, sub-agents, and the autonomous pilot loop. **~40 modules** (36 top-level `.rs` files + `skill/`, `subagent/`, `bin/` subdirectories), the largest crate.

Architecturally, this crate is Theo's **behavioral harness**: it converts a stateless LLM into an agent that can recover state, make bounded progress, verify changes, and hand off cleanly across context windows.

Dependencies: `theo-domain`, `theo-tooling`, `theo-infra-llm`, `tokio`, `serde`, `futures`, `toml`.

## Module Organization

```
theo-agent-runtime/src/
│
├── Core Loop
│   ├── run_engine.rs        # AgentRunEngine — the main execution loop
│   ├── agent_loop.rs        # AgentLoop — thin facade over RunEngine
│   ├── agent_message.rs     # AgentMessage enum (LLM, compaction, branch)
│   ├── convergence.rs       # ConvergenceEvaluator (git diff + edit success)
│   └── loop_state.rs        # ContextLoopState — phase tracking (Explore→Edit→Verify→Done)
│
├── Budget & Enforcement
│   ├── budget_enforcer.rs   # Per-run limits: tokens, iterations, tool calls, time
│   ├── capability_gate.rs   # Per-tool and per-path access control
│   └── failure_tracker.rs   # Persisted failure patterns → corrective suggestions
│
├── Context Management
│   ├── compaction.rs        # Heuristic compaction (80% threshold, preserve tail=6)
│   ├── context_metrics.rs   # Per-run context usage statistics
│   └── session_bootstrap.rs # Cross-session progress and boot messages
│
├── Self-Improvement
│   ├── evolution.rs         # EvolutionLoop — structured retry with reflection (max 5)
│   ├── reflector.rs         # HeuristicReflector — failure classification
│   ├── correction.rs        # CorrectionEngine — strategy selection (dead code)
│   └── sensor.rs            # SensorRunner — computational verification after writes
│
├── State Persistence
│   ├── state_manager.rs     # StateManager — file-backed crash recovery
│   ├── session_tree.rs      # SessionTree — append-only JSONL DAG
│   ├── persistence.rs       # SnapshotStore trait + FileSnapshotStore
│   └── snapshot.rs          # RunSnapshot — full run checkpoint with checksum
│
├── Tool Execution
│   ├── tool_bridge.rs       # Tool registry → LLM definitions + tool dispatch
│   ├── tool_call_manager.rs # ToolCallState machine + capability gating
│   └── hooks.rs             # Shell hook system (pre/post/sensor)
│
├── Autonomous Loop
│   ├── pilot.rs             # PilotLoop — autonomous dev loop with circuit breaker
│   └── roadmap.rs           # Markdown roadmap parser + task marking
│
├── Delegation
│   └── subagent/mod.rs      # SubAgentManager — spawn capability-gated sub-agents
│
├── Extensibility
│   ├── skill/mod.rs         # SkillRegistry — data-driven skills from markdown
│   ├── skill/bundled.rs     # Built-in skills (commit, test, review, build, explain)
│   ├── plugin.rs            # TOML-manifested shell-script plugins
│   ├── extension.rs         # Extension trait — middleware hooks
│   └── project_config.rs    # .theo/config.toml + THEO_* env overrides
│
├── Event System
│   └── event_bus.rs         # In-process pub/sub + tokio::broadcast
│
├── Observability
│   ├── metrics.rs           # RuntimeMetrics — token cost, tool success, LLM timing
│   ├── dlq.rs               # Dead-letter queue for permanently failed calls
│   └── observability.rs     # StructuredLogListener (dead code)
│
├── Configuration
│   ├── config.rs            # AgentConfig — 18+ fields, AgentMode, MessageQueues
│   └── retry.rs             # RetryExecutor — exponential backoff wrapper
│
└── Scheduling (dead code)
    └── scheduler.rs         # Priority-queue task scheduler
```

## AgentRunEngine (run_engine.rs)

The core execution loop. ~2400 lines. Single `execute_with_history()` method that runs the full agent cycle.

### Harness Responsibilities

The runtime exists to enforce four session-level behaviors:

1. **Rehydrate state** from durable artifacts such as session history, plans, wiki memory, graph context, and prior progress.
2. **Favor incremental progress** over one-shot task completion.
3. **Close the loop with verification** using done gates, tests, sensors, and convergence checks.
4. **Leave durable breadcrumbs** so the next session can resume without reconstructing intent from code diff alone.

### Struct Fields

| Field | Type | Purpose |
|---|---|---|
| `run` | `AgentRun` | Run state machine + iteration counter |
| `task_id` | `TaskId` | Current task being executed |
| `task_manager` | `Arc<TaskManager>` | Task lifecycle management |
| `tool_call_manager` | `Arc<ToolCallManager>` | Tool call state machine |
| `event_bus` | `Arc<EventBus>` | Domain event publishing |
| `client` | `LlmClient` | LLM HTTP client |
| `registry` | `Arc<ToolRegistry>` | Available tools |
| `config` | `AgentConfig` | Runtime configuration |
| `budget_enforcer` | `BudgetEnforcer` | Budget limits |
| `convergence` | `ConvergenceEvaluator` | Done verification |
| `failure_tracker` | `FailurePatternTracker` | Failure pattern detection |
| `context_loop_state` | `ContextLoopState` | Phase tracking |
| `working_set` | `WorkingSet` | Hot files/events/hypotheses |
| `context_metrics` | `ContextMetrics` | Context usage statistics |
| `message_queues` | `MessageQueues` | Steering + follow-up injection |
| `graph_context` | `Option<Arc<dyn GraphContextProvider>>` | GRAPHCTX |

### System Prompt Assembly

Messages assembled in order before the main loop:

1. **System prompt** — from `.theo/system-prompt.md` or config default
2. **Project context** — from `.theo/theo.md`
3. **Cross-run memories** — from `~/.config/theo/memory/`
4. **Episode summaries** — last 5 episodes from `.theo/wiki/episodes/`
5. **Session boot context** — previous session progress + git log
6. **GRAPHCTX planning injection** — top-5 relevant files if graph is Ready
7. **Skills** — available skill triggers and descriptions
8. **Session history** — prior REPL messages
9. **Task objective** — current user request

The ordering matters. Stable repository knowledge and prior-session artifacts should dominate ephemeral prompt steering.

### Session Protocol

Theo's runtime should behave like a fresh worker joining an ongoing engineering shift (analogy from `docs/pesquisas/effective-harnesses-for-long-running-agents.md`). Every run follows the same five-step protocol:

1. **Rehydrate** — recover state from `session.jsonl`, plans, wiki memory, git history, episode summaries, and prior progress notes.
2. **Verify baseline** — before any new edit, confirm the workspace is not already broken (build passes, tests run). A session that starts with broken state and then edits without verifying makes the problem worse.
3. **Pick one bounded unit** — choose the highest-priority incomplete work item. Never try to one-shot a whole feature list.
4. **Execute with verification** — every mutation is followed by computational sensors (edit verify hooks, `cargo check`, `cargo test`) and, at done, the convergence evaluator.
5. **Leave clean** — persist snapshot, episode summary, progress notes, and commit. The next session must be able to resume without archaeology.

### Session Bootstrap Checklist (first turns of every run)

From `effective-harnesses-for-long-running-agents.md`, adapted to Theo's artifacts:

```
1. Read .theo/state/{latest}/session.jsonl header  → where did we stop?
2. Read last N episode summaries                   → what did we learn?
3. Read active plans in .theo/plans/ or docs/plans → what was intended?
4. git log --oneline -20                           → what shipped recently?
5. Read .theo/theo.md + docs/architecture/         → project map
6. GRAPHCTX query_context(task, budget)            → structural context
7. cargo check / cargo test (fast subset)          → baseline health
8. Now — pick the next bounded work item
```

This checklist is encoded in **System Prompt Assembly** (steps 4–6) and the runtime's pre-loop initialization (steps 1–3, 7). Steps 8 becomes the model's first autonomous decision.

### Done Gate (Multi-Layer Verification)

When the agent calls `done`:

| Gate | Check | On Failure |
|---|---|---|
| Gate 0 | `done_attempts > 3` → force accept | Accept with warning |
| Gate 1 | Convergence criteria (git diff + edit success) | BLOCKED, replanning |
| Gate 2 | `cargo test -p <crate>` (60s timeout, fallback `cargo check`) | BLOCKED with errors |

### Meta-Tools

5 meta-tools handled directly by RunEngine (not in ToolRegistry):

| Tool | Purpose |
|---|---|
| `done` | Signal task completion (triggers done gate) |
| `subagent` | Delegate to a sub-agent (Explorer/Implementer/Verifier/Reviewer) |
| `subagent_parallel` | Run multiple sub-agents concurrently via `JoinSet` |
| `skill` | Invoke a packaged skill (InContext or SubAgent mode) |
| `batch` | Execute up to 25 tool calls in parallel (meta-tools blocked) |

## PilotLoop (pilot.rs)

Autonomous development loop that runs until a "promise" is fulfilled.

### Exit Conditions

| Condition | Trigger |
|---|---|
| `PromiseFulfilled` | N consecutive completion signals (default: 2) with real progress |
| `FixPlanComplete` | All checkboxes in `.theo/fix_plan.md` checked |
| `RateLimitExhausted` | >100 loops/hour |
| `CircuitBreakerOpen` | 3 consecutive no-progress OR 5 consecutive same-error |
| `MaxCallsReached` | Total loops > 50 (configurable) |
| `UserInterrupt` | Ctrl+C |

### Circuit Breaker State Machine

```
Closed ──(no progress × 3)──→ Open ──(cooldown 300s)──→ HalfOpen
                                 ↑                          │
                                 └──(failure in HalfOpen)───┘
HalfOpen ──(progress)──→ Closed
```

### Evolution Loop Integration

After each loop iteration:
1. `evolution.record_attempt(strategy, outcome, files, error, duration, tokens)`
2. If failure: `evolution.reflect()` → generates `Reflection` → injected as system message
3. Evolution context (attempt history + reflections) included in next loop prompt

> **Not the same as the Karpathy ratchet.** The runtime's `EvolutionLoop` (`evolution.rs`) is an **in-run, attempt-level** reflection mechanism: it watches *this task's* failures and feeds reflections back to *this task's* next iteration. The Karpathy ratchet in `apps/theo-benchmark/runner/evolve.py` is an **offline, prompt-mutation** harness: it mutates the system prompt itself across benchmark runs and keeps mutations that raise smoke-suite scores. The two are complementary (one improves a run, the other improves the harness) but operate at different scopes and must not be conflated.

## Sensor System (sensor.rs)

Computational verification after write tools:

```
Write tool succeeds
    │
    ▼
SensorRunner::fire(tool_name, file_path, project_dir)
    │ (tokio::spawn — async, non-blocking)
    ▼
.theo/hooks/edit.verify.sh receives JSON stdin
    │
    ▼
SensorResult { tool_name, file_path, output, exit_code, duration_ms }
    │ (accumulated in pending queue)
    ▼
drain_pending() at top of next iteration
    │
    ▼
Injected as system message: "[SENSOR OK/ISSUE] file (via tool): output"
    │
    ▼
SensorExecuted DomainEvent published
```

In harness-engineering terms, sensors are Theo's primary **feedback controls**. They should tell the agent what actually happened after mutation, not just whether a command exited `0`.

## Compaction (compaction.rs)

Heuristic context-window management:

- **Threshold**: 80% of `context_window_tokens`
- **Preserve tail**: Last 6 messages kept verbatim
- **Old tool results**: Truncated to 200 chars
- **Summary**: Includes task objective, current phase, target files, recent errors
- **Emergency mode**: On context overflow error, compact to 50% and retry

Compaction is a **continuity mechanism**, not just a token-saving trick. The Anthropic research is explicit: *"compaction isn't sufficient"* on its own — it doesn't always pass perfectly clear instructions to the next agent. Theo therefore combines three layers:

1. **Compaction** (in-window) keeps the current LLM call alive.
2. **Snapshot + session-tree** (across-window) allows exact resumption within a run.
3. **Progress artifacts** (across-run) — episode summaries, wiki learnings, `.theo/plans/` — give the *next* process a map.

A compaction summary that loses the current objective or the next action is a bug, not a space-saving win. The summary is the handoff message to the next context window.

## Feature List Artifact (roadmap)

The Anthropic research identifies a specific failure mode: agents declare victory on the whole project after a few features ship. Their mitigation is a JSON `feature_list` with `"passes": false` entries that only flip when verified end-to-end, and strongly-worded instructions that *"it is unacceptable to remove or edit tests because this could lead to missing or buggy functionality."*

Theo's analogous artifacts today are `.theo/plans/` and the roadmap parser (`roadmap.rs`), which the `PilotLoop` uses as its exit condition. These are **prose-based** and therefore read by the model as narrative rather than enforced as structured state. The known gap is a tipified features file.

### Target schema (roadmap — not yet implemented)

```jsonc
// .theo/features.json
{
  "schema": "theo.features.v1",
  "features": [
    {
      "id": "auth-login-happy-path",
      "category": "functional",             // functional | nonfunctional | migration
      "description": "User logs in with email+password and lands on /app",
      "steps": [
        "Navigate to /login",
        "Fill credentials and submit",
        "Expect 302 → /app and session cookie set"
      ],
      "verified_by": ["tests/auth/login.rs", "e2e/login.spec.ts"],
      "passes": false,                      // flipped only after end-to-end verification
      "blocked_by": []
    }
  ]
}
```

### Integration points

When implemented, the features file will be wired into three places:

1. **Done-gate (Gate 3)** — `cargo test` passing is necessary but no longer sufficient. At least one `features.json` entry's `passes` must have flipped from `false` to `true` during the run (or a justified `blocked_by` must be documented).
2. **PilotLoop exit condition** — `FixPlanComplete` is replaced by `FeatureListComplete` when all `passes == true`.
3. **Sub-agent routing** — the `Verifier` role gets read+write on `features.json`; other roles are read-only. JSON format chosen deliberately: the Anthropic research notes *"the model is less likely to inappropriately change or overwrite JSON files compared to Markdown files."*

This artifact, together with the behaviour sensors in `10-application-legibility.md`, is how Theo intends to close the Behaviour harness gap.

## Sub-Agents (subagent/)

| Role | Max Iterations | Capabilities |
|---|---|---|
| `Explorer` | 15 | Read-only: read, grep, glob, codebase_context |
| `Implementer` | 30 | Full: read, grep, glob, edit, write, bash |
| `Verifier` | 10 | Test-focused: read, grep, bash, glob |
| `Reviewer` | 10 | Read + analyze: read, grep, glob |

Constraints: `MAX_DEPTH = 1` (no recursive spawning). Sub-agents get filtered tool schemas (never see `subagent`, `skill`, `subagent_parallel`). Fresh context per invocation.

The Anthropic research leaves open whether specialized sub-agents outperform a single general-purpose coding agent for long-running work (*"It's still unclear whether a single, general-purpose coding agent performs best across contexts, or if better performance can be achieved through a multi-agent architecture"* — `effective-harnesses-for-long-running-agents.md` §Future work).

Theo's **current stance — not yet empirically validated**: specialized roles are useful for **variety reduction** (Ashby's Law — a regulator needs at least as much variety as the system it regulates; narrowing sub-agent scope makes guides and sensors tractable). The primary coding agent stays general; sub-agents get a capability-gated slice of the harness.

Open benchmark work (`apps/theo-benchmark`) is expected to answer this empirically. Until then, the docs treat the sub-agent bet as a hypothesis, not a conclusion.

## Session Persistence (state_manager.rs + session_tree.rs)

```
StateManager
    │ wraps
    ▼
SessionTree (append-only JSONL DAG)
    │ persists to
    ▼
.theo/state/{run_id}/session.jsonl

Entry types:
  Header { version, cwd, created_at }
  Message { role, content, parent_id }
  Compaction { summary, replaced_ids }
  ModelChange { from, to }
  BranchSummary { summary, from_id }
```

`build_context()` reconstructs root-to-leaf message path through the DAG, selecting the longest branch after any compaction or branching.

This append-only session tree is one of the runtime's key anti-amnesia mechanisms for long-running work.

## Configuration (config.rs)

```rust
pub struct AgentConfig {
    pub max_iterations: usize,         // default: 200
    pub model: String,                 // default: "gpt-4o"
    pub system_prompt: String,
    pub max_tokens: u32,               // default: 16384
    pub temperature: f32,              // default: 0.0
    pub context_loop_interval: usize,  // default: 5
    pub context_window_tokens: usize,  // default: 128000
    pub is_subagent: bool,
    pub mode: AgentMode,               // Agent | Plan | Ask
    pub doom_loop_threshold: Option<usize>,  // default: 3
    pub aggressive_retry: bool,        // for benchmark mode
    pub tool_execution_mode: ToolExecutionMode,
    // ... more fields
}
```

`AgentMode::Plan` restricts write tools to `.theo/plans/` only and blocks `think`.
