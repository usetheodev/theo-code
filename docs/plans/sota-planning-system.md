# Plan: SOTA Planning System — JSON Canonico + Manus Principles

## Context

O `roadmap.rs` atual (282 linhas) usa parsing de markdown com string matching fragil — mesma abordagem que Claude Code, Cursor, Codex CLI. Nenhum competidor valida schema de plans. A meeting 20260426-122956 (16 agentes, REVISED) aprovou o redesign com 6 modificacoes criticas.

**Objetivo:** Substituir o roadmap parser por um sistema de planning SOTA com JSON canonico, serde/schemars, dependency DAG, e typed tools. Primeiro AI coding assistant com schema-validated plans.

**Fonte de verdade:** `.claude/meetings/20260426-122956-planning-system-sota-redesign.md`

**Pesquisa:** `outputs/roadmap-alternatives.md` e `outputs/meeting-sota-research.md`

---

## Arquitetura Geral

```
┌─────────────────────────────────────────────────────────────┐
│  LLM Tool Call (JSON)                                       │
│  plan_create({ title, goal, phases: [...] })                │
│  plan_update_task({ task_id: 3, status: "completed" })      │
└────────────────────────┬────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────┐
│  Rust Structs (serde + #[non_exhaustive])                   │
│  Plan { phases: Vec<Phase> }                                │
│  Phase { tasks: Vec<PlanTask> }                             │
│  PlanTask { id: PlanTaskId, status, depends_on, ... }       │
│                                                             │
│  Plan::validate() — unique IDs, deps exist, no cycles       │
│  Plan::topological_order() — Kahn's algorithm               │
│  Plan::next_actionable_task() — first pending w/ deps met   │
└────────────────────────┬────────────────────────────────────┘
                         │
              ┌──────────┴──────────┐
              ▼                     ▼
   ┌──────────────────┐   ┌──────────────────┐
   │ .theo/plans/     │   │ Terminal/UI      │
   │  plan.json       │   │ plan.to_markdown()│
   │  findings.json   │   │ (read-only view) │
   │  progress.json   │   │                  │
   │ (canonical)      │   │                  │
   └──────────────────┘   └──────────────────┘
```

### Por que JSON e nao Markdown

| Aspecto | Markdown (atual) | JSON (proposto) |
|---------|------------------|-----------------|
| Schema validation | Nenhuma | serde automatico |
| LLM gera corretamente | ~80% | ~95% (tool calling e JSON) |
| Parse em Rust | 100 linhas de string matching | `serde_json::from_str` (1 linha) |
| Migracao de formato | Quebra silenciosa | `#[serde(default)]` + campo `version` |
| Atualizar 1 campo | Re-render arquivo inteiro | Deserialize → mutate → serialize |

### Manus Principles Incorporados

| Principio | Como implementado |
|-----------|-------------------|
| Filesystem = memory | 3 arquivos JSON em .theo/plans/ |
| Attention manipulation | plan.to_markdown() injetado no system prompt antes de decisoes |
| 5-Question Reboot Test | RebootCheck struct como guard automatico no pilot loop |
| Error logging | PlanErrorEntry com attempt tracking |
| Session recovery | plan_checksum sha256, zero acoplamento com IDE |
| Never repeat failures | Error log com attempt number impede repeticao |
| 2-Action Rule | log_entry tool para salvar findings apos buscas |

---

## Fase 1: Tipos em theo-domain (plan.rs)

### 1.1 Criar newtypes para IDs de planning

**Arquivo:** `crates/theo-domain/src/identifiers.rs`

O macro `define_identifier!` existente gera IDs string-based com timestamp+random (tipo `TaskId(String)`). Para planning, IDs sao **sequenciais u32** dentro de um plano — semantica diferente. Criar newtypes manuais simples:

```rust
// NAO usar define_identifier! — esses IDs sao u32 sequenciais, nao strings
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlanTaskId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PhaseId(pub u32);

impl fmt::Display for PlanTaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "T{}", self.0)
    }
}

impl fmt::Display for PhaseId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "P{}", self.0)
    }
}
```

### 1.2 Criar modulo `plan.rs` em theo-domain

**Arquivo novo:** `crates/theo-domain/src/plan.rs`

Tipos decididos na meeting (D2), seguindo padroes existentes:
- Timestamps: `u64` via `clock::now_millis()` (padrao de `task.rs:204`)
- Serde: `Serialize, Deserialize` (padrao de todo o dominio)
- Enums: `#[non_exhaustive]` (recomendacao code-reviewer)
- Defaults: `#[serde(default)]` em campos opcionais (forward compat)

```rust
use serde::{Deserialize, Serialize};
use crate::identifiers::{PlanTaskId, PhaseId};

pub const PLAN_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Plan {
    pub version: u32,
    pub title: String,
    pub goal: String,
    pub current_phase: PhaseId,
    pub phases: Vec<Phase>,
    #[serde(default)]
    pub decisions: Vec<PlanDecision>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Phase {
    pub id: PhaseId,
    pub title: String,
    pub status: PhaseStatus,
    pub tasks: Vec<PlanTask>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanTask {
    pub id: PlanTaskId,
    pub title: String,
    pub status: PlanTaskStatus,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub dod: String,
    #[serde(default)]
    pub depends_on: Vec<PlanTaskId>,
    #[serde(default)]
    pub rationale: String,
    #[serde(default)]
    pub outcome: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PlanTaskStatus {
    Pending,
    InProgress,
    Completed,
    Skipped,
    Blocked,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PhaseStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanDecision {
    pub decision: String,
    pub rationale: String,
    pub timestamp: u64,
}
```

**Nota sobre naming (meeting — ontology-manager):**
- `PlanTaskStatus` (nao `TaskStatus`) — evita conflito com `TaskState` existente (9 variantes, governa agent loop)
- `PlanTask` (nao `Task`) — evita shadowing de `theo_domain::task::Task`
- `PlanDecision` (nao `Decision`) — distingue de ADRs arquiteturais

### 1.3 Validacao pura em Plan (theo-domain)

Metodos no `impl Plan` — logica pura, sem IO:

```rust
impl Plan {
    /// Valida invariantes: IDs unicos, deps existem, sem ciclos, phases validas.
    pub fn validate(&self) -> Result<(), PlanValidationError> { ... }

    /// Kahn's algorithm — retorna ordem topologica ou erro de ciclo.
    pub fn topological_order(&self) -> Result<Vec<PlanTaskId>, PlanValidationError> { ... }

    /// Primeira task Pending cujas dependencias estao todas Completed.
    pub fn next_actionable_task(&self) -> Option<&PlanTask> { ... }

    /// Todas as tasks flat (across all phases).
    pub fn all_tasks(&self) -> Vec<&PlanTask> { ... }

    /// Renderiza markdown read-only (view layer, NUNCA parse back).
    pub fn to_markdown(&self) -> String { ... }

    /// Gera prompt para o agent executar uma task especifica.
    pub fn task_to_agent_prompt(&self, task: &PlanTask) -> String { ... }
}
```

### 1.4 Error types

```rust
#[derive(Debug, thiserror::Error)]
pub enum PlanValidationError {
    #[error("duplicate task ID: {0}")]
    DuplicateTaskId(PlanTaskId),
    #[error("task {task_id} depends on non-existent task {missing_dep}")]
    InvalidDependency { task_id: PlanTaskId, missing_dep: PlanTaskId },
    #[error("dependency cycle detected")]
    CycleDetected,
    #[error("invalid phase reference: {0}")]
    InvalidPhaseRef(PhaseId),
}

#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),  // std::io::Error direto, NAO String
    #[error("invalid plan format: {0}")]
    InvalidFormat(String),
    #[error("unsupported plan version: found {found}, max supported {max_supported}")]
    UnsupportedVersion { found: u32, max_supported: u32 },
    #[error(transparent)]
    Validation(#[from] PlanValidationError),
}
```

### 1.5 Registrar modulo

**Arquivo:** `crates/theo-domain/src/lib.rs` — adicionar `pub mod plan;`

### 1.6 Nenhuma dependencia nova

theo-domain ja tem `serde`, `serde_json`, `thiserror`. Nao precisa de schemars nesta fase (YAGNI — feature-gate quando tools precisarem, conforme arch-validator).

---

## Fase 2: Plan I/O em theo-agent-runtime

### 2.1 Criar modulo `plan_store.rs`

**Arquivo novo:** `crates/theo-agent-runtime/src/plan_store.rs`

Funcoes de I/O com atomic write (padrao de `roadmap.rs:179` e `persistence.rs`):

```rust
use std::path::Path;
use theo_domain::plan::{Plan, PlanError, PLAN_FORMAT_VERSION};

/// Carrega plan de JSON, valida, retorna.
pub fn load_plan(path: &Path) -> Result<Plan, PlanError> {
    let content = std::fs::read_to_string(path)?;
    let plan: Plan = serde_json::from_str(&content)
        .map_err(|e| PlanError::InvalidFormat(e.to_string()))?;
    if plan.version > PLAN_FORMAT_VERSION {
        return Err(PlanError::UnsupportedVersion {
            found: plan.version,
            max_supported: PLAN_FORMAT_VERSION,
        });
    }
    plan.validate()?;
    Ok(plan)
}

/// Salva plan como JSON pretty. Atomic: write temp → rename.
pub fn save_plan(path: &Path, plan: &Plan) -> Result<(), PlanError> {
    plan.validate()?;  // Nunca salvar plan invalido
    let json = serde_json::to_string_pretty(plan)
        .map_err(|e| PlanError::InvalidFormat(e.to_string()))?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, json.as_bytes())?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

/// Encontra plan mais recente em .theo/plans/ (prefere .json, fallback .md).
pub fn find_latest_plan(project_dir: &Path) -> Option<std::path::PathBuf> {
    let plans_dir = project_dir.join(".theo").join("plans");
    if let Some(json_plan) = find_latest_by_ext(&plans_dir, "json") {
        return Some(json_plan);
    }
    // Fallback: .md com deprecation warning (migracao)
    find_latest_by_ext(&plans_dir, "md")
}

fn find_latest_by_ext(dir: &Path, ext: &str) -> Option<std::path::PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut files: Vec<std::path::PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some(ext))
        .collect();
    files.sort();
    files.last().cloned()
}
```

### 2.2 Atualizar pilot loop — `run_from_plan`

**Arquivo:** `crates/theo-agent-runtime/src/pilot/mod.rs`

Novo metodo `run_from_plan` que complementa `run_from_roadmap`:

```rust
pub async fn run_from_plan(&mut self, plan_path: &Path) -> PilotResult {
    let mut plan = match plan_store::load_plan(plan_path) {
        Ok(p) => p,
        Err(e) => {
            return self.build_result(ExitReason::Error(
                format!("Failed to load plan: {e}")
            ));
        }
    };

    self.last_git_sha = get_git_sha(&self.project_dir).await;

    loop {
        let task = match plan.next_actionable_task() {
            Some(t) => t.clone(),
            None => break,
        };

        if let Some(reason) = self.check_core_guards() {
            return self.build_result(reason);
        }

        // Mark in_progress + persist
        update_task_status(&mut plan, task.id, PlanTaskStatus::InProgress);
        if let Err(e) = plan_store::save_plan(plan_path, &plan) {
            tracing::warn!("Failed to save plan progress: {e}");
        }

        self.loop_count += 1;
        let sha_before = get_git_sha(&self.project_dir).await;
        let prompt = plan.task_to_agent_prompt(&task);

        let loop_bus = self.build_iteration_bus();
        let registry = create_default_registry();
        let mut agent = AgentLoop::new(self.agent_config.clone(), registry);
        if let Some(ref gc) = self.graph_context {
            agent = agent.with_graph_context(gc.clone());
        }

        let result = agent
            .run_with_history(&prompt, &self.project_dir, self.session_messages.clone(), Some(loop_bus))
            .await;

        self.track_tokens_and_files(&result);
        self.record_exchange(&prompt, &result);

        let progress = detect_git_progress(&self.project_dir, &sha_before).await;
        self.last_git_sha = get_git_sha(&self.project_dir).await;
        self.update_counters(&result, &progress);

        // Update task status based on result
        let new_status = if result.success {
            PlanTaskStatus::Completed
        } else {
            PlanTaskStatus::Failed
        };
        update_task_status(&mut plan, task.id, new_status);
        plan.updated_at = theo_domain::clock::now_millis();
        if let Err(e) = plan_store::save_plan(plan_path, &plan) {
            tracing::warn!("Failed to save plan completion: {e}");
        }
    }

    self.build_result(ExitReason::PromiseFulfilled)
}
```

### 2.3 Backward compat

`run_from_roadmap` permanece durante migracao. CLI detecta formato:
- `.json` → `run_from_plan`
- `.md` → `run_from_roadmap` (legacy, deprecation warning)

---

## Fase 3: Tools em theo-tooling

### 3.1 Expandir `crates/theo-tooling/src/plan/mod.rs`

Atualmente so tem `PlanExitTool` (53 linhas stub). Expandir com 6 tools (meeting D3):

| Tool | ID | Schema params | Descricao |
|------|----|---------------|-----------|
| CreatePlanTool | `plan_create` | `title`, `goal`, `phases` (array de objetos) | Cria plan.json em .theo/plans/ |
| UpdateTaskTool | `plan_update_task` | `task_id` (u32), `status` (string), `outcome?` | Muda status de uma PlanTask |
| AdvancePhaseTool | `plan_advance_phase` | (nenhum) | Marca phase complete, avanca current_phase |
| LogEntryTool | `plan_log` | `kind` (finding/error/decision), `content`, `rationale?` | Log unificado em findings/progress |
| GetPlanSummaryTool | `plan_summary` | (nenhum) | Retorna plan.to_markdown() |
| GetNextTaskTool | `plan_next_task` | (nenhum) | Retorna proxima task actionable via toposort |

Cada tool segue o padrao existente de `ReadTool`/`TaskCreateTool`:
- `struct XTool; impl Tool for XTool`
- `fn schema()` com `ToolParam` + `input_examples`
- `async fn execute()` com `ToolContext` para `ctx.project_dir`
- Atomic write via `plan_store::save_plan`
- `fn category() -> ToolCategory::Orchestration`

### 3.2 Registrar tools

**Arquivo:** `crates/theo-tooling/src/registry/mod.rs`

Adicionar as 6 tools na `create_default_registry()`.

### 3.3 Tool manifest

**Arquivo:** `crates/theo-tooling/src/tool_manifest.rs`

Adicionar entries com `ToolExposure::DefaultRegistry`, `ToolStatus::Implemented`.

---

## Fase 4: Facade e CLI

### 4.1 Atualizar facade

**Arquivo:** `crates/theo-application/src/facade.rs`

```rust
// Novo
pub use theo_agent_runtime::plan_store::{find_latest_plan, load_plan};
// Legacy (manter durante migracao)
pub use theo_agent_runtime::roadmap::{find_latest_roadmap, parse_roadmap};
```

### 4.2 Atualizar CLI

**Arquivo:** `apps/theo-cli/src/pilot.rs`

1. Primeiro tenta `find_latest_plan()` → `.json`
2. Se nao encontra, tenta `find_latest_roadmap()` → `.md` com warning
3. Rota para `run_from_plan` ou `run_from_roadmap` conforme formato

---

## Fase 5: Deprecar roadmap.rs

### O que deletar (apos migracao estavel)
- `parse_roadmap_content()` e todo string matching (linhas 63-115)
- `TaskBuilder`, `parse_task_header`, `parse_field_line` (linhas 190-251)
- `mark_task_completed()` (substituido por `plan_store::save_plan`)
- `RoadmapTask` struct (substituido por `PlanTask`)
- `RoadmapError` (substituido por `PlanError`)

### O que sobrevive
- `parse_checkbox_progress()` e `parse_checkbox_progress_from_file()` — usados pelo fix_plan.md, ortogonal ao planning system.

---

## Fase 6: Findings e Progress (runtime types)

Tipos em `theo-agent-runtime` (NAO domain — meeting D7):

**`crates/theo-agent-runtime/src/plan_findings.rs`**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanFindings {
    pub version: u32,
    pub requirements: Vec<String>,
    pub research: Vec<PlanFinding>,
    pub resources: Vec<PlanResource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanFinding {
    pub summary: String,
    pub source: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanResource {
    pub title: String,
    pub url: String,
}
```

**`crates/theo-agent-runtime/src/plan_progress.rs`**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanProgress {
    pub version: u32,
    pub sessions: Vec<PlanSession>,
    pub errors: Vec<PlanErrorEntry>,
    pub reboot_check: RebootCheck,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanSession {
    pub started_at: u64,
    pub actions: Vec<String>,
    pub files_modified: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanErrorEntry {
    pub error: String,
    pub attempt: u32,
    pub resolution: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebootCheck {
    pub where_am_i: String,
    pub where_going: String,
    pub goal: String,
    pub learned: String,
    pub done: String,
}
```

---

## TDD Plan (RED-GREEN-REFACTOR)

### Fase 1 Tests (theo-domain) — RED first

```
RED 1:  test_plan_task_id_serde_roundtrip
        PlanTaskId(42) serializa e deserializa corretamente

RED 2:  test_plan_serde_roundtrip
        Plan completo com phases/tasks serializa e volta identico

RED 3:  test_plan_validate_rejects_duplicate_task_ids
        Duas tasks com PlanTaskId(1) → Err(DuplicateTaskId)

RED 4:  test_plan_validate_rejects_orphan_dependency
        Task depends_on PlanTaskId(99) inexistente → Err(InvalidDependency)

RED 5:  test_plan_validate_rejects_cycle
        A→B→C→A → Err(CycleDetected)

RED 6:  test_plan_topological_order_respects_deps
        T1→T3, T2→T3 → toposort retorna T3 depois de T1 e T2

RED 7:  test_plan_next_actionable_task_with_deps
        T1 pending (no deps), T2 pending (depends T1) → returns T1
        Apos T1 completed → returns T2

RED 8:  test_plan_next_actionable_task_all_done
        Todas tasks completed → returns None

RED 9:  test_plan_to_markdown_renders_phases_and_tasks
        Plan com 2 phases, 3 tasks → markdown contem headers, checkboxes

RED 10: test_plan_schema_evolution_missing_optional_field
        JSON sem campo "outcome" deserializa com outcome = None

RED 11: test_plan_task_status_serde_all_variants
        Cada variante de PlanTaskStatus roundtrips corretamente

RED 12: test_plan_validate_accepts_valid_plan
        Plan bem formado → Ok(())

VERIFY: cargo test -p theo-domain -- plan
```

### Fase 2 Tests (theo-agent-runtime) — RED first

```
RED 13: test_load_plan_from_json_file
        Cria tempfile com JSON valido → load_plan retorna Ok(Plan)

RED 14: test_save_plan_atomic_write
        save_plan → arquivo existe com conteudo correto, sem .json.tmp residual

RED 15: test_load_plan_rejects_invalid_json
        JSON malformado → Err(InvalidFormat)

RED 16: test_load_plan_rejects_future_version
        version: 999 → Err(UnsupportedVersion)

RED 17: test_find_latest_plan_prefers_json
        Diretorio com .md e .json → retorna .json

VERIFY: cargo test -p theo-agent-runtime -- plan
```

### Fase 3 Tests (theo-tooling) — RED first

```
RED 18: test_tool_plan_create_writes_valid_json
        Execute CreatePlanTool → plan.json existe e valida

RED 19: test_tool_plan_update_task_changes_status
        Cria plan → update_task(completed) → status muda no arquivo

RED 20: test_tool_plan_next_task_follows_deps
        Plan com deps → next_task retorna task correta

RED 21: test_tool_plan_summary_returns_markdown
        Plan exists → summary retorna string com headers

VERIFY: cargo test -p theo-tooling -- plan
```

---

## Arquivos a Criar/Modificar

### Criar
| Arquivo | Conteudo |
|---------|----------|
| `crates/theo-domain/src/plan.rs` | Plan, Phase, PlanTask, PlanTaskStatus, PhaseStatus, PlanDecision, PlanValidationError, PlanError |
| `crates/theo-agent-runtime/src/plan_store.rs` | load_plan, save_plan, find_latest_plan |
| `crates/theo-agent-runtime/src/plan_findings.rs` | PlanFindings, PlanFinding, PlanResource |
| `crates/theo-agent-runtime/src/plan_progress.rs` | PlanProgress, PlanErrorEntry, RebootCheck, PlanSession |

### Modificar
| Arquivo | Mudanca |
|---------|---------|
| `crates/theo-domain/src/identifiers.rs` | + PlanTaskId(u32), PhaseId(u32) newtypes |
| `crates/theo-domain/src/lib.rs` | + `pub mod plan;` |
| `crates/theo-tooling/src/plan/mod.rs` | Expandir de 53 → ~400 linhas (6 tools) |
| `crates/theo-tooling/src/registry/mod.rs` | + registrar 6 plan tools |
| `crates/theo-tooling/src/tool_manifest.rs` | + 6 entries |
| `crates/theo-agent-runtime/src/lib.rs` | + `pub mod plan_store;` |
| `crates/theo-agent-runtime/src/pilot/mod.rs` | + `run_from_plan()`, manter `run_from_roadmap()` |
| `crates/theo-application/src/facade.rs` | + re-exports de plan_store |
| `apps/theo-cli/src/pilot.rs` | Detectar .json vs .md, rotar para metodo correto |

### Deletar (Fase 5, apos migracao)
| Arquivo/Codigo | Motivo |
|----------------|--------|
| `roadmap.rs` linhas 63-251 | String matching parser substituido por serde |
| `RoadmapTask` struct | Substituido por `PlanTask` |
| `RoadmapError` enum | Substituido por `PlanError` |

---

## Decisoes da Meeting Incorporadas

| # | Decisao | Autor | Status |
|---|---------|-------|--------|
| D1 | JSON canonico em .theo/plans/ | consensus | Incorporado |
| D2 | Tipos revisados com newtypes, u64 timestamps | code-reviewer | Incorporado |
| D3 | 6 tools (nao 9) — merge log tools, drop reboot_check tool | chief-architect | Incorporado |
| D4 | validate() + topological_order() em theo-domain | chief-architect + validator | Incorporado |
| D5 | Migracao incremental .md → .json | consensus | Incorporado |
| D6 | SOTA T1: schema + DAG + outcome field | research-agent | Incorporado |
| D7 | RebootCheck/ErrorEntry em runtime, nao domain | ontology-manager + chief-architect | Incorporado |
| D8 | PlanError::Io wrapa std::io::Error, nao String | code-reviewer | Incorporado |
| D9 | #[non_exhaustive] nos enums | code-reviewer | Incorporado |
| D10 | schemars feature-gated (YAGNI por agora) | arch-validator | Incorporado |

---

## SOTA Tiers (Roadmap Futuro)

| Tier | Feature | Justificativa |
|------|---------|---------------|
| **T1 (esta implementacao)** | JSON canonico + serde + schema-in-tool-definition | Nenhum competidor tem |
| **T1** | Dependency DAG com pre-execution cycle detection | HTN-inspired, ninguem ordena por dependencia |
| **T1** | PlanTask.outcome field | Foundation para feedback loop |
| **T2 (proximo)** | Adaptive subtask replanning on failure | GoalAct +12% success rate |
| **T2** | Multi-agent plan sharing (sub-agents claim tasks) | Unico ao Theo |
| **T2** | Plan-aware retrieval boost (files do task ativo no RRF) | retrieval-engineer |
| **T3 (futuro)** | Plan observability metrics (completion rate, drift) | evolution-agent |
| **T3** | Plan pattern corpus (completed plans → reusable patterns) | memory-synthesizer |

---

## Verificacao

```bash
# Fase 1: Domain types + validation
cargo test -p theo-domain -- plan

# Fase 2: Plan I/O
cargo test -p theo-agent-runtime -- plan

# Fase 3: Tools
cargo test -p theo-tooling -- plan

# Full workspace
cargo test

# Clippy clean
cargo clippy --workspace -- -D warnings
```
