# theo-test-memory-fixtures — Revisao

> **Contexto**: Fixtures de teste compartilhadas entre testes de integracao de memory/wiki.
>
> **Invariante**: `publish = false`. Nao deve aparecer em nenhum grafo de dependencia de producao.
>
> **Status global**: deep-review concluido em 2026-04-25. 9 tests passando, 0 falhas. `cargo clippy --lib --tests` silent. `Cargo.toml` confirmado: `publish = false` + apenas dev-dependency em outros crates.

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `mock_llm` | `MockCompilerLLM`, `CompilerCall`, `CompilerResponse` — mock de LLM compilador para testes. | Revisado |
| 2 | `mock_retrieval` | `MockRetrievalEngine`, `ScoredEntry` — mock do retrieval engine. | Revisado |

---

## Notas de Deep-Review

### 1. mock_llm
`MockCompilerLLM { calls: Mutex<Vec<CompilerCall>>, responses: Mutex<VecDeque<CompilerResponse>> }`. Pre-queue de responses + capture de calls para asserts post-test. `CompilerCall { prompt, system, model }`, `CompilerResponse { content, tokens_in, tokens_out }`. Determinismo: nao depende de network, side-effect-free.

### 2. mock_retrieval
`MockRetrievalEngine` impl `MemoryRetrieval` com Vec<ScoredEntry> pre-staged. `ScoredEntry { id, content, score, source }`. Permite test-time reordering + threshold testing sem subir tantivy.

**Invariantes verificados:**
- `Cargo.toml`: `publish = false` ✓ — nunca publicado em registry, nunca em grafo de producao
- Aparece apenas como `[dev-dependencies]` nos consumers (theo-infra-memory, theo-engine-retrieval)
- 9 tests passando (sanity dos proprios mocks)
- `cargo clippy --lib --tests` silent

Sem follow-ups bloqueadores.
