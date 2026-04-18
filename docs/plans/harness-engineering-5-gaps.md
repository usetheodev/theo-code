# Plano: Harness Engineering — 5 Gaps para o Theo Code

## Context

Leitura de 9 documentos de pesquisa (NLAH, VeRO, ProjDevBench, OpenDev, Böckeler) identificou 5 gaps entre estado da arte em harness engineering e o Theo Code. Reunião 20260416-121943 aprovou com veredito REVISED: implementar P0→P0.5→P1→P1.5, defer P3.

**Insight-chave:** A infraestrutura para 3 dos 5 gaps JÁ EXISTE — o trabalho é INTEGRAÇÃO, não criação.

## Dependency Graph

```
P0 (build fix) ──→ P0.5 (sensors) ──→ P1.5 (evolution)
                └─→ P1 (file-state) ─→ P1.5 (evolution)
```

P0.5 e P1 são independentes entre si (parallelizáveis após P0).

---

## PR 1: P0 — Corrigir theo-application (Small, ~15 min)

### Problema
26 erros: imports faltando no test module de `graph_context_service.rs:1260-1307`.

### Modificar
- `crates/theo-application/src/use_cases/graph_context_service.rs`

### Mudança
Adicionar ao bloco `#[cfg(test)] mod tests` (após `use super::*;`):
```rust
use theo_engine_parser::types::{ReferenceKind, SymbolKind};
use theo_engine_graph::bridge::{ReferenceKindDto, SymbolKindDto};
```

### TDD
1. RED: `cargo test -p theo-application` — 26 erros
2. GREEN: Adicionar 2 linhas de import
3. VERIFY: `cargo test --workspace` — todos passam

---

## PR 2: P0.5 — Computational Sensors (Medium, ~2-3h)

### O que existe
- `HookRunner` em `hooks.rs` — execução de shell scripts com timeout
- Pre/post hooks integrados em `run_engine.rs:1476-1708`
- `ContextLoopState.record_edit_attempt()` rastreia files editados
- EventBus publica `ToolCallCompleted`

### Gap
Post-hooks descartam output. Sem mecanismo para rodar sensors pós-edit e injetar resultado no contexto do LLM.

### Criar
- `crates/theo-agent-runtime/src/sensor.rs` — `SensorRunner`, `SensorResult`

### Modificar
- `crates/theo-domain/src/event.rs` — adicionar `SensorExecuted` ao `EventType`
- `crates/theo-agent-runtime/src/hooks.rs` — método `run_sensor_hook()` que retorna output
- `crates/theo-agent-runtime/src/run_engine.rs` — integrar sensor após write tools
- `crates/theo-agent-runtime/src/lib.rs` — expor módulo sensor

### Tipos novos

```rust
// sensor.rs
pub struct SensorResult {
    pub tool_name: String,
    pub file_path: String,
    pub output: String,
    pub exit_code: i32,
    pub duration_ms: u64,
}

pub struct SensorRunner {
    hook_runner: HookRunner,
    pending: Arc<Mutex<Vec<SensorResult>>>,
}
```

### Integração em run_engine.rs

**Após write tool sucesso (~linha 1694):**
```rust
if success && is_write_tool(name) {
    if let Some(ref sensor) = sensor_runner {
        sensor.fire(name, &file_path, &project_dir); // async spawn
    }
}
```

**Antes do LLM call (topo de cada iteração):**
```rust
for result in sensor_runner.drain_pending() {
    messages.push(Message::system(&format!(
        "[SENSOR] {}: {}", result.file_path, result.output
    )));
}
```

### Hook convention
`.theo/hooks/edit.verify.sh` — recebe JSON stdin com file_path, tool_name, project_dir.

### TDD
1. RED: `test_sensor_runs_only_after_successful_write_tool`
2. RED: `test_sensor_result_captured_not_discarded`
3. RED: `test_sensor_result_injected_as_system_message`
4. RED: `test_sensor_executed_event_published`
5. GREEN: Implementar SensorRunner + integração
6. VERIFY: `cargo test -p theo-agent-runtime -- sensor && cargo test -p theo-domain -- event`

---

## PR 3: P1 — File-Backed State (Medium-Large, ~3-4h)

### O que existe
- `SessionTree` em `session_tree.rs` — COMPLETO (JSONL append-only, 14 testes)
- `FileSnapshotStore` em `persistence.rs` — COMPLETO (JSON + checksum)
- `EpisodeSummary` em `theo-domain/episode.rs` — COMPLETO
- `record_session_exit()` salva metrics/episodes/bootstrap no exit
- `session_bootstrap.rs` — contexto cross-session para `--continue`

### Gap
SessionTree nunca instanciada no AgentRunEngine. Estado vive in-memory. Crash = perda total. Episodes escritos mas nunca carregados de volta.

### Criar
- `crates/theo-agent-runtime/src/state_manager.rs` — `StateManager`

### Modificar
- `crates/theo-agent-runtime/src/run_engine.rs` — campo `state_manager`, wire no loop
- `crates/theo-agent-runtime/src/session_bootstrap.rs` — carregar episode summaries
- `crates/theo-agent-runtime/src/config.rs` — `session_dir` config
- `crates/theo-agent-runtime/src/lib.rs` — expor módulo state_manager

### Tipo novo

```rust
// state_manager.rs
pub struct StateManager {
    session_tree: SessionTree,
    state_dir: PathBuf,  // .theo/state/{run_id}/
}

impl StateManager {
    pub fn create(project_dir: &Path, run_id: &str) -> Result<Self>;
    pub fn load(project_dir: &Path, run_id: &str) -> Result<Self>;
    pub fn append_message(&mut self, role: &str, content: &str) -> Result<()>;
    pub fn build_context(&self) -> Vec<Message>;
    pub fn load_episode_summaries(project_dir: &Path) -> Vec<EpisodeSummary>;
}
```

### TDD
1. RED: `test_state_manager_creates_session_tree_on_disk`
2. RED: `test_state_manager_append_and_reload`
3. RED: `test_state_manager_build_context_converts_to_messages`
4. RED: `test_episode_summaries_loadable_for_resume`
5. GREEN: Implementar StateManager + integração
6. VERIFY: `cargo test -p theo-agent-runtime -- state_manager`

---

## PR 4: P1.5 — Self-Evolution Loop (Large, ~4-5h)

### O que existe
- `CorrectionEngine` em `correction.rs` — RetryLocal/Replan/Subtask/AgentSwap
- `HeuristicReflector` em `reflector.rs` — NoProgressLoop, RepeatedSameError (Phase 1)
- `PilotLoop` em `pilot.rs` — circuit breaker, consecutive_no_progress tracking

### Gap
Sem reflection estruturado entre tentativas. Reflector é threshold-based, não strategy-revising.

### Criar
- `crates/theo-domain/src/evolution.rs` — `AttemptRecord`, `Reflection`, `MAX_EVOLUTION_ATTEMPTS`
- `crates/theo-agent-runtime/src/evolution.rs` — `EvolutionLoop`

### Modificar
- `crates/theo-agent-runtime/src/pilot.rs` — integrar EvolutionLoop
- `crates/theo-domain/src/lib.rs` — expor módulo evolution
- `crates/theo-agent-runtime/src/lib.rs` — expor módulo evolution

### Tipos novos

```rust
// theo-domain/src/evolution.rs
pub const MAX_EVOLUTION_ATTEMPTS: u32 = 5;

pub struct AttemptRecord {
    pub attempt_number: u32,
    pub strategy_used: CorrectionStrategy,
    pub outcome: AttemptOutcome,
    pub files_edited: Vec<String>,
    pub error_summary: Option<String>,
    pub duration_ms: u64,
    pub tokens_used: u64,
}

pub enum AttemptOutcome { Success, Failure, Partial }

pub struct Reflection {
    pub prior_attempt: u32,
    pub what_failed: String,
    pub why_it_failed: String,
    pub what_to_change: String,
    pub recommended_strategy: CorrectionStrategy,
}
```

```rust
// theo-agent-runtime/src/evolution.rs
pub struct EvolutionLoop {
    correction_engine: CorrectionEngine,
    reflector: HeuristicReflector,
    attempts: Vec<AttemptRecord>,
    reflections: Vec<Reflection>,
    max_attempts: u32,
}
```

### Integração em pilot.rs

```rust
let mut evolution = EvolutionLoop::new(event_bus.clone());
loop {
    if evolution.is_exhausted() { break; }
    let ctx = evolution.build_evolution_prompt();
    // ... execute agent ...
    evolution.record_attempt(&result);
    if let Some(reflection) = evolution.reflect() {
        inject_reflection_to_messages(&reflection);
    }
}
```

### Depende de
- P0.5 (sensor results enriquecem reflection)
- P1 (attempt history persistível via StateManager)

### TDD
1. RED: `test_evolution_loop_caps_at_5_attempts`
2. RED: `test_reflection_references_prior_attempt`
3. RED: `test_reflection_strategy_differs_from_failed`
4. RED: `test_evolution_prompt_contains_attempt_history`
5. RED: `test_attempt_lineage_traceable`
6. GREEN: Implementar EvolutionLoop
7. VERIFY: `cargo test -p theo-domain -- evolution && cargo test -p theo-agent-runtime -- evolution`

---

## P3: DEFERRED

### Gap 3 (Harness Portável)
YAGNI. Proto-NLAHs existem. Revisitar quando demanda surgir.

### Gap 5 (Agent Optimization)
Prerequisitos: Gap 4 sensors + benchmark profissional + métricas sólidas.

---

## Resumo de Impacto

| PR | Arquivos | Testes novos | Linhas est. | Crates tocados |
|----|----------|-------------|-------------|----------------|
| P0 | 1 | 0 (fix) | 2 | theo-application |
| P0.5 | 5 | ~5 | ~200 | theo-domain, theo-agent-runtime |
| P1 | 5 | ~5 | ~300 | theo-agent-runtime |
| P1.5 | 5 | ~6 | ~400 | theo-domain, theo-agent-runtime |
| **Total** | **~12** | **~16** | **~900** | **3 crates** |

## Verificação Final

```bash
# Após todos os PRs:
cargo test --workspace  # Todos 1879+ testes + ~16 novos
cargo clippy --workspace  # Zero warnings
```
