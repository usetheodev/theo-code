# Meeting — 2026-04-01

## Proposta
Implementar Tool Registry com context engineering no theo-tooling. Cada tool declara seu schema e categoria, registry suporta filtros e geração automática de LLM tool definitions. Eliminar schemas hardcoded do tool_bridge.

## Participantes
- governance (Principal Engineer) — APPROVE com condições
- qa (QA Staff Engineer) — CONDICIONAL
- graphctx (Compiler Engineer) — APPROVE
- tooling (Systems Engineer) — APPROVE com guards

## Analises

### Governance
- Fronteiras arquiteturais respeitadas. DRY violation legítima eliminada.
- Exige: default methods no trait, remover ToolProfile do escopo (YAGNI), ToolSchema derive Serialize.

### QA
- 133 testes passando (98 tooling, 15 runtime, 20 domain). Risco não é regressão nos existentes — é divergência silenciosa de schemas.
- Exige: snapshot tests comparando schema pré/pós, testes de contrato para schema() em cada tool, schema() com default compilável.

### GraphCtx
- Grafo de dependências correto, sem circular. Breaking change mitigado por defaults.
- Alerta: mudança em theo-domain invalida cache de todos os downstream (recompilação completa).

### Tooling
- Risco teórico de schema injection (mitigado: tools são internos, compilados juntos).
- Exige: Registry valida schemas, schemas imutáveis após registro, validação em dois pontos.

## Conflitos
1. **ToolProfile**: Governance diz remover (YAGNI). Resolução: REMOVER do escopo — adicionar quando houver consumer concreto.
2. **Validação de schema**: Tooling quer centralizar no Registry. Resolução: Registry FAZ validação básica (tipo, properties obrigatórias) ao registrar. Tool valida args em runtime.
3. **Nível de risco**: Tooling diz CRITICAL, outros dizem MEDIUM. Resolução: MEDIUM — tools são internas, sem carregamento dinâmico.

## Veredito
**APPROVED**

## Escopo Aprovado
- `crates/theo-domain/src/tool.rs` — adicionar ToolSchema, ToolCategory, ToolDefinition, métodos com default no trait
- `crates/theo-domain/src/lib.rs` — re-exportar novos tipos
- `crates/theo-tooling/src/registry/mod.rs` — filtros por categoria, geração de definitions, validação básica de schema
- `crates/theo-tooling/src/read/mod.rs` — implementar schema() e category()
- `crates/theo-tooling/src/write/mod.rs` — implementar schema() e category()
- `crates/theo-tooling/src/edit/mod.rs` — implementar schema() e category()
- `crates/theo-tooling/src/bash/mod.rs` — implementar schema() e category()
- `crates/theo-tooling/src/grep/mod.rs` — implementar schema() e category()
- `crates/theo-tooling/src/glob/mod.rs` — implementar schema() e category()
- `crates/theo-tooling/src/apply_patch/mod.rs` — implementar schema() e category()
- `crates/theo-tooling/src/webfetch/mod.rs` — implementar schema() e category()
- `crates/theo-tooling/src/ls/mod.rs` — implementar schema() e category()
- `crates/theo-tooling/src/lsp/mod.rs` — implementar schema() e category()
- `crates/theo-tooling/src/websearch/mod.rs` — implementar schema() e category()
- `crates/theo-tooling/src/codesearch/mod.rs` — implementar schema() e category()
- `crates/theo-tooling/src/task/mod.rs` — implementar schema() e category()
- `crates/theo-tooling/src/skill/mod.rs` — implementar schema() e category()
- `crates/theo-tooling/src/question/mod.rs` — implementar schema() e category()
- `crates/theo-tooling/src/todo/mod.rs` — implementar schema() e category()
- `crates/theo-tooling/src/invalid/mod.rs` — implementar schema() e category()
- `crates/theo-tooling/src/batch/mod.rs` — implementar schema() e category()
- `crates/theo-tooling/src/multiedit/mod.rs` — implementar schema() e category()
- `crates/theo-tooling/src/plan/mod.rs` — implementar schema() e category()
- `crates/theo-agent-runtime/src/tool_bridge.rs` — consumir tool.schema() em vez de hardcoded

## Condicoes
1. **Default methods no trait** — `schema()` e `category()` DEVEM ter implementação default para migração incremental
2. **SEM ToolProfile** — removido do escopo por YAGNI. Adicionar apenas quando houver consumer concreto
3. **Snapshot tests** — testes comparando schema gerado vs schema hardcoded anterior para cada tool do default registry
4. **Testes de contrato** — cada tool que implementa schema() deve ter teste validando JSON válido com type + properties
5. **ToolSchema derive Serialize** — obrigatório para geração de LLM definitions
6. **Registry valida schema** — validação básica (type presente, properties é object) ao registrar tool
