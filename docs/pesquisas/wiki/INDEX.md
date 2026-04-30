# Theo Wiki — Pesquisa SOTA

## Objetivo
Wiki compilada por LLM para **humanos** entenderem codebases.
Ler código é moroso e complicado. O Theo Wiki resolve esse problema.

## O Contrato Fundamental

```
HUMANO = LEITOR     → lê, navega, consulta. Nunca escreve.
WIKI AGENT = ESCRITOR → sub-agente built-in, roda em background,
                        ativado por triggers automáticos.
                        Único escritor do wiki.
MANUAL = OPCIONAL   → theo wiki generate força update. Raro.
```

O wiki é VIVO. O Wiki Agent reage a eventos (commit, ADR, tests,
session end, cron) e mantém tudo atualizado sem intervenção humana.

## Arquitetura: Skeleton + Enrichment
- **Skeleton** (tree-sitter, grátis): estrutura, arquivos, símbolos, APIs, deps
- **Enrichment** (LLM via Wiki Agent): "o que faz", "por quê", "como funciona", "o que quebra"
- O skeleton já existe. O enrichment é o que transforma inventário em documentação.

## Triggers do Wiki Agent
| Trigger | Ação |
|---|---|
| git commit | Re-enrich module pages afetadas |
| ADR novo | Decision page + module pages atualizadas |
| cargo test | Test coverage atualizada |
| Session end | Ingere insights da sessão |
| Cron | Lint completo + freshness check |
| Manual (opcional) | `theo wiki generate` — full rebuild |

## Crates alvo
- `theo-engine-retrieval::wiki` — generator, renderer, persistence, lookup
- `theo-agent-runtime` — Wiki Agent como sub-agente + trigger system
- `theo-application` — WikiBackend impl, enrichment
- `theo-tooling` — wiki tools (generate, query, lint)
- `theo-domain` — WikiBackend trait

## Thresholds Summary
- Level 1 (3.0): Wiki Agent + cold start + enrichment + manual override
- Level 2 (4.0): Triggers ativos, ADR ingest, lint periódico, humano nunca intervém
- Level 3 (5.0): Todos triggers, compounding, concept promotion, freshness < 48h

## Arquivos nesta pasta
- `wiki-system-sota.md` — Full SOTA research (v3, 2026-04-30)
