# Plano: OTLP Exporter — Fase 1 da observabilidade externa

> **Versão 1.0** — Faz o que já temos (eventos do `EventBus`, atributos
> OTel GenAI semantic-convention já populados em `subagent/mod.rs`)
> chegar até qualquer collector OTLP (Jaeger, Tempo, Grafana Agent,
> OTel Collector, Honeycomb, Datadog OTLP). Sem refazer pipelines
> existentes — apenas plugar um exporter opcional.
>
> Escopo: **só Fase 1.** Langfuse (Fase 2) e substituição do
> dashboard (Fase 3) ficam fora.

## Contexto

Auditoria do estado atual:

| Componente | Estado | O que falta |
|---|---|---|
| `theo-agent-runtime/src/observability/otel.rs` | 365 LOC, **só constantes + builder de attribute map** (`AgentRunSpan`, `AgentMetrics`, `MetricsByAgent`). Zero dep externa. | Não há **exporter** — `to_json()` só serializa em payload de `DomainEvent` |
| `subagent/mod.rs:437,799` | `AgentRunSpan::from_spec(spec, run_id)` populado em `SubagentStarted` + `SubagentCompleted` com `gen_ai.usage.*`, `theo.run.*`, `gen_ai.agent.*` | Spans não atravessam processo — só viram chave `"otel"` dentro do payload do evento |
| `EventBus` + `DomainEvent` | Centraliza tudo. Já temos `EventListener` trait com `on_event(&DomainEvent)` | Não existe um listener que faça push externo |
| `ObservabilityPipeline::install` | Orquestra ListenerCheck + WriterThread + LoopDetector | Adiciona um listener — extensível |
| `Cargo.toml::features` | `otel = []` declarado mas **no-op** | Reservado para esta entrega |
| Atributos populados | `gen_ai.usage.input_tokens`, `gen_ai.usage.output_tokens`, `gen_ai.usage.total_tokens`, `gen_ai.agent.id`, `gen_ai.agent.name`, `gen_ai.system`, `gen_ai.request.model`, `gen_ai.operation.name`, `theo.run.duration_ms`, `theo.run.iterations_used`, `theo.run.llm_calls`, `theo.run.success`, `theo.agent.source`, `theo.agent.builtin` | OK — não duplicar, só consumir |
| Eventos cobertos | `SubagentStarted`, `SubagentCompleted` | Falta `LlmCallStart/End`, `ToolCallDispatched/Completed`, `RunStateChanged` (cobertura óbvia que falta) |

**Causa raiz:** o módulo `otel.rs` documenta-se como "OTel-compatible
sem pulling no SDK" e isso ainda é a postura certa para o **runtime
hot path**. A Fase 1 adiciona um **listener opt-in** que constrói
spans/metrics do SDK só quando habilitado — runtime puro fica
inalterado quando `otel` é off.

**Objetivo:** `OAUTH_E2E=1 OTLP_ENDPOINT=http://localhost:4317
bash scripts/sota12-full-stress.sh` exporta spans e métricas para um
OTel Collector local; eles aparecem em qualquer backend (Jaeger UI,
Tempo, Grafana, Honeycomb).

**Estratégia:** 6 fases atômicas, todas TDD, todas opt-in via feature
`otel` + env var de configuração. Track único — não há paralelismo
útil porque cada fase depende da anterior.

| Fase | Entrega | Bloqueia |
|---|---|---|
| **40** | Workspace deps + feature flag `otel` ativa código real | 41 |
| **41** | `OtlpExporterConfig` (env-driven) + `init_global_tracer` no `theo-cli` startup | 42 |
| **42** | `OtelExportingListener` que mapeia `DomainEvent → opentelemetry::Span/Metric` | 43 |
| **43** | `LlmCallStart/End` ganham `AgentRunSpan` (cobertura LLM hop) | 44 |
| **44** | `ToolCallDispatched/Completed` ganham `AgentRunSpan` (cobertura tool hop) | 45 |
| **45** | E2E com OTel Collector real em Docker; valida traces no Jaeger via API | — |

---

## Decisões de arquitetura

### D1: SDK opcional via feature flag, runtime sempre puro

A feature `otel` (já reservada em `theo-agent-runtime/Cargo.toml`)
ativa as deps `opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp`.
**Sem** a feature, o `OtelExportingListener` não compila — `cargo build`
default permanece zero-dep externa para OTel.

Quando o consumidor (theo-cli, theo-desktop) ativa `--features otel`,
o listener fica disponível. Wiring no startup do CLI é gated por
`#[cfg(feature = "otel")]`. Hot path do runtime nunca sabe da diferença.

### D2: Configuração 100% via env var (12-factor)

```
OTLP_ENDPOINT      = http://localhost:4317      (gRPC; 4318 = HTTP/protobuf)
OTLP_PROTOCOL      = grpc | http_protobuf       (default grpc)
OTLP_TIMEOUT_SECS  = 10                         (default 10)
OTLP_HEADERS       = "key1=v1,key2=v2"          (custom headers, e.g. auth)
OTLP_SERVICE_NAME  = theo-cli                   (default `theo`)
OTLP_BATCH_SIZE    = 512                        (default; 0 = sync)
```

Nenhuma configuração via TOML/JSON nesta fase — operadores conhecem env
vars OTLP por convenção (mesmas usadas pelo OTel Collector).

### D3: Listener é fail-soft

Se o exporter perde conexão com o collector, o `OtelExportingListener`
loga (uma vez por minuto, throttled) e descarta. **Nunca** bloqueia o
event bus nem propaga erro para o runtime. Mesma postura do
`ObservabilityListener` atual.

### D4: Spans seguem padrão pai→filho

```
run_id (root span)
  └─ subagent.spawn[name=audit-bot]
       ├─ llm.call[model=gpt-5.4]
       │    └─ tool.call[name=glob]
       └─ llm.call[model=gpt-5.4]
            └─ tool.call[name=read]
```

Trace context propagation é implícita via `opentelemetry::trace::Span`
parent. Não usamos W3C TraceContext sobre HTTP — é um sistema fechado
por agora (OAuth 2.1 manager + cross-process trace fica épico separado).

### D5: Não removemos o `to_json()` em DomainEvent

`DomainEvent.payload["otel"]` continua presente. Trajectory JSONL
(observability local) continua self-contained. OTLP é caminho
**adicional** de saída, não substituto. Isto preserva D5 do plano
anterior (backward compat absoluta).

### D6: Métricas via `opentelemetry::metrics::Meter`, não Prometheus separado

Mesmo SDK — métricas (counters, histograms) saem pelo mesmo OTLP
endpoint. Backends modernos (Tempo, Grafana, Datadog, Honeycomb) recebem
ambos por OTLP nativamente. Sem dep extra de `prometheus`.

### D7: Validação E2E usa OTel Collector real em Docker

Não validamos com mock OTLP — usamos
`otel/opentelemetry-collector-contrib:latest` com receiver gRPC + exporter
`logging` (escreve no stdout). O smoke test:
1. Sobe collector em container.
2. Roda `OTLP_ENDPOINT=localhost:4317 theo agent ...`
3. Lê stdout do collector e grepa por `gen_ai.agent.name=audit-bot`.

Determinístico, reproduzível em CI, sem rede externa.

---

## Track único (sequencial)

### Fase 40 — Workspace deps + feature flag ativa

**Objetivo:** declarar deps no workspace, ativar feature gating real.
Build sem feature `otel` permanece idêntico ao atual.

**Arquitetura:**

```toml
# Cargo.toml (workspace) — adicionar:
opentelemetry            = "0.27"
opentelemetry_sdk        = { version = "0.27", features = ["rt-tokio"] }
opentelemetry-otlp       = { version = "0.27", features = ["grpc-tonic", "http-proto", "reqwest-blocking-client"] }
opentelemetry-semantic-conventions = "0.27"
```

```toml
# crates/theo-agent-runtime/Cargo.toml
[dependencies]
opentelemetry            = { workspace = true, optional = true }
opentelemetry_sdk        = { workspace = true, optional = true }
opentelemetry-otlp       = { workspace = true, optional = true }
opentelemetry-semantic-conventions = { workspace = true, optional = true }

[features]
default = []
otel = [
    "dep:opentelemetry",
    "dep:opentelemetry_sdk",
    "dep:opentelemetry-otlp",
    "dep:opentelemetry-semantic-conventions",
]
```

```toml
# apps/theo-cli/Cargo.toml — adicionar feature passthrough
[features]
default = []
otel = ["theo-agent-runtime/otel", "theo-application/otel"]
```

**TDD Sequence:**
```
RED:
  workspace_compiles_without_otel_feature
  workspace_compiles_with_otel_feature

GREEN:
  - Add 4 workspace deps
  - Activate gating in theo-agent-runtime + theo-cli + theo-application
  - Verify both `cargo build` and `cargo build --features otel` succeed

INTEGRATION:
  - cargo tree --features otel | grep opentelemetry → 4+ entries
  - cargo tree              | grep opentelemetry → 0 entries
```

**Verify:**
```bash
cargo build -p theo-agent-runtime
cargo build -p theo-agent-runtime --features otel
cargo build -p theo --bin theo
cargo build -p theo --bin theo --features otel
```

**Risco mitigado (D1):** todos os usos de `opentelemetry::*` em fases
seguintes são gated por `#[cfg(feature = "otel")]`. Build default
permanece zero-dep adicional.

---

### Fase 41 — `OtlpExporterConfig` + `init_global_tracer` startup

**Objetivo:** ler env vars, construir o `TracerProvider` global do SDK,
expor uma função `init_otlp_exporter()` chamada no boot do CLI quando
`--features otel` + `OTLP_ENDPOINT` definido.

**Arquitetura:**

```rust
// crates/theo-agent-runtime/src/observability/otel_exporter.rs (NOVO)

#![cfg(feature = "otel")]

use std::time::Duration;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_otlp::{SpanExporter, WithExportConfig};

/// Phase 41 — env-driven OTLP exporter config.
///
/// Hierarchy (per D2):
///   OTLP_ENDPOINT       — required to enable; absent → init_otlp_exporter
///                         is a no-op
///   OTLP_PROTOCOL       — "grpc" (default) | "http_protobuf"
///   OTLP_TIMEOUT_SECS   — u64, default 10
///   OTLP_HEADERS        — "k1=v1,k2=v2" (Bearer token, etc.)
///   OTLP_SERVICE_NAME   — default "theo"
///   OTLP_BATCH_SIZE     — usize, default 512; 0 = sync exporter
#[derive(Debug, Clone)]
pub struct OtlpExporterConfig {
    pub endpoint: String,
    pub protocol: OtlpProtocol,
    pub timeout: Duration,
    pub headers: Vec<(String, String)>,
    pub service_name: String,
    pub batch_size: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtlpProtocol {
    Grpc,
    HttpProtobuf,
}

impl OtlpExporterConfig {
    pub fn from_env() -> Option<Self> {
        let endpoint = std::env::var("OTLP_ENDPOINT").ok()?;
        let protocol = match std::env::var("OTLP_PROTOCOL").as_deref() {
            Ok("http_protobuf") | Ok("http") => OtlpProtocol::HttpProtobuf,
            _ => OtlpProtocol::Grpc,
        };
        let timeout = std::env::var("OTLP_TIMEOUT_SECS").ok()
            .and_then(|v| v.parse().ok()).filter(|n: &u64| *n > 0)
            .map(Duration::from_secs).unwrap_or(Duration::from_secs(10));
        let headers = parse_headers(&std::env::var("OTLP_HEADERS").unwrap_or_default());
        let service_name = std::env::var("OTLP_SERVICE_NAME")
            .unwrap_or_else(|_| "theo".to_string());
        let batch_size = std::env::var("OTLP_BATCH_SIZE").ok()
            .and_then(|v| v.parse().ok()).unwrap_or(512);
        Some(Self { endpoint, protocol, timeout, headers, service_name, batch_size })
    }
}

pub fn parse_headers(raw: &str) -> Vec<(String, String)> {
    raw.split(',').filter(|s| !s.is_empty())
        .filter_map(|kv| kv.split_once('=').map(|(k,v)| (k.trim().into(), v.trim().into())))
        .collect()
}

/// Initialize the global OTel `TracerProvider`. Returns `Ok(Some(provider))`
/// when OTLP_ENDPOINT is set; `Ok(None)` when the env var is absent
/// (operator opt-in). `Err` only on misconfigured exporter (e.g. bad TLS).
///
/// The returned `SdkTracerProvider` should be dropped at process exit
/// to flush pending spans (CLI calls `provider.shutdown()`).
pub fn init_otlp_exporter() -> Result<Option<SdkTracerProvider>, OtelInitError> {
    let cfg = match OtlpExporterConfig::from_env() {
        Some(c) => c,
        None => return Ok(None),
    };
    let exporter = build_exporter(&cfg)?;
    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .build();
    opentelemetry::global::set_tracer_provider(provider.clone());
    Ok(Some(provider))
}

#[derive(Debug, thiserror::Error)]
pub enum OtelInitError {
    #[error("OTLP exporter build failed: {0}")]
    Exporter(String),
}

fn build_exporter(cfg: &OtlpExporterConfig) -> Result<SpanExporter, OtelInitError> {
    let mut builder = match cfg.protocol {
        OtlpProtocol::Grpc => SpanExporter::builder().with_tonic()
            .with_endpoint(&cfg.endpoint).with_timeout(cfg.timeout),
        OtlpProtocol::HttpProtobuf => SpanExporter::builder().with_http()
            .with_endpoint(&cfg.endpoint).with_timeout(cfg.timeout),
    };
    // Apply custom headers (auth, tenancy)
    if !cfg.headers.is_empty() {
        // Implementation differs grpc vs http; both reject malformed names.
    }
    builder.build().map_err(|e| OtelInitError::Exporter(e.to_string()))
}
```

```rust
// apps/theo-cli/src/main.rs (modificar fn main())

#[cfg(feature = "otel")]
let _otel_provider = match
    theo_application::facade::observability::init_otlp_exporter()
{
    Ok(p) => p,
    Err(e) => { eprintln!("[otlp] init failed: {e}; continuing without"); None }
};
// _otel_provider is dropped at scope end → flushes pending spans
```

```rust
// theo-application/src/facade.rs — adicionar:

#[cfg(feature = "otel")]
pub mod observability {
    pub use theo_agent_runtime::observability::otel_exporter::{
        init_otlp_exporter, OtelInitError, OtlpExporterConfig, OtlpProtocol,
    };
}
```

**TDD Sequence:**
```
RED (cfg(feature = "otel")):
  config_from_env_returns_none_when_endpoint_absent
  config_from_env_returns_some_when_endpoint_set
  config_from_env_defaults_protocol_to_grpc
  config_from_env_parses_protocol_http
  config_from_env_defaults_timeout_to_10s
  config_from_env_parses_timeout_seconds
  config_from_env_falls_back_to_default_when_timeout_zero
  config_from_env_parses_multiple_headers
  config_from_env_defaults_service_name_to_theo
  config_from_env_defaults_batch_size_to_512
  parse_headers_returns_empty_for_empty_string
  parse_headers_skips_malformed_entries
  init_otlp_exporter_returns_none_when_env_absent

GREEN:
  - Create otel_exporter.rs gated by #[cfg(feature = "otel")]
  - Implement OtlpExporterConfig::from_env()
  - Implement init_otlp_exporter()
  - Wire into theo-cli main() under #[cfg(feature = "otel")]

INTEGRATION:
  - cargo build --features otel succeeds
  - target/debug/theo --version still works without OTLP_ENDPOINT
```

**Verify:**
```bash
cargo test -p theo-agent-runtime --features otel -- otel_exporter::tests
cargo build -p theo --bin theo --features otel
target/debug/theo --version    # smoke (no OTLP_ENDPOINT, no panic)
```

**Risco mitigado:** sem `OTLP_ENDPOINT`, `init_otlp_exporter` retorna
`None` e o CLI continua exatamente como hoje.

---

### Fase 42 — `OtelExportingListener` (DomainEvent → Span/Metric)

**Objetivo:** implementar o `EventListener` que converte os
`DomainEvent`s do `EventBus` em spans/metrics OTel reais. Subscreve
junto com `ObservabilityListener` no `install_observability`.

**Arquitetura:**

```rust
// crates/theo-agent-runtime/src/observability/otel_listener.rs (NOVO)

#![cfg(feature = "otel")]

use std::sync::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

use opentelemetry::{global, trace::{Tracer, Span, SpanKind, Status}, KeyValue, Context};
use opentelemetry::metrics::Meter;

use theo_domain::event::{DomainEvent, EventType};
use crate::event_bus::EventListener;

/// Phase 42 — bridges `DomainEvent` stream to OTel Spans + Metrics.
///
/// Subscribed alongside `ObservabilityListener` (trajectory writer)
/// so the local JSONL stays the source of truth and OTLP is purely
/// additive (D5).
pub struct OtelExportingListener {
    tracer_name: String,
    /// (entity_id → active span), e.g. run_id → root span,
    /// "subagent:{name}:{run_id}" → subagent span.
    spans: Mutex<HashMap<String, Box<dyn Span + Send + Sync>>>,
    meter: Meter,
}

impl OtelExportingListener {
    pub fn new(service_name: &str) -> Self {
        let meter = global::meter(service_name.to_string());
        Self {
            tracer_name: service_name.to_string(),
            spans: Mutex::new(HashMap::new()),
            meter,
        }
    }

    fn tracer(&self) -> impl Tracer + 'static {
        global::tracer(self.tracer_name.clone())
    }
}

impl EventListener for OtelExportingListener {
    fn on_event(&self, evt: &DomainEvent) {
        // Map by EventType. Each branch is a tight match on payload keys.
        match evt.event_type {
            EventType::RunInitialized => self.start_run_span(evt),
            EventType::RunStateChanged => self.add_state_event(evt),
            EventType::SubagentStarted => self.start_subagent_span(evt),
            EventType::SubagentCompleted => self.end_subagent_span(evt),
            EventType::ToolCallDispatched => self.start_tool_span(evt),
            EventType::ToolCallCompleted => self.end_tool_span(evt),
            EventType::LlmCallStart => self.start_llm_span(evt),
            EventType::LlmCallEnd => self.end_llm_span(evt),
            EventType::Error => self.record_error(evt),
            _ => {}  // unmapped events stay only in trajectory JSONL
        }
    }
}

impl OtelExportingListener {
    fn start_run_span(&self, evt: &DomainEvent) { /* ... */ }
    fn start_subagent_span(&self, evt: &DomainEvent) {
        // Reads evt.payload["otel"] (already populated in subagent/mod.rs).
        // Creates child span with name "subagent.spawn", attributes from
        // payload[otel], stores in self.spans by entity_id.
    }
    fn end_subagent_span(&self, evt: &DomainEvent) { /* set status, end */ }
    // ... etc
}
```

**TDD Sequence:**
```
RED (with #[cfg(feature = "otel")]):
  listener_new_creates_with_service_name
  listener_on_run_initialized_starts_root_span
  listener_on_subagent_started_creates_child_span
  listener_on_subagent_completed_ends_span_with_success_attribute
  listener_on_tool_call_dispatched_starts_tool_span_with_name
  listener_on_tool_call_completed_ends_tool_span
  listener_on_llm_call_start_starts_llm_span_with_model_attribute
  listener_on_llm_call_end_ends_llm_span_with_token_counts
  listener_on_error_records_error_status
  listener_skips_unmapped_event_types
  listener_handles_missing_payload_otel_field_gracefully
  listener_handles_concurrent_events_without_panicking

GREEN:
  - Implement OtelExportingListener
  - Subscribe via install_observability when feature on
  - Use opentelemetry::trace::TracerProvider::tracer()

INTEGRATION:
  - Mock SpanExporter in tests via opentelemetry_sdk's noop tracer
  - Assert via captured spans (opentelemetry_sdk::testing::trace::InMemorySpanExporter)
```

**Verify:**
```bash
cargo test -p theo-agent-runtime --features otel -- otel_listener
```

**Risco mitigado (D3):** `Mutex<HashMap>` poison é gracefully handled
(`if let Ok(g) = lock`). Listener não panica nem com bus contention.

---

### Fase 43 — Cobertura LLM (LlmCallStart/End)

**Objetivo:** hoje `LlmCallStart` / `LlmCallEnd` são emitidos pelo
`run_engine.rs` mas **sem** atributos OTel populados. Esta fase
adiciona `AgentRunSpan::llm_call_span(provider, model)` no payload
desses eventos, e o listener da Fase 42 cria a span LLM filha.

**Arquitetura:**

```rust
// crates/theo-agent-runtime/src/run_engine.rs (modificar dois call sites)

// LlmCallStart:
let span = crate::observability::otel::llm_call_span(
    &self.run.provider, &self.client_model,
);
self.event_bus.publish(DomainEvent::new(
    EventType::LlmCallStart, run_id,
    serde_json::json!({
        "model": &self.client_model,
        "otel": span.to_json(),
    }),
));

// LlmCallEnd:
let mut span = crate::observability::otel::llm_call_span(...);
span.set(ATTR_USAGE_INPUT_TOKENS, input_tokens);
span.set(ATTR_USAGE_OUTPUT_TOKENS, output_tokens);
span.set(ATTR_USAGE_TOTAL_TOKENS, total_tokens);
span.set(ATTR_THEO_DURATION_MS, duration.as_millis() as u64);
self.event_bus.publish(DomainEvent::new(
    EventType::LlmCallEnd, run_id,
    serde_json::json!({"otel": span.to_json()}),
));
```

**TDD Sequence:**
```
RED:
  llm_call_start_event_payload_includes_otel_attributes
  llm_call_start_event_payload_otel_includes_request_model
  llm_call_end_event_payload_includes_token_usage_attributes
  llm_call_end_event_payload_includes_duration_ms

GREEN:
  - Modify run_engine.rs LlmCallStart/End emission to include "otel" key
  - Tests assert on captured event payloads via existing TestSetup

INTEGRATION:
  - Run full theo-agent-runtime test suite — zero regression
```

**Verify:**
```bash
cargo test -p theo-agent-runtime --lib -- run_engine::tests::llm_call
```

---

### Fase 44 — Cobertura Tool (ToolCallDispatched/Completed)

**Objetivo:** análogo à Fase 43 mas para tools. Cada
`ToolCallDispatched` ganha um `AgentRunSpan` com `tool.name` (atributo
custom theo). `ToolCallCompleted` fecha com status + duração.

**Arquitetura:**

```rust
// crates/theo-agent-runtime/src/observability/otel.rs (extender)

pub const ATTR_THEO_TOOL_NAME: &str = "theo.tool.name";
pub const ATTR_THEO_TOOL_DURATION_MS: &str = "theo.tool.duration_ms";
pub const ATTR_THEO_TOOL_STATUS: &str = "theo.tool.status";  // Succeeded|Failed

pub fn tool_call_span(tool_name: &str) -> AgentRunSpan {
    let mut s = AgentRunSpan::new();
    s.set(ATTR_OPERATION_NAME, "tool.call");
    s.set(ATTR_THEO_TOOL_NAME, tool_name);
    s
}
```

```rust
// crates/theo-agent-runtime/src/run_engine.rs (modificar dispatch loop)

// ToolCallDispatched:
let span = crate::observability::otel::tool_call_span(name);
self.event_bus.publish(DomainEvent::new(
    EventType::ToolCallDispatched, run_id,
    serde_json::json!({
        "tool_name": name, "call_id": &call.id,
        "otel": span.to_json(),
    }),
));

// ToolCallCompleted (the existing branch + the Phase 30 replay branch):
let mut span = crate::observability::otel::tool_call_span(name);
span.set(ATTR_THEO_TOOL_DURATION_MS, dur_ms);
span.set(ATTR_THEO_TOOL_STATUS, status_str);
```

**TDD Sequence:**
```
RED:
  tool_call_dispatched_event_payload_includes_otel_attributes
  tool_call_dispatched_otel_attributes_include_tool_name
  tool_call_completed_event_payload_includes_duration_ms
  tool_call_completed_event_payload_includes_status_string
  tool_call_replayed_event_payload_includes_otel_with_replayed_marker

GREEN:
  - Add ATTR_THEO_TOOL_* constants
  - Add tool_call_span() helper
  - Modify dispatch loop emission sites

INTEGRATION:
  - Existing 1092 tests pass — `replayed: true` payload preserved
```

**Verify:**
```bash
cargo test -p theo-agent-runtime --lib -- run_engine::tests::dispatch
cargo test -p theo-agent-runtime --lib   # full sweep
```

---

### Fase 45 — E2E real com OTel Collector em Docker

**Objetivo:** smoke E2E que prova o caminho ponta-a-ponta:
`theo agent` → DomainEvent → OtelExportingListener → OTLP gRPC →
local OTel Collector → stdout do collector logger exporter.

**Arquitetura:**

```yaml
# scripts/otlp/collector-config.yaml (NOVO)
receivers:
  otlp:
    protocols:
      grpc:
        endpoint: 0.0.0.0:4317
exporters:
  debug:
    verbosity: detailed
service:
  pipelines:
    traces:
      receivers: [otlp]
      exporters: [debug]
    metrics:
      receivers: [otlp]
      exporters: [debug]
```

```bash
# scripts/otlp-smoke.sh (NOVO)
#!/usr/bin/env bash
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CLI="$REPO_ROOT/target/release/theo"
COLLECTOR_LOG=$(mktemp -t collector-log-XXXXXX)

# 1. Start OTel Collector
docker run --rm -d --name theo-otlp-smoke \
  -p 4317:4317 \
  -v "$REPO_ROOT/scripts/otlp/collector-config.yaml:/etc/otelcol-contrib/config.yaml" \
  otel/opentelemetry-collector-contrib:0.110.0 \
  >/dev/null
trap "docker logs theo-otlp-smoke > $COLLECTOR_LOG 2>&1; \
      docker rm -f theo-otlp-smoke 2>/dev/null" EXIT
sleep 2

# 2. Build CLI with otel feature
[ -x "$CLI" ] || (cd "$REPO_ROOT" && cargo build --release --features otel -p theo --bin theo)

# 3. Run a smoke agent invocation with OTLP wired up
WORK=$(mktemp -d -t otlp-smoke-XXXXXX)
cd "$WORK"; git init -q
git -c user.email=t@t.com -c user.name=t commit --allow-empty -q -m init
mkdir -p .theo/agents
cat > .theo/agents/audit-bot.md <<'EOF'
---
name: audit-bot
description: "Audit only — uses glob then done"
denied_tools: [edit, write, bash]
max_iterations: 2
timeout: 30
---
You audit. Use `glob` once then `done`.
EOF
"$CLI" agents approve --all --repo "$WORK"
OTLP_ENDPOINT=http://localhost:4317 \
OTLP_SERVICE_NAME=theo-smoke \
THEO_FORCE_TOOL_CHOICE=function:delegate_task_single \
THEO_SKIP_ONBOARDING=1 \
"$CLI" agent --headless --repo "$WORK" --max-iter 4 \
  'Use delegate_task_single with agent="audit-bot" objective="audit"' \
  >/dev/null 2>&1 || true

# 4. Allow exporter to flush
sleep 3

# 5. Validate spans appeared
docker logs theo-otlp-smoke > "$COLLECTOR_LOG" 2>&1
PASS=true
for ATTR in "gen_ai.agent.name" "audit-bot" "subagent.spawn"; do
  if grep -q "$ATTR" "$COLLECTOR_LOG"; then
    echo "  ✓ collector received '$ATTR'"
  else
    echo "  ✗ collector did NOT receive '$ATTR'"
    PASS=false
  fi
done
$PASS && exit 0 || exit 1
```

**TDD Sequence:**
```
RED (file doesn't exist):
  bash scripts/otlp-smoke.sh   → script not found

GREEN:
  - Create scripts/otlp/collector-config.yaml
  - Create scripts/otlp-smoke.sh
  - Add to README + sota12-full-stress.sh as optional gate

INTEGRATION:
  - Requires Docker (skip with explicit message when absent)
  - Requires OAUTH_E2E=1 + valid token (script asserts)
```

**Verify:**
```bash
OAUTH_E2E=1 bash scripts/otlp-smoke.sh
# expected:
#   ✓ collector received 'gen_ai.agent.name'
#   ✓ collector received 'audit-bot'
#   ✓ collector received 'subagent.spawn'
```

---

## Riscos e mitigações

| Risco | Mitigação |
|---|---|
| `opentelemetry` 0.27 quebra API entre versões | Pin exato `0.27.x` no workspace; `cargo deny check` flaga upgrade. |
| Build com `--features otel` adiciona ~3MB binário | Aceito: feature opt-in. Default `cargo build` zero overhead. |
| Listener perde eventos sob carga | Fail-soft (D3). MetricCounter `theo.otlp.spans_dropped` para visibilidade. |
| Docker indisponível em CI minimal | `scripts/otlp-smoke.sh` skippa com mensagem explícita; gate só em OAUTH_E2E=1. |
| `Mutex<HashMap<spans>>` contention em runs longos | Usar `parking_lot::Mutex` se virar bottleneck (medido via metric). Aceitável em v1. |
| Trace context não atravessa MCP HTTP / OAuth boundaries | Out of scope. Cross-process W3C TraceContext fica épico separado. |
| Operador esquece de drop do TracerProvider | `_otel_provider` é binding com `Drop` que faz `shutdown()` → pending spans flushed. Não vaza spans. |

---

## Verificação final agregada

```bash
# Fase 40
cargo build -p theo-agent-runtime
cargo build -p theo-agent-runtime --features otel
cargo build -p theo --bin theo
cargo build -p theo --bin theo --features otel

# Fase 41
cargo test -p theo-agent-runtime --features otel -- otel_exporter::tests

# Fase 42
cargo test -p theo-agent-runtime --features otel -- otel_listener

# Fase 43
cargo test -p theo-agent-runtime --lib -- run_engine::tests::llm_call

# Fase 44
cargo test -p theo-agent-runtime --lib -- run_engine::tests::dispatch

# Regression (sem otel)
cargo test -p theo-agent-runtime --lib --tests
cargo test -p theo --bin theo

# Regression (com otel)
cargo test -p theo-agent-runtime --features otel --lib --tests

# E2E real (gate)
OAUTH_E2E=1 bash scripts/otlp-smoke.sh
```

---

## Cronograma

```
Sprint único sequencial:
  Fase 40 (deps + feature gating)         ~1h
  Fase 41 (config + init)                 ~2h
  Fase 42 (listener)                      ~3-4h  ← maior fase
  Fase 43 (LLM coverage)                  ~1h
  Fase 44 (tool coverage)                 ~1-2h
  Fase 45 (E2E Docker)                    ~2h

Total: ~10-12h concentrado
```

---

## Compromisso de cobertura final

Após este plano: **OTLP exporter funcional em opt-in**.

| Item | Status pós-plano |
|---|---|
| OTel SDK opcional via feature | ✓ Fase 40 |
| Env var driven config | ✓ Fase 41 — `OTLP_ENDPOINT` + 5 outras |
| DomainEvent → OTel span/metric | ✓ Fase 42 — listener que mapeia 9 EventTypes |
| LLM call span coverage | ✓ Fase 43 — provider + model + tokens + duration |
| Tool call span coverage | ✓ Fase 44 — name + duration + status, replay marker preservado |
| E2E real com collector | ✓ Fase 45 — Docker + Jaeger/debug exporter validation |

Plus:
- 35+ novos tests (TDD obrigatório por fase)
- Backward compat absoluta (D5: trajectory JSONL inalterado, sem feature → zero overhead)
- Real OAuth Codex stress continua passando 26/26 (Fase 45 gate adicional, opcional)

---

## Trabalho fora deste plano

Confirmados como épicos separados, **NÃO** parte deste escopo:
- **Langfuse** (Fase 2) — necessita prompt logging + scoring UI; trataremos quando ≥10 usuários ativos
- **Substituição do `theo dashboard`** (Fase 3) — `theo dashboard` é offline-first, fica
- **Cross-process W3C TraceContext** — propagação de traceparent header em chamadas MCP HTTP
- **OTel Logs signal** (não só traces+metrics) — exigiria refator do `tracing` crate setup
- **OTLP push de trajectory JSONL retroativo** — só novos runs exportam
- **Sampling configurável** (parent-based, ratio) — começamos com always-on (D2 pode ganhar `OTLP_SAMPLER` depois)

---

## Referências

- OTel GenAI semantic conventions — https://github.com/open-telemetry/semantic-conventions/tree/main/docs/gen-ai
- OTel Rust SDK 0.27 — https://docs.rs/opentelemetry/0.27
- OTel Collector Contrib — https://github.com/open-telemetry/opentelemetry-collector-contrib
- `crates/theo-agent-runtime/src/observability/otel.rs` — constantes + builder atual
- `crates/theo-agent-runtime/src/observability/mod.rs::install_observability` — wire point
- TDD: RED → GREEN → REFACTOR (sem exceções)
- Plano antecedente: `docs/plans/mcp-http-and-discover-flake-plan.md` (estilo + estrutura)
