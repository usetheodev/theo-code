# Rubric SOTA — Context Engineering (Critério de Convergência)

**Versão:** 2.0 (deep-research revision)
**Data:** 2026-04-19
**Baseado em:** opendev compaction stages, hermes MemoryProvider, gemini-cli overflow preemption, Anthropic long-running agents, OpenAI harness engineering.

---

## Critérios de Convergência

Um feature de context engineering está "convergido" quando TODOS os critérios abaixo são satisfeitos por testes automatizados.

---

### C1 — Budget Total ≤ 200k Tokens (Hard Limit)

**O que:** Nenhuma chamada LLM com contexto acima de 200.000 tokens (input).

**Teste obrigatório:**
```rust
// theo-agent-runtime/tests/budget_enforcement.rs
#[test]
fn budget_enforcer_blocks_call_above_200k_tokens() {
    // Arrange: messages exceeding 200k estimated
    // Act: pass through BudgetEnforcer
    // Assert: Err(BudgetExceeded { used, limit: 200_000 })
}
```

**Métrica:** `context_metrics.input_tokens ≤ 200_000` em 100% das chamadas.

---

### C2 — System Prompt Base ≤ 10k Tokens

**O que:** System prompt fixo (sem attachments lazy) ≤10.000 tokens.

**Teste obrigatório:**
```rust
#[test]
fn base_system_prompt_under_10k_tokens() {
    // Arrange: system prompt with zero lazy attachments
    // Assert: estimated_tokens <= 10_000
}
```

**Responsável:** `theo-agent-runtime/src/session_bootstrap.rs`

---

### C3 — Tool Schemas Carregados Lazy

**O que:** Schemas de tools não usadas no turno não aparecem no contexto. Carregamento via `ContextCollector::should_fire`.

**Testes:**
```rust
#[test]
fn tool_schema_not_injected_when_collector_gate_false()
#[test]
fn tool_schema_injected_only_for_tools_relevant_to_current_turn()
```

**Pragmatismo (Gap 2):** core tools (≤10) sempre carregadas; outras sob demanda via tool de busca.

---

### C4 — Separação Short-Term vs. Long-Term Memory (Estrutural)

**O que:** Tipos distintos. Memória longa entra só via scoring de relevância.

**Invariante:**
- `WorkingContext` — in-memory, escopo de sessão
- `SessionSummary` — persistida, injetada no boot (≤2k tokens)
- Memória longa via `MemoryProvider::prefetch` com relevance score ≥ 0.5 (cosine) ou fence XML

**Testes:**
```rust
#[test]
fn session_summary_injected_only_at_session_start()
#[test]
fn long_term_memory_below_relevance_threshold_not_injected()
#[test]
fn memory_context_block_wrapped_in_xml_fence() // <memory-context>...</memory-context>
```

---

### C5 — Compaction Staged por Threshold de Occupancy

**O que:** Graduada em 6 estágios (opendev pattern):

| Threshold | Stage | Ação |
|-----------|-------|------|
| 0% | None | Sem modificação |
| ≥70% | Warning | Log (once per session) |
| ≥80% | Mask | `[ref: tool result {id} — see history]` em tool results antigos |
| ≥85% | Prune | `[pruned]` em outputs fora do tail |
| ≥90% | Aggressive | Masking com tail menor |
| ≥99% | Compact | LLM summarization do miolo |

**Testes:**
```rust
#[test]
fn at_80_percent_tool_results_masked_not_pruned()
#[test]
fn at_85_percent_old_tool_outputs_pruned()
#[test]
fn at_99_percent_llm_compaction_triggered()
#[test]
fn compaction_idempotent_no_duplicate_summaries()
#[test]
fn system_messages_never_touched_by_compaction()
#[test]
fn last_n_messages_always_preserved_integrally()
#[test]
fn protected_tool_types_never_masked() // read_file, skill, plan
```

---

### C6 — Tool Pair Sanitizer Pós-Compaction (P0)

**O que:** Após qualquer compaction, chamar sanitizer que remove orphaned results e injeta stubs para orphaned calls.

**Testes:**
```rust
#[test]
fn sanitizer_removes_tool_result_without_matching_call()
#[test]
fn sanitizer_injects_stub_for_call_without_result()
#[test]
fn sanitizer_idempotent_when_pairs_already_valid()
```

**Responsável:** `theo-agent-runtime/src/context/sanitizer.rs`

---

### C7 — Overflow Preemptivo Antes de Cada Turno

**O que:** Antes de cada chamada LLM: `remaining = model_token_limit - last_prompt_tokens; if estimated > remaining → forçar compaction Compact OU retornar erro`.

**Testes:**
```rust
#[test]
fn overflow_preemption_triggers_compaction_at_99_percent()
#[test]
fn overflow_returns_err_when_compaction_insufficient()
#[test]
fn model_token_limit_known_for_all_registered_providers()
```

**Responsável:** `theo-infra-llm/src/model_limits.rs` + `theo-agent-runtime/src/agent_loop.rs`

---

### C8 — Calibração Real via `usage.prompt_tokens`

**O que:** Após cada chamada LLM, atualizar contador com valor real da API. Delta heurístico só para mensagens novas.

**Testes:**
```rust
#[test]
fn calibration_updates_baseline_from_api_usage()
#[test]
fn invalidate_calibration_after_compaction_mutates_messages()
#[test]
fn uncalibrated_falls_back_to_heuristic_full_recount()
```

**Métrica:** erro de estimativa (MAE) ≤2% em sessões calibradas (vs. ±15% heurístico puro).

---

### C9 — MemoryProvider Trait com Lifecycle

**O que:** `theo-domain/src/memory.rs` define:

```rust
pub trait MemoryProvider: Send + Sync {
    fn prefetch(&self, query: &str) -> BoxFuture<String>;
    fn sync_turn(&self, user: &str, assistant: &str) -> BoxFuture<()>;
    fn on_pre_compress(&self, messages: &[Message]) -> BoxFuture<String>;
    fn on_session_end(&self) -> BoxFuture<()> { ... }
}
```

**Invariantes:**
- Error isolation: falha de um provider nunca bloqueia outros
- Content injected via fence XML `<memory-context>...</memory-context>`
- `BuiltinMemoryProvider` default backed by `$THEO_HOME/MEMORY.md`

**Testes:**
```rust
#[test]
fn provider_failure_does_not_block_other_providers()
#[test]
fn prefetch_content_wrapped_in_memory_context_fence()
#[test]
fn on_pre_compress_called_before_llm_summarization()
```

---

### C10 — SessionSummary Estruturada e Compacta

**O que:** Struct persistida entre sessões com campos limitados.

```rust
pub struct SessionSummary {
    pub task_objective: String,      // ≤200 chars
    pub completed_steps: Vec<Step>,  // ≤20 items
    pub pending_steps: Vec<Step>,    // ≤10 items
    pub files_modified: Vec<PathBuf>,// ≤30 items
    pub errors_encountered: Vec<String>, // ≤5 items
}
```

**Testes:**
```rust
#[test]
fn session_summary_serialized_under_2k_tokens()
#[test]
fn session_summary_injected_at_boot_not_mid_session()
#[test]
fn session_summary_survives_serialization_roundtrip()
```

---

### C11 — Masking com Sentinelas Canônicas

**O que:** Três sentinelas documentadas, idempotência garantida.

| Sentinela | Uso |
|---|---|
| `[ref: tool result {id} — see history]` | Mask stage |
| `[pruned]` | Prune stage |
| `[summary: ...]` | Compact stage |

**Testes:**
```rust
#[test]
fn sentinel_ref_preserves_tool_call_id()
#[test]
fn re_masking_detects_existing_ref_sentinel_skips()
#[test]
fn pruned_content_has_no_tool_call_id_risk()
```

---

### C12 — JIT Subdir Instruction Loading

**O que:** Quando tool de leitura acessa `packages/foo/bar.ts`, traversa subindo procurando `CLAUDE.md`/`THEO.md` e injeta no próximo turno.

**Testes:**
```rust
#[test]
fn jit_loader_discovers_subdir_instructions_on_file_read()
#[test]
fn jit_loader_does_not_reload_already_loaded_path()
#[test]
fn jit_loader_respects_workspace_root_boundary()
```

**Responsável:** `theo-agent-runtime/src/context/jit_loader.rs`

---

### C13 — System Prompt Composicional com Guards

**O que:** Seções rendered via `Option<SectionData>` + guard booleano em runtime.

**Seções esperadas:** preamble, core_mandates, tools, sandbox, git, mcps, subdir_instructions.

**Testes:**
```rust
#[test]
fn git_section_omitted_when_not_a_git_repo()
#[test]
fn sandbox_section_omitted_when_bash_disabled()
#[test]
fn mcps_section_omitted_when_no_mcps_registered()
```

---

### C14 — Progressive Disclosure Skills (Two-Tier)

**O que:** `skills_list` retorna só `{name, description, category}`; `skill_view` retorna conteúdo + `linked_files` map.

**Testes:**
```rust
#[test]
fn skills_list_returns_minimal_metadata_only()
#[test]
fn skill_view_returns_content_and_linked_files()
#[test]
fn skill_view_loads_referenced_file_lazily()
```

**Responsável:** `theo-tooling/src/tools/skills_list.rs` + `skill_view.rs`

---

### C15 — Anti-Thrashing em Compaction

**O que:** Track `ineffective_compression_count`; se últimas 2 economizaram <10%, skip compaction.

**Testes:**
```rust
#[test]
fn anti_thrashing_skips_after_two_ineffective_compressions()
#[test]
fn counter_resets_after_effective_compression()
```

---

## Checklist Pré-Merge

Para qualquer PR que toque context management:

- [ ] `cargo test -p theo-agent-runtime` passa sem regressões
- [ ] Nenhuma chamada LLM em testes ultrapassa 200k tokens estimados
- [ ] System prompt base ≤ 10k (teste unitário)
- [ ] Collectors implementam `should_fire` com gate real (não `true`)
- [ ] `CompactionStage` invocado nos thresholds documentados
- [ ] `SessionSummary` ≤ 2k tokens serializado
- [ ] `sanitize_tool_pairs` chamado após qualquer compaction
- [ ] Overflow preemption ativo antes de cada LLM call
- [ ] `usage.prompt_tokens` calibra contador após cada chamada

---

## Anti-Padrões a Rejeitar

- Eager loading de TODOS os tool schemas no system prompt
- System prompt com >100 linhas de regras detalhadas de coding
- `CadenceGate::new(1)` em collector de info estática
- Compaction com threshold único
- `SessionSummary` que serializa conversa inteira
- Budget enforcement que avisa mas não bloqueia
- Masking sem chamar sanitizer depois
- LLM summarization sem template estruturado (Active Task etc.)
- Memory provider sem error isolation
- JIT loader que recarrega paths já carregados
- Dependência de tiktoken/external tokenizer (preferir heurística)

---

## Métricas de Convergência

| Métrica | Target | Como medir |
|---|---|---|
| Tokens em bootstrap ÷ total | <5% | `context_metrics.bootstrap_tokens / total_tokens` |
| Erros API por overflow | 0 | contador em `theo-infra-llm` |
| Erro de estimativa heurística | ≤2% (calibrado) | MAE entre estimate e `usage.prompt_tokens` |
| Compactações LLM-powered | ≤40% das compactações | `stage_histogram[Compact]` |
| Cold-start turns | ≤1 | turnos até primeira modificação útil |
| Tool selection accuracy (lazy) | ≥95% baseline | benchmark de 50 prompts |

---

## Definição de "CONVERGIDO"

Evolução pronta para merge quando:

1. Todos os 15 critérios (C1-C15) têm testes passando
2. Score hygiene (theo-evaluate.sh) ≥ baseline
3. 4/6 hipóteses (H1-H6) validadas empiricamente
4. Métricas M1-M3 dos papers alinhadas
5. Sem anti-padrões na listagem acima
6. SOTA average ≥ 2.5 no rubric de 5 dimensões (Pattern Fidelity, Architectural Fit, Completeness, Testability, Simplicity)
# SOTA Criteria — Tool Calling 2.0 Adoption

**Version:** 3.0 (Tool Calling 2.0 cycle)
**Date:** 2026-04-20
**Baseline:** 588d0b6 score 72.3

## Rubric (each dimension 0-3, CONVERGED at avg >= 2.5)

### 1. Pattern Fidelity
- **3** — input_examples, dynamic filtering, batch_execute all land with Anthropic-traceable semantics
- **2** — 2 of 3 land faithfully
- **1** — 1 lands
- **0** — none

### 2. Architectural Fit
- **3** — new surface in `theo-domain`; `theo-tooling` and `theo-agent-runtime` consume via trait; no new external deps; no unwrap in production code
- **2** — one boundary friction but no violation
- **1** — cross-crate duplication to avoid import
- **0** — violates `theo-domain → nothing` or adds unwrap

### 3. Completeness
- **3** — 3+ tools carry input_examples; webfetch emits filtered output; batch_execute has dispatch + binding semantics + integration test
- **2** — 2 of the 3 above are fully wired
- **1** — trait / schema surface changes but no tool adopts
- **0** — scaffolding only

### 4. Testability
- **3** — per-feature unit tests (schema serialization, HTML reducer pure function, batch variable binding), plus integration tests on the agent-runtime dispatch path
- **2** — unit only
- **1** — smoke only
- **0** — none

### 5. Simplicity
- **3** — each change <= 200 LOC, no premature abstractions, no new deps, no new workspace members
- **2** — one change crosses 200 LOC but justified
- **1** — speculative abstractions
- **0** — refactor sprawl

## Guardrails

- Hygiene floor: `cargo check --workspace --tests` stays at 0 warnings, 0 errors (baseline was already fixed)
- Harness score must stay >= 72.3 (baseline)
- Pre-commit hook passes WITHOUT `--no-verify` on every commit
- Each change <= 200 LOC; decompose larger changes
- TDD: every behavior change starts with a failing test
- `theo-domain` stays dependency-free
- No new external crates; no new workspace members

## Done Definition

- `cargo test --workspace` still passes (2556+ tests)
- `cargo check --workspace --tests` 0 warnings
- At least 3 tools declare a non-empty `input_examples`
- Webfetch reducer strips `<script>`, `<style>`, and at least nav/header/footer on a representative HTML fixture
- `batch_execute` meta-tool: schema present in `registry_to_definitions`, dispatch returns per-step results, integration test covers a 2-step chain with variable binding
- SOTA rubric average >= 2.5
