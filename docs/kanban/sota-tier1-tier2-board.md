# Kanban — SOTA Tier 1 + Tier 2

**Source:** [sota-tier1-tier2-plan.md](../plans/sota-tier1-tier2-plan.md)
**Created:** 2026-04-26
**Last updated:** 2026-04-26

## Progress

```
[····························] 0% (0/19 done)
```

| Column | Count | Cards |
|---|---|---|
| backlog | 18 | T1.1, T1.2, T2.1, T3.1, T4.1, T5.1, T5.2, T6.1, T7.1, T8.1, T9.1, T10.1, T11.1, T12.1, T13.1, T14.1, T15.1, T16.1 |
| ready | 1 | T0.1 |
| doing | 0 | — |
| review | 0 | — |
| done | 0 | — |

## Phase Summary

| Phase | Title | Total | Done | Progress |
|---|---|---|---|---|
| 0 | Foundations — schema bump | 1 | 0 | 0% |
| 1 | Multimodal / Vision | 2 | 0 | 0% |
| 2 | Browser automation | 1 | 0 | 0% |
| 3 | LSP real | 1 | 0 | 0% |
| 4 | Computer Use | 1 | 0 | 0% |
| 5 | Auto-test-generation | 2 | 0 | 0% |
| 6 | Adaptive replanning | 1 | 0 | 0% |
| 7 | Multi-agent claim + parallel | 1 | 0 | 0% |
| 8 | Cross-encoder reranker | 1 | 0 | 0% |
| 9 | Skill marketplace | 1 | 0 | 0% |
| 10 | Cost-aware routing | 1 | 0 | 0% |
| 11 | Compaction stages on | 1 | 0 | 0% |
| 12 | Continuous SOTA eval | 1 | 0 | 0% |
| 13 | DAP integration | 1 | 0 | 0% |
| 14 | Live tool streaming UI | 1 | 0 | 0% |
| 15 | External docs RAG | 1 | 0 | 0% |
| 16 | RLHF feedback export | 1 | 0 | 0% |

## Dependency Graph (Live)

```
T0.1 [ready]
  │
  ├──▶ T1.1 [backlog] ──▶ T1.2 [backlog] ─┐
  │                                        │
  ├──▶ T2.1 [backlog] ─────────────────────┤
  │                                        │
  └──▶ T3.1 [backlog] ─────────────────────┤
                                           │
                       ┌───────────────────┴───────────────────┐
                       ▼                                       ▼
                  T4.1 [backlog]                          T5.1 [backlog]
                       │                                       │
                       │                                       ▼
                       │                                  T5.2 [backlog]
                       │                                       │
                       └───────────────────┬───────────────────┘
                                           ▼
                                      T6.1 [backlog]
                                           │
                                           ▼
                                      T7.1 [backlog]
                                           │
                       ┌───────────────────┼───────────────────┐
                       ▼                   ▼                   ▼
                  T8.1 [backlog]      T9.1 [backlog]      T10.1 [backlog]
                       │                   │                   │
                       └───────────────────┼───────────────────┘
                                           │
                                ┌──────────┴──────────┐
                                ▼                     ▼
                          T11.1 [backlog]       T12.1 [backlog]
                                │                     │
                                └──────────┬──────────┘
                                           │
                       ┌───────────────────┼───────────────────┐
                       ▼                   ▼                   ▼
                 T13.1 [backlog]     T14.1 [backlog]     T15.1 [backlog]
                       │                   │                   │
                       └───────────────────┼───────────────────┘
                                           ▼
                                     T16.1 [backlog]
```

Status annotations: `[done]`, `[review]`, `[doing]`, `[ready]`, `[backlog]`

---

## Backlog

### Phase 1 — Multimodal / Vision

#### T1.1 — Tool `screenshot`

| Field | Value |
|---|---|
| **Phase** | 1: Multimodal / Vision |
| **Status** | backlog |
| **Complexity** | M |
| **Dependencies** | T0.1 |
| **Blocks** | T1.2, T4.1, T5.1 |
| **Files** | 5 files |
| **Tests** | 4 RED tests |
| **Acceptance Criteria** | 6 criteria |
| **Plan ref** | [T1.1](../plans/sota-tier1-tier2-plan.md#t11--tool-screenshot) |

**Objective:** Tool nativa que captura tela inteira ou janela específica, retorna `ContentBlock::ImageBase64`.

**Key deliverables:**
- `crates/theo-tooling/src/screenshot/mod.rs` (NEW) — implementação com xcap
- `crates/theo-tooling/src/registry/mod.rs` — registrar (gated por `vision_enabled`)
- `crates/theo-tooling/src/tool_manifest.rs` — manifest entry
- `crates/theo-domain/src/tool.rs` — `with_image_block` helper
- `crates/theo-agent-runtime/src/tool_bridge.rs` — propagar image blocks ao Message

#### T1.2 — Tool `read_image`

| Field | Value |
|---|---|
| **Phase** | 1: Multimodal / Vision |
| **Status** | backlog |
| **Complexity** | S |
| **Dependencies** | T1.1 |
| **Blocks** | T4.1, T5.1 |
| **Files** | 3 files |
| **Tests** | 3 RED tests |
| **Acceptance Criteria** | 3 criteria |
| **Plan ref** | [T1.2](../plans/sota-tier1-tier2-plan.md#t12--tool-read_image-file--vision-block) |

**Objective:** Tool que lê PNG/JPEG/WebP do filesystem e retorna como vision block.

**Key deliverables:**
- `crates/theo-tooling/src/read_image/mod.rs` (NEW) — reusa `image_pipeline.rs` de T1.1
- `crates/theo-tooling/src/registry/mod.rs` — registrar
- `crates/theo-tooling/src/tool_manifest.rs` — manifest entry

### Phase 2 — Browser automation

#### T2.1 — `browser` tool family via Playwright sidecar

| Field | Value |
|---|---|
| **Phase** | 2: Browser automation |
| **Status** | backlog |
| **Complexity** | L |
| **Dependencies** | T0.1 |
| **Blocks** | T4.1, T5.1 |
| **Files** | 6 files |
| **Tests** | 7 RED tests |
| **Acceptance Criteria** | 3 criteria |
| **Plan ref** | [T2.1](../plans/sota-tier1-tier2-plan.md#t21--browser-tool-family-via-playwright-sidecar) |

**Objective:** Tools `browser_open/click/type/screenshot/eval/close` operando contra chromium headless via Playwright Node sidecar.

**Key deliverables:**
- `crates/theo-tooling/src/browser/mod.rs` (NEW)
- `crates/theo-tooling/src/browser/sidecar.rs` (NEW) — gerência do processo Playwright
- `crates/theo-tooling/src/browser/cdp_client.rs` (NEW) — WS CDP wrapper
- `crates/theo-tooling/scripts/playwright_sidecar.js` (NEW) — Node sidecar
- `crates/theo-tooling/src/registry/mod.rs` + manifest

### Phase 3 — LSP real

#### T3.1 — `lsp-client` + integração com rust-analyzer/pyright/tsserver

| Field | Value |
|---|---|
| **Phase** | 3: LSP real |
| **Status** | backlog |
| **Complexity** | L |
| **Dependencies** | T0.1 |
| **Blocks** | T4.1, T5.1, T13.1 (reusa `JsonRpcStdio`) |
| **Files** | 5 files |
| **Tests** | 7 RED tests |
| **Acceptance Criteria** | 3 criteria |
| **Plan ref** | [T3.1](../plans/sota-tier1-tier2-plan.md#t31--lsp-client-crate--integração-com-rust-analyzerpyrighttsserver) |

**Objective:** Substituir stub LSP por cliente real com rename/find_references/goto_definition/hover/code_actions.

**Key deliverables:**
- `crates/theo-tooling/src/lsp/client.rs` (NEW)
- `crates/theo-tooling/src/lsp/discovery.rs` (NEW)
- `crates/theo-tooling/src/lsp/operations.rs` (NEW)
- `crates/theo-tooling/src/lsp/mod.rs` — substituir stub
- workspace Cargo.toml — `lsp-types = "0.97"`

### Phase 4 — Computer Use

#### T4.1 — Anthropic Computer Use adapter + tool family

| Field | Value |
|---|---|
| **Phase** | 4: Computer Use |
| **Status** | backlog |
| **Complexity** | M |
| **Dependencies** | T1.2, T2.1, T3.1 |
| **Blocks** | T6.1 |
| **Files** | 3 files |
| **Tests** | 4 RED tests |
| **Acceptance Criteria** | 3 criteria |
| **Plan ref** | [T4.1](../plans/sota-tier1-tier2-plan.md#t41--anthropic-computer-use-adapter--tool-family) |

**Objective:** Tool `computer_screenshot/click/type/key/scroll` mapeada à Anthropic Computer Use API.

**Key deliverables:**
- `crates/theo-infra-llm/src/providers/anthropic.rs` — registrar `computer_20250124`
- `crates/theo-tooling/src/computer/mod.rs` (NEW) — xdotool/cliclick wrappers
- `crates/theo-tooling/src/registry/mod.rs` — gate por provider (D6)

### Phase 5 — Auto-test-generation

#### T5.1 — Tool `gen_property_test` via proptest

| Field | Value |
|---|---|
| **Phase** | 5: Auto-test-generation |
| **Status** | backlog |
| **Complexity** | S |
| **Dependencies** | T1.2, T2.1, T3.1 |
| **Blocks** | T5.2 |
| **Files** | 3 files |
| **Tests** | 3 RED tests |
| **Acceptance Criteria** | 2 criteria |
| **Plan ref** | [T5.1](../plans/sota-tier1-tier2-plan.md#t51--tool-gen_property_test-via-proptest) |

**Objective:** Tool que recebe `function_signature` e gera arquivo de testes proptest.

**Key deliverables:**
- `crates/theo-tooling/src/test_gen/property.rs` (NEW)
- `crates/theo-tooling/src/test_gen/mod.rs` (NEW)
- `crates/theo-tooling/src/registry/mod.rs`

#### T5.2 — Tool `gen_mutation_test` via cargo-mutants

| Field | Value |
|---|---|
| **Phase** | 5: Auto-test-generation |
| **Status** | backlog |
| **Complexity** | S |
| **Dependencies** | T5.1 |
| **Blocks** | T6.1 |
| **Files** | 1 file |
| **Tests** | 2 RED tests |
| **Acceptance Criteria** | 2 criteria |
| **Plan ref** | [T5.2](../plans/sota-tier1-tier2-plan.md#t52--tool-gen_mutation_test-via-cargo-mutants) |

**Objective:** Tool que invoca `cargo-mutants --check` e relata sobreviventes.

**Key deliverables:**
- `crates/theo-tooling/src/test_gen/mutation.rs` (NEW)

### Phase 6 — Adaptive replanning

#### T6.1 — `Plan::replan(failure_context)` + `replan` tool

| Field | Value |
|---|---|
| **Phase** | 6: Adaptive replanning |
| **Status** | backlog |
| **Complexity** | L |
| **Dependencies** | T4.1, T5.2 |
| **Blocks** | T7.1 |
| **Files** | 5 files |
| **Tests** | 6 RED tests |
| **Acceptance Criteria** | 3 criteria |
| **Plan ref** | [T6.1](../plans/sota-tier1-tier2-plan.md#t61--planreplanfailure_context--replan-tool) |

**Objective:** Quando uma task falha ≥N vezes, chamar LLM para mutar o plano automaticamente.

**Key deliverables:**
- `crates/theo-domain/src/plan.rs` — adicionar `Plan::apply_patch(patch)`
- `crates/theo-domain/src/plan_patch.rs` (NEW) — `PlanPatch` enum
- `crates/theo-tooling/src/plan/mod.rs` — tool `plan_replan`
- `crates/theo-agent-runtime/src/pilot/mod.rs` — gatilho automático
- `crates/theo-application/src/use_cases/replan_advisor.rs` (NEW)

### Phase 7 — Multi-agent claim + parallel

#### T7.1 — `Plan::claim_task` + `assignee` field + worktree-per-agent

| Field | Value |
|---|---|
| **Phase** | 7: Multi-agent claim + parallel |
| **Status** | backlog |
| **Complexity** | L |
| **Dependencies** | T6.1 |
| **Blocks** | T8.1, T9.1, T10.1 |
| **Files** | 4 files |
| **Tests** | 6 RED tests |
| **Acceptance Criteria** | 3 criteria |
| **Plan ref** | [T7.1](../plans/sota-tier1-tier2-plan.md#t71--planclaim_task--assignee-field--worktree-per-agent) |

**Objective:** N sub-agents executam `next_actionable_task` em paralelo, cada um em worktree próprio (CAS lock-free).

**Key deliverables:**
- `crates/theo-domain/src/plan.rs` — `PlanTask.assignee` + `version_counter`
- `crates/theo-agent-runtime/src/plan_store.rs` — `claim_task` (CAS)
- `crates/theo-agent-runtime/src/pilot/mod.rs` — `run_parallel_from_plan(n_workers)`
- `crates/theo-agent-runtime/src/subagent/manager.rs` — pool ↔ plan integration

### Phase 8 — Cross-encoder reranker

#### T8.1 — Always-on reranker, runtime gate

| Field | Value |
|---|---|
| **Phase** | 8: Cross-encoder reranker |
| **Status** | backlog |
| **Complexity** | M |
| **Dependencies** | T7.1 |
| **Blocks** | T11.1, T12.1 |
| **Files** | 4 files |
| **Tests** | 3 RED tests |
| **Acceptance Criteria** | 2 criteria |
| **Plan ref** | [T8.1](../plans/sota-tier1-tier2-plan.md#t81--always-on-reranker-runtime-gate) |

**Objective:** Cabar `CrossEncoderReranker` na pipeline RRF default; gate via config, não feature.

**Key deliverables:**
- `crates/theo-engine-retrieval/Cargo.toml` — remover `feature = "reranker"`
- `crates/theo-engine-retrieval/src/lib.rs`
- `crates/theo-engine-retrieval/src/pipeline.rs` — adicionar reranker stage
- `crates/theo-engine-retrieval/src/reranker.rs` — remover `#[cfg]`

### Phase 9 — Skill marketplace

#### T9.1 — `skill_catalog` wired + `theo skill install/list/view`

| Field | Value |
|---|---|
| **Phase** | 9: Skill marketplace |
| **Status** | backlog |
| **Complexity** | M |
| **Dependencies** | T7.1 |
| **Blocks** | T11.1, T12.1 |
| **Files** | 3 files |
| **Tests** | 4 RED tests |
| **Acceptance Criteria** | 3 criteria |
| **Plan ref** | [T9.1](../plans/sota-tier1-tier2-plan.md#t91--skill_catalog-wired--theo-skill-installlistview) |

**Objective:** Remover `#[allow(dead_code)]` do `skill_catalog.rs`; cabar nos AgentLoop e CLI.

**Key deliverables:**
- `crates/theo-agent-runtime/src/skill_catalog.rs` — remover allow, expor traits
- `crates/theo-application/src/use_cases/skills.rs` (NEW) — install/list/view
- `apps/theo-cli/src/main.rs` — subcommand `skill`

### Phase 10 — Cost-aware routing

#### T10.1 — `ComplexityClassifier` cabo no AgentLoop

| Field | Value |
|---|---|
| **Phase** | 10: Cost-aware routing |
| **Status** | backlog |
| **Complexity** | M |
| **Dependencies** | T7.1 |
| **Blocks** | T11.1, T12.1 |
| **Files** | 3 files |
| **Tests** | 3 RED tests |
| **Acceptance Criteria** | 2 criteria |
| **Plan ref** | [T10.1](../plans/sota-tier1-tier2-plan.md#t101--complexityclassifier-cabo-no-agentloop) |

**Objective:** AgentLoop classifica task → tier → modelo (Haiku/Sonnet/Opus). A/B esperado: ≥20% redução custo.

**Key deliverables:**
- `crates/theo-infra-llm/src/routing/auto.rs` — cabar
- `crates/theo-agent-runtime/src/agent_loop.rs` — consumir router
- `crates/theo-agent-runtime/src/config.rs` — `RoutingConfig.cost_aware`

### Phase 11 — Compaction stages on

#### T11.1 — Wire `compaction_stages` + `compaction_summary`

| Field | Value |
|---|---|
| **Phase** | 11: Compaction stages on |
| **Status** | backlog |
| **Complexity** | M |
| **Dependencies** | T8.1, T9.1, T10.1 |
| **Blocks** | T13.1, T14.1, T15.1 |
| **Files** | 4 files |
| **Tests** | 3 RED tests |
| **Acceptance Criteria** | 2 criteria |
| **Plan ref** | [T11.1](../plans/sota-tier1-tier2-plan.md#t111--wire-compaction_stages--compaction_summary) |

**Objective:** Remover `#[allow(dead_code)]` dos stages; cabar Prune+Compact após threshold.

**Key deliverables:**
- `crates/theo-agent-runtime/src/compaction_stages.rs` — remover allow
- `crates/theo-agent-runtime/src/compaction_summary.rs` — remover allow
- `crates/theo-agent-runtime/src/compaction/mod.rs` — adicionar stages
- `crates/theo-application/src/use_cases/auxiliary_llm.rs` (NEW)

### Phase 12 — Continuous SOTA evaluation

#### T12.1 — GitHub Actions `eval` job

| Field | Value |
|---|---|
| **Phase** | 12: Continuous SOTA eval |
| **Status** | backlog |
| **Complexity** | M |
| **Dependencies** | T8.1, T9.1, T10.1 |
| **Blocks** | T13.1, T14.1, T15.1 |
| **Files** | 3 files |
| **Tests** | 0 (CI workflow) |
| **Acceptance Criteria** | 3 criteria |
| **Plan ref** | [T12.1](../plans/sota-tier1-tier2-plan.md#t121--github-actions-eval-job) |

**Objective:** PR runs reduced terminal-bench (10 tasks); main runs full nightly. Gate por label `[bench]`.

**Key deliverables:**
- `.github/workflows/eval.yml` (NEW)
- `apps/theo-benchmark/runner/ci_smoke.py` (NEW)
- `docs/audit/eval-baseline.md` (NEW) — baseline 2026-04-26

### Phase 13 — DAP integration

#### T13.1 — `dap-client` + tool family `debug_*`

| Field | Value |
|---|---|
| **Phase** | 13: DAP integration |
| **Status** | backlog |
| **Complexity** | M |
| **Dependencies** | T11.1, T12.1 |
| **Blocks** | T16.1 |
| **Files** | 4 files |
| **Tests** | 4 RED tests |
| **Acceptance Criteria** | 2 criteria |
| **Plan ref** | [T13.1](../plans/sota-tier1-tier2-plan.md#t131--dap-client--tool-family-debug_) |

**Objective:** Tools `debug_set_breakpoint/step/eval/watch` contra `lldb-vscode`/`debugpy`/`vscode-js-debug`.

**Key deliverables:**
- `crates/theo-tooling/src/dap/client.rs` (NEW) — reusa `JsonRpcStdio` de T3.1
- `crates/theo-tooling/src/dap/discovery.rs` (NEW)
- `crates/theo-tooling/src/dap/operations.rs` (NEW)
- `crates/theo-tooling/src/dap/mod.rs` (NEW)

### Phase 14 — Live tool streaming UI

#### T14.1 — `PartialToolResult` plumbing + TUI live render

| Field | Value |
|---|---|
| **Phase** | 14: Live tool streaming UI |
| **Status** | backlog |
| **Complexity** | M |
| **Dependencies** | T11.1, T12.1 |
| **Blocks** | T16.1 |
| **Files** | 4 files |
| **Tests** | 3 RED tests |
| **Acceptance Criteria** | 2 criteria |
| **Plan ref** | [T14.1](../plans/sota-tier1-tier2-plan.md#t141--partialtoolresult-plumbing--tui-live-render) |

**Objective:** Tools de longa duração emitem chunks; TUI renderiza com debounce de 50ms.

**Key deliverables:**
- `crates/theo-tooling/src/plan/mod.rs` — emitir partial em `plan_create`
- `crates/theo-tooling/src/browser/mod.rs` — emitir partial em `browser_eval`
- `apps/theo-cli/src/render/streaming.rs` (NEW)
- `apps/theo-cli/src/tui/*` — consumir

### Phase 15 — External docs RAG

#### T15.1 — Tool `docs_search` + index Tantivy local

| Field | Value |
|---|---|
| **Phase** | 15: External docs RAG |
| **Status** | backlog |
| **Complexity** | M |
| **Dependencies** | T11.1, T12.1 |
| **Blocks** | T16.1 |
| **Files** | 3 files |
| **Tests** | 3 RED tests |
| **Acceptance Criteria** | 2 criteria |
| **Plan ref** | [T15.1](../plans/sota-tier1-tier2-plan.md#t151--tool-docs_search--index-tantivy-local) |

**Objective:** Indexar docs.rs/MDN/npm sob demanda em `~/.cache/theo/docs/<lang>.tantivy`; tool busca cross-source.

**Key deliverables:**
- `crates/theo-tooling/src/docs_search/mod.rs` (NEW)
- `crates/theo-tooling/src/docs_search/sources.rs` (NEW) — fetchers crates.io, MDN, npm
- `crates/theo-tooling/src/docs_search/index.rs` (NEW)

### Phase 16 — RLHF feedback export

#### T16.1 — Trajectory rating + export tool

| Field | Value |
|---|---|
| **Phase** | 16: RLHF feedback export |
| **Status** | backlog |
| **Complexity** | M |
| **Dependencies** | T13.1, T14.1, T15.1 |
| **Blocks** | — |
| **Files** | 3 files |
| **Tests** | 3 RED tests |
| **Acceptance Criteria** | 3 criteria |
| **Plan ref** | [T16.1](../plans/sota-tier1-tier2-plan.md#t161--trajectory-rating--export-tool) |

**Objective:** `rating: Option<i8>` em trajectory entries; CLI `theo trajectory export-rlhf <out.jsonl>` gera dataset axolotl/trl-ready.

**Key deliverables:**
- `crates/theo-domain/src/event.rs` — `EventType::TurnRated`
- `crates/theo-agent-runtime/src/observability/trajectories.rs` — rating field
- `apps/theo-cli/src/trajectory.rs` (NEW) — export command

---

## Ready

### Phase 0 — Foundations — schema bump

#### T0.1 — Bump `theo_infra_llm::types::Message` para `Vec<ContentBlock>`

| Field | Value |
|---|---|
| **Phase** | 0: Foundations — schema bump |
| **Status** | ready |
| **Complexity** | L |
| **Dependencies** | none |
| **Blocks** | T1.1, T2.1, T3.1, T4.1 (todo o resto cascade) |
| **Files** | 7 files (1 + 26 providers) |
| **Tests** | 7 RED tests |
| **Acceptance Criteria** | 10 criteria |
| **Plan ref** | [T0.1](../plans/sota-tier1-tier2-plan.md#t01--bump-theo_infra_llmtypesmessage-para-veccontentblock) |

**Objective:** Permitir mensagens com `text`, `image_url`, `image_base64` no payload OA-compat interno. Migrar `Message.content: Option<String>` para `Option<Vec<ContentBlock>>`.

**Key deliverables:**
- `crates/theo-infra-llm/src/types.rs` — `ContentBlock` enum + `Message.content` refactor
- `crates/theo-infra-llm/src/providers/anthropic.rs` — adapter blocks→Anthropic
- `crates/theo-infra-llm/src/providers/openai.rs` — adapter blocks→OpenAI
- `crates/theo-infra-llm/src/providers/*.rs` — todos os 26 providers (extract text para legacy)
- `crates/theo-domain/src/tool.rs` — `FileAttachment::to_content_block()` helper
- `crates/theo-agent-runtime/src/state_manager.rs` — bump SCHEMA_VERSION v1→v2 + migration
- `crates/theo-agent-runtime/src/transcript_indexer.rs` — handle ambos formatos

---

## Doing

_Empty_

## Review

_Empty_

## Done

_Empty_

---

## History

| Date | Card | From | To | Note |
|---|---|---|---|---|
| 2026-04-26 | — | — | — | Board created from sota-tier1-tier2-plan.md (19 tasks, 17 phases) |
| 2026-04-26 | T0.1 | backlog | ready | No dependencies — Phase 0 entry point |
