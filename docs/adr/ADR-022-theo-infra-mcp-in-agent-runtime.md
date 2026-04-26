# ADR-022: `theo-agent-runtime` may depend on `theo-infra-mcp`

**Status:** Aceito
**Data:** 2026-04-25
**Autor:** Audit remediation (T0.4 from agent-runtime-remediation-plan.md)
**Escopo:** `.claude/rules/architecture.md`, `scripts/check-arch-contract.sh`, `crates/theo-agent-runtime/Cargo.toml`, `crates/theo-application/Cargo.toml`
**Fecha:** find_p3_002 (parcial — `theo-infra-mcp` half), find_p5_001 (the regex bug that previously hid this dep)

---

## Contexto

`crates/theo-agent-runtime/Cargo.toml` declara:

```toml
theo-infra-mcp.workspace = true
```

usado por `crates/theo-agent-runtime/src/subagent/mcp_tools.rs` para
construir `McpToolAdapter` que converte respostas MCP em `Message`s do
runtime.

Esta dep estava invisível ao gate antes de T0.1 (find_p5_001).

## Avaliação arquitetural

Diferente de `theo-isolation` (ADR-021), o caso de `theo-infra-mcp` é
mais discutível: MCP é um protocolo de transporte específico, e numa
arquitetura purista o runtime falaria com qualquer MCP server através
de um trait `McpClient` definido em `theo-domain`.

Alternativas avaliadas:

| Alternativa | Análise |
|---|---|
| **A. Aceitar a dep** (escolhida) | Pragmática: `McpToolAdapter` precisa de tipos concretos do MCP SDK. Trait abstration adicionaria 1 indirection sem variação real (só temos 1 transport hoje). YAGNI. |
| **B. Definir `McpClient` em theo-domain** | Adicionaria abstração especulativa. MCP tem ~20 surface types (`Tool`, `Content`, `ResourceContent`, etc.) que duplicaríamos no domain. Violaria DRY no sentido de "um único representação canônica". |
| **C. Mover `mcp_tools.rs` para `theo-application`** | Possível mas quebra a fronteira: subagent é um conceito do runtime, não da camada de aplicação. Coesão domínio ficaria pior. |

## Decisão

`theo-infra-mcp` é uma dep autorizada de `theo-agent-runtime`. Por
transitividade arquitetural, também é autorizada em `theo-application`.

Esta autorização vem com **2 observações de risco** registradas em
remediação posterior:

1. **find_p6_003** — respostas MCP devem passar por `fence_untrusted`
   antes de virar `Message::tool_result`. Endereçada em T2.2.
2. **find_p5_003 / FIND-P6-012** — `theo-infra-mcp` traz transitive
   `rustls-webpki 0.103.12` (RUSTSEC-2026-0104). Endereçada em T0.2.

Caso `theo-infra-mcp` ganhe um segundo transport (gRPC, HTTP) no
futuro, este ADR deve ser revisitado e potencialmente superseded por
uma extração de trait `McpClient` em `theo-domain`.

## Consequências

- O gate `arch-contract` passa a verde para essa dep.
- Reforça a importância de fencing (T2.2) e de manter o supply chain
  patcheado (T0.2) — risco de aceitar a dep concreta.
- Marca um precedente: deps de transporte específico podem ficar em
  runtime quando há apenas 1 transport e a abstração seria especulativa.

## Referências

- ADR-016 — orchestrator deps base
- ADR-021 — outro caso similar (theo-isolation)
- find_p3_002, find_p5_001 (deep review)
- find_p6_003 (T2.2 follow-up — fence MCP responses)
- find_p5_003 (T0.2 follow-up — CVE bumps)
