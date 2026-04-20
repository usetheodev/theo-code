# Research: Context Engineering, Memória e Budget 200k

**Data:** 2026-04-19 (revisão deep-research)
**Prompt:** Gerenciar memória longa/curta, budget ≤200k tokens, isolamento de ruído de tools/MCPs/tasks/project info.
**Fontes pesquisadas:** opendev (Rust), hermes-agent (Python), gemini-cli (TS), pi-mono (TS), papers em `docs/pesquisas/` (Anthropic, OpenAI, Böckeler/Fowler).

---

## Resumo Executivo

O problema central é que o contexto de um agente de código acumula ruído — schemas de tools não usadas, project docs irrelevantes, task state antigo, MCPs carregados eagerly — consumindo tokens que deveriam estar disponíveis para o working context. A pesquisa cruzada em 4 codebases de referência + 4 papers identifica **três eixos convergentes**:

1. **Compaction em estágios** disparada por thresholds de occupancy (70/80/85/90/99%) — padrão unânime em opendev e hermes, ausente em gemini-cli (que faz threshold único + overflow binário).
2. **Separação estrutural** entre memória curta (turno/sessão, efêmera) e memória longa (persistida via artefatos in-repo ou providers plugáveis) — consenso nos 3 papers (Anthropic, OpenAI, Böckeler/Fowler).
3. **Lazy attachments** via trait `ContextCollector`/`MemoryProvider` com `should_fire`/`prefetch` — cada fonte decide se dispara neste turno, em vez de eager-load do system prompt inicial.

**Invariantes derivados:** system prompt base ≤10k tokens, total ≤200k, tool schemas sob demanda, memória longa passa por scoring de relevância, handoff estruturado entre sessões (≤2k tokens).

**Métrica esperada:** redução de 30-50% no consumo de tokens em sessões longas, eliminação do cold-start de 5-10 turnos em novas sessões, zero erros 400 por overflow.

---

## Padrões Extraídos

### Padrão 1: Compaction em 6 Estágios com Calibração por Usage Real

**Fonte primária:** `referencias/opendev/crates/opendev-context/src/compaction/levels.rs:6-18` + `compaction/compactor/mod.rs:100-201`
**Fonte secundária:** `referencias/hermes-agent/agent/context_compressor.py:310-330` (anti-thrashing)

**Descrição:** Em vez de threshold único, opendev define `OptimizationLevel::{None, Warning, Mask, Prune, Aggressive, Compact}` mapeados para 0/70/80/85/90/99% de ocupação. Cada nível aplica estratégia diferente. Crucial: `update_from_api_usage(total_tokens, msg_count)` calibra o contador com tokens REAIS da API — heurística só para delta das novas mensagens desde calibração. `invalidate_calibration()` zera após qualquer modificação estrutural.

```rust
// opendev pattern — sem dependência de tiktoken
pub fn check_usage(&mut self, messages: &[ApiMessage], system: &str) -> OptimizationLevel
pub fn update_from_api_usage(&mut self, total_tokens: u64, message_count: usize)
pub fn invalidate_calibration(&mut self)
```

**Anti-thrashing (hermes):** track `ineffective_compression_count`; se últimas 2 compactações economizaram <10%, skip.

**Como aplicar em theo-code:**
- Crate: `theo-agent-runtime`
- Módulos: `src/context/budget.rs` (ContextBudget struct), `src/context/tokens.rs` (estimate_tokens heurística)
- O `usage.prompt_tokens` retornado por `theo-infra-llm::Client` alimenta a calibração
- `ineffective_count: u8` como field anti-thrashing

**Métrica esperada:** redução de 40-60% das chamadas LLM-powered compaction (caro), pois masking/pruning resolvem a maioria dos casos.

---

### Padrão 2: Tokenização Heurística Zero-Deps

**Fonte:** `referencias/opendev/crates/opendev-context/src/compaction/tokens.rs:1-36`

**Descrição:** Função pura que aproxima cl100k_base sem bibliotecas. Ratio final 0.75 tokens/word.

```rust
pub fn count_tokens(text: &str) -> usize {
    let word_count: usize = text.split_whitespace().map(|word| {
        let len = word.len();
        if len > 12 { return len.div_ceil(4); }
        let punct = word.chars().filter(|c| c.is_ascii_punctuation()).count();
        1 + punct.div_ceil(2)
    }).sum();
    (word_count * 3 + 2) / 4
}
```

**Como aplicar:** copiar para `theo-agent-runtime/src/context/tokens.rs` como função livre `estimate_tokens`.

**Métrica:** <1μs para mensagem de 1k chars; permite checagem de budget em todo turno sem overhead.

---

### Padrão 3: Masking com Sentinelas e PROTECTED_TOOL_TYPES

**Fonte:** `referencias/opendev/crates/opendev-context/src/compaction/compactor/stages.rs:46-105`

**Descrição:** `mask_old_observations(messages, level)` substitui `content` de tool results antigos por sentinela `[ref: tool result {id} — see history]`, mantendo `tool_call_id` para validação de pares. Sentinelas específicas por estágio:

| Sentinela | Stage | Efeito |
|---|---|---|
| `[ref: tool result {id} — see history]` | Mask (80%) | Mantém tool_call_id, conteúdo simbólico |
| `[pruned]` | Prune (85%) | Remove outputs completos |
| `[summary: ...]` | Compact (99%) | Substitui por resumo LLM |

Lista `PROTECTED_TOOL_TYPES = ["skill", "invoke_skill", "present_plan", "read_file", "web_screenshot", "vlm"]` — nunca mascarados. Idempotente (detecta `"[ref:"` existente).

**Como aplicar:**
- `theo-agent-runtime/src/context/stages.rs`
- `PROTECTED_TOOL_TYPES` em theo-code = `["read_file", "graph_context", "skill", "plan"]` (revisar tools atuais)
- A categoria de tool (`ToolCategory`) já existe em `theo-tooling` — usar para decidir proteção

---

### Padrão 4: Tool Pair Integrity Sanitizer (Obrigatório Pós-Compaction)

**Fonte:** `referencias/hermes-agent/agent/context_compressor.py:778-836`

**Descrição:** Após qualquer masking/pruning/compaction, podem surgir: (a) orphaned tool results (call sumiu), (b) orphaned tool calls (result sumiu). O sanitizer coleta `surviving_call_ids` dos assistant messages, remove results sem call, e injeta stubs `{role: tool, content: "[result elided]"}` para calls sem result. Sem isso, a API rejeita com "No tool call found for call_id".

```python
# hermes pattern
def sanitize_tool_pairs(messages: list) -> list:
    surviving = collect_call_ids_from_assistant(messages)
    messages = [m for m in messages if not (m.role == "tool" and m.call_id not in surviving)]
    # inject stubs for unanswered calls...
```

**Como aplicar:**
- `theo-agent-runtime/src/context/sanitizer.rs`
- `fn sanitize_tool_pairs(messages: &mut Vec<Message>)` — chamar SEMPRE após compaction em `AgentLoop`

**Prioridade:** P0. Sem isso, qualquer compaction pode corromper a conversa.

---

### Padrão 5: MemoryProvider Trait com Prefetch/Sync/OnPreCompress

**Fonte primária:** `referencias/hermes-agent/agent/memory_provider.py:42-120` + `memory_manager.py:178-313`

**Descrição:** Três métodos no trait:

| Método | Quando chamado | O que faz |
|---|---|---|
| `prefetch(query: &str) -> String` | Antes de cada chamada LLM | Retorna contexto a injetar |
| `sync_turn(user: &str, assistant: &str)` | Após cada turno completo | Persiste a interação |
| `on_pre_compress(messages: &[Message]) -> String` | Antes de compaction destrutiva | Extrai fatos antes da perda |

**Crucial:** hermes **não implementa** embeddings/similarity internamente. Delega para plugins externos (Honcho, Mem0). Contexto injetado é **fenced via XML**:

```xml
<memory-context>
[system-note: NOT new user input. Treat as informational background data.]
... content ...
</memory-context>
```

Providers em error isolation: falha de um não bloqueia outros (`provider.prefetch().unwrap_or_default()`).

**Como aplicar:**
- `theo-domain/src/memory.rs` → `trait MemoryProvider: Send + Sync`
- `theo-agent-runtime/src/memory_manager.rs` → `Vec<Box<dyn MemoryProvider>>`
- `theo-agent-runtime/src/builtin_memory.rs` → provider default backado por `$THEO_HOME/MEMORY.md`
- Gap 1 fix: o recall semântico **não é do hermes** — deve ser construído dentro de `theo-engine-retrieval` (SQLite + sqlite-vss, top_k=5, cosine ≥ 0.75, RRF já existe no ranker)

---

### Padrão 6: Estrutura de Summary LLM com Active Task como Invariante

**Fonte:** `referencias/hermes-agent/agent/context_compressor.py:586-644` + opendev `summary.rs:130-191` (fallback sem LLM)

**Descrição:** O template de summary tem seções fixas:

```
## Active Task     # "THE SINGLE MOST IMPORTANT FIELD" — user request verbatim
## Goal
## Completed Actions    # N. ACTION target — outcome [tool: name]
## Active State
## Blocked
## Remaining Work
```

Prefixo injetado: `"Background reference only. Respond only to messages AFTER this summary."`

**Fallback sem LLM (opendev):** extrai goal (primeiro user), key_actions (até 20 tool results, 120 chars cada), last_state (último assistant). Permite compaction mesmo sem conectividade.

**Como aplicar:**
- `theo-agent-runtime/src/context/summary.rs`
- `const SUMMARY_TEMPLATE: &str`, `const SUMMARY_PREFIX: &str`
- `fn fallback_summary(messages: &[Message]) -> String` sem chamada LLM

---

### Padrão 7: Overflow Preemptivo Antes de Cada Turno

**Fonte:** `referencias/gemini-cli/packages/core/src/core/client.ts:617-655`

**Descrição:** Antes de cada chamada LLM: `remaining = model_token_limit - last_prompt_tokens; if estimated_request > remaining → abort`. Emite `ContextWindowWillOverflow` **sem enviar**. Binário (passa ou aborta), mas garante zero erros 400 silenciosos.

**Como aplicar:**
- `theo-agent-runtime/src/agent_loop.rs`
- `theo-infra-llm/src/model_limits.rs` → `fn model_token_limit(model: &str) -> u64`
- Erro tipado: `AgentError::ContextWindowOverflow { estimated, remaining }`

**Integra com Padrão 1:** se overflow iminente, forçar `OptimizationLevel::Compact` antes de abortar.

---

### Padrão 8: Memória Hierárquica JIT por Subdiretório

**Fonte:** `referencias/gemini-cli/packages/core/src/context/memoryContextManager.ts:49-159`

**Descrição:** Quando tool de leitura acessa `packages/foo/bar.ts`, o sistema traversa subindo procurando `GEMINI.md` em `packages/foo/`, `packages/`, raiz — injeta no contexto apenas arquivos ainda não carregados. Três camadas: global (`~/.gemini/`), extension, project (workspace).

**Como aplicar:**
- `theo-agent-runtime/src/context/jit_loader.rs`
- `HashMap<PathBuf, bool>` de paths já carregados
- Hook no executor de `read_file` e tools similares: emite evento → collector injeta CLAUDE.md/THEO.md do diretório no próximo turno
- System prompt base carrega apenas global + root CLAUDE.md; subdir loads JIT

**Métrica:** system prompt inicial de ~15k → ≤10k tokens (docs detalhadas entram só sob demanda).

---

### Padrão 9: System Prompt Composicional com Feature-Guards

**Fonte:** `referencias/gemini-cli/packages/core/src/prompts/promptProvider.ts:138-244`

**Descrição:** System prompt montado por `withSection(key, factory, guard)`. Seções: preamble, coreMandates, primaryWorkflows, sandbox, git, tools, mcps. Cada uma só é renderizada se `guard` for true (tem git? YOLO mode? tem MCPs?). Permite override total via `GEMINI_SYSTEM_MD` env var.

**Como aplicar:**
- `theo-agent-runtime/src/session_bootstrap.rs` (já existe) → refatorar para `SystemPromptOptions`
- Cada seção = `Option<SectionData>`, renderização por builder
- Ex: seção "sandbox rules" só entra se bash está habilitado; seção "git workflow" só se repo detectado

---

### Padrão 10: Progressive Disclosure via Skills (2-tier Tool)

**Fonte:** `referencias/hermes-agent/tools/skills_tool.py:647-1000`

**Descrição:** Skills são `SKILL.md` com YAML frontmatter em `$THEO_HOME/skills/<name>/`. Tool `skills_list` retorna APENAS `{name, description, category}` — minimal. Tool `skill_view` retorna conteúdo completo + `linked_files` map; agente chama `skill_view(name, file_path)` para carregar referenced files lazily. Três tiers: list → main → refs.

**Como aplicar:**
- `theo-tooling/src/tools/skills_list.rs` + `skill_view.rs`
- `serde_yaml` para parse de frontmatter
- Storage em `$THEO_HOME/skills/<name>/SKILL.md`

**Métrica:** cada skill adicionada custa ~30 tokens no prompt (descrição), não ~500 tokens (conteúdo).

---

### Padrão 11: Repositório como Sistema de Registro (Memória Longa Estruturada)

**Fonte primária:** `docs/pesquisas/harness-engineering-openai.md:133-137` + `effective-harnesses-for-long-running-agents.md:35-38`

**Descrição:** Consenso em 3 papers: memória longa = artefatos versionados in-repo. Anthropic: `claude-progress.txt` + `feature_list.json`. OpenAI: `exec-plans/active/` + `QUALITY_SCORE.md`. Qualquer conhecimento fora do repo é invisível ao agente. `SessionSummary` persistida compacta (≤2k tokens) evita cold-start.

**Como aplicar:**
- `theo-agent-runtime/src/persistence.rs` (já existe) — estender
- `SessionSummary { task_objective, completed_steps, pending_steps, files_modified, errors }`
- No boot, `BuiltinMemoryProvider::prefetch` injeta summary compacta (≤2k) em vez de reler arquivos
- Integra com `Snapshot` existente em `src/snapshot.rs`

---

### Padrão 12: Progressive Disclosure — AGENTS.md Como Mapa

**Fonte:** `docs/pesquisas/harness-engineering-openai.md:84-95`

**Descrição:** "Give Codex a map, not a 1000-page manual." AGENTS.md ≤100 linhas com table-of-contents apontando para `docs/`. Quando tudo é "importante", nada é. Monolítico tem rotting instantâneo. Docs domínio-específicas entram como attachments via `ContextCollector`, disparados por relevância.

**Como aplicar:**
- Triagem do `CLAUDE.md` atual do theo-code (~15k) → separar "sempre no contexto" de "sob demanda"
- CLAUDE.md base ≤100 linhas com ponteiros
- Docs detalhadas em `docs/` lidas via `read_file` pelo agente quando relevantes

---

## Hipóteses a Validar

**H1** — Lazy loading de tool schemas reduz system prompt em 12-18k tokens em sessão com 20+ tools, sem degradar taxa de seleção correta. *Falsificar:* medir tokens pre/pós em benchmark de 50 prompts; comparar taxa de tool-correct.

**H2** — Compaction em 5+ estágios (opendev pattern) reduz chamadas LLM para compaction em ≥60% vs. threshold único. *Falsificar:* simular 100 sessões de 50 turnos, contar quantas atingem estágio LLM.

**H3** — `SessionSummary` estruturada (≤2k tokens) elimina 70% dos tool calls de orientação nos primeiros 5 turnos de nova sessão. *Falsificar:* comparar N leituras de arquivo de progresso em 20 sessões com vs. sem summary.

**H4** — Overflow preemptivo (gemini-cli pattern) + compaction automática em 99% elimina 100% dos erros 400 por contexto excedido. *Falsificar:* inject prompts artificialmente grandes; contar erros API antes/depois.

**H5** — Calibração via `usage.prompt_tokens` real da API reduz erro de estimativa heurística de ±15% para ±2% em sessões longas. *Falsificar:* logar `estimated vs. actual` em 50 turnos; medir MAE.

**H6** — MemoryProvider com recall semântico (top_k=5, cosine ≥0.75) sobre `theo-engine-retrieval` aumenta continuidade cross-session sem poluir contexto (≤2k tokens injetados por turno). *Falsificar:* medir recall@5 em 30 queries de follow-up; verificar tokens injetados.

---

## Gaps Teóricos (revistos)

**Gap 1 — Recall semântico de memória longa — RESOLVIDO PARCIALMENTE:** hermes externaliza para plugins (Honcho, Mem0). **Solução para theo-code:** construir recall nativo em `theo-engine-retrieval` com SQLite + sqlite-vss, reusando RRF ranker existente. Não há dependência de plugin externo — theo-code já tem embedding infra.

**Gap 2 — Pré-filtro de tools por relevância:** nenhuma referência resolve isso sem circularidade. **Pragmatismo:** manter schemas das N tools mais frequentes sempre; tool de "busca de tools" para descoberta; ADR necessário para definir N (sugestão: 8-10 core tools sempre + tool-discovery).

**Gap 3 — Budget multi-agent:** sub-agentes compartilham budget pai ou têm isolamento? **Decisão necessária:** propor budget isolado (sub-agent tem seu próprio 200k, output ≤4k tokens retorna ao pai) — padrão opendev de "agent fleet" sugere isolamento, mas não formaliza.

**Gap 4 — Threshold ótimo:** nenhum paper publica número específico. Opendev usa 70/80/85/90/99%. **Adotar como padrão** — são os mais documentados e testados em produção.

---

## Métricas Top-3 para Validar Evolução (dos papers)

**M1 — Context Efficiency per Session:** tokens em bootstrap/orientação ÷ total. Target: <5%.
**M2 — Bad Pattern Lifetime:** dias entre introdução e correção de AI slop. Target: ≤2 dias.
**M3 — Clean Session Rate:** % sessões que terminam com repo mergeável (compila, testes verdes, commit + progress update). Target: >90%.

---

## Priorização de Implementação (Crate Work Order Leaf-First)

| Ordem | Padrão | Crate | Esforço | Impacto |
|---|---|---|---|---|
| 1 | Tokenização heurística (P2) | theo-agent-runtime | XS (1h) | Base para tudo |
| 2 | Tool pair sanitizer (P4) | theo-agent-runtime | S (2h) | P0 correctness |
| 3 | Budget com calibração (P1) | theo-agent-runtime | M (1d) | Zero overflow |
| 4 | Overflow preemptivo (P7) | theo-agent-runtime + theo-infra-llm | S (4h) | UX improvement |
| 5 | MemoryProvider trait (P5) | theo-domain + theo-agent-runtime | M (1d) | Habilita longa |
| 6 | Masking + stages (P3) | theo-agent-runtime | M (1d) | Token savings |
| 7 | Summary template (P6) | theo-agent-runtime | S (4h) | Quality compact |
| 8 | SessionSummary (P11) | theo-agent-runtime | M (1d) | Cold-start fix |
| 9 | JIT subdir loader (P8) | theo-agent-runtime | M (1-2d) | Progressive disc |
| 10 | SystemPrompt composicional (P9) | theo-agent-runtime | S (4h) | Guard sections |
| 11 | Skills (P10) | theo-tooling | L (2d) | Nova feature |
| 12 | CLAUDE.md triagem (P12) | docs/ (não-código) | S (2h) | Quick win |
# Evolution Research — Smart Model Routing for Code Agents

**Prompt source:** `outputs/smart-model-routing-plan.md` (6 phases R0-R5, 40 acceptance criteria)
**Underlying research:** `outputs/smart-model-routing.md` (3245 words, 7 sections, audited 5 reference repos)
**Date:** 2026-04-20
**Baseline:** 75.150 (L1=99.8, L2=50.5)

## 1. Starting context (from the prior deep-research)

theo-code today has **zero routing code** (grep-verified). `AgentConfig.model: String` at `config.rs:252` is the only selection — one model for the entire session. The prior research mapped the 2026 SOTA and 5 reference-repo patterns and recommended a 5-phase incremental build:

- R1: domain trait surface in `theo-domain` (zero-dep)
- R2: rule-based classifier in `theo-infra-llm` (ported from hermes-agent)
- R3: wire into `RunEngine` at the `ChatRequest` build site
- R4: extend to compaction + subagent phases + TOML config
- R5: fallback cascade on errors (overflow / 429 / timeout)

## 2. Reference patterns (already extracted in full in `outputs/smart-model-routing.md`)

Three patterns drive the implementation, each tied to a reference file:

| Pattern | Source | Role in plan |
|---|---|---|
| Slot-based model config | `referencias/opendev/crates/opendev-models/src/config/agent.rs:22-66` | Shape of `.theo/config.toml` `[routing.slots.*]` blocks (R4) |
| One-line cascade (`override ?? default`) | `referencias/Archon/packages/providers/src/claude/provider.ts:562` | The single call-site router invocation (R3) |
| Rule-based classifier w/ complex-keywords | `referencias/hermes-agent/agent/smart_model_routing.py:62-107` | The R2 rules; keywords **paraphrased** (AGPL) |

## 3. Plan adaptations for evolution-loop scope

The plan's R0 specifies files under `apps/theo-benchmark/` — **which is out of scope** for the evolution loop (`CANNOT modify: apps/theo-benchmark/`). Adapting:

- **R0 fixture location:** `.theo/fixtures/routing/*.json` (explicitly allowed path)
- **R0 runner:** cargo integration test in `crates/theo-infra-llm/tests/routing_metrics.rs` that loads the fixture and reports `{avg_cost_per_task, task_success_rate, p50_turn_latency}` as JSON on stdout
- **R0 CLI:** deferred; tests invoked via `cargo test -p theo-infra-llm --test routing_metrics`

All other phases (R1-R5) land inside allowed paths (`crates/*/src/`, `crates/*/tests/`, `.theo/`).

## 4. Execution order (locked)

```
R0 (fixture + metrics harness)  ──▶  R1 (domain trait)  ──▶  R2 (rules)
                                                               ↓
R5 (fallback)  ◀──  R4 (compaction + subagent + TOML)  ◀──  R3 (wire)
```

Linear dependency chain; each phase satisfies the global DoD (see `.theo/evolution_criteria.md`) before the next begins.

## 5. Acceptance criteria snapshot

40 AC tests total across 6 phases (R0=4, R1=6, R2=8, R3=6, R4=8, R5=8). Each AC is a named test; the completion promise "TODAS TASKS, E DODS CONCLUIDOS E VALIDADOS" is satisfied only when every AC passes and every global DoD gate is green.

Full AC list: `outputs/smart-model-routing-plan.md` §2 (per-phase tables).

## 6. What "done" looks like (per the plan's §0)

| Metric | Baseline | Target |
|---|---|---|
| `avg_cost_per_task` | NullRouter equivalent (today) | ≥ 30% lower on mixed fixture |
| `task_success_rate` | today | parity (never regress) |
| `p50_turn_latency` | today | ≤ +5% |
| Workspace tests | 2724 | ≥ 2724 |
| Harness score | 75.150 | ≥ 75.150 |

Cost/latency targets are **ratios, not absolutes** (per §4 of the plan) — environment-dependent numbers are out of scope.
