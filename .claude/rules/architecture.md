# Regras Arquiteturais

## Bounded Contexts — Fronteiras Invioláveis

1. **Code Intelligence Engine**: `theo-engine-graph`, `theo-engine-parser`, `theo-engine-retrieval`
   - Parser e graph são read-only sobre código-fonte
   - Retrieval consome graph, nunca o contrário

2. **Agent Runtime**: `theo-agent-runtime`
   - Orquestra LLM + tools + governance
   - TODA tool call passa pelo Decision Control Plane

3. **Governance & Safety**: `theo-governance`
   - Policy engine, impact analysis, métricas
   - Obrigatória no caminho crítico

4. **Infrastructure**: `theo-infra-llm`, `theo-infra-auth`, `theo-tooling`
   - Implementações concretas atrás de traits do domain

## Dependências Permitidas

```
theo-domain         → (nenhuma — tipos puros)
theo-engine-*       → theo-domain
theo-governance     → theo-domain
theo-infra-*        → theo-domain
theo-tooling        → theo-domain
theo-agent-runtime  → theo-domain, theo-governance
theo-api-contracts  → theo-domain
theo-application    → todos os crates acima
apps/*              → theo-application, theo-api-contracts
```

## Proibições

- Apps NUNCA importam crates engine/infra diretamente
- `theo-domain` NUNCA depende de outros crates
- Dependências circulares são proibidas
- Nada de benchmark/research no runtime de produção
