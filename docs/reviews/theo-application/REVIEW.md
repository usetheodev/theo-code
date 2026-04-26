# theo-application — Revisao

> **Contexto**: Camada de use cases. Unica camada que pode depender de todos os crates. Apps (`theo-cli`, `theo-desktop`) consomem APENAS esta camada + `theo-api-contracts`.
>
> **Papel**: orquestra fluxos cross-crate sem vazar dependencias internas para os apps.
>
> **Status global**: deep-review concluido em 2026-04-25. 141 tests passando entre lib + integracao, 0 falhas. `cargo clippy -p theo-application --lib --tests` zero warnings em codigo proprio. Hygiene gates verdes.

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `facade` | Fachada unificada de alto nivel exposta aos apps. | Revisado |
| 2 | `use_cases::agents_dashboard` | Use case: agregar dados para dashboard de agents. | Revisado |
| 3 | `use_cases::auth` | Use case: fluxos de autenticacao (login, logout, refresh). | Revisado |
| 4 | `use_cases::context_assembler` | Use case: assembly de contexto de codigo + memoria + working set. | Revisado |
| 5 | `use_cases::conversion` | Use case: conversao entre formatos (DTO ↔ domain). | Revisado |
| 6 | `use_cases::extraction` | Use case: extracao de informacoes de artefatos. | Revisado |
| 7 | `use_cases::graph_context_service` | Use case: servico de consulta ao grafo de codigo. | Revisado |
| 8 | `use_cases::guardrail_loader` | Use case: carregamento de guardrails do projeto. | Revisado |
| 9 | `use_cases::impact` | Use case: analise de impacto (o que uma mudanca afeta). | Revisado |
| 10 | `use_cases::memory_factory` | Use case: factory de `MemoryEngine` configurado. | Revisado |
| 11 | `use_cases::memory_lint` | Use case: lint de entradas de memoria. | Revisado |

## Modulos NAO listados originalmente (presentes em `use_cases/`)

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 12 | `use_cases::memory_ui` | Use case: surface UI para memorias (Tauri/CLI). | Revisado |
| 13 | `use_cases::observability_ui` | Use case: surface UI para observabilidade (metrics, traces). | Revisado |
| 14 | `use_cases::pipeline` | Pipeline orchestrator de retrieval + GRAPHCTX. | Revisado |
| 15 | `use_cases::router_loader` | Loader de `RoutingConfig` a partir de `.theo/config.toml`. | Revisado |
| 16 | `use_cases::run_agent_session` | Use case: executar uma sessao de agent end-to-end. | Revisado |
| 17 | `use_cases::transcript_indexer_impl` | Implementacao concreta de `TranscriptIndexer` (Tantivy). | Revisado |
| 18 | `use_cases::wiki_backend_impl` | Implementacao concreta do backend de wiki. | Revisado |
| 19 | `use_cases::wiki_enrichment` | Use case: enriquecimento LLM-driven de paginas wiki. | Revisado |
| 20 | `use_cases::wiki_highlevel` | Use case: API high-level de wiki (search, render). | Revisado |

---

## Notas de Deep-Review por Dominio

> Auditoria orientada a: (1) responsabilidade unica, (2) dependencias permitidas pelo bounded-context (apps NAO importam crates internos diretamente), (3) cobertura de testes, (4) clippy/hygiene.

### 1. facade (111 LOC)
Fachada narrow — re-exports do subset estritamente necessario para `apps/*`. Documentado caller-by-caller (cada `pub use` lista qual app consome). Modulos: `agent`, `llm`, `tooling`, `mcp`, `observability` (cfg(otel)), `handoff_guardrail`, `auth`. Fundacao para resolver as violations de `scripts/check-arch-contract.sh`. Hygiene: zero achados; o modulo e intencionalmente fino.

### 2. use_cases::agents_dashboard (194 LOC)
Phase 15 sota-gaps-plan. `list_agents`, `get_agent`, `RecentRun`, `AgentStats`, `AgentDetail`. Filtra runs por `agent_name`. Iter Iter desta revisao: `sort_by(b.started_at.cmp(&a.started_at))` → `sort_by_key(Reverse(...))` (clippy::stable_sort_primitive). Testes inline cobrem load + filter + stats accumulation. Sem deps externas extras.

### 3. use_cases::auth (62 LOC)
Headless `login` / `logout` flows. Wrapping de `theo-infra-auth::OpenAIAuth` + `CopilotAuth` para apps. Modulo minimal e focado.

### 4. use_cases::context_assembler (409 LOC)
Compose de context package per agent step: codebase context (GRAPHCTX) + memoria + working set. Drives `inject_episode_history`, `inject_prefetch`. Cobertura via `compaction_sanitizer_integration.rs`. Tamanho legitimo dado o numero de signals fundidas.

### 5. use_cases::conversion (28 LOC)
Conversoes parser ↔ graph DTO. Modulo trivial mas centralizador (DRY entre extraction.rs e graph_context_service.rs).

### 6. use_cases::extraction (197 LOC)
Tree-sitter parse → bridge::FileData. Driver de extracao incremental para o code wiki + GRAPHCTX. Cobertura via wiki_enrichment tests + graph_context tests.

### 7. use_cases::graph_context_service (1178 LOC)
Implementacao concreta de `GraphContextProvider` orquestrando 3 engines (parser → graph → retrieval). E o maior modulo do crate por boas razoes — fusiona BM25 + dense embeddings + graph signals via RRF (Reciprocal Rank Fusion). Iter desta revisao: `#[allow(dead_code)]` no campo `scorer` (presente quando feature `tantivy-backend` esta off mas sem readers no caminho de fallback). **Followup nao-bloqueador**: candidato a futura split (Tier 0/1/2 retrieval modules separados) — fora do escopo do REVIEW desta camada.

### 8. use_cases::guardrail_loader (47 LOC)
Phase 23 sota-gaps-followup. Carrega `.theo/handoff_guardrails.toml` em `GuardrailChain`. Modulo minimal. Cobertura inline.

### 9. use_cases::impact (170 LOC)
BFS-based impact analysis para GRAPHCTX. Dado um symbol, encontra dependentes/dependencies via grafo. Foco bem definido.

### 10. use_cases::memory_factory (67 LOC)
Phase 0 T0.2. `build_memory_engine(config, project_dir)` retorna `Option<Arc<dyn MemoryProvider>>`. `attach_memory_to_config` wrapper. Cobertura inline (memory factory tests). Iter desta revisao migrou reads para `config.memory().enabled` (T4.1 Iter 64).

### 11. use_cases::memory_lint (10 LOC)
Re-export wrapper. ADR-004 bounded-context: apps nunca importam `theo-infra-memory` direto.

### 12. use_cases::memory_ui (89 LOC)
Surface UI para apps (Tauri / CLI TUI). Modulo wrapper sobre `theo-infra-memory`.

### 13. use_cases::observability_ui (280 LOC)
Surface UI para observabilidade (agent dashboard, tools-by-usage, tools-by-failure-rate). Iter desta revisao: `sort_by` → `sort_by_key(Reverse)` (clippy fix, mesmo padrao do agents_dashboard).

### 14. use_cases::pipeline (520 LOC)
Pipeline orchestrator que combina retrieval + GRAPHCTX para responder query. Drives o context_assembler. Tamanho ainda dentro do soft-target do crate.

### 15. use_cases::router_loader (80 LOC)
Phase 27 sota-gaps-followup gap #4. Le `.theo/config.toml` `[routing]` section em `RoutingConfig`. Iter desta revisao: removeu unused import `ModelRouter` no test (clippy fix).

### 16. use_cases::run_agent_session (147 LOC)
End-to-end execucao de sessao. Iter Iter 64 migrou guard `config.api_key.is_none()` para `config.llm().api_key.is_none()` (T4.1).

### 17. use_cases::transcript_indexer_impl (223 LOC)
Tantivy-backed `TranscriptIndexer`. Feature-gated por `tantivy-backend`. Wired em memory_factory::attach_memory_to_config (Iter 64).

### 18. use_cases::wiki_backend_impl (160 LOC)
Backend concreto de wiki. Implementa o trait wiki abstracto.

### 19. use_cases::wiki_enrichment (195 LOC)
Enriquecimento LLM-driven de paginas wiki (commands `/explain`, `/summarize`).

### 20. use_cases::wiki_highlevel (267 LOC)
API high-level: search, render, list. Apps consomem este modulo em vez de tocar `theo-engine-retrieval` direto.

---

## Conclusao

Todos os 20 modulos da camada `theo-application` foram revisitados:
- 11 listados no REVIEW original — agora **Revisado**
- 9 modulos adicionais (`memory_ui`, `observability_ui`, `pipeline`, `router_loader`, `run_agent_session`, `transcript_indexer_impl`, `wiki_backend_impl`, `wiki_enrichment`, `wiki_highlevel`) que existiam em `use_cases/` mas nao apareciam no REVIEW — agora documentados e revisados.

**Hygiene fixes aplicados nesta auditoria:**
- `graph_context_service.rs:65` — `#[allow(dead_code)]` no campo `scorer` (cfg-gated, sem readers no fallback path).
- `agents_dashboard.rs:150,177` — `sort_by` → `sort_by_key(Reverse)` (2 sites).
- `observability_ui.rs:261` — mesmo padrao.
- `router_loader.rs:187` — removed unused import `ModelRouter` no test.

**Estado final:**
- 141 tests passando, 0 falhas
- `cargo clippy -p theo-application --lib --tests` zero warnings em codigo proprio
- Warnings remanescentes apenas em deps externos (theo-infra-mcp 2, theo-infra-llm 4)
- Bounded-context invariant preservado: apps consomem `theo-application::facade` + `theo-api-contracts` exclusivamente

Sem follow-ups bloqueadores. O candidato `graph_context_service.rs` (1178 LOC) a futura split em Tier 0/1/2 retrieval modules e nao-bloqueador e fora do escopo deste REVIEW.
