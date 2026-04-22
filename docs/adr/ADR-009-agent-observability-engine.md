# ADR-009: Agent Observability Engine

- **Status**: Accepted
- **Date**: 2026-04-22
- **Deciders**: Meeting 20260422-150959 (16 agents, verdict REVISED)
- **Context**: Agent trajectory reconstruction, loop detection, efficiency metrics, benchmark integration

## Context

The agent runtime (`theo-agent-runtime`) already has observability primitives dispersed across 5 modules:

| Module | What it captures | Storage |
|---|---|---|
| `metrics.rs` | LLM calls, tokens, cost, tool success, iteration timing | In-memory `Arc<RwLock<RuntimeMetrics>>` |
| `context_metrics.rs` | Artifact refetch, action repetition, hypothesis changes, causal links, failure fingerprints | Serializable `ContextMetricsReport` → `.theo/metrics/{run_id}.json` |
| `observability.rs` | Raw DomainEvent → JSONL | `StructuredLogListener` → any `Write` sink |
| `reflector.rs` | NoProgressLoop, RepeatedSameError classification | Ephemeral (per-iteration guidance) |
| `failure_tracker.rs` | Cross-session pattern counting, suggestion threshold | `.theo/failure_patterns.json` |

**Problem**: These primitives are not composed. There is no:
- Unified trajectory projection from events
- Derived efficiency metrics computed from the event stream
- Result-aware loop detection (current: input-only, no output hash)
- Formal reliability contract for the event pipeline
- Event taxonomy for downstream consumption
- Crash recovery protocol for persistent storage

## Decision

### D1: Module, not crate

Create `crates/theo-agent-runtime/src/observability/` as a module directory. Consolidate existing `observability.rs`, `metrics.rs`, `context_metrics.rs` as submodules. Add new submodules for trajectory projection, derived metrics, and the async writer.

**Rationale**: YAGNI. No external consumer exists. Promote to crate only when `theo-application` or `theo-cli` needs direct import (Rule of 3).

### D2: Minimal domain extension

Add to `theo-domain` only:
- `TrajectoryId(String)` newtype (follows existing `RunId`, `TaskId`, `CallId` pattern)
- `#[non_exhaustive] pub enum EventKind { Lifecycle, Tooling, Reasoning, Context, Retrieval, Failure }` — canonical taxonomy

No composite structs in `theo-domain`. All projection and computation types live in the observability module.

**Rationale**: Preserves domain purity. `TrajectoryStep` was rejected (70%+ semantic overlap with `ToolCallRecord` + `DomainEvent`).

### D3: Projection model, not canonical entity

Trajectories are **computed projections** over `DomainEvent` streams, not persisted entities.

---

## Formal Specifications

### Spec 1: Event Pipeline Reliability Contract

Four invariants that the observability pipeline MUST satisfy:

#### INV-1: `at_least_observed`

> Every `DomainEvent` published to an `EventBus` with a subscribed `ObservabilityListener` MUST be delivered to that listener's `on_event` callback exactly once.

**Scope**: This invariant holds for the listener dispatch path, independent of the EventBus log buffer. The FIFO drop in `publish()` affects only `EventBus::events()` (the queryable log), NOT listener dispatch. Listeners receive events inline during `publish()`, before the method returns.

**Proof sketch**: In `event_bus.rs`, `publish()` first appends to `log` (line ~65), then iterates over all listeners calling `on_event` (line ~80). The listener dispatch is not conditional on log capacity. Therefore FIFO drop does not affect listener delivery.

**Test contract**:
```
GIVEN EventBus with max_events=5 and ObservabilityListener subscribed
WHEN 100 events are published
THEN ObservabilityListener.on_event() is called exactly 100 times
AND EventBus.events().len() == 5 (only last 5 retained in log)
```

#### INV-2: `drop_detectable`

> If the `ObservabilityListener`'s internal channel is full when `on_event` attempts `try_send`, the drop MUST be counted in an `AtomicU64` counter, and the next successfully sent event MUST carry a `dropped_since_last: u64` field in its envelope.

**Implementation**: The JSONL writer thread checks `dropped_since_last > 0` on every received message and writes a sentinel line:
```json
{"kind":"drop_sentinel","dropped_count":17,"at_sequence":4583}
```

**Test contract**:
```
GIVEN ObservabilityListener with channel capacity=10
WHEN 50 events published in burst (faster than writer can drain)
THEN dropped_count + received_count == 50
AND at least one drop_sentinel line exists in JSONL output
```

#### INV-3: `per_run_ordering`

> Within a single `run_id`, events MUST be written to JSONL in the same order they were published to the EventBus. Cross-run ordering is undefined.

**Implementation**: Each JSONL line carries a monotonic `sequence_number: u64` per writer instance. The writer thread processes its channel FIFO. Since `EventBus::publish()` dispatches listeners under a single-threaded loop (no parallel dispatch), events arrive in publish order.

**Test contract**:
```
GIVEN a run that publishes events E1, E2, E3 in order
WHEN JSONL file is read back
THEN sequence numbers are strictly monotonic: seq(E1) < seq(E2) < seq(E3)
AND timestamps are non-decreasing: ts(E1) <= ts(E2) <= ts(E3)
```

#### INV-4: `writer_failure_visibility`

> If the JSONL writer thread encounters an I/O error, it MUST:
> (a) increment an `AtomicU64` write_errors counter,
> (b) NOT silently drop the event,
> (c) attempt to write a diagnostic line on recovery.

**Implementation**: On write error, the writer buffers the failed event in a bounded retry queue (capacity=100). On next successful write, it drains the retry queue first, then writes:
```json
{"kind":"writer_recovered","buffered_events":3,"error":"disk full","recovery_sequence":4590}
```

If the retry queue itself overflows, events are dropped with the counter incremented (falls back to INV-2 semantics).

**Test contract**:
```
GIVEN ObservabilityListener writing to a mock writer that fails on writes 5-8
WHEN 20 events are published
THEN write_errors counter == 4
AND JSONL output contains all 20 events (retried) + 1 writer_recovered sentinel
AND no event is silently lost
```

---

### Spec 2: Event Taxonomy (`EventKind`)

Every `EventType` maps to exactly one `EventKind`. This mapping is a pure function, deterministic, no runtime state.

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    Lifecycle,   // Run/task state transitions
    Tooling,     // Tool call lifecycle
    Reasoning,   // Cognitive events (hypothesis, decision, constraint)
    Context,     // Context management (overflow, retrieval, compaction)
    Failure,     // Errors, budget exceeded
    Streaming,   // Content/reasoning deltas (excluded from trajectory v1)
}

impl EventType {
    pub fn kind(&self) -> EventKind {
        match self {
            // Lifecycle
            EventType::TaskCreated | EventType::TaskStateChanged |
            EventType::RunInitialized | EventType::RunStateChanged => EventKind::Lifecycle,

            // Tooling
            EventType::ToolCallQueued | EventType::ToolCallDispatched |
            EventType::ToolCallCompleted | EventType::ToolCallProgress |
            EventType::SensorExecuted => EventKind::Tooling,

            // Reasoning
            EventType::HypothesisFormed | EventType::HypothesisInvalidated |
            EventType::DecisionMade | EventType::ConstraintLearned => EventKind::Reasoning,

            // Context
            EventType::ContextOverflowRecovery | EventType::RetrievalExecuted => EventKind::Context,

            // Failure
            EventType::Error | EventType::BudgetExceeded => EventKind::Failure,

            // Streaming
            EventType::ContentDelta | EventType::ReasoningDelta => EventKind::Streaming,

            // Tooling (todo is tool-adjacent)
            EventType::TodoUpdated => EventKind::Tooling,

            // LLM calls are Context (they manage the conversation)
            EventType::LlmCallStart | EventType::LlmCallEnd => EventKind::Context,
        }
    }
}
```

**Trajectory v1 includes**: `Lifecycle`, `Tooling`, `Reasoning`, `Context`, `Failure`.
**Trajectory v1 excludes**: `Streaming` (high cardinality, low diagnostic value in v1).

---

### Spec 3: Projection Model

A trajectory is a **deterministic, idempotent projection** from a `Vec<DomainEvent>` filtered by `run_id`.

#### Definition

```rust
pub struct TrajectoryProjection {
    pub run_id: RunId,
    pub trajectory_id: TrajectoryId,
    pub steps: Vec<ProjectedStep>,
    pub metrics: DerivedMetrics,
    pub integrity: IntegrityReport,
}

pub struct ProjectedStep {
    pub sequence: u64,           // Monotonic within this projection
    pub event_type: EventType,
    pub event_kind: EventKind,
    pub timestamp: u64,
    pub entity_id: String,
    pub payload_summary: String, // Truncated to 500 chars
    pub duration_ms: Option<u64>, // For tool calls: completed_at - created_at
    pub tool_name: Option<String>,
    pub outcome: Option<StepOutcome>,
}

#[derive(Debug, Clone, Copy)]
pub enum StepOutcome {
    Success,
    Failure { retryable: bool },
    Timeout,
    Skipped,
}
```

#### Formal properties

**P1: Determinism** — Given the same ordered `Vec<DomainEvent>`, the projection MUST produce bit-identical `TrajectoryProjection` (excluding `trajectory_id` which is generated). Tested via proptest.

**P2: Idempotence** — `project(events) == project(project_inverse(project(events)))` — projecting, serializing to JSONL, reading back, and re-projecting produces the same result.

**P3: Tolerance to missing events** — If events are missing (detected via sequence gap or drop sentinel), the projection MUST:
- Set `integrity.complete = false`
- Set `integrity.missing_sequences = vec![gap_start..gap_end]`
- Compute metrics with `confidence: f64` degraded proportionally: `confidence = 1.0 - (missing_events / total_expected_events)`
- Never panic or produce undefined values

**P4: Tolerance to out-of-order events** — Events within a single JSONL file are guaranteed ordered (INV-3). If consuming from multiple sources (e.g., sub-agent + parent), the projection MUST sort by `(timestamp, sequence_number)` before processing.

#### IntegrityReport

```rust
pub struct IntegrityReport {
    pub complete: bool,
    pub total_events_expected: u64,  // From RunInitialized to terminal state
    pub total_events_received: u64,
    pub missing_sequences: Vec<std::ops::Range<u64>>,
    pub drop_sentinels_found: u64,
    pub writer_recoveries_found: u64,
    pub confidence: f64,  // 0.0..=1.0
    pub schema_version: u32,
}
```

---

### Spec 4: Derived Metrics (Surrogate Metrics)

All 5 v1 metrics are explicitly **surrogate metrics** — proxies for agent quality, not ground truth. Each metric includes its formula, edge cases, and confidence degradation rule.

```rust
pub struct DerivedMetrics {
    pub doom_loop_frequency: SurrogateMetric,
    pub llm_efficiency: SurrogateMetric,
    pub context_waste_ratio: SurrogateMetric,
    pub hypothesis_churn_rate: SurrogateMetric,
    pub time_to_first_tool_ms: SurrogateMetric,
}

pub struct SurrogateMetric {
    pub value: f64,
    pub confidence: f64,       // Degrades with missing events
    pub denominator: u64,      // For rate metrics: what divided by what
    pub numerator: u64,
    pub is_surrogate: bool,    // Always true for v1
    pub caveat: &'static str, // Human-readable limitation
}
```

#### M1: `doom_loop_frequency`

```
numerator   = count of ToolCallCompleted events where hash(tool_name, normalized_args)
              matches a previous event within sliding window of W=10 steps
denominator = total ToolCallCompleted events
value       = numerator / denominator  (safe_div → 0.0 if denominator == 0)
caveat      = "Counts input-identical repetitions only. Semantically equivalent
               but lexically different calls are not detected."
```

**Edge cases**: `denominator == 0` → `value = 0.0, confidence = 0.0` (no data). Window smaller than W → use actual window size.

#### M2: `llm_efficiency`

```
numerator   = count of distinct (tool_name, outcome=Success) pairs across the run
denominator = total LlmCallEnd events
value       = numerator / denominator  (safe_div → 0.0)
caveat      = "Measures tool-call diversity per LLM round. A 'useful' call is defined
               as success + distinct tool+args — this is a heuristic, not causal."
```

**Edge cases**: Run with 0 LLM calls → `value = 0.0, confidence = 0.0`. Run with 0 tool calls → `value = 0.0` (LLM calls produced no action).

#### M3: `context_waste_ratio`

```
numerator   = count of ContextOverflowRecovery events
denominator = count of RunStateChanged events where new_state != terminal
              (approximates iteration count)
value       = numerator / denominator  (safe_div → 0.0)
caveat      = "Measures how often context window overflowed, not how much was wasted.
               Zero overflows does not mean context was used efficiently."
```

#### M4: `hypothesis_churn_rate`

```
numerator   = count of HypothesisInvalidated events
denominator = count of HypothesisFormed events
value       = numerator / denominator  (safe_div → 0.0)
caveat      = "Assumes uniform hypothesis quality. High churn may indicate either
               poor reasoning or productive exploration — interpret with task context."
```

**Edge cases**: 0 hypotheses formed → `value = 0.0, confidence = 0.0`. More invalidations than formations (if hypotheses from prior context) → value > 1.0 is valid.

#### M5: `time_to_first_tool_ms`

```
value       = timestamp(first ToolCallDispatched) - timestamp(RunInitialized)
              If no ToolCallDispatched exists: value = total_run_duration_ms
caveat      = "Includes LLM inference time. High values may indicate slow model
               response, not agent indecision."
```

**Confidence degradation**: For all metrics, if `integrity.confidence < 1.0`, each metric's confidence is multiplied by `integrity.confidence`. Metrics computed from missing event types get `confidence = 0.0`.

---

### Spec 5: Loop Detection — Per-Tool-Class Normalization

Loop detection lives in `reflector.rs` (extended), NOT in the observability module. The observability module consumes `LoopDetected` events emitted by the reflector.

#### Normalization protocol

Each tool class defines its own normalization function. The normalizer strips ephemeral content (timestamps, temp paths, random IDs) to produce a stable hash.

```rust
pub trait ToolNormalizer: Send + Sync {
    fn normalize_args(&self, args: &serde_json::Value) -> Vec<u8>;
    fn normalize_output(&self, output: &str) -> Vec<u8>;
}
```

**Per-tool-class normalizers**:

| Tool class | Args normalization | Output normalization |
|---|---|---|
| `read_file` | Keep `path` only, strip line ranges | Hash first 1KB of output |
| `write_file` / `edit_file` | Keep `path` + content hash | Keep success/failure only |
| `bash` | Strip env vars, normalize whitespace, sort flags | Strip timestamps, PIDs, temp paths (`/tmp/xxx`), ANSI codes. Hash remaining. |
| `grep` / `glob` | Keep pattern + path, strip options | Hash file list (sorted) |
| `web_search` / `web_fetch` | Keep URL/query only | Hash first 2KB |
| `subagent` | Keep role + objective hash | Keep success/failure + summary hash |
| Default (unknown) | Hash full args JSON | Hash first 1KB of output |

**Normalization rules for `bash`** (the hardest case):
```
1. Strip leading/trailing whitespace
2. Collapse multiple spaces/tabs to single space
3. Remove ANSI escape sequences: \x1b\[[0-9;]*[a-zA-Z]
4. Replace /tmp/[a-zA-Z0-9_-]+ with /tmp/<TEMP>
5. Replace PID-like numbers after "pid=" or "PID " with <PID>
6. Replace ISO timestamps (YYYY-MM-DDTHH:MM:SS...) with <TS>
7. Replace Unix timestamps (10+ digits) with <UNIX_TS>
8. Hash the normalized string with xxhash64
```

#### Detection algorithm

```rust
struct LoopDetector {
    window: VecDeque<ToolFingerprint>,  // Sliding window, max W=10
    consecutive_identical: u32,
}

struct ToolFingerprint {
    tool_name: String,
    args_hash: u64,     // xxhash64 of normalized args
    output_hash: u64,   // xxhash64 of normalized output
    iteration: usize,
}

impl LoopDetector {
    fn record(&mut self, fp: ToolFingerprint) -> LoopVerdict {
        // Check if (tool_name, args_hash, output_hash) matches any in window
        let full_match = self.window.iter().any(|prev|
            prev.tool_name == fp.tool_name &&
            prev.args_hash == fp.args_hash &&
            prev.output_hash == fp.output_hash
        );

        if full_match {
            self.consecutive_identical += 1;
        } else {
            self.consecutive_identical = 0;
        }

        self.window.push_back(fp);
        if self.window.len() > 10 {
            self.window.pop_front();
        }

        match self.consecutive_identical {
            0..=1 => LoopVerdict::Ok,
            2 => LoopVerdict::Warning,   // Log but don't intervene
            3..=4 => LoopVerdict::Correct, // Inject corrective prompt
            _ => LoopVerdict::HardStop,    // Emit BudgetExceeded, abort
        }
    }
}

enum LoopVerdict {
    Ok,
    Warning,  // Observability event only
    Correct,  // Inject system message: "You are repeating the same action..."
    HardStop, // Abort the run
}
```

#### Whitelist: expected repetitions

Some tool sequences are expected and MUST NOT trigger loop detection:

```rust
const EXPECTED_SEQUENCES: &[(&str, &str)] = &[
    ("write_file", "read_file"),   // Verify after write
    ("edit_file", "read_file"),    // Verify after edit
    ("bash", "read_file"),         // Check output after command
    ("edit_file", "bash"),         // Compile/test after edit
    ("write_file", "bash"),        // Compile/test after write
];
```

When tool B follows tool A and `(A, B)` is in `EXPECTED_SEQUENCES`, the consecutive_identical counter is NOT incremented even if the full fingerprint matches. This prevents flagging legitimate edit-verify cycles.

---

### Spec 6: Failure Mode Operational Semantics

Each failure mode has a **formal predicate** — an observable condition that can be evaluated mechanically from the event stream.

#### FM-1: `NoProgressLoop` (existing)

```
PREDICATE: consecutive_iterations_without_edit >= 2
WHERE consecutive_iterations_without_edit =
    count of RunStateChanged(Executing→Evaluating) events since last
    ToolCallCompleted where tool_name in {"write_file", "edit_file", "apply_patch"}
    with outcome=Success
```

#### FM-2: `RepeatedSameError` (existing)

```
PREDICATE: consecutive_identical_errors >= 2
WHERE consecutive_identical_errors =
    count of consecutive Error events where
    normalize_error(payload.message) == normalize_error(prev_error.message)
AND normalize_error strips: line numbers, file paths, timestamps, stack frames
```

#### FM-3: `PrematureTermination` (new)

```
PREDICATE: run reached terminal state (Converged | Aborted)
    AND total_successful_edits == 0
    AND total_iterations >= 2
    AND no BudgetExceeded event exists
```

**Sensor**: Check at run exit. If predicate holds, emit `DomainEvent(EventType::Error, payload: {"failure_mode": "PrematureTermination"})`.

**Operational meaning**: Agent decided it was done without making any changes, and wasn't forced to stop by budget. Indicates either misunderstanding the task or premature `done` call.

#### FM-4: `WeakVerification` (new)

```
PREDICATE: exists ToolCallCompleted where tool_name in {"write_file", "edit_file"}
    AND outcome=Success
    AND no subsequent SensorExecuted event exists within next 3 iterations
    AND no subsequent ToolCallCompleted where tool_name="bash" exists within next 3 iterations
```

**Sensor**: Evaluated lazily at run exit by scanning the event stream. For each successful edit, check if a verification action (sensor or bash command) followed within 3 iterations.

**Operational meaning**: Agent made changes but never verified them. High correlation with bugs shipped.

#### FM-5: `TaskDerailment` (new)

```
PREDICATE: exists a contiguous sequence of >= 5 tool calls
    WHERE none of the tool calls reference any file in the initial_context
    AND the sequence does not contain a "done" call
    AND the sequence is not preceded by a ContextOverflowRecovery event
```

**Sensor**: Requires tracking `initial_context` (files mentioned in the task prompt or first retrieval). Evaluated at run exit.

**Operational meaning**: Agent wandered into unrelated territory without recovering. Indicates lost focus.

**Note**: `initial_context` is defined as: files referenced in the first `RetrievalExecuted` event's payload, union with files explicitly named in the task objective.

#### FM-6: `ConversationHistoryLoss` (new)

```
PREDICATE: exists ContextOverflowRecovery event
    AND within 3 iterations after recovery:
        exists ToolCallCompleted where tool_name in {"read_file", "grep", "glob"}
        AND the target path was already in hot_files before compaction
```

**Sensor**: Compare files accessed post-compaction with files that were in the working set pre-compaction. If the agent re-reads files it already had, compaction lost critical context.

**Operational meaning**: Context compaction removed information the agent still needed, forcing redundant work.

---

### Spec 7: JSONL Storage Protocol

#### File format

Path: `.theo/trajectories/{run_id}.jsonl`

Each line is a self-contained JSON object with envelope:

```json
{
  "v": 1,
  "seq": 42,
  "ts": 1713800000000,
  "run_id": "01HXR...",
  "kind": "event",
  "event_type": "ToolCallCompleted",
  "event_kind": "Tooling",
  "entity_id": "call-01HXR...",
  "payload": { ... },
  "dropped_since_last": 0
}
```

**Envelope fields** (always present):
- `v: u32` — schema version (starts at 1, incremented on breaking changes)
- `seq: u64` — monotonic sequence number per writer instance
- `ts: u64` — event timestamp (Unix ms)
- `run_id: String` — which run this event belongs to
- `kind: String` — discriminator: `"event"`, `"drop_sentinel"`, `"writer_recovered"`, `"summary"`

**Special lines**:
```json
{"v":1,"seq":100,"ts":...,"run_id":"...","kind":"drop_sentinel","dropped_count":5}
{"v":1,"seq":105,"ts":...,"run_id":"...","kind":"writer_recovered","buffered_events":3,"error":"disk full"}
{"v":1,"seq":999,"ts":...,"run_id":"...","kind":"summary","metrics":{...},"integrity":{...}}
```

The `summary` line is always the LAST line, written at run exit. It contains the full `DerivedMetrics` and `IntegrityReport`.

#### Crash recovery protocol

**Problem**: Process crash mid-write leaves partial JSON on the last line.

**Protocol**:

1. **Detection**: On read, if `serde_json::from_str(last_line)` fails, the last line is truncated.

2. **Recovery**:
   ```
   IF last line is invalid JSON:
       discard last line
       SET integrity.complete = false
       SET integrity.missing_sequences = [last_valid_seq + 1 ..]
       COMPUTE metrics from valid lines only
       LOG warning: "Trajectory {run_id}: last line truncated, {n} events recovered"
   ```

3. **Prevention**: The writer calls `flush()` after every N=100 lines (not per line — too expensive). On graceful shutdown, `flush()` + `sync_data()` (fsync) is called.

4. **Atomic summary**: The summary line is written as:
   ```
   write temp_path = {run_id}.summary.tmp
   fsync temp_path
   append temp_path content to main JSONL
   fsync main JSONL
   delete temp_path
   ```
   If crash occurs: on next startup, if `.summary.tmp` exists, append it to main JSONL.

#### Rotation and retention

- **No rotation within a run**: One file per run, unbounded within the run.
- **Estimated size**: ~200 bytes/event × 500 events/run = ~100KB/run typical.
- **Retention**: No automatic eviction in v1. Manual `rm .theo/trajectories/*.jsonl` is acceptable. Automated retention is P2.

#### StructuredLogListener migration

1. `StructuredLogListener` is replaced by `TrajectoryWriter` in the same crate.
2. `TrajectoryWriter` writes the same data (DomainEvent as JSON line) but with the envelope format above.
3. Migration is one-shot: old format files (no `v` field) are not migrated, they coexist in `.theo/trajectories/legacy/`.
4. No dual-write period. The switch happens in one commit.

---

### Spec 8: Async Writer Architecture

```
┌─────────────┐     on_event()      ┌──────────────────┐
│  EventBus   │ ──────────────────▶  │ ObservabilityListener │
│  (publish)  │     sync, O(1)       │  try_send(bytes)      │
└─────────────┘                      └────────┬─────────────┘
                                              │ mpsc::SyncSender<Vec<u8>>
                                              ▼
                                     ┌──────────────────┐
                                     │  Background Thread │
                                     │  (std::thread)     │
                                     │  ┌──────────────┐  │
                                     │  │ BufWriter<File>│ │
                                     │  │ seq counter   │  │
                                     │  │ retry queue   │  │
                                     │  │ flush timer   │  │
                                     │  └──────────────┘  │
                                     └──────────────────┘
```

**Why `std::thread` not `tokio::spawn`**: The writer does blocking file I/O. Running blocking I/O on a tokio worker thread is an anti-pattern (starves other tasks). A dedicated OS thread with `mpsc::SyncSender` (bounded, capacity=4096) is the correct pattern.

**`on_event` implementation**:
```rust
fn on_event(&self, event: &DomainEvent) {
    // Filter: skip Streaming events
    if event.event_type.kind() == EventKind::Streaming {
        return;
    }

    // Serialize to bytes (avoids clone of serde_json::Value)
    let bytes = match serde_json::to_vec(event) {
        Ok(b) => b,
        Err(_) => {
            self.serialization_errors.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };

    // Non-blocking send
    if self.sender.try_send(bytes).is_err() {
        self.dropped_events.fetch_add(1, Ordering::Relaxed);
    }
}
```

**Background thread loop**:
```rust
fn writer_loop(receiver: mpsc::Receiver<Vec<u8>>, mut writer: BufWriter<File>, ...) {
    let mut seq: u64 = 0;
    let mut lines_since_flush: u64 = 0;
    let mut dropped_since_last: u64 = 0;

    while let Ok(event_bytes) = receiver.recv() {
        // Check for drops
        let current_drops = dropped_counter.swap(0, Ordering::Relaxed);
        dropped_since_last += current_drops;

        if dropped_since_last > 0 {
            // Write drop sentinel
            write_sentinel(&mut writer, seq, dropped_since_last);
            seq += 1;
            dropped_since_last = 0;
        }

        // Write event with envelope
        write_envelope(&mut writer, seq, &event_bytes);
        seq += 1;
        lines_since_flush += 1;

        // Periodic flush
        if lines_since_flush >= 100 {
            let _ = writer.flush();
            lines_since_flush = 0;
        }
    }

    // Graceful shutdown: write summary, flush, fsync
    write_summary(&mut writer, seq, &metrics, &integrity);
    let _ = writer.flush();
    if let Ok(f) = writer.into_inner() {
        let _ = f.sync_data();
    }
}
```

---

## Rejected Alternatives

1. **Separate crate `theo-observability`** — rejected because no external consumer exists and it would create premature boundary overhead.

2. **`TrajectoryStep` as domain entity** — rejected because 70% semantic overlap with `DomainEvent + ToolCallRecord`. Projection layer avoids duplication.

3. **Deterministic replay in v1** — rejected (YAGNI). Requires capturing LLM responses and random seeds. Trajectory visualization is sufficient for v1.

4. **OTel crate dependency** — rejected. Align naming conventions with OTel GenAI semantics, but no runtime dependency. OTel export is P3.

5. **LLM-as-judge for failure classification** — rejected for v1. Heuristic predicates are cheaper, faster, and deterministic. LLM judge is P3 (already planned as Phase 4 of reflector).

6. **Per-event fsync** — rejected. Too expensive (~5ms per fsync on SSD). Batch flush every 100 lines with fsync on graceful shutdown is the right tradeoff.

## Consequences

- Observability module consolidates 5 existing files into a cohesive directory structure
- Event pipeline has formal reliability contract with 4 testable invariants
- Loop detection becomes result-aware with per-tool-class normalization
- Failure taxonomy grows from 2 to 6 modes, each with formal predicates
- JSONL storage has explicit crash recovery and schema versioning
- All derived metrics are labeled as surrogate metrics with confidence scores
- StructuredLogListener is replaced (no dual-write migration)

## References

- Meeting ata: `.claude/meetings/20260422-150959-agent-observability-engine.md`
- MAST failure taxonomy: https://arxiv.org/abs/2503.13657
- Terminal-Bench 2.0: https://www.tbench.ai/news/announcement-2-0
- SWE-Bench Pro: https://arxiv.org/abs/2509.16941
- Anthropic eval practices: https://www.anthropic.com/engineering/demystifying-evals-for-ai-agents
- OpenTelemetry GenAI conventions: https://uptrace.dev/blog/opentelemetry-ai-systems
