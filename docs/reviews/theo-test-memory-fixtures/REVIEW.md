# theo-test-memory-fixtures — Revisao

> **Contexto**: Fixtures de teste compartilhadas entre testes de integracao de memory/wiki.
>
> **Invariante**: `publish = false`. Nao deve aparecer em nenhum grafo de dependencia de producao.

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `mock_llm` | `MockCompilerLLM`, `CompilerCall`, `CompilerResponse` — mock de LLM compilador para testes. | Pendente |
| 2 | `mock_retrieval` | `MockRetrievalEngine`, `ScoredEntry` — mock do retrieval engine. | Pendente |
