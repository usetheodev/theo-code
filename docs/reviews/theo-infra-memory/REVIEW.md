# theo-infra-memory — Revisao

> **Contexto**: Infrastructure layer do agent memory subsystem. Fan-out sobre multiplos `MemoryProvider`s.
>
> **Dependencias permitidas** (ADR-011): `theo-domain`, `theo-engine-retrieval` (opcional, feature-gated `tantivy-backend`).

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `builtin` | `BuiltinMemoryProvider` — provider padrao embutido. | Pendente |
| 2 | `engine` | `MemoryEngine` + `EngineStats` — coordenador fan-out. | Pendente |
| 3 | `fs_util::atomic_write` | Escrita atomica temp-file-plus-rename (evita torn files). | Pendente |
| 4 | `lint` | `run_lint`, `LessonMetric`, `LintInputs`, `LintIssue`, `LintThresholds`, `Severity`, `render_json`. | Pendente |
| 5 | `retrieval::tantivy_adapter` | Adaptador tantivy para retrieval de memoria (feature-gated). | Pendente |
| 6 | `retrieval (core)` | `MemoryRetrieval`, `RetrievalBackedMemory`, `ScoredMemory`, `SourceType`, `ThresholdConfig`, `pack_within_budget`. | Pendente |
| 7 | `security::scan` | `scan` + `InjectionReason` — deteccao de injection em memorias. | Pendente |
| 8 | `session_search_fs` | `FsSessionSearch` + `render_hits` — busca em sessoes via filesystem. | Pendente |
| 9 | `wiki::compiler` | Compilador wiki: codigo → paginas. | Pendente |
| 10 | `wiki::hash` | `HashManifest`, `SourceHash` — controle de mudanca. | Pendente |
| 11 | `wiki::lint` | `lint_pages` — lint de paginas wiki. | Pendente |
| 12 | `wiki::parse_page` | Parser de paginas wiki. | Pendente |
