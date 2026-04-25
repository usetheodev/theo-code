# ADR-023: Temporary direct import of `theo-agent-runtime` from `apps/theo-cli`

**Status:** Aceito (com sunset)
**Data:** 2026-04-25
**Autor:** Audit remediation (T0.4 helper for T3.3)
**Escopo:** `scripts/check-arch-contract.sh`, `apps/theo-cli/src/{dashboard_agents,runtime_features,subagent_admin}.rs`
**Fecha (temporariamente):** find_p3_009
**Sunset target:** Quando T3.3 (encapsulamento via `theo-application`) for mergeada — alvo Q3 2026.

---

## Contexto

O contrato arquitetural de camadas é:

```
apps/* → theo-application → (engine + infra)
```

Três arquivos em `apps/theo-cli/src/` violam essa regra ao importar
`theo_agent_runtime` diretamente:

- `dashboard_agents.rs`
- `runtime_features.rs`
- `subagent_admin.rs`

Antes de T0.1, o gate detectava as violações mas era reportado em modo
`--report` (exit 0). Após T0.1 corrigir o regex e T0.4 aplicar mode
strict, essas violações começariam a falhar o CI imediatamente — antes
que T3.3 (encapsulamento via `theo-application`) seja completada.

## Decisão

Adicionar `theo-agent-runtime` ao allowlist de `apps/theo-cli` no
`scripts/check-arch-contract.sh`, **explicitamente marcado como
temporário**:

```bash
['apps/theo-cli']='theo-application theo-api-contracts theo-domain theo-agent-runtime'  # ADR-023 sunset
```

Esta exceção **NÃO se aplica a outros apps** (`theo-desktop`,
`theo-marklive`).

A exceção termina quando T3.3 for mergeada — três use cases serão
movidos para `theo-application` e os 3 imports diretos removidos. Após
isso, este ADR é superseded e o allowlist volta a `theo-application
theo-api-contracts theo-domain`.

## Consequências

- O CI passa enquanto T3.3 não é trabalhada.
- A exceção é visível para todo desenvolvedor que ler o gate ou este
  ADR — não há "amnésia institucional" sobre a violação pendente.
- Se T3.3 demorar muito (>1 trimestre), este ADR deve ser revisitado:
  ou T3.3 é priorizada, ou a exceção é re-justificada com um ADR novo.

## Sunset criteria

T3.3 fechada quando todas as 3 condições forem verdadeiras:

1. `grep -r "use theo_agent_runtime" apps/theo-cli/src/` retorna 0
2. Allowlist de `apps/theo-cli` no gate volta a `theo-application
   theo-api-contracts theo-domain` (sem `theo-agent-runtime`)
3. CI verde com a allowlist restritiva

## Referências

- find_p3_009 (deep review — CLI layering)
- T3.3 do `agent-runtime-remediation-plan.md`
- ADR-016 (regra de camadas base)
