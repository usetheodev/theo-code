# Meeting — 2026-04-07 (Code Wiki v2: Review FAANG + Scope Decision)

## Proposta
Redesign da Code Wiki baseado em review externo (6/10). Introduzir IR canônico, proveniência, seções operacionais.

## Participantes
- Governance, GraphCTX

## Análises

### Governance
APPROVE v1 pragmática. Review correto em princípio, mas 10 fases viola YAGNI. v1 com: WikiDoc IR, proveniência, seções operacionais, manifest versionado. Defer: flow pages, chunks, evals, enrichment.

### GraphCTX
Campos deriváveis do graph HOJE: entry_points (in-degree 0), proveniência (file_path + lines), flow chains (2-hop BFS sobre Calls), responsibilities estruturais (SymbolKind distribution). Failure modes e invariants requerem LLM — defer.

## Conflitos
1. Review pede tudo agora vs YAGNI diz v1 primeiro
2. Flow pages: review quer antes de enrichment, viável com 2-hop. Incluir como stretch goal v1.

## Decisão: v1 pragmática com melhorias do review

### Incluir na v1:
- WikiDoc struct (IR canônico) com campos determinísticos
- Proveniência (source_files + symbols por seção)
- Entry points (graph-derived)
- Seções: Files, Public API, Entry Points, Dependencies, Call Flow, Test Coverage
- Manifest versionado (schema_version, generator_version, graph_hash)
- Renderização markdown com [[wiki-links]]
- Disk persistence + cache invalidation

### Defer para v2:
- Failure modes, Invariants (requerem LLM)
- Flow pages (stretch goal, viável mas complexo)
- Retrieval chunks + integração query_context
- LLM enrichment
- Evals de utilidade
- ApiPage, ConceptPage (vistas adicionais)

## Veredito
**APPROVED**

## Escopo Aprovado
- Novo: `crates/theo-engine-retrieval/src/wiki/` (mod.rs, model.rs, generator.rs, renderer.rs)
- Mod: `crates/theo-engine-retrieval/src/lib.rs`
- Mod: `crates/theo-application/src/use_cases/graph_context_service.rs`

## Condições
1. WikiDoc como IR separado da renderização markdown
2. Proveniência verificável (source_files existem, symbols resolvem)
3. Entry points derivados do graph (in-degree 0 em Calls)
4. Manifest com schema_version para compatibilidade futura
5. Unit tests para cada componente
6. Wiki abre corretamente no Obsidian
