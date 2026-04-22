# Agent Observability Engine — Plano Executavel

**ADR**: `docs/adr/ADR-009-agent-observability-engine.md`
**Meeting**: `.claude/meetings/20260422-150959-agent-observability-engine.md`
**Branch base**: `develop` (eb3ec33)

---

## Dependencias entre Phases

```
Phase 0 ──▶ Phase 1 ──▶ Phase 2 ──▶ Phase 3
                │                       │
                │                       ▼
                │               Phase 3.1 (summary writer)
                │                       │
                │                       ▼
                │               Phase 6 (Dashboard)
                │
                ▼
          Phase 4 (blocked by RFC)
                │
                ▼
          Phase 5
```

Phase 0 e pre-requisito de tudo.
Phases 4 e 5 podem rodar em paralelo apos Phase 1.
Phase 3 depende de Phase 2 (metricas computam sobre projection).
Phase 6 depende de Phase 3 (dashboard consome metricas derivadas).

---

## Phase 0: Foundation

### T0.1 — Add `TrajectoryId` + `EventKind` to `theo-domain`

**O que**: Adicionar newtype `TrajectoryId` e enum `EventKind` ao crate de dominio.

**Arquivos**:
- `crates/theo-domain/src/identifiers.rs` — adicionar `define_identifier!(TrajectoryId, ...)`
- `crates/theo-domain/src/event.rs` — adicionar `EventKind` enum + `EventType::kind()` method

**TDD**:
1. RED: `test_trajectory_id_generate_is_unique()` — dois generates != entre si
2. RED: `test_trajectory_id_new_rejects_empty()` — panic em string vazia
3. RED: `test_event_kind_mapping_is_exhaustive()` — todo `EventType` variant retorna um `EventKind`
4. RED: `test_event_kind_is_deterministic()` — mesmo EventType sempre retorna mesmo EventKind
5. RED: `test_event_kind_lifecycle_variants()` — TaskCreated, TaskStateChanged, RunInitialized, RunStateChanged → Lifecycle
6. RED: `test_event_kind_streaming_excluded_from_trajectory()` — ContentDelta, ReasoningDelta → Streaming
7. GREEN: Implementar macro + enum + match
8. REFACTOR: Nenhum esperado
9. VERIFY: `cargo test -p theo-domain`

**Criterios de aceite**:
- [ ] `TrajectoryId::generate()` produz IDs unicos (mesmo formato que RunId/TaskId)
- [ ] `TrajectoryId::new("")` faz panic
- [ ] `EventKind` tem 6 variants: `Lifecycle, Tooling, Reasoning, Context, Failure, Streaming`
- [ ] `EventKind` e `#[non_exhaustive]`
- [ ] `EventType::kind()` cobre TODOS os 22 variants sem `_ => unreachable!()`
- [ ] `EventKind` implementa `Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq`
- [ ] Zero warnings em `cargo clippy -p theo-domain`

**DoD**: 7 testes passando, zero clippy warnings, `cargo test -p theo-domain` green.

---

### T0.2 — Restructure observability as module directory

**O que**: Converter `observability.rs` de arquivo unico para diretorio `observability/mod.rs`. Mover `metrics.rs` e `context_metrics.rs` como submodulos. Preservar todas as APIs publicas sem breaking changes.

**Arquivos**:
- `crates/theo-agent-runtime/src/observability.rs` → `crates/theo-agent-runtime/src/observability/mod.rs`
- `crates/theo-agent-runtime/src/metrics.rs` → `crates/theo-agent-runtime/src/observability/metrics.rs`
- `crates/theo-agent-runtime/src/context_metrics.rs` → `crates/theo-agent-runtime/src/observability/context_metrics.rs`
- `crates/theo-agent-runtime/src/lib.rs` — atualizar declaracoes de modulo + re-exports

**TDD**:
1. NENHUM teste novo necessario — refactor puro
2. VERIFY: `cargo test -p theo-agent-runtime` — TODOS os 530+ testes existentes passam
3. VERIFY: `cargo build --workspace` — nenhum breaking change em consumidores

**Criterios de aceite**:
- [ ] Diretorio `observability/` existe com `mod.rs`, `metrics.rs`, `context_metrics.rs`
- [ ] `lib.rs` declara `pub mod observability` (nao mais `metrics` e `context_metrics` separados)
- [ ] Re-exports preservam API publica: `use theo_agent_runtime::observability::metrics::*` funciona
- [ ] Re-exports de compatibilidade: `pub use observability::metrics;` e `pub use observability::context_metrics;` em `lib.rs`
- [ ] Todos os 530+ testes existentes passam sem modificacao
- [ ] `cargo build --workspace` sem erros
- [ ] `cargo clippy --workspace` sem warnings novos

**DoD**: Zero testes quebrados, zero erros de compilacao no workspace, diff mostra apenas moves + re-exports.

---

## Phase 1: Async Writer Pipeline

### T1.1 — ObservabilityListener with mpsc channel

**O que**: Implementar `ObservabilityListener` que implementa `EventListener`, filtra `Streaming` events, serializa `&DomainEvent` para bytes, e envia via `mpsc::SyncSender`. Satisfaz INV-1 (at_least_observed) parcialmente.

**Arquivos**:
- `crates/theo-agent-runtime/src/observability/listener.rs` (novo)
- `crates/theo-agent-runtime/src/observability/mod.rs` — adicionar `pub mod listener`

**TDD**:
1. RED: `test_listener_filters_streaming_events()` — ContentDelta e ReasoningDelta nao chegam ao channel
2. RED: `test_listener_sends_non_streaming_events()` — ToolCallCompleted chega ao channel
3. RED: `test_listener_on_event_is_nonblocking()` — medir duracao de on_event < 1ms para 1000 events
4. RED: `test_listener_counts_dropped_events()` — channel capacity=1, burst 100 events, `dropped_events()` > 0
5. RED: `test_listener_counts_serialization_errors()` — (se aplicavel — DomainEvent sempre serializa, pode ser edge case)
6. GREEN: Struct com `SyncSender<Vec<u8>>`, `AtomicU64` dropped/serialization counters
7. REFACTOR: Extrair constantes (channel capacity default = 4096)
8. VERIFY: `cargo test -p theo-agent-runtime`

**Criterios de aceite**:
- [ ] `ObservabilityListener::new(sender, dropped_counter, serialization_counter)` — construtor
- [ ] Implementa `EventListener` trait (Send + Sync)
- [ ] `on_event` retorna em < 1ms mesmo com channel cheio (try_send, nao send)
- [ ] Eventos `Streaming` (ContentDelta, ReasoningDelta) sao filtrados — NUNCA entram no channel
- [ ] `dropped_events: &AtomicU64` incrementa em cada `try_send` que falha
- [ ] Serializa com `serde_json::to_vec(&event)` — zero clones de `serde_json::Value`
- [ ] Thread-safe: pode ser compartilhado via `Arc<ObservabilityListener>`

**DoD**: 5 testes passando, INV-1 test contract satisfeito, cargo clippy clean.

---

### T1.2 — Background writer thread with JSONL envelope

**O que**: Thread OS dedicada que drena o `mpsc::Receiver<Vec<u8>>`, escreve cada evento com envelope (v, seq, ts, run_id, kind, event_kind) em JSONL. Satisfaz INV-3 (per_run_ordering).

**Arquivos**:
- `crates/theo-agent-runtime/src/observability/writer.rs` (novo)
- `crates/theo-agent-runtime/src/observability/envelope.rs` (novo)

**TDD**:
1. RED: `test_writer_creates_file_at_expected_path()` — `.theo/trajectories/{run_id}.jsonl`
2. RED: `test_envelope_contains_all_required_fields()` — v, seq, ts, run_id, kind, event_kind presentes em cada linha
3. RED: `test_sequence_numbers_are_strictly_monotonic()` — publish 10 events, parse JSONL, assert seq 0..9
4. RED: `test_schema_version_is_1()` — todos os `v` fields == 1
5. RED: `test_event_kind_field_matches_event_type()` — ToolCallCompleted → "Tooling"
6. RED: `test_flush_every_100_lines()` — mock writer que conta flush() calls
7. RED: `test_graceful_shutdown_fsyncs()` — drop sender, verificar que writer drena e faz fsync
8. GREEN: `spawn_writer_thread(receiver, run_id, base_path) -> JoinHandle<()>`
9. REFACTOR: Extrair `write_envelope()` como funcao pura testavel independente
10. VERIFY: `cargo test -p theo-agent-runtime`

**Criterios de aceite**:
- [ ] Writer roda em `std::thread::spawn` (NAO tokio::spawn)
- [ ] Usa `BufWriter<File>` para eficiencia
- [ ] Cada linha JSONL contem: `{"v":1,"seq":N,"ts":M,"run_id":"...","kind":"event","event_type":"...","event_kind":"...","entity_id":"...","payload":{...},"dropped_since_last":0}`
- [ ] `seq` e estritamente monotonic (0, 1, 2, ...)
- [ ] `flush()` chamado a cada 100 linhas
- [ ] No shutdown (channel closed): flush + sync_data (fsync)
- [ ] Path: `.theo/trajectories/{run_id}.jsonl` — cria diretorio se nao existir
- [ ] INV-3 test contract satisfeito

**DoD**: 7 testes passando, JSONL parseable com `serde_json::from_str` em cada linha.

---

### T1.3 — Drop detection sentinel (INV-2)

**O que**: Writer thread detecta eventos dropados via `AtomicU64` counter e escreve sentinel line no JSONL.

**Arquivos**:
- `crates/theo-agent-runtime/src/observability/writer.rs` — estender writer loop

**TDD**:
1. RED: `test_drop_sentinel_written_when_events_dropped()` — channel capacity=2, burst 50, parse JSONL, assert exists line with `"kind":"drop_sentinel"`
2. RED: `test_drop_sentinel_contains_dropped_count()` — parse sentinel, assert `dropped_count` > 0
3. RED: `test_drop_sentinel_has_correct_sequence()` — sentinel sequence is monotonic with surrounding events
4. RED: `test_no_sentinel_when_no_drops()` — normal flow, assert zero sentinel lines
5. GREEN: Check `dropped_counter.swap(0)` before each write, emit sentinel if > 0
6. VERIFY: `cargo test -p theo-agent-runtime`

**Criterios de aceite**:
- [ ] Sentinel format: `{"v":1,"seq":N,"ts":M,"run_id":"...","kind":"drop_sentinel","dropped_count":K}`
- [ ] `dropped_count` reflete exatamente quantos eventos foram perdidos desde o ultimo write
- [ ] Sentinel ocupa um sequence number (incrementa seq)
- [ ] Se nao houve drops, nenhum sentinel e escrito
- [ ] INV-2 test contract satisfeito: `dropped_count + received_count == total_published`

**DoD**: 4 testes passando, INV-2 formalmente verificado.

---

### T1.4 — Writer failure recovery (INV-4)

**O que**: Writer thread lida com erros de I/O sem perder eventos. Retry queue bounded, recovery sentinel.

**Arquivos**:
- `crates/theo-agent-runtime/src/observability/writer.rs` — estender error handling

**TDD**:
1. RED: `test_writer_retries_on_io_error()` — mock writer que falha em writes 3-5, assert todos os 10 eventos aparecem no output final
2. RED: `test_writer_recovery_sentinel_emitted()` — apos recovery, parse JSONL, assert `"kind":"writer_recovered"` com `buffered_events` correto
3. RED: `test_retry_queue_bounded_at_100()` — mock writer que falha 150 vezes seguidas, assert retry_queue.len() <= 100 e excess falls back to drop counter
4. RED: `test_write_errors_counter_incremented()` — mock writer failing, assert `write_errors` AtomicU64 > 0
5. GREEN: Bounded VecDeque<Vec<u8>> retry queue, drain on recovery
6. VERIFY: `cargo test -p theo-agent-runtime`

**Criterios de aceite**:
- [ ] I/O error nao causa panic nem drop silencioso
- [ ] Eventos que falham vao para retry queue (capacity=100)
- [ ] Na proxima write com sucesso: drain retry queue primeiro, depois write recovery sentinel
- [ ] Recovery sentinel: `{"v":1,"seq":N,...,"kind":"writer_recovered","buffered_events":K,"error":"..."}`
- [ ] Se retry queue overflow: incrementa dropped_events (falls back to INV-2)
- [ ] `write_errors: AtomicU64` acessivel para diagnostico externo
- [ ] INV-4 test contract satisfeito

**DoD**: 4 testes passando, INV-4 formalmente verificado, zero silent event loss.

---

### T1.5 — Crash recovery on read

**O que**: Reader de JSONL que tolera ultima linha truncada (crash mid-write) e reporta integridade.

**Arquivos**:
- `crates/theo-agent-runtime/src/observability/reader.rs` (novo)

**TDD**:
1. RED: `test_reader_parses_valid_jsonl()` — 10 linhas validas, assert 10 envelopes parsed
2. RED: `test_reader_tolerates_truncated_last_line()` — 9 validas + 1 truncada, assert 9 parsed + integrity.complete == false
3. RED: `test_reader_detects_sequence_gaps()` — seq 0,1,2,5,6 → missing_sequences = [3..5]
4. RED: `test_reader_counts_drop_sentinels()` — 2 sentinel lines → drop_sentinels_found == 2
5. RED: `test_reader_counts_writer_recoveries()` — 1 recovery line → writer_recoveries_found == 1
6. RED: `test_reader_computes_confidence()` — 8 of 10 expected events → confidence == 0.8
7. RED: `test_reader_handles_empty_file()` — empty file → 0 events, complete == false, confidence == 0.0
8. GREEN: `read_trajectory(path) -> Result<(Vec<TrajectoryEnvelope>, IntegrityReport), Error>`
9. VERIFY: `cargo test -p theo-agent-runtime`

**Criterios de aceite**:
- [ ] Parseia linhas validas, ignora ultima se truncada
- [ ] Detecta gaps em sequence numbers → `missing_sequences`
- [ ] Conta sentinels (drop e recovery) separadamente
- [ ] `IntegrityReport` com todos os campos populados
- [ ] `confidence = 1.0 - (missing / expected)` clamped a [0.0, 1.0]
- [ ] Nunca panic em input corrompido

**DoD**: 7 testes passando, reader robusto contra qualquer JSONL malformado.

---

### T1.6 — Replace StructuredLogListener

**O que**: Substituir `StructuredLogListener` por `ObservabilityListener` + writer thread em todos os call sites. One-shot migration.

**Arquivos**:
- `crates/theo-agent-runtime/src/observability/mod.rs` — remover ou deprecar `StructuredLogListener`
- `crates/theo-agent-runtime/src/observability/structured_log.rs` — manter como legacy com `#[deprecated]`
- Todos os call sites que constroem `StructuredLogListener` — substituir
- `crates/theo-agent-runtime/src/run_engine.rs` — wiring do novo listener

**TDD**:
1. RED: `test_run_engine_produces_trajectory_jsonl()` — integrar com run_engine mock, verificar que `.theo/trajectories/{run_id}.jsonl` e criado
2. GREEN: Wire ObservabilityListener no setup do run_engine
3. VERIFY: `cargo test -p theo-agent-runtime` — todos os 530+ testes passam
4. VERIFY: `cargo build --workspace` — zero breaking changes

**Criterios de aceite**:
- [ ] `StructuredLogListener` marcado `#[deprecated(note = "Use ObservabilityListener")]`
- [ ] Nenhum call site novo usa `StructuredLogListener`
- [ ] `ObservabilityListener` wired como default listener no run_engine
- [ ] Todos os testes existentes passam (zero regressions)
- [ ] Old JSONL files (sem envelope) nao sao afetados — coexistem em diretorio diferente

**DoD**: Migration completa, zero tests quebrados, `StructuredLogListener` deprecated.

---

## Phase 2: Projection Model

### T2.1 — ProjectedStep + StepOutcome + TrajectoryProjection types

**O que**: Definir as structs de projecao no modulo observability. Serde roundtrip.

**Arquivos**:
- `crates/theo-agent-runtime/src/observability/projection.rs` (novo)

**TDD**:
1. RED: `test_projected_step_serde_roundtrip()` — serialize + deserialize == original
2. RED: `test_step_outcome_all_variants_serialize()` — Success, Failure{retryable:true}, Timeout, Skipped
3. RED: `test_trajectory_projection_serde_roundtrip()` — full struct
4. RED: `test_payload_summary_truncated_at_500_chars()` — payload de 1000 chars → summary de 500
5. GREEN: Structs com derives
6. VERIFY: `cargo test -p theo-agent-runtime`

**Criterios de aceite**:
- [ ] `ProjectedStep` com todos os campos do ADR Spec 3
- [ ] `StepOutcome` enum com 4 variants
- [ ] `TrajectoryProjection` com run_id, trajectory_id, steps, metrics, integrity
- [ ] Todos implementam `Serialize, Deserialize, Debug, Clone`
- [ ] `payload_summary` truncado a 500 chars

**DoD**: 4 testes passando, tipos usaveis para projection function.

---

### T2.2 — Projection function

**O que**: Funcao pura `project(run_id, events) -> TrajectoryProjection`. Satisfaz P1 (determinism) e P4 (out-of-order tolerance).

**Arquivos**:
- `crates/theo-agent-runtime/src/observability/projection.rs` — adicionar `pub fn project()`

**TDD**:
1. RED: `test_projection_deterministic()` — mesmos events, dois calls → resultado identico (exceto trajectory_id)
2. RED: `test_projection_sorts_by_timestamp_then_sequence()` — events fora de ordem → steps em ordem
3. RED: `test_projection_extracts_tool_name_from_tool_call_events()` — ToolCallCompleted payload com tool_name
4. RED: `test_projection_computes_duration_for_tool_calls()` — ToolCallQueued + ToolCallCompleted → duration_ms
5. RED: `test_projection_maps_step_outcome_from_tool_state()` — Succeeded→Success, Failed→Failure, Timeout→Timeout
6. RED: `test_projection_empty_events_returns_empty_steps()` — edge case
7. RED: proptest `prop_projection_deterministic()` — random events always produce same projection
8. GREEN: Implementar project()
9. VERIFY: `cargo test -p theo-agent-runtime`

**Criterios de aceite**:
- [ ] P1 (Determinism): mesma entrada → mesma saida (exceto trajectory_id)
- [ ] P4 (Out-of-order tolerance): sort por (timestamp, sequence) antes de processar
- [ ] Extrai `tool_name` de payloads de `ToolCallCompleted`
- [ ] Computa `duration_ms` para tool calls com timestamps pareados
- [ ] Mapeia `ToolCallState` → `StepOutcome`
- [ ] Funcao pura: nenhum side effect, nenhum I/O

**DoD**: 7 testes passando (incluindo proptest), P1 e P4 formalmente verificados.

---

### T2.3 — IntegrityReport + confidence computation

**O que**: Computar IntegrityReport durante projecao. Satisfaz P3 (tolerance to missing events).

**Arquivos**:
- `crates/theo-agent-runtime/src/observability/projection.rs` — estender project()

**TDD**:
1. RED: `test_integrity_complete_when_no_gaps()` — 10 events seq 0..9 → complete = true, confidence = 1.0
2. RED: `test_integrity_incomplete_when_gap_detected()` — seq 0,1,5,6 → complete = false, missing_sequences = [2..5]
3. RED: `test_integrity_confidence_degrades_with_missing()` — 8 of 10 → confidence = 0.8
4. RED: `test_integrity_counts_drop_sentinels()` — 2 drop sentinels in events → drop_sentinels_found = 2
5. RED: `test_integrity_confidence_clamped_0_to_1()` — edge case: 0 expected → confidence = 0.0
6. GREEN: Logica de gap detection + confidence computation
7. VERIFY: `cargo test -p theo-agent-runtime`

**Criterios de aceite**:
- [ ] `IntegrityReport` com todos os campos do ADR
- [ ] Gaps detectados por descontinuidades em sequence numbers
- [ ] `confidence = 1.0 - (missing / expected)` clamped a [0.0, 1.0]
- [ ] Drop sentinels e writer recoveries contados separadamente
- [ ] `schema_version` = 1 hardcoded em v1
- [ ] P3 formalmente verificado: missing events → confidence degrada, never panic

**DoD**: 5 testes passando, P3 verificado.

---

### T2.4 — Idempotence verification (P2)

**O que**: Verificar que project → serialize → deserialize → project produz resultado identico.

**Arquivos**:
- `crates/theo-agent-runtime/src/observability/projection.rs` — teste adicional

**TDD**:
1. RED: `test_projection_idempotent_through_serde()` — project(events) → to_json → from_json → compare fields
2. RED: proptest `prop_projection_idempotent()` — random events, roundtrip always equal
3. GREEN: Garantir que todos os campos sobrevivem roundtrip (f64 precision, etc)
4. VERIFY: `cargo test -p theo-agent-runtime`

**Criterios de aceite**:
- [ ] P2 (Idempotence) formalmente verificado
- [ ] f64 fields sobrevivem roundtrip sem perda de precisao significativa (epsilon < 1e-10)

**DoD**: 2 testes passando (incluindo proptest), P2 verificado.

---

## Phase 3: Derived Metrics

### T3.1 — SurrogateMetric type

**O que**: Definir `SurrogateMetric` e `DerivedMetrics` structs.

**Arquivos**:
- `crates/theo-agent-runtime/src/observability/derived_metrics.rs` (novo)

**TDD**:
1. RED: `test_surrogate_metric_serde_roundtrip()` — serialize + deserialize
2. RED: `test_surrogate_metric_always_marked_surrogate()` — is_surrogate == true
3. RED: `test_derived_metrics_default_all_zero()` — Default impl has all values 0.0
4. GREEN: Structs com derives + Default
5. VERIFY: `cargo test -p theo-agent-runtime`

**Criterios de aceite**:
- [ ] `SurrogateMetric` com value, confidence, numerator, denominator, is_surrogate, caveat
- [ ] `DerivedMetrics` com 5 campos (doom_loop_frequency, llm_efficiency, context_waste_ratio, hypothesis_churn_rate, time_to_first_tool_ms)
- [ ] `caveat` e `&'static str` — lifetime estatico, sem allocacao
- [ ] Serde roundtrip funciona (caveat serializa como string)

**DoD**: 3 testes passando.

---

### T3.2 — doom_loop_frequency metric

**O que**: Computar doom_loop_frequency de uma lista de ProjectedSteps.

**TDD**:
1. RED: `test_doom_loop_zero_when_no_repetitions()` — 10 tool calls distintos → 0.0
2. RED: `test_doom_loop_detects_identical_calls()` — 3 identicos em 10 → 0.3 (na window)
3. RED: `test_doom_loop_window_size_10()` — repeticao no step 0 e step 15 nao detectada (fora da window)
4. RED: `test_doom_loop_zero_denominator()` — 0 tool calls → value=0.0, confidence=0.0
5. RED: `test_doom_loop_caveat_present()` — caveat nao e string vazia
6. GREEN: `compute_doom_loop_frequency(steps: &[ProjectedStep]) -> SurrogateMetric`
7. VERIFY: `cargo test -p theo-agent-runtime`

**Criterios de aceite**:
- [ ] Formula: repetitions_in_window / total_tool_calls
- [ ] Sliding window de W=10
- [ ] Hash usa tool_name + payload_summary (proxy para args no projection level)
- [ ] Edge case: 0 tool calls → value=0.0, confidence=0.0
- [ ] `caveat` preenchido conforme ADR

**DoD**: 5 testes passando.

---

### T3.3 — llm_efficiency metric

**TDD**:
1. RED: `test_llm_efficiency_perfect_run()` — 5 distinct successful tools / 5 LLM calls → 1.0
2. RED: `test_llm_efficiency_no_tools()` — 5 LLM calls, 0 tools → 0.0
3. RED: `test_llm_efficiency_no_llm_calls()` — 0 LLM calls → 0.0, confidence=0.0
4. RED: `test_llm_efficiency_duplicate_tools_not_counted()` — same tool called 3x, 3 LLM calls → 1/3
5. GREEN: `compute_llm_efficiency(steps: &[ProjectedStep]) -> SurrogateMetric`
6. VERIFY: `cargo test -p theo-agent-runtime`

**Criterios de aceite**:
- [ ] Formula: distinct_successful_tool_pairs / total_llm_calls
- [ ] Distinct = (tool_name, outcome=Success) unique pairs
- [ ] Edge cases com denominador zero tratados

**DoD**: 4 testes passando.

---

### T3.4 — context_waste_ratio metric

**TDD**:
1. RED: `test_context_waste_zero_overflows()` — 0 overflow events, 10 iterations → 0.0
2. RED: `test_context_waste_high_overflow()` — 5 overflows, 10 iterations → 0.5
3. RED: `test_context_waste_no_iterations()` — 0 iterations → 0.0, confidence=0.0
4. GREEN: `compute_context_waste_ratio(steps: &[ProjectedStep]) -> SurrogateMetric`
5. VERIFY: `cargo test -p theo-agent-runtime`

**DoD**: 3 testes passando.

---

### T3.5 — hypothesis_churn_rate metric

**TDD**:
1. RED: `test_churn_rate_no_hypotheses()` — 0 formed → 0.0, confidence=0.0
2. RED: `test_churn_rate_no_invalidations()` — 5 formed, 0 invalidated → 0.0
3. RED: `test_churn_rate_all_invalidated()` — 3 formed, 3 invalidated → 1.0
4. RED: `test_churn_rate_more_invalidated_than_formed()` — 2 formed, 4 invalidated → 2.0 (valid)
5. GREEN: `compute_hypothesis_churn_rate(steps: &[ProjectedStep]) -> SurrogateMetric`
6. VERIFY: `cargo test -p theo-agent-runtime`

**DoD**: 4 testes passando.

---

### T3.6 — time_to_first_tool_ms metric

**TDD**:
1. RED: `test_ttft_normal_case()` — RunInitialized at t=1000, first ToolCallDispatched at t=1500 → 500ms
2. RED: `test_ttft_no_tool_calls()` — RunInitialized at t=1000, last event at t=5000 → 4000ms (total duration)
3. RED: `test_ttft_no_run_initialized()` — edge case → 0ms, confidence=0.0
4. GREEN: `compute_time_to_first_tool(steps: &[ProjectedStep]) -> SurrogateMetric`
5. VERIFY: `cargo test -p theo-agent-runtime`

**DoD**: 3 testes passando.

---

### T3.7 — Metrics aggregator + summary line writer

**O que**: Agregar todas as 5 metricas, computar DerivedMetrics, escrever summary como ultima linha do JSONL.

**Arquivos**:
- `crates/theo-agent-runtime/src/observability/derived_metrics.rs` — `compute_all()`
- `crates/theo-agent-runtime/src/observability/writer.rs` — summary line no shutdown

**TDD**:
1. RED: `test_compute_all_returns_all_5_metrics()` — assert todos os 5 campos preenchidos
2. RED: `test_compute_all_with_empty_steps()` — tudo 0.0, confidence 0.0
3. RED: `test_summary_line_is_last_in_jsonl()` — parse JSONL, last line has `"kind":"summary"`
4. RED: `test_summary_line_contains_metrics_and_integrity()` — parse summary, assert DerivedMetrics + IntegrityReport presentes
5. RED: `test_confidence_degraded_by_integrity()` — integrity.confidence=0.5 → all metric confidences *= 0.5
6. GREEN: `compute_all(steps, integrity) -> DerivedMetrics` + wire no writer shutdown
7. VERIFY: `cargo test -p theo-agent-runtime`

**Criterios de aceite**:
- [ ] `compute_all()` chama as 5 funcoes individuais
- [ ] Multiplica cada metric.confidence por integrity.confidence
- [ ] Summary line e SEMPRE a ultima linha do JSONL
- [ ] Summary line escrita atomicamente (temp file → append → fsync conforme ADR Spec 7)

**DoD**: 5 testes passando, summary line integrada ao writer.

---

### T3.8 — Token & cost metrics

**O que**: Extrair breakdown de tokens e custo do AgentResult + RuntimeMetrics para o RunReport.

**Metricas**:
- `input_tokens: u64`
- `output_tokens: u64`
- `cache_read_tokens: u64`
- `cache_write_tokens: u64`
- `reasoning_tokens: u64`
- `total_cost_usd: f64`
- `cache_hit_rate: f64` — cache_read_tokens / (input_tokens + cache_read_tokens), safe_div → 0.0
- `tokens_per_successful_edit: f64` — total_tokens / successful_edits, safe_div → 0.0

**TDD**:
1. RED: `test_token_breakdown_all_fields_populated()`
2. RED: `test_cache_hit_rate_zero_when_no_cache()`
3. RED: `test_cache_hit_rate_computed_correctly()` — 200 cache_read, 800 input → 0.2
4. RED: `test_tokens_per_edit_zero_when_no_edits()`
5. RED: `test_cost_usd_accumulated_correctly()`
6. GREEN: `compute_token_metrics(result: &AgentResult, token_usage: &TokenUsage) -> TokenMetrics`
7. VERIFY: `cargo test -p theo-agent-runtime`

**DoD**: 5 testes passando.

---

### T3.9 — Agent loop phase metrics

**O que**: Capturar distribuicao de tempo e contagem por fase do loop (Explore/Edit/Verify/Done).

**Metricas**:
- `phase_distribution: HashMap<LoopPhase, PhaseMetrics>` onde `PhaseMetrics { iterations: u32, duration_ms: u64, pct: f64 }`
- `total_iterations: u32`
- `done_blocked_count: u32` — vezes que o agente tentou convergir mas git diff vazio
- `convergence_rate: f64` — converged_runs / total_runs
- `budget_utilization: BudgetUtilization` — `{ iterations_pct: f64, tokens_pct: f64, time_pct: f64 }`
- `evolution_attempts: u32` — tentativas com estrategias de correcao
- `evolution_success: bool` — se alguma tentativa teve sucesso

**TDD**:
1. RED: `test_phase_distribution_sums_to_100_pct()`
2. RED: `test_done_blocked_tracked()`
3. RED: `test_budget_utilization_correct()` — 50 of 200 iterations → 25%
4. RED: `test_evolution_attempts_counted()`
5. GREEN: `compute_loop_metrics(loop_state: &ContextLoopState, budget: &BudgetUsage, evolution: Option<&EvolutionLoop>) -> LoopMetrics`
6. VERIFY: `cargo test -p theo-agent-runtime`

**DoD**: 4 testes passando.

---

### T3.10 — Per-tool breakdown metrics

**O que**: Metricas individuais por tool_name extraidas dos ProjectedSteps.

**Metricas** por tool:
- `call_count: u32`
- `success_count: u32`
- `failure_count: u32`
- `avg_latency_ms: f64`
- `max_latency_ms: u64`
- `retry_count: u32`
- `success_rate: f64`

**TDD**:
1. RED: `test_per_tool_counts_correct()` — 5 read_file (4 success, 1 fail) → counts match
2. RED: `test_per_tool_latency_computed()` — known durations → avg correct
3. RED: `test_per_tool_sorted_by_call_count()` — most used first
4. RED: `test_per_tool_empty_when_no_tools()`
5. GREEN: `compute_tool_breakdown(steps: &[ProjectedStep]) -> Vec<ToolBreakdown>`
6. VERIFY: `cargo test -p theo-agent-runtime`

**DoD**: 4 testes passando.

---

### T3.11 — Context & compaction metrics

**O que**: Metricas de tamanho de contexto e eficiencia de compactacao.

**Metricas**:
- `avg_context_size_tokens: f64` — media de tokens por iteracao
- `max_context_size_tokens: u64` — pico
- `context_growth_rate: f64` — (final_size - initial_size) / iterations
- `compaction_count: u32` — quantas vezes compactou
- `compaction_savings_ratio: f64` — 1.0 - (tokens_after / tokens_before), media
- `refetch_rate: f64` — artefatos re-buscados / total fetches (direto do ContextMetrics)
- `action_repetition_rate: f64` — acoes repetidas / total acoes (direto do ContextMetrics)
- `usefulness_avg: f64` — media dos usefulness scores por community

**TDD**:
1. RED: `test_context_growth_positive_when_growing()`
2. RED: `test_compaction_savings_correct()` — 10k before, 3k after → 0.7
3. RED: `test_refetch_rate_from_context_metrics()` — round-trip from ContextMetrics
4. RED: `test_usefulness_avg_computed()` — 3 communities [0.5, 0.8, 0.2] → avg 0.5
5. GREEN: `compute_context_metrics(ctx: &ContextMetrics, compaction_events: &[...]) -> ContextHealthMetrics`
6. VERIFY: `cargo test -p theo-agent-runtime`

**DoD**: 4 testes passando.

---

### T3.12 — Memory & episode metrics

**O que**: Metricas de memoria e episodios usados durante o run.

**Metricas**:
- `episodes_injected: u32` — episodios injetados no prompt
- `episodes_created: u32` — episodios gerados neste run
- `hypotheses_formed: u32` — hipoteses criadas
- `hypotheses_invalidated: u32` — hipoteses descartadas
- `hypotheses_active: u32` — hipoteses ainda ativas no final
- `constraints_learned: u32` — constraints gerados
- `failure_fingerprints_new: u32` — novos fingerprints este run
- `failure_fingerprints_recurrent: u32` — fingerprints que ja tinham sido vistos antes

**TDD**:
1. RED: `test_episode_counts_from_events()`
2. RED: `test_hypothesis_counts_from_events()`
3. RED: `test_constraints_counted_from_events()`
4. RED: `test_fingerprints_new_vs_recurrent()`
5. GREEN: `compute_memory_metrics(steps: &[ProjectedStep], tracker: &FailurePatternTracker) -> MemoryMetrics`
6. VERIFY: `cargo test -p theo-agent-runtime`

**DoD**: 4 testes passando.

---

### T3.13 — RunReport aggregate + update compute_all

**O que**: Compor todas as metricas em um unico `RunReport` e atualizar `compute_all()` e o summary line.

**Struct**:
```rust
pub struct RunReport {
    pub surrogate_metrics: DerivedMetrics,    // 5 metricas originais
    pub token_metrics: TokenMetrics,          // T3.8
    pub loop_metrics: LoopMetrics,            // T3.9
    pub tool_breakdown: Vec<ToolBreakdown>,   // T3.10
    pub context_health: ContextHealthMetrics, // T3.11
    pub memory_metrics: MemoryMetrics,        // T3.12
    pub integrity: IntegrityReport,           // T2.3
}
```

**TDD**:
1. RED: `test_run_report_serde_roundtrip()`
2. RED: `test_run_report_all_sections_populated()`
3. RED: `test_summary_line_contains_full_run_report()` — JSONL summary agora tem RunReport completo
4. GREEN: Compor todas as sub-funcoes, atualizar writer
5. VERIFY: `cargo test -p theo-agent-runtime`

**Criterios de aceite**:
- [ ] RunReport agrega TODAS as categorias de metricas
- [ ] Summary line do JSONL contem RunReport completo (nao so DerivedMetrics)
- [ ] Serde roundtrip funciona
- [ ] Dashboard (Phase 6) consome RunReport

**DoD**: 3 testes passando, summary line atualizada.

---

## Phase 4: Loop Detection (BLOCKED ate RFC)

> **Pre-requisito**: RFC de loop detection aprovado definindo tipos, thresholds, whitelist.

### T4.1 — ToolNormalizer trait + per-tool normalizers

**O que**: Trait de normalizacao + implementacoes por classe de tool conforme ADR Spec 5.

**Arquivos**:
- `crates/theo-agent-runtime/src/observability/normalizer.rs` (novo)

**TDD**:
1. RED: `test_bash_normalizer_strips_ansi()` — input com ANSI → output sem ANSI
2. RED: `test_bash_normalizer_replaces_temp_paths()` — `/tmp/abc123` → `/tmp/<TEMP>`
3. RED: `test_bash_normalizer_replaces_timestamps()` — ISO + Unix timestamps substituidos
4. RED: `test_bash_normalizer_replaces_pids()` — `pid=12345` → `pid=<PID>`
5. RED: `test_read_file_normalizer_keeps_only_path()` — strips line_start, line_end do args
6. RED: `test_edit_file_normalizer_hashes_content()` — diferentes conteudos → diferentes hashes
7. RED: `test_default_normalizer_hashes_full_args()` — unknown tool → hash de todo o JSON
8. RED: `test_normalizer_deterministic()` — mesmo input, mesmo hash
9. GREEN: Trait + 7 implementacoes
10. VERIFY: `cargo test -p theo-agent-runtime`

**Criterios de aceite**:
- [ ] `ToolNormalizer` trait com `normalize_args()` e `normalize_output()`
- [ ] 7 implementacoes conforme tabela do ADR
- [ ] Bash normalizer aplica todas as 8 regras do ADR
- [ ] Hashing via xxhash64 (ou hash padrao do Rust — avaliar dep)
- [ ] Deterministic: mesmo input → mesmo output, always

**DoD**: 8 testes passando.

---

### T4.2 — LoopDetector with sliding window

**O que**: Detector de loop com sliding window e verdicts escalonados.

**Arquivos**:
- `crates/theo-agent-runtime/src/observability/loop_detector.rs` (novo) — ou estender `reflector.rs`

**TDD**:
1. RED: `test_no_loop_with_distinct_calls()` — 10 diferentes → Ok
2. RED: `test_warning_at_2_consecutive()` — 2 identicos → Warning
3. RED: `test_correct_at_3_consecutive()` — 3 identicos → Correct
4. RED: `test_hard_stop_at_5_consecutive()` — 5 identicos → HardStop
5. RED: `test_counter_resets_on_different_call()` — 2 identical, 1 different, 2 identical → max Warning
6. RED: `test_window_size_is_10()` — match com item 11 passos atras nao detectado
7. RED: `test_result_aware_detection()` — same tool+args but different output → no loop
8. GREEN: `LoopDetector::record()` conforme ADR
9. VERIFY: `cargo test -p theo-agent-runtime`

**Criterios de aceite**:
- [ ] Sliding window de 10 items (VecDeque)
- [ ] Verdicts: Ok (0-1), Warning (2), Correct (3-4), HardStop (5+)
- [ ] Result-aware: hash inclui output, nao so args
- [ ] Counter reseta quando call diferente aparece

**DoD**: 7 testes passando.

---

### T4.3 — Expected sequence whitelist

**O que**: Pares de tool sequences que NAO devem triggerar loop detection.

**TDD**:
1. RED: `test_write_then_read_not_flagged()` — write_file → read_file (same path) → Ok, nao Warning
2. RED: `test_edit_then_bash_not_flagged()` — edit_file → bash → Ok
3. RED: `test_unknown_pair_still_flagged()` — grep → grep → Warning (nao esta na whitelist)
4. GREEN: Checar `EXPECTED_SEQUENCES` antes de incrementar counter
5. VERIFY: `cargo test -p theo-agent-runtime`

**Criterios de aceite**:
- [ ] 5 pares whitelisted conforme ADR
- [ ] Whitelist aplicada somente quando B segue imediatamente A
- [ ] Pares nao-whitelisted continuam sendo detectados normalmente

**DoD**: 3 testes passando.

---

### T4.4 — LoopDetector integration with reflector

**O que**: Integrar LoopDetector no HeuristicReflector existente. Emitir DomainEvents de loop detection.

**Arquivos**:
- `crates/theo-agent-runtime/src/reflector.rs` — estender com LoopDetector
- `crates/theo-agent-runtime/src/run_engine.rs` — wire no loop principal

**TDD**:
1. RED: `test_reflector_emits_corrective_guidance_on_loop()` — 3 calls identicos → guidance string retornada
2. RED: `test_reflector_loop_verdict_correct_contains_message()` — mensagem especifica sobre repeticao
3. GREEN: Instanciar LoopDetector dentro de HeuristicReflector, chamar `.record()` a cada tool call
4. VERIFY: `cargo test -p theo-agent-runtime`

**DoD**: 2 testes passando, LoopDetector integrado sem breaking changes no reflector API.

---

## Phase 5: Failure Taxonomy

### T5.1 — FM-3: PrematureTermination sensor

**O que**: Detectar runs que convergem sem fazer edits.

**Arquivos**:
- `crates/theo-agent-runtime/src/observability/failure_sensors.rs` (novo)

**TDD**:
1. RED: `test_premature_termination_detected()` — run converge, 0 edits, 3 iterations, no budget exceeded → true
2. RED: `test_not_premature_if_edits_made()` — 1 edit success → false
3. RED: `test_not_premature_if_budget_exceeded()` — 0 edits but BudgetExceeded → false
4. RED: `test_not_premature_if_single_iteration()` — total_iterations < 2 → false
5. GREEN: `detect_premature_termination(steps: &[ProjectedStep]) -> bool`
6. VERIFY: `cargo test -p theo-agent-runtime`

**Criterios de aceite**:
- [ ] Predicado exato conforme ADR FM-3
- [ ] Avaliado no run exit (nao durante execucao)
- [ ] Retorna bool — evento emitido pelo caller, nao pelo sensor

**DoD**: 4 testes passando.

---

### T5.2 — FM-4: WeakVerification sensor

**TDD**:
1. RED: `test_weak_verification_detected()` — edit success, no bash/sensor in next 3 iterations → true
2. RED: `test_verification_present_clears_flag()` — edit + bash within 3 → false
3. RED: `test_sensor_execution_counts_as_verification()` — edit + SensorExecuted within 3 → false
4. RED: `test_multiple_edits_each_checked()` — 2 edits, 1 verified, 1 not → true (at least one weak)
5. GREEN: `detect_weak_verification(steps: &[ProjectedStep]) -> bool`
6. VERIFY: `cargo test -p theo-agent-runtime`

**DoD**: 4 testes passando.

---

### T5.3 — FM-5: TaskDerailment sensor

**TDD**:
1. RED: `test_derailment_detected()` — 5 consecutive tool calls with no initial_context files → true
2. RED: `test_no_derailment_when_context_files_used()` — tool calls reference initial files → false
3. RED: `test_no_derailment_if_preceded_by_overflow_recovery()` — ContextOverflowRecovery before sequence → false
4. RED: `test_initial_context_from_first_retrieval()` — extract files from RetrievalExecuted payload
5. GREEN: `detect_task_derailment(steps: &[ProjectedStep], initial_context: &HashSet<String>) -> bool`
6. VERIFY: `cargo test -p theo-agent-runtime`

**DoD**: 4 testes passando.

---

### T5.4 — FM-6: ConversationHistoryLoss sensor

**TDD**:
1. RED: `test_history_loss_detected()` — overflow event, then re-read of hot file within 3 iterations → true
2. RED: `test_no_loss_without_overflow()` — re-read but no ContextOverflowRecovery → false
3. RED: `test_no_loss_when_new_files_read()` — read file NOT in hot_files pre-compaction → false
4. GREEN: `detect_conversation_history_loss(steps: &[ProjectedStep], pre_compaction_hot_files: &HashSet<String>) -> bool`
5. VERIFY: `cargo test -p theo-agent-runtime`

**Criterios de aceite**:
- [ ] Predicado exato conforme ADR FM-6
- [ ] Requer `pre_compaction_hot_files` como input (caller deve rastrear)
- [ ] Window de 3 iterations apos ContextOverflowRecovery

**DoD**: 3 testes passando.

---

### T5.5 — Extend FailurePattern enum + integrate sensors

**O que**: Adicionar 4 novos variants ao `FailurePattern` e chamar sensors no run exit.

**Arquivos**:
- `crates/theo-agent-runtime/src/reflector.rs` — extend FailurePattern
- `crates/theo-agent-runtime/src/run_engine.rs` — call sensors at session exit

**TDD**:
1. RED: `test_failure_pattern_has_6_variants()` — match exhaustivo funciona com 6 variants
2. RED: `test_run_exit_evaluates_all_sensors()` — mock run com premature termination + weak verification → ambos detectados
3. GREEN: Extend enum, wire sensors
4. VERIFY: `cargo test -p theo-agent-runtime`

**Criterios de aceite**:
- [ ] `FailurePattern` tem 6 variants: NoProgressLoop, RepeatedSameError, PrematureTermination, WeakVerification, TaskDerailment, ConversationHistoryLoss
- [ ] `#[non_exhaustive]` no enum
- [ ] Sensors avaliados em `record_session_exit()` ou equivalente
- [ ] Failures detectados emitidos como `DomainEvent(EventType::Error, payload: {"failure_mode": "..."})`

**DoD**: 2 testes passando, enum estendido, sensors wired.

---

## Phase 6: Dashboard (Visualizacao)

> **Dependencias**: Phase 3 (metricas derivadas computadas), Phase 1 (writer produz JSONL)
> **Stack**: React 18 + TypeScript + Tailwind + Radix UI + Framer Motion + Recharts
> **Local**: `apps/theo-ui/src/features/observability/`
> **Pagina existente**: `MonitoringPage.tsx` (stub vazio — sera substituido)

### T6.1 — Tauri commands: expor trajectories para o frontend

**O que**: Criar comandos Tauri (`invoke`) no backend Rust que leem JSONL e retornam dados estruturados para o frontend. Sem comandos, o frontend nao tem acesso aos dados.

**Arquivos**:
- `apps/theo-desktop/src-tauri/src/commands/observability.rs` (novo)
- `apps/theo-desktop/src-tauri/src/commands/mod.rs` — registrar novos comandos

**Comandos**:
- `list_runs() -> Vec<RunSummary>` — lista runs com metricas resumidas (do summary line do JSONL)
- `get_run_trajectory(run_id: String) -> TrajectoryProjection` — trajectory completa de um run
- `get_run_metrics(run_id: String) -> DerivedMetrics` — apenas metricas derivadas
- `compare_runs(run_ids: Vec<String>) -> Vec<DerivedMetrics>` — metricas lado a lado

**TDD**:
1. RED: `test_list_runs_returns_empty_when_no_trajectories()` — dir vazio → vec vazio
2. RED: `test_list_runs_parses_summary_from_jsonl()` — 2 JSONL files → 2 RunSummary
3. RED: `test_get_run_trajectory_returns_projection()` — JSONL valido → TrajectoryProjection com steps
4. RED: `test_get_run_trajectory_not_found_returns_error()` — run_id invalido → erro tipado
5. GREEN: Implementar comandos usando `reader.rs` (T1.5) e `projection.rs` (T2.2)
6. VERIFY: `cargo test -p theo-desktop`

**Criterios de aceite**:
- [ ] 4 comandos Tauri registrados e invocaveis via `invoke()`
- [ ] Comandos usam reader + projection do modulo observability (nao reimplementam)
- [ ] Erros retornam mensagem legivel (nao panic)
- [ ] `RunSummary` inclui: run_id, timestamp, success, total_steps, total_tool_calls, doom_loop_frequency, llm_efficiency

**DoD**: 4 testes passando, comandos acessiveis via Tauri IPC.

---

### T6.2 — TypeScript types + IPC contract

**O que**: Definir tipos TypeScript que espelham os tipos Rust do observability. Usar `ts-rs` ou definir manualmente com validacao cruzada.

**Arquivos**:
- `apps/theo-ui/src/features/observability/types.ts` (novo)

**Tipos**:
```typescript
interface RunSummary {
  run_id: string;
  timestamp: number;
  success: boolean;
  total_steps: number;
  total_tool_calls: number;
  duration_ms: number;
  metrics: DerivedMetrics;
}

interface DerivedMetrics {
  doom_loop_frequency: SurrogateMetric;
  llm_efficiency: SurrogateMetric;
  context_waste_ratio: SurrogateMetric;
  hypothesis_churn_rate: SurrogateMetric;
  time_to_first_tool_ms: SurrogateMetric;
}

interface SurrogateMetric {
  value: number;
  confidence: number;
  numerator: number;
  denominator: number;
  is_surrogate: boolean;
  caveat: string;
}

interface ProjectedStep {
  sequence: number;
  event_type: string;
  event_kind: EventKind;
  timestamp: number;
  entity_id: string;
  payload_summary: string;
  duration_ms: number | null;
  tool_name: string | null;
  outcome: StepOutcome | null;
}

type EventKind = "Lifecycle" | "Tooling" | "Reasoning" | "Context" | "Failure" | "Streaming";
type StepOutcome = "Success" | { Failure: { retryable: boolean } } | "Timeout" | "Skipped";

interface IntegrityReport {
  complete: boolean;
  total_events_expected: number;
  total_events_received: number;
  missing_sequences: Array<{ start: number; end: number }>;
  drop_sentinels_found: number;
  writer_recoveries_found: number;
  confidence: number;
  schema_version: number;
}

interface TrajectoryProjection {
  run_id: string;
  trajectory_id: string;
  steps: ProjectedStep[];
  metrics: DerivedMetrics;
  integrity: IntegrityReport;
}
```

**Criterios de aceite**:
- [ ] Todos os tipos Rust do observability tem equivalente TypeScript
- [ ] Discriminated unions para enums (EventKind, StepOutcome)
- [ ] Nullability alinhada com `Option<T>` do Rust
- [ ] Arquivo exporta tudo — single import point

**DoD**: Tipos compilam com `tsc --noEmit`, alinham com output real dos comandos Tauri.

---

### T6.3 — Observability data hook + state

**O que**: Custom React hook `useObservability()` que carrega dados via Tauri invoke e gerencia estado local.

**Arquivos**:
- `apps/theo-ui/src/features/observability/hooks/useObservability.ts` (novo)

**API**:
```typescript
function useObservability() {
  return {
    runs: RunSummary[];           // Lista de runs
    selectedRun: TrajectoryProjection | null;
    loading: boolean;
    error: string | null;
    loadRuns: () => Promise<void>;
    selectRun: (runId: string) => Promise<void>;
    compareRuns: (runIds: string[]) => Promise<DerivedMetrics[]>;
  };
}
```

**TDD** (Vitest):
1. RED: `test_useObservability_starts_with_empty_state()`
2. RED: `test_loadRuns_populates_runs_array()`
3. RED: `test_selectRun_fetches_trajectory()`
4. RED: `test_error_state_on_invoke_failure()`
5. GREEN: Hook com useState + invoke calls
6. VERIFY: `cd apps/theo-ui && npm test`

**Criterios de aceite**:
- [ ] Hook encapsula TODOS os invoke calls — componentes nunca chamam invoke diretamente
- [ ] Loading state durante fetch
- [ ] Error state com mensagem legivel
- [ ] Re-render minimo (nao refetch desnecessario)

**DoD**: 4 testes passando (Vitest + React Testing Library ou mock de invoke).

---

### T6.4 — Dashboard page: metrics cards

**O que**: Substituir `MonitoringPage.tsx` stub por dashboard real. Primeiro componente: 5 metric cards mostrando as surrogate metrics do run selecionado.

**Arquivos**:
- `apps/theo-ui/src/features/observability/pages/ObservabilityDashboard.tsx` (novo)
- `apps/theo-ui/src/features/observability/components/MetricCard.tsx` (novo)
- `apps/theo-ui/src/features/monitoring/pages/MonitoringPage.tsx` — redirecionar para dashboard
- `apps/theo-ui/src/app/routes.tsx` — atualizar rota

**Design**:
```
┌─────────────────────────────────────────────────────────┐
│  Agent Observability                    [Run selector ▼] │
├──────────┬──────────┬──────────┬──────────┬─────────────┤
│ Doom Loop│ LLM Eff. │ Ctx Waste│ Hyp Churn│ Time to 1st │
│   0.12   │   0.85   │   0.03   │   0.40   │   1,250ms   │
│ ░░▓░░░░░ │ ████████░│ ░░░░░░░░ │ ░░░░▓░░░ │ ░░░███░░░░  │
│ conf: 92%│ conf: 98%│ conf:100%│ conf: 87%│ conf: 100%  │
└──────────┴──────────┴──────────┴──────────┴─────────────┘
```

Cada MetricCard mostra:
- Nome da metrica
- Valor numerico (formatado)
- Barra de progresso colorida (verde=bom, amarelo=atencao, vermelho=problema)
- Confidence badge
- Tooltip com caveat (do SurrogateMetric)

**TDD**:
1. RED: `test_metric_card_renders_value()`
2. RED: `test_metric_card_shows_confidence_badge()`
3. RED: `test_metric_card_tooltip_shows_caveat()`
4. RED: `test_dashboard_renders_5_cards()`
5. RED: `test_dashboard_shows_empty_state_when_no_runs()`
6. GREEN: Componentes com Tailwind + Radix Tooltip
7. VERIFY: `cd apps/theo-ui && npm test`

**Criterios de aceite**:
- [ ] 5 metric cards visualmente distintos com cores semanticas
- [ ] Confidence < 50% mostra warning visual (borda amarela)
- [ ] Confidence < 20% mostra alerta visual (borda vermelha)
- [ ] Hover no card mostra caveat completo via Radix Tooltip
- [ ] Run selector dropdown lista runs disponiveis por data
- [ ] Empty state quando nenhum run disponivel
- [ ] Responsivo (cards empilham em tela pequena)

**DoD**: 5 testes passando, dashboard renderiza com dados reais via Tauri.

---

### T6.5 — Timeline view: step-by-step execution

**O que**: Visualizacao temporal dos steps de um run. Cada step e uma barra horizontal com cor por EventKind.

**Arquivos**:
- `apps/theo-ui/src/features/observability/components/TimelineView.tsx` (novo)
- `apps/theo-ui/src/features/observability/components/TimelineStep.tsx` (novo)

**Design**:
```
Timeline: Run 01HXR...
──────────────────────────────────────────────
00:00  ● RunInitialized               [Lifecycle]
00:02  ● LlmCallStart                 [Context]
00:15  ● LlmCallEnd (13s)             [Context]
00:15  ● ToolCallDispatched: read_file [Tooling]
00:15  ● ToolCallCompleted (42ms) ✓   [Tooling]
00:16  ● HypothesisFormed             [Reasoning]
00:16  ● ToolCallDispatched: edit_file [Tooling]
00:17  ● ToolCallCompleted (850ms) ✓  [Tooling]
00:17  ● SensorExecuted ✓             [Tooling]
00:30  ⚠ LoopDetected (Warning)       [Failure]
00:45  ● RunStateChanged → Converged  [Lifecycle]
──────────────────────────────────────────────
```

Cores por EventKind:
- `Lifecycle` → cinza
- `Tooling` → azul
- `Reasoning` → roxo
- `Context` → amarelo
- `Failure` → vermelho

**TDD**:
1. RED: `test_timeline_renders_steps_in_order()`
2. RED: `test_timeline_step_shows_event_kind_color()`
3. RED: `test_timeline_step_shows_duration_when_available()`
4. RED: `test_timeline_step_shows_outcome_icon()` — ✓ para success, ✗ para failure
5. RED: `test_timeline_highlights_failure_steps()`
6. GREEN: Componentes com Framer Motion para animacao de entrada
7. VERIFY: `cd apps/theo-ui && npm test`

**Criterios de aceite**:
- [ ] Steps renderizados em ordem cronologica
- [ ] Cada step tem: timestamp relativo, icone de outcome, event_type, event_kind badge colorido
- [ ] Tool calls mostram tool_name e duration_ms
- [ ] Failure steps (EventKind::Failure) destacados em vermelho
- [ ] Click em step expande payload_summary
- [ ] Scroll virtual para runs com 500+ steps (performance)

**DoD**: 5 testes passando, timeline renderiza com dados reais.

---

### T6.6 — Tool usage chart

**O que**: Grafico de barras mostrando distribuicao de tool calls por tool_name. Requer adicionar `recharts` como dependencia.

**Arquivos**:
- `apps/theo-ui/src/features/observability/components/ToolUsageChart.tsx` (novo)
- `apps/theo-ui/package.json` — adicionar `recharts`

**Design**:
```
Tool Usage (23 calls)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
read_file    ████████████  12  (9 ✓, 3 ✗)
edit_file    ████          4   (3 ✓, 1 ✗)
bash         ███           3   (3 ✓)
grep         ██            2   (2 ✓)
write_file   █             1   (1 ✓)
glob         █             1   (1 ✓)
```

**TDD**:
1. RED: `test_tool_chart_renders_bars_per_tool()`
2. RED: `test_tool_chart_shows_success_failure_split()`
3. RED: `test_tool_chart_sorted_by_count_descending()`
4. RED: `test_tool_chart_empty_when_no_tool_calls()`
5. GREEN: Recharts BarChart horizontal com stacked success/failure
6. VERIFY: `cd apps/theo-ui && npm test`

**Criterios de aceite**:
- [ ] Barras horizontais, ordenadas por contagem decrescente
- [ ] Cada barra split em success (verde) e failure (vermelho)
- [ ] Hover mostra contagem exata
- [ ] Header mostra total de tool calls
- [ ] Empty state quando nenhuma tool call

**DoD**: 4 testes passando, `recharts` adicionado ao package.json.

---

### T6.7 — Failure mode indicators

**O que**: Badges/alertas mostrando quais failure modes foram detectados no run.

**Arquivos**:
- `apps/theo-ui/src/features/observability/components/FailureModePanel.tsx` (novo)

**Design**:
```
Failure Analysis
┌──────────────────────┬──────────────────────────┐
│ ✓ NoProgressLoop     │ Not detected             │
│ ✓ RepeatedSameError  │ Not detected             │
│ ⚠ PrematureTermination│ Agent converged with 0 edits │
│ ⚠ WeakVerification   │ 2 edits without verification │
│ ✓ TaskDerailment     │ Not detected             │
│ ✓ HistoryLoss        │ Not detected             │
└──────────────────────┴──────────────────────────┘
```

**TDD**:
1. RED: `test_failure_panel_shows_6_modes()`
2. RED: `test_failure_panel_highlights_detected_modes()`
3. RED: `test_failure_panel_shows_description_for_detected()`
4. GREEN: Componente com Radix icons + conditional styling
5. VERIFY: `cd apps/theo-ui && npm test`

**Criterios de aceite**:
- [ ] 6 failure modes listados (FM-1..FM-6)
- [ ] Modo detectado: badge amarelo/vermelho + descricao
- [ ] Modo nao detectado: badge verde + "Not detected"
- [ ] Visualmente claro: verde = saudavel, vermelho = problema

**DoD**: 3 testes passando.

---

### T6.8 — Run comparison view

**O que**: Selecionar 2+ runs e comparar metricas lado a lado.

**Arquivos**:
- `apps/theo-ui/src/features/observability/components/RunComparison.tsx` (novo)

**Design**:
```
Compare Runs
                    Run A (Apr 22)    Run B (Apr 22)    Delta
Doom Loop Freq      0.12              0.05              -58% ↓
LLM Efficiency      0.72              0.89              +24% ↑
Context Waste       0.10              0.03              -70% ↓
Hypothesis Churn    0.50              0.33              -34% ↓
Time to 1st Tool    1,250ms           890ms             -29% ↓
Total Steps         23                15                -35% ↓
Total Tool Calls    18                12                -33% ↓
Duration            45s               28s               -38% ↓
```

**TDD**:
1. RED: `test_comparison_shows_delta_percentage()`
2. RED: `test_comparison_colors_improvements_green()`
3. RED: `test_comparison_colors_regressions_red()`
4. RED: `test_comparison_handles_single_run()` — sem delta, so valores
5. GREEN: Tabela com calculo de delta e coloracao
6. VERIFY: `cd apps/theo-ui && npm test`

**Criterios de aceite**:
- [ ] Tabela com colunas por run + coluna delta
- [ ] Delta mostra % de mudanca com seta ↑/↓
- [ ] Melhorias em verde, regressoes em vermelho (invertido para doom_loop e waste onde menor = melhor)
- [ ] Suporta 2-5 runs lado a lado
- [ ] Scroll horizontal se necessario

**DoD**: 4 testes passando, comparison funcional com dados reais.

---

### T6.9 — Integrity indicator

**O que**: Indicador visual de integridade da trajectory (confidence, gaps, drops).

**Arquivos**:
- `apps/theo-ui/src/features/observability/components/IntegrityBadge.tsx` (novo)

**Design**:
- `confidence >= 0.95` → Badge verde "Complete"
- `confidence >= 0.70` → Badge amarelo "Partial (87%)" com tooltip listando gaps
- `confidence < 0.70` → Badge vermelho "Degraded (52%)" com warning
- Drop sentinels e writer recoveries mostrados em tooltip

**TDD**:
1. RED: `test_integrity_badge_green_when_complete()`
2. RED: `test_integrity_badge_yellow_when_partial()`
3. RED: `test_integrity_badge_red_when_degraded()`
4. GREEN: Componente com Radix Tooltip
5. VERIFY: `cd apps/theo-ui && npm test`

**Criterios de aceite**:
- [ ] 3 niveis visuais: verde/amarelo/vermelho
- [ ] Tooltip mostra: events expected/received, gaps, sentinels, schema version
- [ ] Posicionado no header do dashboard, ao lado do run selector

**DoD**: 3 testes passando.

---

## Resumo de Contagem

| Phase | Tasks | Testes (minimo) | Invariantes verificados |
|-------|-------|-----------------|------------------------|
| Phase 0 | 2 | 7 + 0 (refactor) | — |
| Phase 1 | 6 | 5+7+4+4+7+1 = 28 | INV-1, INV-2, INV-3, INV-4 |
| Phase 2 | 4 | 4+7+5+2 = 18 | P1, P2, P3, P4 |
| Phase 3 | 13 | 3+5+4+3+4+3+5+5+4+4+4+4+3 = 51 | — |
| Phase 4 | 4 | 8+7+3+2 = 20 | — |
| Phase 5 | 5 | 4+4+4+3+2 = 17 | FM-1..FM-6 |
| Phase 6 | 9 | 4+0+4+5+5+4+3+4+3 = 32 | — |
| **Total** | **43** | **166** | **10** |

## Ordem de Execucao

```
Sprint 1: T0.1 → T0.2 → T1.1 → T1.2 → T1.3 → T1.4
Sprint 2: T1.5 → T1.6 → T2.1 → T2.2 → T2.3 → T2.4
Sprint 3: T3.1 → T3.2 → T3.3 → T3.4 → T3.5 → T3.6 → T3.7 → T3.8 → T3.9 → T3.10 → T3.11 → T3.12 → T3.13
Sprint 4: T4.1 → T4.2 → T4.3 → T4.4 (se RFC aprovado)
Sprint 5: T5.1 → T5.2 → T5.3 → T5.4 → T5.5
Sprint 6: T6.1 → T6.2 → T6.3 → T6.4 → T6.5 → T6.6 → T6.7 → T6.8 → T6.9

Gate de qualidade por sprint:
- cargo test -p theo-agent-runtime → ALL GREEN
- cargo test -p theo-domain → ALL GREEN (Sprint 1 apenas)
- cargo test -p theo-desktop → ALL GREEN (Sprint 6)
- cd apps/theo-ui && npm test → ALL GREEN (Sprint 6)
- cargo clippy --workspace → ZERO warnings
- cargo build --workspace → ZERO errors
```

## DoD Global do Projeto

- [ ] 166+ testes novos passando (134 Rust + 32 TypeScript)
- [ ] 4 invariantes de pipeline (INV-1..4) com test contracts
- [ ] 4 propriedades de projecao (P1..4) formalmente verificados
- [ ] 6 failure modes com predicados operacionais
- [ ] 5 metricas derivadas com formulas, edge cases, e caveats
- [ ] JSONL storage com crash recovery e schema versioning
- [ ] Dashboard visual com: metric cards, timeline, tool chart, failure panel, run comparison, integrity badge
- [ ] Zero breaking changes em APIs publicas existentes
- [ ] Zero clippy warnings no workspace
- [ ] ADR atualizado com status "Implemented"
- [ ] CHANGELOG.md atualizado com entradas por Sprint
