# ADR-010: Architectural Contract — "allowed" means upper bound, not mandate

**Status:** Aceito
**Data:** 2026-04-23
**Autor:** Audit remediation (iteration 2)
**Escopo:** `.claude/rules/architecture.md`, `.claude/rules/architecture-contract.yaml`, `scripts/check-arch-contract.sh`, `crates/theo-engine-parser/Cargo.toml`, `crates/theo-infra-auth/Cargo.toml`
**Fecha T1.4** do plano de remediação (`docs/audit/remediation-plan.md`).

---

## Contexto

A tabela em `.claude/rules/architecture.md` dita:

```
theo-domain         → (nothing)
theo-engine-*       → theo-domain only
theo-governance     → theo-domain only
theo-infra-*        → theo-domain only
theo-tooling        → theo-domain only
theo-agent-runtime  → theo-domain, theo-governance
```

O relatório do audit `/code-audit all` (2026-04-23) interpretou "engine-\* → theo-domain only" como **obrigação** e flageou `theo-engine-parser` e `theo-infra-auth` por não declararem `theo-domain` em `Cargo.toml`. Porém, nenhum dos dois crates importa tipos de `theo-domain` — são crates pragmaticamente independentes:

- **`theo-engine-parser`**: tree-sitter + tipos próprios (`src/types.rs`, ~1700 LOC) — não usa nenhum tipo de domínio compartilhado.
- **`theo-infra-auth`**: OAuth PKCE + device flow — opera sobre `Token`, `Code`, `Pkce`, todos locais.

A questão é se a tabela deve ser lida como:

1. **Mandate**: "cada crate engine-\* DEVE depender de theo-domain" (e, portanto, precisaríamos criar um tipo compartilhado fictício, ou importar algo só para satisfazer a regra).
2. **Upper bound**: "cada crate engine-\* pode depender no máximo de theo-domain" (nenhum dep é aceitável; extra é que é proibido).

## Decisão

Adotamos a **interpretação upper bound (opção 2)**:

> Toda entrada na tabela de direção de dependências define o **conjunto máximo** de workspace crates permitidos. Um crate pode declarar menos deps (inclusive zero) desde que não declare nenhuma que esteja fora do conjunto.

Consequências operacionais:

- `scripts/check-arch-contract.sh` já implementa essa semântica (falha apenas quando aparece uma dep fora do conjunto permitido).
- `theo-engine-parser` e `theo-infra-auth` **permanecem conformes** sem ajuste de `Cargo.toml`.
- `.claude/rules/architecture.md` deve ter uma nota curta esclarecendo a leitura.

## Por que não a interpretação mandate

- Violaria **YAGNI** (Parte II §11): criar/importar tipos só para satisfazer uma regra é abstração sem uso real.
- Violaria **KISS** (Parte II §10): adicionar uma dep "por formalidade" é um acoplamento contra-produtivo.
- Criaria cargo de manutenção: toda vez que `theo-domain` mudar, todos os crates "marcados" teriam que recompilar mesmo sem usar nada dele.

## Riscos conhecidos e mitigação

- **Risco:** drift silencioso — alguém precisa de um tipo que devia estar em theo-domain mas evita porque não quer "acender" a dep.
  **Mitigação:** code review + manter a tabela como alvo aspiracional. Se um tipo precisa ser compartilhado, move-se para theo-domain e a dep é adicionada então.
- **Risco:** esconde falta de teste de contrato.
  **Mitigação:** T1.5 (`check-arch-contract.sh`) já é o gate e roda em CI.

## Consequências

- `theo-engine-parser` e `theo-infra-auth` **compliant sem mudanças**; fechando T1.4 no plano.
- Futuros crates podem declarar zero workspace deps enquanto permanecerem self-contained.
- `.claude/rules/architecture.md` ganhará um pequeno esclarecimento (veja PR que acompanha este ADR — apenas adição de linha "upper bound").
