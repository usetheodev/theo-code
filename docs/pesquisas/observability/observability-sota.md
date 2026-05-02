# Observability for AI Coding Agents -- State of the Art (April 2026)

> **Research target:** raise Theo Code's Observability score from 0.5/5 to 4.0+/5.
>
> **Crates in scope:** `theo-agent-runtime` (cost tracking, trajectory, tracing, event bus),
> `theo-application` (dashboard use cases, metrics aggregation, UI endpoints).

---

## Executive Summary

Observability in AI coding agents has shifted from a nice-to-have dashboard to a foundational design requirement in 2026. The LangChain State of Agent Engineering report shows 89% of organizations implement some form of agent observability, with 62% achieving step-level tracing. OpenTelemetry GenAI semantic conventions reached stable status in early 2026, establishing a vendor-neutral vocabulary for spans, metrics, and events across any GenAI system. Cost tracking is no longer optional -- teams that cannot attribute spend to specific sessions, agents, and tool calls are blind to optimization opportunities.

Theo Code already has a solid observability foundation: an `EventBus` with structured `DomainEvent` dispatch, a `TrajectoryEnvelope` JSONL format with schema versioning, an `ObservabilityPipeline` with background writer threads, an `OtelExportingListener` gated behind a Cargo feature flag, and a `RunReport` that computes surrogate metrics, token breakdowns, loop metrics, tool breakdowns, context health, memory metrics, and error taxonomy. The RLHF trajectory export pipeline (`trajectory_export.rs`) already produces DPO-compatible JSONL with rating filters.

The gaps are in per-session USD cost tracking (no pricing table, no `estimated_cost_usd` populated from real pricing data), dashboard live transport (no WebSocket/SSE endpoint), performance metric percentiles (p50/p95 not computed), agent behavior analytics (no PostHog-style event capture), and audit trail recording (no `CommandHistory` struct). This document maps each gap to prior art and provides concrete implementation guidance.

---

## Table of Contents

1. [Cost Tracking for AI Agents](#1-cost-tracking-for-ai-agents)
2. [OpenTelemetry for Agent Loops](#2-opentelemetry-for-agent-loops)
3. [Trajectory Export Formats](#3-trajectory-export-formats)
4. [Structured Logging](#4-structured-logging)
5. [Langfuse, Braintrust, Helicone](#5-langfuse-braintrust-helicone)
6. [Dashboard Architecture](#6-dashboard-architecture)
7. [Performance Metrics](#7-performance-metrics)
8. [Session Cost Metadata](#8-session-cost-metadata)
9. [Agent Behavior Analytics](#9-agent-behavior-analytics)
10. [Audit Trails](#10-audit-trails)
11. [Evidence Table](#evidence-table)
12. [Thresholds and Targets](#thresholds-and-targets)
13. [Relevance for Theo Code](#relevance-for-theo-code)

---

## 1. Cost Tracking for AI Agents

### 1.1 Prior Art

**OpenDev CostTracker.** OpenDev (Rust, terminal-native coding agent) organizes work into concurrent sessions composed of specialized sub-agents. Each agent executes typed workflows (Execution, Thinking, Compaction) that independently bind to an LLM, enabling fine-grained cost, latency, and capability trade-offs per workflow. Switching providers or optimizing cost requires only a configuration change.

**TokenCost by AgentOps.** Python library that calculates USD cost using Tiktoken for tokenization plus a pricing table covering 400+ LLMs. Input strings and ChatML messages are tokenized, then multiplied by per-model $/1K-token rates. Open-source (MIT), maintained.

**TokenMeter (.NET).** Session-based tracking with `IUsageTracker` interface: `record(modelId, inputTokens, outputTokens, sessionId)` followed by `getSessionStats()`, `getTimeRangeStats()`, `getTodayStats()`. Supports 12 providers natively.

**LiteLLM.** `completion_cost()` combines `token_counter()` and `cost_per_token()` to return USD for a single API call. Default tokenizer support for OpenAI, Anthropic, Llama, Cohere. The pricing is fetched from a community-maintained model registry.

**Bifrost AI Gateway.** Open-source Go gateway that routes all LLM traffic through a single interface, providing per-key, per-team, per-customer budget enforcement.

### 1.2 Token Counting Breakdown

All mature systems track these categories:

| Category | Description | Theo `TokenUsage` field |
|----------|-------------|------------------------|
| Input tokens | Prompt tokens sent | `input_tokens` |
| Output tokens | Completion tokens received | `output_tokens` |
| Cache read tokens | Tokens served from provider cache | `cache_read_tokens` |
| Cache write tokens | Tokens written to provider cache | `cache_write_tokens` |
| Reasoning tokens | Extended thinking tokens (o-series, Claude) | `reasoning_tokens` |
| Audio/image tokens | Multimodal usage | Not tracked (YAGNI) |

Theo's `TokenUsage` struct already covers the five critical categories. The gap is `estimated_cost_usd`: the field exists but is never populated from a real pricing table.

### 1.3 Pricing Table Architecture

Two approaches exist in the wild:

1. **Embedded pricing table** (TokenCost, LiteLLM): a JSON/TOML file shipped with the binary, mapping `(provider, model) -> (input_$/1M, output_$/1M, cache_read_$/1M, cache_write_$/1M)`. Updated on release cycles. Simple, no network dependency.

2. **Remote pricing API** (models.dev, OpenRouter): HTTP endpoint returning current pricing. More accurate for rapidly changing pricing, but introduces a network dependency and latency at startup.

**Recommendation for Theo:** embedded pricing table (TOML) in `theo-domain` or `theo-infra-llm`, loaded at compile time via `include_str!`. The table covers the ~15 models Theo actively supports. A `--update-pricing` CLI flag can fetch from models.dev and overwrite the local table. This avoids runtime network dependency while keeping pricing updatable.

### 1.4 Cost Calculation Formula

```
cost_usd = (input_tokens * input_rate)
         + (output_tokens * output_rate)
         + (cache_read_tokens * cache_read_rate)
         + (cache_write_tokens * cache_write_rate)
         + (reasoning_tokens * reasoning_rate)
```

Where rates are in USD per token (i.e., the per-million rate divided by 1,000,000).

---

## 2. OpenTelemetry for Agent Loops

### 2.1 GenAI Semantic Conventions (Stable, 2026)

The OTel GenAI SIG has standardized these attribute keys:

| Attribute | Description | Theo constant |
|-----------|-------------|---------------|
| `gen_ai.system` | Provider name (anthropic, openai) | `ATTR_SYSTEM` |
| `gen_ai.request.model` | Requested model ID | `ATTR_REQUEST_MODEL` |
| `gen_ai.response.model` | Actual model used | `ATTR_RESPONSE_MODEL` |
| `gen_ai.operation.name` | Operation type (chat, embed) | `ATTR_OPERATION_NAME` |
| `gen_ai.usage.input_tokens` | Input token count | `ATTR_USAGE_INPUT_TOKENS` |
| `gen_ai.usage.output_tokens` | Output token count | `ATTR_USAGE_OUTPUT_TOKENS` |
| `gen_ai.agent.id` | Agent identifier | `ATTR_AGENT_ID` |
| `gen_ai.agent.name` | Human-readable agent name | `ATTR_AGENT_NAME` |

Theo already defines all of these in `otel.rs`. The constants match the official spec verbatim.

### 2.2 Span Hierarchy for ReAct Loops

The canonical span tree for a coding agent loop:

```
agent.run [run_id, gen_ai.agent.name]
  |-- agent.iteration [iteration_number]
  |     |-- llm.call [gen_ai.request.model, gen_ai.system]
  |     |     |-- gen_ai.prompt (event, not span -- PII control)
  |     |     `-- gen_ai.completion (event, not span)
  |     |-- tool.call [theo.tool.name, theo.tool.duration_ms]
  |     |-- tool.call [...]
  |     `-- agent.thinking [duration_ms] (extended thinking span)
  |-- agent.iteration [...]
  |-- agent.compaction [before_tokens, after_tokens, ratio]
  `-- subagent.spawn [gen_ai.agent.name]
        |-- agent.iteration [...]
        `-- ...
```

**Key design decisions from the OTel GenAI SIG:**

1. **Prompts and completions are span events, not attributes.** Attributes are always indexed and have size limits. Span events can be filtered or dropped at the Collector level without touching application code. This prevents PII exposure in the tracing backend.

2. **Compaction as a span.** Compaction is a distinct operation with measurable input/output token counts. It should be a sibling span to iterations, not a child.

3. **Subagent correlation.** The subagent's root span should be a child of the parent agent's run span. Trace context propagation happens via the `run_id` stored in `DomainEvent.entity_id`.

**Theo's current state:** `OtelExportingListener` already implements `agent.run`, `subagent.spawn`, `tool.call`, and `llm.call` spans. The gap is:
- No `agent.iteration` span (iteration boundaries are not instrumented).
- No `agent.compaction` span.
- No `agent.thinking` span for extended thinking.
- Parent-child linkage uses `__no_parent__` placeholder -- subagent and tool spans are not properly parented to the run span.

### 2.3 Trace Volume Management

Agents that loop generate a lot of spans. Best practices:

- **Tail-based sampling**: keep 100% of error traces, sample 10% of success traces.
- **Batch size tuning**: Theo's default `OTLP_BATCH_SIZE=512` is appropriate for single-user CLI use. Multi-tenant deployments should increase to 2048.
- **Event suppression**: suppress `gen_ai.prompt` and `gen_ai.completion` events by default; enable via `OTLP_CAPTURE_PROMPTS=true` for debugging.

### 2.4 Distributed Tracing for Agentic Workflows

When an agent delegates to subagents or MCP servers, trace context must propagate across boundaries. The OTel W3C Trace Context standard (`traceparent` header) is the mechanism. For Theo:

- The main agent run creates a root span.
- Subagent spawning creates a child span linked to the parent's trace context.
- MCP tool calls should propagate `traceparent` in the MCP request headers (MCP spec does not standardize this yet; custom header is acceptable).

---

## 3. Trajectory Export Formats

### 3.1 Hermes Trajectory System

Hermes Agent (v0.11.0, April 2026) by Nous Research is the reference implementation for RL-compatible trajectory export. Key features:

- **JSONL output format**: each line is a scored trajectory with full conversation history, reward, and metadata.
- **Two modes**: Process mode (offline, saves to JSONL without training server) and Serve mode (online RL with Atropos coordination).
- **ShareGPT format**: training data exportable for fine-tuning via SFT.
- **Atropos integration**: trajectory API server that coordinates environment interactions, manages rollout groups, and computes advantages.
- **Trajectory compression**: `trajectory_compressor.py` reduces trajectory size for training datasets.

### 3.2 What Makes Trajectories RL-Compatible

An RL-compatible trajectory must contain:

| Field | Required | Description |
|-------|----------|-------------|
| `messages` | Yes | Full conversation history in ChatML format |
| `reward` / `rating` | Yes | Scalar reward signal (-1 to +1 minimum, continuous preferred) |
| `turn_index` | Yes | Which turn in the conversation the reward applies to |
| `tool_calls` | Yes | Tool invocations with inputs and outputs |
| `tool_results` | Yes | Results of tool executions |
| `metadata.model` | Yes | Which model generated the trajectory |
| `metadata.temperature` | Recommended | Sampling parameters for reproducibility |
| `metadata.run_id` | Recommended | For deduplication and provenance |
| `chosen` / `rejected` | For DPO | Paired trajectories for preference learning |

### 3.3 Common Formats

| Format | Used By | Structure |
|--------|---------|-----------|
| **ShareGPT** | Hermes, Axolotl | `{"conversations": [{"from": "system/human/gpt", "value": "..."}]}` |
| **OpenAI fine-tuning** | OpenAI API | `{"messages": [{"role": "system/user/assistant", "content": "..."}]}` |
| **DPO pairs** | TRL, Axolotl | `{"chosen": [...messages], "rejected": [...messages]}` |
| **GRPO groups** | Hermes/Atropos | `{"group_id": "...", "trajectories": [...], "advantages": [...]}` |
| **Custom JSONL** | Most agents | One JSON object per line, schema varies |

### 3.4 Theo's Current State

Theo's `trajectory_export.rs` implements:
- `RlhfRecord` with `run_id`, `turn_index`, `rating`, `comment`, `timestamp`.
- `RatingFilter` for positive/negative/exact filtering.
- `export_rlhf_dataset()` one-shot helper reading from `.theo/trajectories/`.
- Rating envelopes (`EnvelopeKind::Rating`) with -3..+3 scoring support.

**Gap**: the exported JSONL contains only the rating metadata, not the full conversation. The consumer must join with the state file (`.theo/state/<run>/state.jsonl`) to reconstruct the full trajectory. This is documented ("Joining the rating with the original LLM prompt/response is left to the consumer pipeline") but limits standalone usability. A `--full-trajectory` export mode that produces self-contained DPO pairs would raise RL-compatibility significantly.

---

## 4. Structured Logging

### 4.1 Archon Convention

Archon (the open-source harness builder by coleam00) uses a `{domain}.{action}_{state}` naming convention for structured log events:

```
subagent.spawn_started
subagent.spawn_completed
workflow.step_started
workflow.step_completed
tool.call_dispatched
tool.call_completed
```

Theo already implements this convention in `otel.rs`:

```rust
pub fn log_event(domain: &str, action: &str, state: &str) -> String {
    format!("{}.{}_{}", domain, action, state)
}
```

### 4.2 Pino-Style JSONL Logging

Pino (Node.js) sets the standard for structured JSON logging:

- NDJSON (Newline Delimited JSON) by default.
- Child loggers for context propagation (request ID, session ID).
- Asynchronous I/O via worker threads.
- Redaction of sensitive fields.
- 5x faster than Winston.

**Mapping to Theo's Rust architecture:**

| Pino concept | Theo equivalent |
|-------------|-----------------|
| NDJSON output | `TrajectoryEnvelope` JSONL |
| Child logger with context | `ObservabilityListener` with `run_id` |
| Async worker thread | `spawn_writer_thread()` background writer |
| Redaction | `secret_scrubber.rs` |
| Log levels | `EventKind` (Lifecycle, Tooling, Reasoning, Context, Failure, Streaming) |

Theo's approach is architecturally equivalent to Pino's worker-thread model. The `SyncSender<Vec<u8>>` channel in `ObservabilityListener` provides backpressure (bounded channel, capacity `DEFAULT_CHANNEL_CAPACITY`), while the writer thread handles I/O without blocking event dispatch.

### 4.3 Event Emitter System

Theo's `EventBus` implements a synchronous publish-subscribe pattern with:

- `subscribe(Arc<dyn EventListener>)` for sync listeners.
- `subscribe_broadcast(capacity)` for async consumers (TUI, WebSocket).
- Panic protection via `catch_unwind` on each listener.
- Bounded in-memory log (`VecDeque`, default 10,000 events).
- `events_since(event_id)` for delta polling (dashboard, OTel exporter).

This is functionally equivalent to Archon's event emitter with `step_started/completed`, `node_started` events. The domain events cover the same lifecycle:

| Archon event | Theo `EventType` |
|-------------|------------------|
| `step_started` | `RunInitialized` |
| `step_completed` | `RunStateChanged` |
| `node_started` | `ToolCallDispatched` |
| `node_completed` | `ToolCallCompleted` |

### 4.4 Request ID Correlation

All `DomainEvent` instances carry an `event_id` (UUID) and `entity_id` (run_id, call_id, etc.). The `entity_id` serves as the correlation ID across the event stream. The trajectory JSONL envelope includes `run_id` for cross-line correlation.

**Gap**: there is no top-level `session_id` that spans multiple `--continue` resumptions. Adding a `session_id` field to `DomainEvent` or a session-level correlation in the trajectory envelope would enable cross-resumption analytics.

---

## 5. Langfuse, Braintrust, Helicone

### 5.1 Platform Comparison

| Dimension | Langfuse | Braintrust | Helicone |
|-----------|----------|------------|----------|
| **License** | MIT (open-source) | Proprietary | Open-source |
| **Integration** | SDK (Python, JS, REST) | SDK + zero-code | Proxy-based (base URL change) |
| **Self-hosting** | Yes (Postgres + ClickHouse + Redis + S3) | No | Yes |
| **Cost tracking** | Automatic per model + custom pricing | Per-request with tag attribution | Automatic via proxy |
| **Tracing** | Nested traces, session grouping | Best-in-class nested visualization | Basic request-response |
| **Evals** | None built-in | CI/CD quality gates, automated blocking | None |
| **Multi-agent** | Session-level grouping | Deep nested agent traces | Not supported |
| **Latency overhead** | SDK-based (negligible) | SDK-based (negligible) | Proxy: <1ms p99 |
| **Pricing** | Free self-hosted; $29/mo cloud | Proprietary tiers | Free 10K req/mo; $79/mo Pro |
| **Best for** | Data sovereignty, open-source teams | CI/CD quality gates, prompt regression prevention | Quick setup, minimal code changes |

### 5.2 What They Measure

All three platforms track these core metrics:

| Metric | Langfuse | Braintrust | Helicone |
|--------|----------|------------|----------|
| Token usage (input/output) | Yes | Yes | Yes |
| Cost per request | Yes | Yes | Yes |
| Latency | Yes | Yes | Yes |
| Error rates | Yes | Yes | Yes |
| Prompt versions | Yes | Yes | No |
| Model parameters (temp, top_p) | Yes | Yes | Yes |
| Session grouping | Yes | Yes | No |
| User-level attribution | Yes | Yes (tag-based) | Basic |
| Eval scores | No | Yes (automated) | No |

### 5.3 Integration Patterns for Theo

Since Theo is a Rust CLI, SDK-based integration (Langfuse, Braintrust) would require either:

1. **REST API calls** from Rust (using `reqwest`) to push traces -- adds latency on the critical path.
2. **OTLP export** to Langfuse/Braintrust OTLP endpoints -- both support OTel ingestion. This is the recommended path since Theo already has `OtelExportingListener`.
3. **Post-hoc JSONL upload** -- parse trajectory JSONL and push to platform API after the run completes. No critical-path overhead.

**Recommendation**: option 2 (OTLP export) for real-time observability, option 3 (post-hoc upload) for cost tracking and analytics. The OTLP path is already wired in `otel_listener.rs`.

### 5.4 Self-Hosted Option

For teams that need data sovereignty, the recommended stack is:

```
Theo CLI
  |-- OTLP export (gRPC or HTTP)
  |       |
  |       v
  |   OTel Collector
  |       |-- Jaeger / Tempo (traces)
  |       |-- Prometheus (metrics)
  |       `-- Loki (logs)
  `-- Trajectory JSONL (local, always-on)
```

Langfuse self-hosted requires PostgreSQL + ClickHouse + Redis + S3 -- significant operational overhead. For teams already running Kubernetes, it is viable. For single-developer use, trajectory JSONL + OTel Collector + Jaeger is simpler.

---

## 6. Dashboard Architecture

### 6.1 OpenDev Web Dashboard

OpenDev supports two frontends: a TUI (Textual, blocking modal approvals) and a Web UI (FastAPI + WebSockets, async polling approvals). Both implement a shared `UICallback` contract, keeping the agent layer UI-agnostic. The Web UI supports remote sessions.

Key architectural decisions:

| Decision | OpenDev choice | Rationale |
|----------|---------------|-----------|
| Transport | WebSocket | Bidirectional: UI sends approvals, server pushes events |
| Backend | FastAPI | Python async, native WebSocket support |
| State | In-memory + file-backed | Session state persisted for `--continue` |
| API format | JSON | Universal, no schema negotiation |

### 6.2 WebSocket vs SSE for Live Metrics

| Dimension | WebSocket | SSE (Server-Sent Events) |
|-----------|-----------|--------------------------|
| Direction | Bidirectional | Server-to-client only |
| Protocol | Custom frame protocol | HTTP/1.1 text/event-stream |
| Reconnection | Manual | Built-in (browser auto-reconnect) |
| Binary data | Yes (frames) | No (text only, base64 needed) |
| Proxy compatibility | Requires upgrade | Works through all HTTP proxies |
| Connection limit | No browser limit | 6 per domain (HTTP/1.1) |
| Complexity | Higher (ping/pong, connection management) | Lower (just write events) |

**Recommendation for Theo**: SSE for read-only dashboard (metrics, events, trajectory replay). WebSocket only if bidirectional control is needed (e.g., sending commands to the running agent). SSE is simpler, works through all proxies, and auto-reconnects.

### 6.3 Health Check Endpoints

Standard endpoints for a coding agent dashboard:

| Endpoint | Method | Response | Purpose |
|----------|--------|----------|---------|
| `/health` | GET | `{"status": "ok", "uptime_s": 3600}` | Load balancer / monitoring |
| `/api/v1/runs` | GET | `[RunSummary]` | List all runs |
| `/api/v1/runs/{id}` | GET | `RunReport` | Full report for a run |
| `/api/v1/runs/{id}/events` | GET | SSE stream | Live event stream |
| `/api/v1/runs/{id}/trajectory` | GET | `[TrajectoryEnvelope]` | Raw trajectory data |
| `/api/v1/metrics` | GET | `MetricsByAgent` | Aggregated per-agent metrics |
| `/api/v1/cost` | GET | `CostSummary` | Session/total cost breakdown |

Theo's `observability_ui.rs` already implements `list_runs()` and run detail retrieval. The gap is HTTP transport -- these use cases are pure functions returning data, not HTTP endpoints. An `axum` or `warp` server in `theo-application` or a new `theo-dashboard` binary would expose them.

---

## 7. Performance Metrics

### 7.1 What to Measure

| Metric | Description | Target |
|--------|-------------|--------|
| **Tool execution latency p50** | Median wall-clock time per tool call | < 500ms |
| **Tool execution latency p95** | 95th percentile per tool call | < 2s |
| **LLM response latency p50** | Median time from LLM call start to end | < 3s |
| **LLM response latency p95** | 95th percentile LLM call | < 10s |
| **Time-to-first-token (TTFT)** | Latency before first streaming token | < 500ms |
| **Tokens per second (TPS)** | Output tokens / LLM call duration | > 50 TPS |
| **Compaction ratio** | Tokens before / tokens after compaction | > 2x |
| **Iteration efficiency** | Successful edits / total iterations | > 0.3 |
| **Context utilization** | Tokens used / context window size | 60-85% |

### 7.2 Percentile Calculation

For Rust, percentile calculation on a stream of latency values uses a sorted Vec approach:

```rust
fn percentile(values: &mut Vec<f64>, p: f64) -> f64 {
    if values.is_empty() { return 0.0; }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((p / 100.0) * (values.len() - 1) as f64).round() as usize;
    values[idx.min(values.len() - 1)]
}
```

For streaming/online percentile estimation (when full history is not available), use the P-Square algorithm or t-digest. For Theo's use case (hundreds to low thousands of measurements per run), sorting a Vec is sufficient and simpler (KISS).

### 7.3 Theo's Current State

Theo's `ProjectedStep` already carries `duration_ms: Option<u64>` and `tool_name: Option<String>`. The `ToolBreakdown` struct in `report/metrics.rs` computes per-tool statistics. The gap is:

- **No percentile calculation**: only count, total, and average are computed.
- **No TTFT tracking**: `LlmCallStart` -> first `ContentDelta` latency is not measured.
- **No TPS tracking**: output tokens / LLM call duration is not computed.

### 7.4 Tool Latency Thresholds

Based on aggregated data from production coding agents (OpenDev, Claude Code, Hermes):

| Tool category | p50 target | p95 target | Alert threshold |
|--------------|-----------|-----------|-----------------|
| File read (Read, Glob) | < 50ms | < 200ms | > 500ms |
| File write (Edit, Write) | < 100ms | < 500ms | > 2s |
| Search (Grep) | < 200ms | < 1s | > 3s |
| Shell (Bash) | < 1s | < 10s | > 30s |
| MCP tool call | < 500ms | < 3s | > 10s |

---

## 8. Session Cost Metadata

### 8.1 Cost Tracking Object Design

Based on OpenDev's per-session approach and TokenMeter's `IUsageTracker`:

```json
{
  "cost_tracking": {
    "session_id": "ses-abc123",
    "total_input_tokens": 145200,
    "total_output_tokens": 23400,
    "total_cache_read_tokens": 89000,
    "total_cache_write_tokens": 12000,
    "total_reasoning_tokens": 5600,
    "total_cost_usd": 0.42,
    "api_calls_count": 15,
    "provider_breakdown": {
      "anthropic": {
        "model": "claude-sonnet-4-7",
        "input_tokens": 145200,
        "output_tokens": 23400,
        "cost_usd": 0.42,
        "calls": 15
      }
    },
    "subagent_breakdown": {
      "explorer": { "cost_usd": 0.08, "calls": 3 },
      "implementer": { "cost_usd": 0.34, "calls": 12 }
    },
    "started_at": "2026-04-29T10:00:00Z",
    "last_updated_at": "2026-04-29T10:15:00Z"
  }
}
```

### 8.2 Resumption Across `--continue`

When the user invokes `--continue`, the session must:

1. Load the previous session's `cost_tracking` from disk.
2. Accumulate new token usage on top of the previous totals.
3. Persist the updated totals at every checkpoint and at session end.

**Storage location**: `.theo/sessions/{session_id}/cost.json` alongside the existing state files.

### 8.3 Theo's Current State

Theo's `TokenUsage` struct already accumulates `input_tokens`, `output_tokens`, `cache_read_tokens`, `cache_write_tokens`, `reasoning_tokens`, and `estimated_cost_usd`. The `BudgetEnforcer` tracks `tokens_used` and `tool_calls_used`. The `RunReport.token_metrics` computes per-run breakdowns.

**Gap**: `estimated_cost_usd` is a field on `TokenUsage` but is never populated with real pricing data. There is no cross-session accumulation for `--continue`. There is no provider/model breakdown in the persisted metadata.

---

## 9. Agent Behavior Analytics

### 9.1 PostHog for Agent Analytics

PostHog (open-source, MIT) provides:

- Event capture via API (`capture(event, properties)`).
- Session recording and replay.
- Feature flags and A/B testing.
- LLM Analytics dashboard (token usage, latency, conversation quality).
- Free tier: 1M events, 5K recordings, 1M flag requests per month.

Hermes Agent uses PostHog for production analytics. The integration is lightweight: `POST /capture` with JSON payload containing event name, distinct_id (user/session), and properties.

### 9.2 What to Track for a Coding Agent

| Metric | Event name | Properties | Why |
|--------|-----------|------------|-----|
| Tool usage frequency | `tool.called` | `tool_name`, `duration_ms`, `success` | Identify most/least used tools |
| Tool failure rate | `tool.failed` | `tool_name`, `error_type`, `error_message` | Detect broken tools early |
| Compaction frequency | `compaction.triggered` | `before_tokens`, `after_tokens`, `ratio` | Optimize context management |
| Subagent delegation ratio | `subagent.delegated` | `parent_agent`, `child_agent`, `objective` | Understand delegation patterns |
| Iteration count per task | `task.completed` | `iterations`, `tool_calls`, `duration_s` | Identify efficiency trends |
| Edit success rate | `edit.applied` | `file`, `success`, `sensor_result` | Track code quality |
| Context overflow rate | `context.overflow` | `tokens_before`, `tokens_after` | Right-size context windows |
| Session cost | `session.ended` | `total_cost_usd`, `tokens_used` | Cost optimization |
| Doom loop detections | `doom_loop.detected` | `pattern`, `iteration` | Agent reliability |
| Memory recall effectiveness | `memory.recalled` | `episodes_injected`, `useful_count` | Tune memory system |

### 9.3 Integration Pattern for Theo

Two approaches:

1. **Direct PostHog API** -- `reqwest::Client` POSTing to `/capture` in a background task. ~10 lines of Rust. Fire-and-forget with timeout.

2. **EventBus listener** -- a `PostHogListener` that subscribes to the `EventBus` and batches events for periodic flush. This keeps the analytics path decoupled from business logic.

**Recommendation**: option 2 (EventBus listener), gated behind an opt-in environment variable (`THEO_ANALYTICS_KEY`). The listener maps `DomainEvent` types to PostHog events, batches them, and flushes every 30 seconds or on session end. No analytics without explicit opt-in.

### 9.4 Privacy Considerations

- Never send file contents, prompt text, or code snippets.
- Only send aggregate metrics (counts, durations, sizes).
- Use a hashed machine ID as `distinct_id`, not username.
- Respect `DO_NOT_TRACK` environment variable.
- All analytics must be opt-in, not opt-out.

---

## 10. Audit Trails

### 10.1 CommandHistory Pattern

An audit trail for a coding agent records every significant action for accountability and undo support:

```rust
pub struct CommandHistoryEntry {
    pub timestamp: u64,
    pub command: String,          // "edit", "write", "bash", etc.
    pub action_taken: String,     // "replaced lines 10-20 in src/main.rs"
    pub matching_rule: Option<String>, // governance rule that approved/blocked
    pub tool_call_id: String,
    pub run_id: String,
    pub result: CommandResult,    // Success, Failed, Blocked
    pub undo_data: Option<UndoPayload>, // file snapshot for rollback
}
```

### 10.2 Operation Log for Undo Tracking

The undo system requires:

1. **Pre-state snapshot**: before any write operation, capture the file's content hash and first N bytes for diff reconstruction.
2. **Post-state snapshot**: after the write, capture the new state.
3. **Undo operation**: restore pre-state from snapshot.

Theo's `checkpoint.rs` and `snapshot.rs` already implement file-level snapshots. The gap is a structured operation log that connects snapshots to the governance decisions that authorized them.

### 10.3 Governance Audit

For enterprise use, audit trails must record:

| Field | Description |
|-------|-------------|
| `timestamp` | ISO-8601 timestamp of the action |
| `actor` | Agent name + run_id |
| `action` | Tool name + parameters (redacted) |
| `target` | File path or resource affected |
| `governance_rule` | Which capability gate / governance rule was evaluated |
| `decision` | Allowed / Blocked / Escalated |
| `justification` | Agent's reasoning for the action (if available) |

Theo's `capability_gate.rs` and `handoff_guardrail/` already implement capability-based access control. The audit trail would wrap these decisions in a persistent log.

### 10.4 Theo's Current State

- `checkpoint.rs` -- file-level checkpoints for rollback.
- `snapshot.rs` -- state snapshots.
- `capability_gate.rs` -- capability-based access control.
- `handoff_guardrail/` -- guardrails for subagent delegation.
- `DomainEvent` stream -- all significant events are published to the EventBus.

**Gap**: no structured `CommandHistory` that links tool calls to governance decisions and provides undo data. The `DomainEvent` stream captures the events but does not structure them into an audit-friendly format with explicit undo payloads.

---

## Evidence Table

| Source | Category | Key Finding | Theo Status |
|--------|----------|-------------|-------------|
| [OpenDev](https://github.com/opendev-to/opendev) | Cost tracking | Per-session tokens + cost per provider, typed workflows bound to LLMs | TokenUsage exists; no pricing table |
| [TokenCost](https://github.com/AgentOps-AI/tokencost) | Cost tracking | USD cost from Tiktoken + pricing table for 400+ LLMs | Not integrated |
| [TokenMeter](https://github.com/iyulab/TokenMeter) | Cost tracking | Session-based IUsageTracker with per-session stats | TokenUsage.accumulate() exists |
| [LiteLLM](https://docs.litellm.ai/docs/completion/token_usage) | Cost tracking | completion_cost() combining tokenizer + pricing | Not integrated |
| [OTel GenAI SIG](https://opentelemetry.io/blog/2025/ai-agent-observability/) | OTel | Stable semantic conventions for gen_ai.* attributes | Constants match spec in otel.rs |
| [OpenLLMetry](https://tokenmix.ai/blog/openllmetry-opentelemetry-for-llms-explained-2026) | OTel | Apache 2.0 OTel extension for LLM observability | Not used; Theo has custom OTel listener |
| [Red Hat](https://developers.redhat.com/articles/2026/04/06/distributed-tracing-agentic-workflows-opentelemetry) | OTel | Distributed tracing for agentic workflows with OTel | OtelExportingListener exists |
| [Hermes Agent](https://github.com/NousResearch/hermes-agent) | Trajectory | JSONL export, ShareGPT format, Atropos RL integration | RlhfRecord export exists; no full trajectory |
| [Langfuse](https://langfuse.com/docs/observability/features/token-and-cost-tracking) | Platform | Open-source, self-hosted, token + cost tracking | OTLP-compatible; not integrated |
| [Braintrust](https://www.braintrust.dev/articles/langfuse-alternatives-2026) | Platform | Best nested trace viz, CI/CD quality gates | OTLP-compatible; not integrated |
| [Helicone](https://www.helicone.ai/blog/the-complete-guide-to-LLM-observability-platforms) | Platform | Proxy-based, <1ms p99, Rust-built | Not applicable (CLI, not proxy) |
| [PostHog](https://posthog.com/llm-analytics) | Analytics | LLM Analytics dashboard, event capture API | Not integrated |
| [Pino](https://signoz.io/guides/pino-logger/) | Logging | NDJSON, worker threads, child loggers, redaction | Architecturally equivalent (JSONL + writer thread) |

---

## Thresholds and Targets

### Scoring Rubric

| Score | Definition |
|-------|-----------|
| 0.5/5 | Basic event logging, no cost tracking, no dashboards, no OTel |
| 1.0/5 | Structured JSONL logging, basic event bus, no cost/OTel |
| 2.0/5 | JSONL + event bus + trajectory reader + basic report metrics |
| 3.0/5 | OTel spans + cost tracking + trajectory export + run reports |
| 4.0/5 | OTel with proper parent-child spans + pricing table + percentile metrics + dashboard endpoints + RLHF export |
| 5.0/5 | Full-stack: real-time dashboard + analytics + audit trails + CI/CD quality gates + self-hosted observability stack |

### Current Theo Score: ~2.5/5

Theo has more than the INDEX.md's original 0.5/5 estimate suggests:

| Capability | Exists | Quality |
|------------|--------|---------|
| EventBus (sync + broadcast) | Yes | Production-grade (panic protection, bounded log) |
| TrajectoryEnvelope JSONL | Yes | Schema-versioned, backward-compatible |
| ObservabilityPipeline (background writer) | Yes | Robust (retry, drop sentinels, fsync) |
| OTel attribute constants | Yes | Match GenAI spec verbatim |
| OtelExportingListener | Yes | Functional but parent-child linkage incomplete |
| OTLP exporter (gRPC + HTTP) | Yes | Env-driven, RAII guard, feature-gated |
| RunReport (surrogate + token + loop + tool + context + memory + error metrics) | Yes | Comprehensive |
| RLHF trajectory export | Yes | DPO-compatible, rating filters |
| LoopDetector | Yes | Doom loop detection |
| Failure sensors (4 modes) | Yes | Premature termination, weak verification, task derailment, history loss |
| Per-agent metrics (MetricsByAgent) | Yes | Token, call, duration, success rate tracking |

### Gap Closure Plan to Reach 4.0+/5

| Gap | Priority | Effort | Impact |
|-----|----------|--------|--------|
| Pricing table + `estimated_cost_usd` population | P0 | 2d | +0.5 |
| Fix OTel parent-child span linkage | P0 | 1d | +0.3 |
| Add `agent.iteration` and `agent.compaction` spans | P1 | 1d | +0.2 |
| Percentile calculation (p50/p95) in RunReport | P1 | 0.5d | +0.2 |
| SSE endpoint for live event stream | P1 | 2d | +0.3 |
| Session cost metadata persistence for `--continue` | P1 | 1d | +0.2 |
| Full trajectory export (self-contained DPO pairs) | P2 | 2d | +0.2 |
| PostHog analytics listener (opt-in) | P2 | 1d | +0.1 |
| CommandHistory audit trail | P2 | 2d | +0.2 |

**Total estimated effort**: ~12.5 developer-days for +2.2 points (from ~2.5 to ~4.7).

---

## Relevance for Theo Code

### `theo-agent-runtime` Crate

| Module | Current | Action Required |
|--------|---------|-----------------|
| `observability/otel.rs` | GenAI attribute constants, `AgentRunSpan`, `MetricsByAgent` | Add `agent.iteration`, `agent.compaction` span builders |
| `observability/otel_listener.rs` | `OtelExportingListener` with span hierarchy | Fix parent-child linkage (replace `__no_parent__` with proper run_id lookup) |
| `observability/otel_exporter.rs` | OTLP exporter with env-driven config | Stable; no changes needed |
| `observability/report/metrics.rs` | Token, loop, tool, context, memory, error metrics | Add percentile calculation for tool/LLM latency |
| `observability/envelope.rs` | `TrajectoryEnvelope` with schema versioning | Stable; no changes needed |
| `observability/listener.rs` | `ObservabilityListener` with background writer | Stable; no changes needed |
| `observability/writer.rs` | Background writer thread with retry | Stable; no changes needed |
| `trajectory_export.rs` | RLHF record export with rating filters | Add `--full-trajectory` mode with self-contained DPO pairs |
| `budget_enforcer.rs` | Budget tracking without cost | Integrate pricing table for USD cost enforcement |
| `event_bus.rs` | Sync + broadcast dispatch | Add session_id correlation for cross-continue analytics |

### `theo-application` Crate

| Module | Current | Action Required |
|--------|---------|-----------------|
| `use_cases/observability_ui.rs` | `list_runs()`, run detail retrieval | Add SSE transport for live events |
| `use_cases/agents_dashboard.rs` | Agent dashboard data | Integrate per-agent cost breakdown |
| (new) `ports/http_dashboard.rs` | Does not exist | Create axum/warp HTTP server with health check + SSE + REST endpoints |

### `theo-domain` Crate

| Module | Current | Action Required |
|--------|---------|-----------------|
| `budget.rs` | `TokenUsage` with `estimated_cost_usd` field | Add `PricingTable` struct and `compute_cost()` method |

### `theo-infra-llm` Crate

| Module | Current | Action Required |
|--------|---------|-----------------|
| (provider implementations) | Returns token counts in responses | Populate `estimated_cost_usd` from pricing table after each API call |

---

## Sources

- [OpenTelemetry for AI Systems: LLM and Agent Observability (2026)](https://uptrace.dev/blog/opentelemetry-ai-systems)
- [OpenTelemetry for LLMs: Complete SRE Guide for 2026](https://openobserve.ai/blog/opentelemetry-for-llms/)
- [AI Agent Observability - Evolving Standards and Best Practices](https://opentelemetry.io/blog/2025/ai-agent-observability/)
- [Distributed tracing for agentic workflows with OpenTelemetry](https://developers.redhat.com/articles/2026/04/06/distributed-tracing-agentic-workflows-opentelemetry)
- [OpenLLMetry: OpenTelemetry for LLMs Explained (2026)](https://tokenmix.ai/blog/openllmetry-opentelemetry-for-llms-explained-2026)
- [Datadog LLM Observability natively supports OTel GenAI Semantic Conventions](https://www.datadoghq.com/blog/llm-otel-semantic-convention/)
- [Langfuse alternatives: Top 5 competitors compared (2026)](https://www.braintrust.dev/articles/langfuse-alternatives-2026)
- [Langfuse vs LangSmith vs Braintrust vs Helicone (2026)](https://appscale.blog/en/blog/langfuse-vs-langsmith-vs-braintrust-vs-helicone-2026)
- [5 AI Observability Platforms Compared: Maxim AI, Arize, Helicone, Galileo, Langfuse](https://www.getmaxim.ai/articles/5-ai-observability-platforms-compared-maxim-ai-arize-helicone-braintrust-langfuse/)
- [Best LLM Cost Tracking Tools in 2026](https://www.getmaxim.ai/articles/best-llm-cost-tracking-tools-in-2026/)
- [TokenCost by AgentOps](https://github.com/AgentOps-AI/tokencost)
- [TokenMeter](https://github.com/iyulab/TokenMeter)
- [LiteLLM Completion Token Usage & Cost](https://docs.litellm.ai/docs/completion/token_usage)
- [Langfuse Token & Cost Tracking](https://langfuse.com/docs/observability/features/token-and-cost-tracking)
- [Hermes Agent Documentation](https://hermes-agent.nousresearch.com/docs/)
- [Hermes Agent RL Training](https://hermes-agent.nousresearch.com/docs/user-guide/features/rl-training)
- [OpenDev GitHub](https://github.com/opendev-to/opendev)
- [PostHog LLM Analytics](https://posthog.com/llm-analytics)
- [PostHog: Beginner's guide to testing AI agents](https://posthog.com/blog/testing-ai-agents)
- [AI Agent Observability: A Complete Guide for 2026](https://atlan.com/know/ai-agent-observability/)
- [AI Agent Observability Guide: Telemetry, Traces, Metrics, and Evals](https://www.groundcover.com/learn/observability/ai-agent-observability)
- [Pino Logger: Complete Node.js Guide with Examples (2026)](https://signoz.io/guides/pino-logger/)
- [Building AI Coding Agents for the Terminal (arxiv)](https://arxiv.org/html/2603.05344v1)
