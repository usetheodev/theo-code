# ADR-011: Retrieval may depend on Graph and Parser

**Status:** Aceito
**Data:** 2026-04-23
**Autor:** Audit remediation (iteration 2)
**Escopo:** `.claude/rules/architecture.md`, `.claude/rules/architecture-contract.yaml`, `scripts/check-arch-contract.sh`
**Fecha T1.6** do plano de remediação.

---

## Contexto

O gate `scripts/check-arch-contract.sh` (implementado em T1.5) revelou **52 violações de import ao nível de `src/`** que não apareciam no audit inicial:

- **`theo-engine-retrieval`** importa `theo_engine_graph` e `theo_engine_parser` em ao menos 15 arquivos (`dense_search.rs`, `graph_attention.rs`, `tantivy_search.rs`, `assembly.rs`, `file_retriever.rs`, `wiki/generator.rs`, etc.).
- **`theo-infra-memory`** importa `theo_engine_retrieval` em `retrieval/tantivy_adapter.rs`.

A **tabela** em `architecture.md` diz `theo-engine-* → theo-domain only`, o que proibiria esses imports. Porém, o **texto em prose** do mesmo arquivo diz:

> 1. **Code Intelligence**: `theo-engine-graph`, `theo-engine-parser`, `theo-engine-retrieval`
>    - Parser and graph are read-only over source code
>    - **Retrieval consumes graph, never the reverse**

Existe contradição entre tabela e prose. O código real segue o prose.

Da mesma forma, `theo-infra-memory` ≥ requer similarity search para o backend Tantivy (feature `tantivy-backend`), o que justifica a dep em `theo-engine-retrieval`.

## Decisão

Aceitamos que:

1. **`theo-engine-retrieval` pode depender de `theo-domain`, `theo-engine-graph` e `theo-engine-parser`.** Direção permanece unidirecional (retrieval consome graph/parser, nunca o contrário — graph e parser continuam com dep set `[theo-domain]`).

2. **`theo-infra-memory` pode depender de `theo-domain` e, opcionalmente (feature-gated), de `theo-engine-retrieval`.** A dep é optional + feature flag, alinhada com `tantivy-backend`.

3. A tabela em `architecture.md` é **atualizada** para refletir o prose:
   ```
   theo-engine-graph    → theo-domain
   theo-engine-parser   → theo-domain
   theo-engine-retrieval → theo-domain, theo-engine-graph, theo-engine-parser
   theo-infra-memory    → theo-domain, [theo-engine-retrieval optional]
   ```

4. `architecture-contract.yaml` e o gate `check-arch-contract.sh` devem ser alinhados ao novo contrato. **As 52 violações de import relacionadas a retrieval e infra-memory são dissolvidas** por esta decisão (deixando apenas violações genuínas em agent-runtime e apps).

## Por que não refatorar para seguir a tabela antiga

- **Custo injustificado**: "extrair o intersection para theo-domain" implicaria mover ~20+ tipos (símbolos do grafo, metadados do parser, rankings de busca) para um crate cada vez mais inchado — violaria SRP e aumentaria o acoplamento em vez de diminuí-lo.
- **Semântica já está alinhada com o código**: o prose descreve exatamente como o sistema já funciona. A tabela estava simplificada demais.
- **Não arranha a regra "unidirectional"**: graph → retrieval é proibido; retrieval → graph permanece permitido; ciclos continuam banidos.

## Consequências

- **Gate ganha alinhamento com a realidade** — after atualizar o YAML + bash embed, as 52 violações de import caem para zero nessas crates.
- **As violações que sobram** (agent-runtime importando infra-\* + tooling, e apps importando engine/infra diretamente) permanecem **violações genuínas** a serem resolvidas em T1.1 / T1.2 / T1.3.
- **`architecture.md` precisa de uma pequena revisão** para a tabela — feita no mesmo PR deste ADR.

## Riscos

- **Risco:** abrir precedente para relaxar outras regras.
  **Mitigação:** este ADR lista explicitamente o prose existente como base — não é mudança de política, é reconciliação documental.
- **Risco:** retrieval pode inchar se ganhar mais deps de graph/parser.
  **Mitigação:** `check-sizes.sh` (T4.6) já limita o crescimento por arquivo; os god files em retrieval (`generator.rs`, `assembly.rs`, `file_retriever.rs`) já estão na allowlist com sunset 2026-07-23.
