# theo-agent-runtime

The **Agent Runtime** bounded context of Theo Code: orchestrates the LLM
loop, tool dispatch, sub-agent delegation, memory lifecycle, and
observability for one or more concurrent agent sessions.

> **Warning:** This crate has been deeply audited. The remediation plan
> for the open issues lives at
> [`docs/plans/agent-runtime-remediation-plan.md`](../../docs/plans/agent-runtime-remediation-plan.md)
> and the kanban board at
> [`docs/kanban/agent-runtime-remediation-board.md`](../../docs/kanban/agent-runtime-remediation-board.md).

---

## Architecture

```
AgentLoop (public facade)
  └── AgentRunEngine (state machine: Init → Plan → Execute → Converged|Failed)
        ├── ToolBridge → theo-tooling (21 tools + sandbox bwrap/landlock)
        ├── SubAgentManager → spawn/resume of delegated sub-agents
        ├── Compaction → 80% context-window trigger
        ├── MemoryLifecycle → memory hooks (prefetch, sync, compress)
        ├── HandoffGuardrail → 3-tier validation before spawn
        ├── CheckpointManager → shadow git repos for rollback
        ├── SessionTree → JSONL conversation DAG (crash recovery)
        ├── EventBus → synchronous dispatch to listeners
        ├── BudgetEnforcer → token / iteration / time caps
        └── CancellationTree → hierarchical CancellationToken
```

### 6 sub-domains

| Sub-domain | Components |
|---|---|
| **Run Engine** | `AgentLoop`, `AgentRunEngine` |
| **Sub-agent** | `SubAgentManager`, `SubAgentResumer`, `FileSubagentRunStore` |
| **Memory** | `MemoryLifecycle`, `StateManager`, `SessionTree` |
| **Compaction** | `Compaction`, `tool_pair_integrity` (formerly `sanitizer.rs`) |
| **Safety / Guard** | `HandoffGuardrail`, `BudgetEnforcer`, `DoomLoopTracker`, `CancellationTree`, `EvolutionLoop` |
| **Observability** | `ObservabilityPipeline`, `EventBus` |

### Allowed dependencies (ADR-016 + ADR-021 + ADR-022)

- `theo-domain` (always)
- `theo-governance`
- `theo-infra-llm`
- `theo-infra-auth`
- `theo-tooling`
- `theo-isolation` *(ADR-021)*
- `theo-infra-mcp` *(ADR-022)*

The gate at [`scripts/check-arch-contract.sh`](../../scripts/check-arch-contract.sh)
enforces this — its regex was fixed in T0.1 to recognise the
`.workspace = true` syntax.

---

## System Invariants

The runtime guarantees the following invariants. Each invariant has a
listed validation method; CI must keep them green at all times.

| ID | Name | Validation |
|----|------|------------|
| **INV-001** | Tool pair integrity after compaction | `cargo test --test compaction_sanitizer_integration` |
| **INV-002** | State manager append errors are observable | `cargo test --test state_manager_failure` (T1.3 / T3.8) |
| **INV-003** | `CapabilityGate` fires on every dispatch when configured | `cargo test plugin_tool_blocked_by_capability_gate_read_only` |
| **INV-004** | Sub-agent depth bounded at `MAX_DEPTH = 1` | `cargo test --test subagent_characterization` |
| **INV-005** | Arch gate rejects all unauthorized workspace deps | `bash scripts/check-arch-contract.test.sh` |
| **INV-006** | `tool_pair_integrity.rs` handles only structural pair correctness — secret/PII scrubbing lives in `secret_scrubber.rs` | grep + module docstring |
| **INV-007** | OTel feature path has CI coverage | `.github/workflows/audit.yml` step `cargo test --features otel --test otlp_network_smoke` |
| **INV-008** | User cancellation propagates to in-flight tools (≤ 500 ms) | `cargo test --test cancellation_e2e` |

---

## How to Run Tests

```bash
# All tests (default features)
cargo test -p theo-agent-runtime

# OTel feature path (validates observability export)
cargo test -p theo-agent-runtime --features otel --test otlp_network_smoke

# A specific integration test
cargo test -p theo-agent-runtime --test resume_e2e

# Benchmarks (criterion)
cargo bench -p theo-agent-runtime
```

---

## Common Pitfalls

### 1. **Never silently discard `Result`s from state-changing operations**

The pattern `let _ = sm.append_message(...)` was banned in T1.3. Errors
must be propagated via `tracing::error!` + `EventBus::publish(Error,
...)` so crash-recovery is observable. See ADR `D6` in the remediation
plan.

### 2. **Never use `eprintln!` in production paths**

Use `tracing::warn!` / `error!` / `debug!` instead. The migration was
completed in T3.7. CI greps for new `eprintln!` introductions.

### 3. **Tool result content must always be fenced**

Every `Message::tool_result(...)` constructed in the runtime must be
the output of `fence_untrusted(content, source_label, MAX_BYTES)`. The
helper is in `theo_domain::prompt_sanitizer`. See T2.1, T2.2, T2.4.

### 4. **`CapabilityGate` is always installed (not optional)**

After T2.3, `AgentConfig.capability_set` is `CapabilitySet`, not
`Option<CapabilitySet>`. Default is `CapabilitySet::unrestricted()`,
which still emits audit events. Do not introduce new `Option`-typed
gate fields.

### 5. **Variables prefixed with `_` are dropped immediately**

The `_abort_tx` bug (find_p7_001) cost us silent cancellation. If a
variable's purpose is "keep alive for the scope", use a name like
`_keepalive` and add a comment. `_` alone is too easy to overlook in
review.

### 6. **Compaction boundaries never split a tool_use/tool_result pair**

After T3.4, `compact_older_messages` computes the boundary outside
pairs. `tool_pair_integrity::sanitize_tool_pairs` remains as a defensive
backstop only. Adding a new compaction strategy? Run the boundary tests.

### 7. **IDs are `uuid::Uuid::v4`, not wall-clock derivations**

After T4.6, `generate_run_id` and `EntryId::generate()` use `Uuid::v4`.
Do not introduce wall-clock-based ID generation.

---

## Layout

```
src/
├── lib.rs                        # Module exports
├── agent_loop/                   # Public facade (`AgentLoop`, `AgentResult`)
├── run_engine/                   # AgentRunEngine state machine + dispatch
├── subagent/                     # Sub-agent lifecycle (spawn, resume, finalize)
├── compaction/                   # Context window compaction
├── tool_pair_integrity.rs        # Post-compaction structural fixup (was `sanitizer.rs`)
├── secret_scrubber.rs            # PII / API-key redaction (T4.5)
├── memory_lifecycle/             # Memory hooks
├── handoff_guardrail/            # 3-tier guardrail chain
├── observability/                # Metrics, OTel, events, reports
├── session_tree/                 # JSONL DAG for crash recovery
├── checkpoint.rs                 # Shadow git repos
├── capability_gate.rs            # Dispatch-time capability enforcement
├── tool_call_manager.rs          # Tool call dispatch + tracking
├── event_bus.rs                  # Synchronous pub/sub
├── budget_enforcer.rs            # Token / iteration / time caps
├── cancellation.rs               # CancellationTree
└── ...
tests/                            # Integration tests (~19 files)
benches/                          # Criterion benchmarks
```

---

## References

- [ADR-016](../../docs/adr/ADR-016-agent-runtime-orchestrator-deps.md) — base orchestrator deps
- [ADR-021](../../docs/adr/ADR-021-theo-isolation-in-agent-runtime.md) — `theo-isolation` authorization
- [ADR-022](../../docs/adr/ADR-022-theo-infra-mcp-in-agent-runtime.md) — `theo-infra-mcp` authorization
- [ADR-019](../../docs/adr/ADR-019-unwrap-gate-enforced-baseline.md) — unwrap baseline gate
- [Remediation plan](../../docs/plans/agent-runtime-remediation-plan.md)
- [Kanban board](../../docs/kanban/agent-runtime-remediation-board.md)
- [Deep review report](../../review-output/final_report.md) *(local, gitignored)*
