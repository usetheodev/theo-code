# Evolution Research: Context Manager

## Referências Consultadas

### OpenDev (Rust — referência primária)
**Arquivos:** `opendev-context/src/compaction/mod.rs`, `validated_list.rs`, `pair_validator.rs`, `attachments/mod.rs`, `compaction/artifacts.rs`, `compaction/tokens.rs`, `compaction/compactor/stages.rs`

**Padrões extraídos:**
1. **Staged Compaction**: 6 thresholds progressivos (70%→99%) em vez de cutoff único. Cada estágio é um algoritmo composable (mask → prune → summarize → LLM compact)
2. **Validated Message List**: Enforcement de invariantes em write-time — tracking de pending tool_call IDs, auto-complete de missing results com erros sintéticos
3. **Artifact Index**: Metadata de file operations separada do conversation history, sobrevive compaction. HashMap<path, ArtifactEntry> com operation count e timestamps
4. **Token Heuristics**: Estimativa cl100k_base sem dependência externa. Calibração incremental via API feedback

### QMD (TypeScript)
**Arquivos:** `src/collections.ts`, `src/store.ts`

**Padrões extraídos:**
5. **Hybrid Search Fusion**: BM25 + vector + LLM re-ranking com Reciprocal Rank Fusion. Strong signal detection (BM25 ≥ 0.85) para skip de re-ranking caro
6. **Smart Chunking**: Break points baseados em pattern scoring (H1=100, code blocks=80, blank lines=20) com distance decay quadrático

### Pi-Mono (TypeScript)
**Arquivos:** `compaction/compaction.ts`, `compaction/utils.ts`, `system-prompt.ts`

**Padrões extraídos:**
7. **Budget-Driven Compaction**: Trigger simples (tokens > window - reserve). Cut point detection que respeita turn boundaries
8. **File Operation Tracking**: Sets de read/written/edited files extraídos de tool calls, serializados como XML tags em summaries

## Notas de Adaptação (TS/Python → Rust)
- OpenDev já é Rust — padrões podem ser adaptados diretamente
- QMD `collections.ts` → trait-based approach com generics em Rust
- Pi-Mono `compaction.ts` → pode usar o mesmo pattern de budget check, adaptando para o assembler existente

## Plano de Implementação

**Iteração 1 — BudgetReport (observabilidade de budget)**
- Adicionar struct `BudgetReport` em `graph_context.rs` (theo-domain)
- Campos: allocated, used, skipped_count, skipped_reasons
- Retornar junto com `GraphContextResult`
- Impacto: Completeness +1, Testability +1 (testável isoladamente)

**Iteração 2 — DropReason tracking (métricas de qualidade)**
- Adicionar enum `DropReason` em context_metrics.rs
- Rastrear por que blocos foram excluídos (BudgetExhausted, LowScore, Stale)

**Iteração 3+ — Staged compaction tiers (se convergência não atingida)**
