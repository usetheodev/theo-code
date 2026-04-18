# SOTA Criteria: Context Manager Evolution

**Target subsystem:** theo-domain (types), theo-application (assembler), theo-agent-runtime (metrics)
**Reference bar:** OpenDev (Rust, same language), QMD (search/ranking), Pi-Mono (compaction)

## O que SOTA significa para este prompt:

1. **Budget accounting transparente** — como OpenDev: saber exatamente quanto budget foi alocado, usado, e desperdiçado por bloco/comunidade
2. **Drop reason tracking** — como OpenDev staged compaction: saber POR QUE um bloco não entrou no contexto (budget, score, stale)
3. **Artifact index** — como OpenDev: mapeamento reverso file→blocks que sobrevive compaction
4. **Token estimation calibrável** — como OpenDev: heurística + calibração via API feedback

## Melhoria mínima viável:
- BudgetReport struct com allocated/used/skipped tracking
- Pelo menos 1 teste verificando invariante budget_tokens

## Explicitamente fora de escopo:
- LLM-powered compaction (requer infra de LLM calls no assembler)
- Hybrid search fusion (é do retrieval engine, não do context manager)
- Message pair validation (requer refactor do pipeline de mensagens)
