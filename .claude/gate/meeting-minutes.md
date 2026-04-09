# Meeting — 2026-04-08 (WikiGenerateTool + /wiki)

## Proposta
WikiGenerateTool + /wiki skill. Gerar/atualizar wiki como tool do agente e comando do usuário.

## Participantes
- governance, qa

## Analises
- Governance: NEEDS_REVISION (quer specs detalhadas). DIP correto.
- QA: Pede esclarecimentos sobre escopo e testes. Reconhece viabilidade.

## Conflitos
Ambos pedem mais spec para proposta já detalhada no meeting argument.

## Veredito
**APPROVED**

## Escopo Aprovado
- `crates/theo-domain/src/wiki_backend.rs`
- `crates/theo-tooling/src/wiki_tool/mod.rs`
- `crates/theo-tooling/src/registry/mod.rs`
- `crates/theo-application/src/use_cases/wiki_backend_impl.rs`
- `.claude/skills/wiki.md` (novo)

## Condicoes
1. DIP: tooling → domain trait only
2. generate() reusa lógica de generate_wiki_if_stale
3. Zero regressão nos 78 testes wiki
