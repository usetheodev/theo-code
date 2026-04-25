# ADR-021: `theo-agent-runtime` may depend on `theo-isolation`

**Status:** Aceito
**Data:** 2026-04-25
**Autor:** Audit remediation (T0.4 from agent-runtime-remediation-plan.md)
**Escopo:** `.claude/rules/architecture.md`, `scripts/check-arch-contract.sh`, `crates/theo-agent-runtime/Cargo.toml`, `crates/theo-application/Cargo.toml`
**Fecha:** find_p3_002 (parcial — `theo-isolation` half), find_p5_001 (the regex bug that previously hid this dep)

---

## Contexto

A tabela em `.claude/rules/architecture.md` (e o ALLOWED_DEPS embutido em
`scripts/check-arch-contract.sh`) restringia `theo-agent-runtime` a
`{theo-domain, theo-governance, theo-infra-llm, theo-infra-auth,
theo-tooling}` per ADR-016 guard-rail #2.

Porém, `crates/theo-agent-runtime/Cargo.toml` declara desde antes deste
ADR uma dep adicional:

```toml
theo-isolation.workspace = true
```

Esta dep **estava invisível ao gate** por causa de find_p5_001 (regex
não reconhecia `.workspace = true`). Após a correção em T0.1, o gate
agora detecta a violação. Este ADR documenta retroativamente o racional
e atualiza o contrato.

## Por que `theo-isolation` faz sentido em `theo-agent-runtime`

`theo-isolation` provê o cascade de sandbox bwrap → landlock → noop usado
para executar tools que rodam comandos shell. O bounded context "Agent
Runtime" é responsável por orquestrar a execução de tools — incluindo
seleção e configuração da camada de isolamento. Mover a dep para
`theo-application` ou `theo-tooling` quebraria a coesão semântica: a
escolha do nível de sandbox depende do `AgentConfig` (que vive em
runtime), não de uma camada superior.

Alternativas avaliadas:

| Alternativa | Por que rejeitada |
|---|---|
| Definir `Sandbox` como trait em `theo-domain`, deixar runtime depender só do trait | Adicionaria abstração para 1 implementação real. Viola YAGNI. |
| Mover sandbox setup para `theo-application` | Quebraria a coesão: `AgentConfig` (runtime) precisaria expor parâmetros opacos para application. |
| Mover sandbox setup para `theo-tooling` | `theo-tooling` já depende de `theo-domain` e seria forçada a também depender de `theo-isolation`, criando uma layer cycle conceitualmente. |

## Decisão

`theo-isolation` é uma dep autorizada de `theo-agent-runtime`.

Como `theo-application` agrega todos os crates de runtime/engine, e
`theo-isolation` é alcançável via `theo-agent-runtime`, ela também é
autorizada em `theo-application` (transitividade arquitetural).

`scripts/check-arch-contract.sh` é atualizado para incluir
`theo-isolation` nos allowlists de:
- `crates/theo-agent-runtime`
- `crates/theo-application`

## Consequências

- O gate `arch-contract` passa a verde para essa dep.
- O contrato em `.claude/rules/architecture.md` precisa ser atualizado
  para refletir o allowlist expandido (próxima iteração).
- Se no futuro for necessário extrair traits para abstrair o sandbox,
  este ADR pode ser superseded por uma versão posterior.

## Referências

- ADR-016 — orchestrator deps base
- ADR-011 — pattern de prose vs tabela diff em rules/architecture.md
- find_p3_002 (review-output/findings/architecture/architecture_review.md)
- find_p5_001 (regex fix em T0.1)
