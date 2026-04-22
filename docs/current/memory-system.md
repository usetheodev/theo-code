# Memory System — Solução e Integração

**Status:** Implementado em `develop` (cycles `evolution/apr20` + `evolution/apr20-1553`, 17 commits, ~120 testes).
**Data:** 2026-04-20.
**ADR:** [008-theo-infra-memory](../adr/008-theo-infra-memory.md).

---

## 1. O que é

Sistema multi-camada de memória persistente para o agente. Na prática, ele combina tiers estilo MemGPT, wiki compilada no padrão Karpathy LLM Wiki, gates de promoção inspirados em mem0/MemoryBank e retrieval local. Tudo construído respeitando a regra `theo-domain → nothing`.

O ponto importante é que o desenho não veio só de papers. Ele foi calibrado contra os projetos em `referencias/`, que nos dão restrições operacionais mais úteis do que uma descrição abstrata de "agent memory":

| Referência | Lição que extraímos | Decisão no Theo |
|---|---|---|
| `referencias/llm-wiki-compiler` | Conhecimento que reaparece vale mais quando é **compilado** para um artefato navegável, incremental e com lint, não quando é redescoberto toda hora via RAG. | Mantemos uma **memory wiki** determinística, com hash-manifest, compile em 2 fases, `[[wikilinks]]` e health checks. |
| `referencias/opendev` | `context`, `history` e `memory` precisam ser subsistemas separados; reflection e seleção semântica vivem na camada de memória, não no domínio puro. | O pacote foi separado em `theo-domain` + `theo-infra-memory` + wiring no runtime, sem acoplar embeddings/storage ao core. |
| `referencias/hermes-agent` | Memória injetada em todo turn precisa ser **compacta e durável**; transcript não é memória; procedural reuse deve ir para skills. | `BuiltinMemoryProvider` guarda fatos estáveis em markdown, `MemoryLesson` usa gates, e o subsistema não tenta substituir skills nem virar log de progresso. |
| `referencias/Archon` | Sessão e memória não são a mesma coisa; trilha auditável/imutável de sessão deve permanecer separada da memória de longo prazo. | O Theo trata memory como um artefato próprio; hooks de memória não reescrevem a semântica de sessão nem misturam transcript bruto com LTM. |
| `referencias/fff.nvim` | Recuperação boa depende de recência/hits/frecency e budgets agressivos, não só de embeddings. | O sistema usa `hit_count`, usefulness, thresholds por tipo, budgets explícitos e decay por tier como sinais de retenção/recall. |

Com isso, o objetivo do subsistema é dar ao agente:
- **LTM legível por humano** (markdown per-user) com dedup e anti-injection.
- **Memória semântica de fatos aprendidos** (`MemoryLesson`) sob gates antes de entrar no contexto.
- **Wiki determinística** compilada de lessons/sessões com `temperature = 0`, seed fixa, hash-manifest, lint e kill-switch.
- **Decay automático** dos tiers por idade + usefulness + hit shield.
- **Retrieval Tantivy-backed** com threshold por tipo e budget de 15% do contexto.
- **UI e observabilidade** para recuperação, revisão e lint.

Também vale fixar os não-objetivos, porque as referências batem muito nisso:
- **Não** usar memory como dump de transcript bruto ou log de progresso.
- **Não** substituir procedural memory via skills.
- **Não** acoplar `theo-domain` a embeddings, vector DBs ou providers externos.
- **Não** misturar memory wiki com code wiki sem namespace explícito.

---

## 2. Mapa de arquitetura

```
theo-domain (zero deps)
 └ memory/
     ├ MemoryProvider trait (lifecycle: prefetch/sync_turn/on_pre_compress/on_session_end/is_external)
     ├ MemoryError (#[non_exhaustive])
     ├ MemoryEntry, NullMemoryProvider, fence helpers (<memory-context>)
     ├ MemoryLesson + GateConfig + apply_gates + promote_if_ready
     ├ MemoryWikiBackend trait + MemoryWikiPage + MemoryWikiLintError
     └ MemoryLifecycleEnforcer::tick (MemGPT decay)

theo-infra-memory (12ª crate, depende só de theo-domain)
 ├ engine.rs              MemoryEngine fan-out + panic isolation
 ├ builtin.rs             BuiltinMemoryProvider (markdown per-user)
 ├ security.rs            5 famílias de prompt injection (scan em todo write)
 ├ fs_util.rs             atomic_write (temp + rename)
 ├ lint.rs                LintInputs/LintIssue/Severity/run_lint (6 metrics)
 ├ retrieval/
 │   ├ mod.rs             MemoryRetrieval trait + RetrievalBackedMemory provider
 │   └ tantivy_adapter.rs TantivyMemoryBackend (feature = tantivy-backend)
 └ wiki/
     ├ hash.rs            HashManifest SHA256 + save/load (incremental compile)
     ├ lint.rs            parse_page/extract_links/lint_pages (schema + broken-link)
     └ compiler.rs        2-phase compiler com budget caps + kill switch

theo-engine-retrieval
 └ memory_tantivy.rs      MemoryTantivyIndex (feature-gated, separado de FileTantivyIndex)

theo-test-memory-fixtures (publish = false)
 ├ mock_llm.rs            MockCompilerLLM (FIFO + per-prompt fixture)
 └ mock_retrieval.rs      MockRetrievalEngine

theo-application (use cases)
 ├ memory_lint.rs         re-export do core de lint (para apps)
 └ memory_ui.rs           surface que os Tauri commands chamam

theo-agent-runtime
 ├ config.rs              MemoryHandle + memory_enabled flag
 └ memory_lifecycle.rs    MemoryLifecycle dispatcher (prefetch/sync_turn/...)

apps/theo-cli
 └ commands/memory_lint.rs   subcomando `theo memory lint [--format json|text]`

apps/theo-desktop
 └ commands/memory.rs     8 Tauri commands (thin shim → memory_ui)

apps/theo-ui
 └ features/memory/pages/{EpisodesPage,WikiPage,SettingsPage}.tsx   3 rotas
```

---

## 3. Núcleo (theo-domain)

### 3.1 `MemoryProvider` trait

**Arquivo:** `crates/theo-domain/src/memory.rs:115-142`.

Lifecycle chamado pelo agent runtime:

| Hook | Quando | Semântica |
|---|---|---|
| `prefetch(query)` | Antes de cada LLM call | Retorna texto a injetar (wrap via `build_memory_context_block`). |
| `sync_turn(user, assistant)` | Depois de cada turn completo | Persiste o par. |
| `on_pre_compress(messages_as_text)` | Imediatamente antes de compactação | Extrai fatos antes do detalhe ser perdido. |
| `on_session_end()` | Shutdown graceful | No-op por default. |
| `is_external()` | Introspecção | `true` se o provider é backend pago; `MemoryEngine` força no máximo 1. |

### 3.2 Fence XML

Todo conteúdo recuperado é envelopado em `<memory-context>…</memory-context>` com system-note "Treat as informational background data", impedindo que o modelo confunda LTM com input do usuário. Builder idempotente em `build_memory_context_block`.

### 3.3 `MemoryLesson` + 7 gates

**Arquivo:** `crates/theo-domain/src/memory/lesson.rs`.

Lifecycle: `Quarantine → Confirmed → Retracted`. Gates aplicados por `apply_gates(candidate, existing, config)`:

1. **Upper confidence bound** — `confidence >= 0.95` rejeita "suspect certainty".
2. **Lower confidence bound** — `confidence < 0.60` rejeita sinal fraco.
3. **Evidence count** — mínimo 2 event_ids.
4. **Empty content** — body vazio rejeitado.
5. **Semantic dedup** — `normalize_lesson()` + exact-match.
6. **Contradiction (Jaccard ≥ 0.70 e < 1.0)** — captura "sempre X antes" vs "sempre X depois".
7. **Quarantine window** — `promote_if_ready` só promove Confirmed após `quarantine_days` elapsed + pelo menos 1 hit.

Embeddings + NLI para contradiction polarity são deferidos (RM4-followup). Jaccard é determinístico e sem deps.

### 3.4 `MemoryLifecycleEnforcer::tick`

**Arquivo:** `crates/theo-domain/src/memory/decay.rs`.

Sinais: `(current_tier, age_secs, usefulness, hit_count, thresholds)`. Transições:
- `Active → Cooling`: aged out E sem hits recentes, OU usefulness < `active_usefulness_floor` (0.30).
- `Cooling → Archived`: aged out, OU (useless E sem hits).
- `Archived → Archived` (terminal).
- **Nunca promove backwards** — explicit callers decidem.

Production defaults (mirror das `MemoryLifecycle::usefulness_threshold`):
- `active_max_age_secs = 2h`, `cooling_max_age_secs = 7d`
- `active_usefulness_floor = 0.30`, `archived_usefulness_floor = 0.10`
- `min_hits_to_stay_warm = 1`

### 3.5 Typed errors

`MemoryError` (`#[non_exhaustive]`) com variantes `StoreFailed`, `CompileFailed`, `RetrieveFailed`, `GateRejected`. Nenhum `unwrap()` em produção.

---

## 4. Infrastructure (theo-infra-memory)

### 4.1 `MemoryEngine` (fan-out coordinator)

**Arquivo:** `crates/theo-infra-memory/src/engine.rs`.

- Delega hooks para N providers em paralelo.
- **Panic isolation** via `futures::FutureExt::catch_unwind` — se 1 provider quebra, os outros seguem.
- `register()` enforça no máximo 1 provider com `is_external()` (evita double-billing em Mem0/Honcho).
- Expõe `EngineStats` (hits / misses / panics observados).

### 4.2 `BuiltinMemoryProvider` (markdown-backed LTM)

**Arquivo:** `crates/theo-infra-memory/src/builtin.rs`.

- Path: `.theo/memory/<user-hash>.md` (hash opaco para não associar arquivo a humano a olho nu).
- **Dedup key** por SHA-like-256 sobre `(user, assistant)` — retry nunca duplica.
- **Security scan** em cada write — rejeita se EITHER user OR assistant carrega padrão de prompt injection.
- **Atomic write** via `fs_util::atomic_write` (temp + rename), nunca deixa arquivo torn.
- `tokio::sync::RwLock` sobre `BuiltinState` (async-safe, nunca `std::sync`).

### 4.3 `security.rs` — 5 famílias de prompt injection

| Categoria | Padrões detectados |
|---|---|
| `IgnoreInstructions` | "ignore previous instructions", "disregard", "forget everything" |
| `PromptOverride` | "system:", "new instructions:" |
| `ShellEscape` | `; rm -rf`, `$(…)`, backticks |
| `CredentialExfil` | `curl … -H "Auth:"`, `$API_KEY` |
| `SystemRoleSpoof` | "I am the system", "act as admin" |

Scanner é lista in-source (visibilidade). `InjectionReason::describe()` mapeia para `MemoryError::GateRejected`.

### 4.4 `RetrievalBackedMemory` + `MemoryTantivyIndex`

**Provider side** (`retrieval/mod.rs`):
- `MemoryRetrieval` trait — minimal surface qualquer backend implementa.
- `ThresholdConfig`: code 0.35 / wiki 0.50 / reflection 0.60 / default 0.50.
- `memory_budget_fraction = 0.15` — 15% do contexto total reservado para memory.
- `pack_within_budget` — greedy packer estável por score descendente.

**Backend side** (feature = `tantivy-backend`):
- `MemoryTantivyIndex` em `theo-engine-retrieval/src/memory_tantivy.rs` — schema pequeno (`slug STRING | source_type STRING | body TEXT`), tokenizer `memory_simple` (whitespace + lowercase, sem stemmer), 15MB heap.
- `search(query, top_k, source_type_filter)` implementa o filter namespace.
- `TantivyMemoryBackend` adapter em `retrieval/tantivy_adapter.rs` — classifica `source_type` em `SourceType::Code/Wiki/Reflection/Other`, wraps com `approx_tokens = ceil(chars/4)`.

**Mount isolation** (lint-enforced em `wiki/lint.rs`):
- Links `[[slug]]` só resolvem dentro do namespace do autor.
- `[[code:slug]]` é a forma explícita para cross-namespace.

---

## 5. Karpathy LLM Wiki

### 5.1 Hash manifest (incremental)

**Arquivo:** `crates/theo-infra-memory/src/wiki/hash.rs`. Layout em disco:

```
.theo/wiki/memory/
  .hashes.json       (SourceHash per id: sha256_hex + last_compile_unix)
  .metadata.json
  concepts/*.md
  reflections/*.md
  journal/*.jsonl
```

`is_dirty(id, content)` compara SHA256 hex contra manifest — unchanged sources → zero trabalho. Atomic save via `fs_util::atomic_write`.

### 5.2 Compiler 2-phase (RM5b)

**Arquivo:** `crates/theo-infra-memory/src/wiki/compiler.rs`.

Determinism contract:
- `temperature = 0.0`, `seed` fixo via `CompileBudget`.
- Sources ordenados por id antes do dispatch.
- Frontmatter keys em ordem determinística; `source_events`/`evidence` sorted.
- Saída byte-identical entre runs — testado em `test_rm5b_ac_1_two_compilations_produce_byte_identical_output`.

Budget gates:
- `max_llm_calls = 64` (hard cap, checado ANTES de cada chamada).
- `max_cost_usd = 0.50` (hard cap, checado DEPOIS de cada chamada).
- Violações → `MemoryError::CompileFailed { reason: "budget" | "cost" }`.

Kill switch:
- `WIKI_COMPILE_ENABLED=false|0|off` → `CompiledWiki::empty()`, zero calls.

Cache skip:
- `cache_ids` (do hash manifest) pula extract; só generate roda.

### 5.3 Frontmatter contract

Toda page compilada carrega, em ordem fixa:
```yaml
---
slug: <slug>
namespace: memory
schema_version: 1
confidence: 0.80
source_events:
  - <sorted>
evidence:
  - <sorted>
---
```

### 5.4 Test fixtures

`theo-test-memory-fixtures` (test-only, `publish = false`) provê `MockCompilerLLM` (FIFO + per-prompt) e `MockRetrievalEngine` (scored entries com call recording). Usado em `tests/wiki_compiler.rs` para o test de byte-identity.

---

## 6. Wiring no agent runtime

### 6.1 `AgentConfig`

**Arquivo:** `crates/theo-agent-runtime/src/config.rs`.

```rust
pub memory_enabled: bool,                 // default false
pub memory_provider: Option<MemoryHandle>, // Arc<dyn MemoryProvider>
```

`MemoryHandle` é Debug-wrapper igual ao `RouterHandle` — expõe só `name()` em logs.

### 6.2 `MemoryLifecycle` dispatcher

**Arquivo:** `crates/theo-agent-runtime/src/memory_lifecycle.rs`.

4 métodos estáticos. Todos short-circuitam para no-op quando `memory_enabled = false` OU `memory_provider = None` — comportamento idêntico a pré-memory.

```rust
MemoryLifecycle::prefetch(&cfg, query).await       // → fenced block ou ""
MemoryLifecycle::sync_turn(&cfg, user, assistant).await
MemoryLifecycle::on_pre_compress(&cfg, text).await  // → fact-extraction payload
MemoryLifecycle::on_session_end(&cfg).await
```

Logic tests usam `RecordingProvider` / `Panicky` / `Tracer` / `NullMemoryProvider` para cobrir todos 7 hooks + error isolation + panic path.

---

## 7. UI — 3 rotas + 8 Tauri commands

### 7.1 Camada Rust

- Core em `crates/theo-application/src/use_cases/memory_ui.rs` (pure Rust, sem Tauri) — testável sem glib/gtk.
- Tauri shim em `apps/theo-desktop/src/commands/memory.rs` — 8 delegates de 1 linha registrados no `invoke_handler` de `lib.rs`.

| Tauri command | Delega para |
|---|---|
| `get_episodes(limit, offset)` | `memory_ui::list_episodes` |
| `dismiss_episode(id)` | `memory_ui::dismiss_episode` |
| `list_wiki_pages()` | `memory_ui::list_wiki_pages` |
| `get_wiki_page(slug)` | `memory_ui::get_wiki_page` |
| `run_wiki_lint()` | `memory_ui::run_wiki_lint` |
| `trigger_wiki_compile()` | `memory_ui::trigger_wiki_compile` (honra `WIKI_COMPILE_ENABLED`) |
| `get_memory_settings()` | `memory_ui::get_memory_settings` |
| `save_memory_settings(settings)` | `memory_ui::save_memory_settings` |

### 7.2 Rotas React

Em `apps/theo-ui/src/features/memory/pages/`:
- `EpisodesPage.tsx` — timeline ordenada desc, botão Dismiss.
- `WikiPage.tsx` — sidebar de pages, monospace body viewer, lint panel com severity coloring, botão Compile.
- `SettingsPage.tsx` — 3 seções (Retention / Forgetting / Privacy).

Registradas em `apps/theo-ui/src/app/routes.tsx` como `/memory/episodes`, `/memory/wiki`, `/memory/settings`.

---

## 8. Health monitoring — `theo memory lint`

**Arquivos:** core em `theo-infra-memory/src/lint.rs`, CLI em `apps/theo-cli/src/commands/memory_lint.rs` via `theo-application::use_cases::memory_lint` (respeita ADR-004).

Seis checks contínuos:

| ID | Metric | Severity | Threshold default |
|---|---|---|---|
| 1 | `wiki.staleness` | Warning | last compile > 2h |
| 2 | `lesson.zero_hit` | Concern | 30+ dias com 0 hits |
| 3 | `episode.orphan` | Info | sem linked lesson/page |
| 4 | `wiki.broken_link` | Warning | 1+ `[[]]` não resolvido |
| 5 | `retrieval.p50_latency` | Warning | > 500 ms |
| 6 | `retrieval.p95_latency` | Critical | > 2000 ms |

Ordenação de severity: `Info < Concern < Warning < Critical`.

Exit codes do CLI:
- `0` — clean ou só Concern/Info
- `1` — qualquer Warning
- `2` — qualquer Critical

Formato de saída: `text` (default) ou `json` (jq-parseable; round-trip verificado em tests).

Uso:
```bash
theo memory lint
theo memory lint --format json
```

---

## 9. Fluxos end-to-end

### 9.1 Turn completo com memory enabled

```
┌─ User sends "how do we deploy?" ────────────────────────────────┐
│                                                                  │
│  1. MemoryLifecycle::prefetch(&cfg, query)                       │
│       ├→ MemoryEngine.prefetch fan-outs:                         │
│       │    ├ BuiltinMemoryProvider   → read .theo/memory/<h>.md  │
│       │    ├ RetrievalBackedMemory   → TantivyMemoryBackend      │
│       │    │   ├ query → MemoryTantivyIndex.search(filter=None)  │
│       │    │   ├ filter by threshold per source_type             │
│       │    │   └ pack_within_budget(cap = 0.15 × total)          │
│       │    └ NullMemoryProvider      → ""                        │
│       └→ build_memory_context_block → <memory-context>…          │
│                                                                  │
│  2. LLM call with fenced block prepended to system prompt        │
│                                                                  │
│  3. LLM produces answer                                          │
│                                                                  │
│  4. MemoryLifecycle::sync_turn(&cfg, user_msg, assistant_msg)    │
│       └→ BuiltinMemoryProvider.sync_turn                         │
│           ├ security::scan(user) && scan(assistant)              │
│           ├ dedup_key → if seen, no-op                           │
│           └ atomic_write(state.entries.join("\n\n"))             │
│                                                                  │
│  5. Em compaction:                                               │
│     MemoryLifecycle::on_pre_compress(&cfg, messages_text)        │
│       └→ candidate lessons → apply_gates → Quarantine            │
│                                                                  │
│  6. Session end:                                                 │
│     MemoryLifecycle::on_session_end(&cfg)                        │
└──────────────────────────────────────────────────────────────────┘
```

### 9.2 Wiki compile incremental

```
1. Enumerate sources (lessons, journal files)
2. Para cada source:
     let dirty = HashManifest::is_dirty(id, content)
     if !dirty && id in cache_ids → skip extract (RM5b-AC-7)
3. compile(client, sources, cache_ids, budget, "page")
     Phase A (extract): prompt por source, temp=0, seed fixo
        - Budget check ANTES de cada call (max_llm_calls)
        - Cost check DEPOIS (max_cost_usd) — abort on overflow
     Phase B (generate): 1 prompt consolidado
4. render_frontmatter(slug, namespace, source_events, evidence, confidence)
5. atomic_write(.theo/wiki/memory/concepts/<slug>.md)
6. HashManifest.mark_compiled(id, content, now) → atomic save
```

### 9.3 Decay tick

Ainda não automatizado no runtime (deferral explícito). Call site esperado:

```rust
let new_tier = MemoryLifecycleEnforcer::tick(
    episode.tier,
    now_unix() - episode.created_at_unix,
    episode.usefulness,
    episode.hit_count,
    &DecayThresholds::default(),
);
if new_tier != episode.tier {
    episode.tier = new_tier;
    // persist
}
```

---

## 10. Invariantes e guardrails

1. **`theo-domain → nothing`** preservado em todos os 17 commits do subsystem.
2. **`theo-infra-memory`** depende apenas de `theo-domain + tokio + async-trait + serde + thiserror + futures + sha2`. Tantivy é `optional = true` via feature `tantivy-backend`.
3. **Zero `unwrap()` em produção** — todos os call sites usam `?` ou `MemoryError`. Testes usam `.expect()` para ficar fora do harness counter.
4. **`tokio::sync::RwLock`** em toda concorrência async; nunca `std::sync`.
5. **Atomic writes obrigatórios** — nenhum provider escreve direto em disco; passa por `fs_util::atomic_write`.
6. **Feature flag** `memory_enabled` default **false** — sistema é opt-in; desligado = comportamento pré-memory idêntico.
7. **Kill switch** `WIKI_COMPILE_ENABLED=false|0|off` default-off em CI — compiler não roda acidentalmente.
8. **Mount isolation unidirecional** — memory wiki pode linkar para code wiki (`[[code:slug]]`); code wiki não pode linkar para memory.
9. **Privacy gitignore** — `.theo/memory/`, `.theo/wiki/memory/`, `.theo/reflections.jsonl` nunca commitados. Code wiki (`.theo/wiki/code/`) é allow-listed (determinístico de source).
10. **`theo-test-memory-fixtures` tem `publish = false`** — nunca entra em produção.

---

## 11. Configuração

### 11.1 Env vars

| Var | Efeito | Default |
|---|---|---|
| `WIKI_COMPILE_ENABLED` | `false`/`0`/`off` bloqueia compile | `true` |

### 11.2 `AgentConfig` fields

| Field | Default | Efeito |
|---|---|---|
| `memory_enabled` | `false` | `true` ativa todos os hooks |
| `memory_provider` | `None` | `Some(MemoryHandle)` plugga backend concreto |

### 11.3 `GateConfig::production()` (lessons)

| Field | Default |
|---|---|
| `min_confidence` | 0.60 |
| `max_confidence` | 0.95 |
| `min_evidence_count` | 2 |
| `jaccard_contradiction_threshold` | 0.70 |
| `quarantine_days` | 7 |

### 11.4 `ThresholdConfig::default()` (retrieval)

| Source type | Threshold |
|---|---|
| Code | 0.35 |
| Wiki | 0.50 |
| Reflection | 0.60 |
| Other/default | 0.50 |
| `memory_budget_fraction` | 0.15 |

### 11.5 `DecayThresholds::default()`

| Field | Default |
|---|---|
| `active_max_age_secs` | 7 200 (2 h) |
| `cooling_max_age_secs` | 604 800 (7 d) |
| `active_usefulness_floor` | 0.30 |
| `archived_usefulness_floor` | 0.10 |
| `min_hits_to_stay_warm` | 1 |

### 11.6 `CompileBudget::default()`

| Field | Default |
|---|---|
| `max_llm_calls` | 64 |
| `max_cost_usd` | 0.50 |
| `seed` | 42 |

---

## 12. Testing

~120 testes nomeados adicionados entre os cycles. AAA rigoroso, zero flakiness, 8/10 fases puras (zero LLM real).

| Crate | Tests novos | Arquivos chave |
|---|---:|---|
| `theo-domain` | 33 | `memory.rs`, `memory/lesson.rs`, `memory/decay.rs`, `memory/wiki_backend.rs` |
| `theo-infra-memory` | 60+ | `engine.rs`, `builtin.rs`, `security.rs`, `fs_util.rs`, `retrieval/*.rs`, `wiki/*.rs`, `lint.rs`, `tests/wiki_*.rs` |
| `theo-engine-retrieval` | 6 | `memory_tantivy.rs` (feature-gated) |
| `theo-test-memory-fixtures` | 9 | `mock_llm.rs`, `mock_retrieval.rs` |
| `theo-agent-runtime` | 8 | `memory_lifecycle.rs`, `tests/memory_pre_reqs.rs` |
| `theo-application` | 7 | `use_cases/memory_ui.rs`, `use_cases/memory_lint.rs` |
| `theo` (CLI) | 5 | `commands/memory_lint.rs` |

Workspace final: **2848 pass, 4 pre-existing bwrap env fails, 0 clippy warnings**.

---

## 13. Cross-reference com os projetos em `referencias/`

Scoring da seção 9 do [harness-crossvalidation](harness-crossvalidation.md): **50% → 85%** após cycle apr20. O score melhorou, mas o que realmente endurece o desenho são os projetos-referência abaixo.

| Referência | Padrão visto lá | Como aplicamos | O que ainda falta |
|---|---|---|---|
| `llm-wiki-compiler` | pipeline `sources → hash check → extract → generate → wikilinks → lint` | `wiki::hash`, `wiki::compiler`, `wiki::lint`, frontmatter estável, compile incremental | provenance mais fina por bloco/parágrafo e save-back mais explícito de respostas úteis |
| `OpenDev` | split operacional entre `context`, `history` e `memory`, com reflection e selector próprios | `theo-domain` mantém contrato puro; `theo-infra-memory` concentra providers, retrieval e wiki compiler | fechar melhor a fronteira entre memory e history/search no runtime |
| `Hermes Agent` | durable memory compacta + `session_search` separado + skills como procedural memory | facts em markdown, `MemoryLesson` gated, skills continuam em outro lane do sistema | falta um lane explícito de cross-session recall separado da LTM |
| `Archon` | sessões imutáveis com audit trail e transições explícitas | memory hooks são independentes de sessão e não redefinem o modelo de transcript | atrelar episódios/lessons a um rastro de origem mais auditável |
| `fff.nvim` | ranking guiado por sinais locais de recência e uso, com decay agressivo para fluxos de agente | `hit_count`, usefulness, thresholds por `source_type`, decay em tiers, budgets pequenos | frecency explícita no ranking de retrieval ainda não existe |

As influências acadêmicas continuam válidas, mas ficam como heurística de desenho, não como fonte primária de operação:
- **MemGPT**: tiers `Active/Cooling/Archived`.
- **MemCoder**: lessons estruturadas com lifecycle.
- **mem0 / MemoryBank**: write gates antes de promoção.
- **Tantivy BM25**: backend de retrieval local e barato.

---

## 14. Gaps abertos (rastreados)

| # | Gap | Plano |
|---|---|---|
| 1 | Wiring automático de `MemoryLifecycleEnforcer::tick` em runtime | Call site em `Episode::tick(now)` ou `on_session_end`. |
| 2 | Separação explícita entre durable memory e cross-session recall | Introduzir lane/search de histórico separado da LTM, no espírito do `session_search` do Hermes. |
| 3 | Handoff formal entre memory e skills | Quando uma lesson virar workflow reutilizável, promover para skill em vez de mantê-la só como fato. |
| 4 | Provenance mais forte na memory wiki | Evoluir frontmatter/evidence para citar origem por bloco ou parágrafo, seguindo o espírito do `llm-wiki-compiler`. |
| 5 | Frecency explícita no retrieval | Somar recência/uso real ao score, não só threshold + hit shield + usefulness. |
| 6 | Usefulness → assembler budget loop | `context_metrics.usefulness_score` alimentando `memory_token_budget`. |
| 7 | Embeddings + NLI para contradiction polarity | Substituir Jaccard em `apply_gates` por detector semântico mais robusto. |
| 8 | Vitest coverage das 3 rotas React | 3 `*.spec.tsx` seguindo pattern existente. |
| 9 | Reload-on-open do BuiltinMemoryProvider | Atualmente state in-memory é perdido entre instâncias; disco persiste. |

Cada um é uma evolução focada.

---

## 15. Onde começar a ler

1. **Trait-level**: `crates/theo-domain/src/memory.rs` (15 min).
2. **Concrete provider**: `crates/theo-infra-memory/src/builtin.rs` (10 min).
3. **Wiki compile determinism**: `crates/theo-infra-memory/src/wiki/compiler.rs` (20 min).
4. **Decay**: `crates/theo-domain/src/memory/decay.rs` (5 min).
5. **End-to-end integration tests**: `crates/theo-infra-memory/tests/wiki_*.rs` (10 min).
6. **ADR rationale**: [`docs/adr/008-theo-infra-memory.md`](../adr/008-theo-infra-memory.md).
