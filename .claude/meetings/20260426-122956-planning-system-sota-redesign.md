---
id: 20260426-122956
date: 2026-04-26
topic: "Redesign do sistema de planning — blend Manus principles + JSON canonico com serde/schemars — SOTA"
verdict: REVISED
participants: 16
branch: develop
commit: 0a974c9
---

# Reuniao: Planning System SOTA Redesign

## Pauta

### Contexto
O `roadmap.rs` atual (282 linhas) usa parsing de markdown com string matching fragil. Nenhum competidor (Claude Code, Cursor, Codex CLI, Aider, SWE-agent) valida schema de plans. Analisamos o repo `planning-with-files` (Manus-style) que tem conceitos excelentes (3 arquivos, attention manipulation, error logging, 5-Question Reboot Test) mas a mesma fragilidade de grep em markdown.

### Proposta Original
- JSON canonico em disco (`.theo/plans/`)
- serde + schemars para schema validation
- Markdown como view layer (nunca parse back)
- 3 arquivos: plan.json, findings.json, progress.json
- 10 tipos novos em theo-domain
- 9 tools em theo-tooling
- Manus principles: filesystem as memory, attention manipulation, error logging, session recovery

### Questoes a Decidir
1. Estrutura final dos tipos (naming, placement, deduplication)
2. Superficie de tools (quantas, quais)
3. Integracao com pilot loop existente
4. Session recovery sem acoplamento a Claude Code
5. O que faz isso GENUINAMENTE SOTA

## Posicoes por Agente

### Estrategia
| Agente | Posicao | Resumo |
|--------|---------|--------|
| chief-architect | APPROVE c/ condicoes | Boundary correto. Reduzir para 6 tools. ErrorEntry/RebootCheck/Decision nao pertencem a theo-domain. Toposort em theo-domain como metodo puro. SOTA gap: falta feedback loop pos-execucao. |
| evolution-agent | APPROVE | Desbloqueia plan mutation programatica, self-evaluation, multi-agent coordination. Recomendar dual representation (JSON canonical + markdown view). Adicionar outcome field por step. SOTA gap: falta plan critique loop e cost tracking. |

### Conhecimento
| Agente | Posicao | Resumo |
|--------|---------|--------|
| knowledge-compiler | APPROVE | findings.json e a melhor fonte para wiki proposals. Precisa de wiki_relevant flag e schema_version. Lifecycle: plans ativos alimentam wiki, arquivados viram read-only com confidence decaying. |
| ontology-manager | CONCERN | TaskStatus duplica TaskState existente (9 variantes). ErrorEntry duplica ErrorClass + AttemptRecord. Finding vs Reflection precisa definicao explicita. RebootCheck e implementation-specific demais para domain. Decision → renomear AgentDecision. |
| data-ingestor | CONCERN | Finding precisa de source_location (file + line + git_sha). Sem checksum por finding, deduplicacao impossivel. UTF-8 + LF normalization obrigatorio no writer. Converter .md com mapping explicito antes de deprecar. |
| wiki-expert | APPROVE c/ concerns | Indexar metadata do plan no BM25, nao JSON raw. to_markdown() e a wiki view. Backlinks bidirecionais (code ↔ plan) desbloqueiam Layer 3 operacional. Filtro de noise antes de indexar findings. |

### Qualidade
| Agente | Posicao | Resumo |
|--------|---------|--------|
| validator | CONCERN | Atomic write (temp→rename) obrigatorio. serde NAO previne IDs duplicados — validate_plan() obrigatoria pos-deserializacao. Orphan dependencies tao perigoso quanto ciclos. #[serde(default)] em todo campo opcional. Advisory flock para concurrent writers. |
| linter | APPROVE | 282 linhas de string matching deletaveis. TaskBuilder, parse_task_header, parse_field_line — tudo morre. parse_checkbox_progress sobrevive (ortogonal). Migracao incremental correta. |
| retrieval-engineer | APPROVE c/ concerns | Plan context no system prompt (alta atencao). get_next_task retorna 1 task (~100-200 tokens) — correto para token budget. Plan-aware retrieval: files do task ativo ganham boost no RRF. Precisa eval set de 10-15 queries. Findings precisam eviction por max-tokens. |
| memory-synthesizer | APPROVE c/ concerns | findings.json como knowledge estruturado — excelente para sintese. Error patterns across plans sao training signal. Decision rationale valioso para fine-tuning SE explicito. Precisa plan_summary field no completion. Lifecycle eviction obrigatorio. |

### Engineering
| Agente | Posicao | Resumo |
|--------|---------|--------|
| code-reviewer | CONCERN | created_at/updated_at DEVE ser u64 (padrao do dominio via clock::now_millis). IDs DEVEM ser newtypes (PlanTaskId, PhaseId) via define_identifier!. depends_on: Vec<PlanTaskId>, nao Vec<u32>. #[non_exhaustive] nos enums. PlanTask para nao shadowing Task existente. |
| graphctx-expert | APPROVE c/ concerns | Task.files pode ser expandido via import graph (2-hop). suggest_files(description) via retrieval pipeline auto-popula files. Validar file refs contra filesystem no create_plan. |
| arch-validator | ABSTAIN | Sem codigo para validar. schemars em theo-domain precisa ser feature-gated. Filesystem access via ToolContext, nao IO direto. |
| test-runner | APPROVE | 8 RED tests propostos. Round-trip serde, cycle detection, atomic write, tool integration flow, schema evolution, migration, malformed JSON rejection. Recomendar proptest para DAG validation. |
| frontend-dev | APPROVE c/ condicoes | Tauri event PlanUpdated segue padrao existente. PlanBoard com Radix Accordion/Checkbox. File watcher no backend Rust. Wizard e YAGNI — comecar com inline editing. |

### Pesquisa
| Agente | Posicao | Resumo |
|--------|---------|--------|
| research-agent | APPROVE | Bar competitivo e absurdamente baixo. SOTA real exige: (1) HTN-inspired hierarchical decomposition com typed state machine, (2) dependency DAG com pre-execution validation, (3) adaptive subtask replanning on failure, (4) multi-agent plan sharing, (5) plan observability metrics, (6) plan pattern corpus. NAO fazer: LTL model checking, Graph-of-Thought, evolutionary mutation. |

## Conflitos e Resolucoes

### Conflito 1: TaskStatus vs TaskState existente
**ontology-manager** flaggou duplicacao semantica com `TaskState` (9 variantes) em `theo-domain/task.rs`.
**Resolucao:** ACEITO. Usar `PlanTaskStatus` como nome distinto. Sao conceitos diferentes: `TaskState` governa o agent loop state machine, `PlanTaskStatus` governa o lifecycle de planejamento. Documentar a distincao no tipo.

### Conflito 2: Timestamps String vs u64
**code-reviewer** flaggou que todo o dominio usa `u64` unix-millis via `clock::now_millis()`.
**Resolucao:** ACEITO. `created_at: u64` e `updated_at: u64`. Consistencia com o dominio existente.

### Conflito 3: Raw u32 IDs vs newtypes
**code-reviewer** flaggou que `identifiers.rs` tem `define_identifier!` macro.
**Resolucao:** ACEITO. Criar `PlanTaskId(u32)` e `PhaseId(u32)`. `depends_on: Vec<PlanTaskId>`.

### Conflito 4: RebootCheck/ErrorEntry/Decision em theo-domain
**ontology-manager** + **chief-architect** concordam: muito implementation-specific.
**Resolucao:** ACEITO. Mover `RebootCheck`, `ErrorEntry` para `theo-agent-runtime`. `Decision` → renomear `PlanDecision` e manter em domain (cruza boundaries).

### Conflito 5: 9 tools e demais
**chief-architect** recomenda 6 max para attention budget da LLM.
**Resolucao:** ACEITO. Merge log_finding/log_error/log_decision → `log_entry(kind)`. Drop `reboot_check` como tool — vira guard automatico no pilot loop. **6 tools finais**: `create_plan`, `update_task`, `advance_phase`, `log_entry`, `get_plan_summary`, `get_next_task`.

### Conflito 6: schemars em theo-domain
**arch-validator** questionou pureza.
**Resolucao:** Feature-gate: `schemars = { version = "1", optional = true }` em theo-domain. Derive `JsonSchema` atras de `#[cfg_attr(feature = "jsonschema", derive(JsonSchema))]`. theo-tooling habilita a feature.

## Decisoes Finais

### D1: Formato canonico = JSON
- Arquivos em `.theo/plans/`: `plan.json`, `findings.json`, `progress.json`
- Markdown e view-only via `to_markdown()`, nunca parse back
- Atomic write: write-to-temp → rename (preservar padrao atual)
- Advisory flock para proteger concurrent writers

### D2: Tipos em theo-domain (revisados)
```rust
// theo-domain/src/plan.rs

pub struct Plan {
    pub version: u32,
    pub title: String,
    pub goal: String,
    pub current_phase: PhaseId,
    pub phases: Vec<Phase>,
    pub decisions: Vec<PlanDecision>,
    pub created_at: u64,    // unix-millis via clock::now_millis()
    pub updated_at: u64,
}

pub struct Phase {
    pub id: PhaseId,
    pub title: String,
    pub status: PhaseStatus,
    pub tasks: Vec<PlanTask>,
}

pub struct PlanTask {
    pub id: PlanTaskId,
    pub title: String,
    pub status: PlanTaskStatus,
    pub files: Vec<String>,
    pub description: String,
    pub dod: String,
    pub depends_on: Vec<PlanTaskId>,
    pub rationale: String,           // memory-synthesizer: task-level rationale
    pub outcome: Option<String>,     // evolution-agent: post-execution outcome
}

#[non_exhaustive]
pub enum PlanTaskStatus { Pending, InProgress, Completed, Skipped, Blocked, Failed }

#[non_exhaustive]
pub enum PhaseStatus { Pending, InProgress, Completed }

pub struct PlanDecision {
    pub decision: String,
    pub rationale: String,
    pub timestamp: u64,
}

// Newtypes
pub struct PlanTaskId(pub u32);
pub struct PhaseId(pub u32);
```

Tipos runtime-only (theo-agent-runtime):
- `RebootCheck` — guard automatico no pilot loop
- `ErrorEntry` — log de erros por sessao
- `PlanProgress` — session log + errors
- `PlanFindings` — research + requirements

### D3: 6 Tools em theo-tooling
| Tool | Descricao |
|------|-----------|
| `create_plan` | Cria plan.json via tool call JSON |
| `update_task` | Muda status de uma PlanTask |
| `advance_phase` | Marca phase complete, avanca current_phase |
| `log_entry` | Log unificado: finding, error, ou decision (kind enum) |
| `get_plan_summary` | Retorna markdown view do plan (read-only) |
| `get_next_task` | Retorna primeira PlanTask pending com deps satisfeitas |

### D4: Validacao em theo-domain
```rust
impl Plan {
    pub fn validate(&self) -> Result<(), PlanValidationError>;
    // Checks: unique task IDs, all depends_on exist, no cycles, valid phase refs
    
    pub fn topological_order(&self) -> Result<Vec<&PlanTask>, PlanValidationError>;
    // Kahn's algorithm, pure function, no IO
    
    pub fn next_actionable_task(&self) -> Option<&PlanTask>;
    // First Pending task with all depends_on Completed
}
```

### D5: Migracao incremental
1. P0: Tipos em theo-domain + tools em theo-tooling (novo sistema)
2. P1: `find_latest_plan()` detecta `.json` primeiro, fallback `.md` com warning
3. P2: Converter one-time de `.md` → `.json` 
4. P3: Remover `parse_roadmap_content()` e todo string matching

### D6: SOTA Features (Tiers)
| Tier | Feature | Justificativa |
|------|---------|---------------|
| T1 (P0) | JSON canonico + serde/schemars + schema-in-tool-definition | Nenhum competidor tem. Foundation. |
| T1 (P0) | Dependency DAG com pre-execution cycle detection | HTN-inspired. Nenhum competidor ordena por dependencia. |
| T1 (P0) | PlanTask.outcome field | Minimal structure para feedback loop futuro. |
| T2 (P1) | Adaptive subtask replanning on failure | GoalAct mostra +12% success rate. Replaneja branch, nao plan inteiro. |
| T2 (P1) | Multi-agent plan sharing | Sub-agents claim tasks, orchestrator ve global state. Unico ao Theo. |
| T2 (P1) | Plan-aware retrieval boost | Files do task ativo ganham weight no RRF fusion. |
| T3 (P2) | Plan observability metrics | Completion rate, drift score, cost accuracy. |
| T3 (P2) | Plan pattern corpus | Completed plans com outcomes → patterns reusaveis. |

### D7: Session Recovery
- `plan_checksum: sha256` no JSON — on reload, if checksum matches, resume
- `RebootCheck` como guard automatico no pilot loop (nao tool manual)
- Zero acoplamento com formato de sessao do Claude Code ou qualquer IDE

## Plano TDD

### RED Phase (ordem de execucao)
```
RED 1: test_plan_serde_roundtrip
  → Plan serializes to JSON and deserializes back identically
  → Validates all field types including newtypes (PlanTaskId, PhaseId)
  → VERIFY: cargo test -p theo-domain

RED 2: test_plan_validate_rejects_duplicate_task_ids
  → Plan with two PlanTask { id: PlanTaskId(1) } → Err(DuplicateTaskId)
  → VERIFY: cargo test -p theo-domain

RED 3: test_plan_validate_rejects_orphan_dependency
  → Task depends_on PlanTaskId(99) which doesn't exist → Err(InvalidDependency)
  → VERIFY: cargo test -p theo-domain

RED 4: test_plan_validate_rejects_dependency_cycle
  → A→B→C→A → Err(CycleDetected)
  → VERIFY: cargo test -p theo-domain

RED 5: test_plan_topological_order_respects_deps
  → T1→T2, T3→T2, T1→T3 → order satisfies all edges
  → VERIFY: cargo test -p theo-domain

RED 6: test_plan_next_actionable_task_respects_deps
  → T1 pending (no deps), T2 pending (depends T1) → returns T1
  → After T1 completed → returns T2
  → VERIFY: cargo test -p theo-domain

RED 7: test_plan_schema_evolution_default_fields
  → JSON from v1 (missing `outcome` field) deserializes into current struct
  → outcome == None, no panic
  → VERIFY: cargo test -p theo-domain

RED 8: test_plan_to_markdown_renders_correctly
  → Plan with 2 phases, 3 tasks → markdown contains headers, checkboxes, deps
  → VERIFY: cargo test -p theo-domain

RED 9: test_tool_create_plan_writes_valid_json
  → create_plan tool → .theo/plans/plan.json exists and round-trips
  → VERIFY: cargo test -p theo-tooling

RED 10: test_tool_update_task_changes_status_atomically
  → update_task(id, Completed) → re-read file → task status == Completed
  → Original file not corrupted on simulated failure
  → VERIFY: cargo test -p theo-tooling

RED 11: test_tool_get_next_task_follows_toposort
  → create_plan with deps → get_next_task returns correct task
  → VERIFY: cargo test -p theo-tooling

RED 12: test_migration_md_to_json_preserves_tasks
  → Parse old SAMPLE_ROADMAP markdown → convert → serialize JSON → deserialize
  → Assert task count, titles, fields preserved
  → VERIFY: cargo test -p theo-agent-runtime
```

### GREEN Phase
Implementacao minima para cada RED test passar, na ordem.

### REFACTOR Phase
Apos todos os testes verdes:
- Extrair `validate_plan()` de metodos individuais
- Consolidar error types
- Remover roadmap.rs string matching code

## Action Items

- [ ] **code-reviewer** — Definir newtypes `PlanTaskId(u32)`, `PhaseId(u32)` em `theo-domain/src/identifiers.rs` — P0
- [ ] **chief-architect** — Criar `theo-domain/src/plan.rs` com tipos revisados (D2) — P0
- [ ] **validator** — Implementar `Plan::validate()` com cycle detection, orphan deps, unique IDs — P0
- [ ] **test-runner** — Executar RED 1-8 em theo-domain antes de qualquer implementacao — P0
- [ ] **chief-architect** — Implementar 6 tools em theo-tooling (D3) — P0
- [ ] **test-runner** — Executar RED 9-11 em theo-tooling — P0
- [ ] **ontology-manager** — Documentar distincao TaskState vs PlanTaskStatus no tipo — P0
- [ ] **arch-validator** — Validar feature-gate schemars em theo-domain — P0
- [ ] **linter** — Deprecar roadmap.rs string matching apos P1 completo — P1
- [ ] **data-ingestor** — Construir converter .md → .json one-time — P1
- [ ] **retrieval-engineer** — Criar eval set 10-15 queries para plan-aware retrieval — P2
- [ ] **frontend-dev** — PlanBoard component com Tauri event PlanUpdated — P2
- [ ] **research-agent** — Prototipar adaptive subtask replanning (T2 SOTA) — P2
- [ ] **memory-synthesizer** — Plan pattern corpus pipeline apos 100+ plans — P3

## Veredito Final

**REVISED**: Proposta aprovada com 6 modificacoes criticas dos agentes:
1. Newtypes para IDs (code-reviewer)
2. u64 timestamps (code-reviewer)  
3. 6 tools, nao 9 (chief-architect)
4. RebootCheck/ErrorEntry em runtime, nao domain (ontology-manager + chief-architect)
5. validate_plan() obrigatoria pos-deserializacao (validator)
6. schemars feature-gated em theo-domain (arch-validator)

O sistema resultante sera o primeiro AI coding assistant com schema-validated plans, dependency DAG, pre-execution validation, e typed tool definitions — genuinamente SOTA por nao ter competidor com qualquer dessas features.
