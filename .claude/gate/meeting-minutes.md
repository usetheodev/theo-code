# Meeting — 2026-04-06 (fastembed Decision — BM25-only default)

## Proposta
Eliminar fastembed como bottleneck (28s). Duas ações: fix warm path bug + fastembed como feature flag.

## Participantes
- **governance** — Opção C (feature flag, default BM25-only). Neural = 20% do score. Fallback já existe.
- **graphctx** — Confirmou: NeuralEmbedder em build() não score(). quantized_docs cacheáveis. BM25+graph+centrality cobrem 80%.

## Decisão
**D + C combinados:**
1. Fix D: cmd_stats warm path não deve triggerar scorer
2. Opção C: fastembed off by default. Neural habilitado via THEO_NEURAL=1

## Veredito
**APPROVED**

## Escopo Aprovado
- `crates/theo-engine-retrieval/src/search.rs` (skip NeuralEmbedder quando THEO_NEURAL não set)
- `apps/theo-cli/src/main.rs` (fix cmd_stats warm triggering scorer)
- `crates/theo-application/src/use_cases/pipeline.rs` (ensure lazy scorer path correct)

## Condições
1. NeuralEmbedder::new() só chamado se THEO_NEURAL=1 env var set
2. Fallback TF-IDF já existe — deve ser o default path
3. Testes devem passar tanto com neural ON quanto OFF
4. Benchmark: stats WARM < 1s, context COLD < 5s, context WARM < 2s
