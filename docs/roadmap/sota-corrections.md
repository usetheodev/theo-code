# SOTA Corrections: Budget Analysis → Implementation

> Problemas encontrados no budget analysis de projetos grandes (FFmpeg 4.6K files, Linux GPU 7.4K files).
> Cada fix tem DoD testavel.

## Problemas e Fixes

### FIX-1: Adaptive token budget por repo size
**Problema**: Budget fixo de 4K tokens. Em FFmpeg, 26 files = 0.56% do repo.
**Fix**: Budget escala com sqrt(file_count). Min 4K, max 32K.
**Crate**: theo-application/context_assembler.rs

### FIX-2: Incremental content hash (skip unchanged files)
**Problema**: compute_project_hash le TODOS os arquivos. Linux GPU = 3.8s.
**Fix**: Mtime como pre-filtro, so re-hash se mtime mudou. Cache em .theo/hash_cache.json.
**Crate**: theo-application/graph_context_service.rs

### FIX-3: Adaptive budget split (hot_files vs structural vs events)
**Problema**: Hot files competem com structural context no budget. 20 hot files = 75% do budget.
**Fix**: Reservar proporcoes fixas: 15% task/step, 25% events+hot, 60% structural.
**Crate**: theo-application/context_assembler.rs

### FIX-4: Community summary compression
**Problema**: ~150 tokens per file. Muito para repos grandes.
**Fix**: Tier de compressao: top-3 communities = full, next-5 = compressed (signatures only), rest = names only.
**Crate**: theo-engine-retrieval/assembly.rs (ja tem assemble_with_code — extend)

### FIX-5: SOTA eval thresholds
**Problema**: Thresholds atuais conservadores (MRR≥0.80). SOTA exige ≥0.92 Recall.
**Fix**: Subir gates + adicionar metricas faltantes ao eval CI.
**Crate**: theo-engine-retrieval/tests/eval_golden.rs
