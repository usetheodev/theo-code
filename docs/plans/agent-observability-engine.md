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

## Resumo de Contagem

| Phase | Tasks | Testes (minimo) | Invariantes verificados |
|-------|-------|-----------------|------------------------|
| Phase 0 | 2 | 7 + 0 (refactor) | — |
| Phase 1 | 6 | 5+7+4+4+7+1 = 28 | INV-1, INV-2, INV-3, INV-4 |
| Phase 2 | 4 | 4+7+5+2 = 18 | P1, P2, P3, P4 |
| Phase 3 | 7 | 3+5+4+3+4+3+5 = 27 | — |
| Phase 4 | 4 | 8+7+3+2 = 20 | — |
| Phase 5 | 5 | 4+4+4+3+2 = 17 | FM-1..FM-6 |
| **Total** | **28** | **110** | **10** |

## Ordem de Execucao

```
Sprint 1: T0.1 → T0.2 → T1.1 → T1.2 → T1.3 → T1.4
Sprint 2: T1.5 → T1.6 → T2.1 → T2.2 → T2.3 → T2.4
Sprint 3: T3.1 → T3.2 → T3.3 → T3.4 → T3.5 → T3.6 → T3.7
Sprint 4: T4.1 → T4.2 → T4.3 → T4.4 (se RFC aprovado)
Sprint 5: T5.1 → T5.2 → T5.3 → T5.4 → T5.5

Gate de qualidade por sprint:
- cargo test -p theo-agent-runtime → ALL GREEN
- cargo test -p theo-domain → ALL GREEN (Sprint 1 apenas)
- cargo clippy --workspace → ZERO warnings
- cargo build --workspace → ZERO errors
```

## DoD Global do Projeto

- [ ] 110+ testes novos passando
- [ ] 4 invariantes de pipeline (INV-1..4) com test contracts
- [ ] 4 propriedades de projecao (P1..4) formalmente verificados
- [ ] 6 failure modes com predicados operacionais
- [ ] 5 metricas derivadas com formulas, edge cases, e caveats
- [ ] JSONL storage com crash recovery e schema versioning
- [ ] Zero breaking changes em APIs publicas existentes
- [ ] Zero clippy warnings no workspace
- [ ] ADR atualizado com status "Implemented"
- [ ] CHANGELOG.md atualizado com entradas por Sprint
