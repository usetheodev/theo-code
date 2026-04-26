# ADR-012: Front-end Major Upgrade Strategy

**Status:** Aceito
**Data:** 2026-04-23
**Autor:** Audit remediation (iteration 5)
**Escopo:** `apps/theo-ui/package.json`, `docs/audit/remediation-plan.md` T3.4
**Fecha T3.4** do plano de remediação.

---

## Contexto

O audit de 2026-04-23 (`npm outdated`) apontou quatro majors pendentes em
`apps/theo-ui`:

| Pacote | Atual | Latest | Breaking-change notes |
| --- | --- | --- | --- |
| `react` | 18.3.1 | 19.x | Novo JSX Transform, `use` hook, `useFormStatus`; mudou semântica de refs, `forwardRef` descontinuado gradual. Requer `@types/react@19` + codemod. |
| `react-router` | 6.30.3 | 7.x | Data APIs consolidadas, `RouterProvider` como default, breaking em loaders/actions se usamos `<Route>` legado. |
| `tailwindcss` | 3.4.19 | 4.x | Novo engine (Oxide), `@import` via CSS em vez de `@tailwind`, breaking em `tailwind.config.*`, muitos plugins ainda não portaram. |
| `typescript` | 5.9.3 | 6.x | Ainda em alpha; sem ETA estável. |

O plano de remediação exige **decisão documentada** para cada major — seja
migração (com PR) ou "não migrar agora" com gatilho explícito.

## Decisão

| Pacote | Decisão | Gatilho para revisitar |
| --- | --- | --- |
| `react 18 → 19` | **Adiar** (próximo quarter) | Quando (a) `@tauri-apps/api` confirmar compat com React 19 **ou** (b) um feature precisar de `use` hook / Actions novos. |
| `react-router 6 → 7` | **Adiar** (próximo quarter) | Quando começarmos a migração do React 19 (vai junto; os codemods têm dependência). |
| `tailwindcss 3 → 4` | **Adiar** (6 meses) | Quando plugins que usamos (`tailwindcss-animate`) publicarem versão compat com v4. |
| `typescript 5 → 6` | **Adiar** (sem previsão) | Quando TS 6.x sair de alpha e virar recomendado pelo time Vite + tauri-apps. |

Em todos os casos a decisão atual é **"não migrar agora"**.

## Justificativas

### Por que adiar React 19

1. **Risco × benefício desequilibrado agora.** React 19 adiciona APIs que
   não usamos hoje (`use`, Actions, `useFormStatus`). Nenhum feature do
   roadmap em 90 dias exige esses primitives.
2. **Matriz de compat com Tauri v2.** A versão atual do `@tauri-apps/api@2`
   lista React 18 como testado; React 19 funciona em desktop mas ainda não
   é recomendado oficialmente. Preferimos esperar o selo verde.
3. **Codemod custo.** `@types/react` pede patches manuais em ~35 locais
   (`forwardRef`, implicit children, ref callbacks). É trabalho real que
   não entrega valor ao usuário.

### Por que adiar React Router 7

- Reuso de loaders/actions legados em ~6 rotas; migração exige refactor
  paralelo ao React 19. **Deve ir junto** com o upgrade do React.

### Por que adiar Tailwind 4

- `tailwindcss-animate` (usado em 9 componentes) ainda expõe apenas
  configs para Tailwind 3. Upgrade precoce quebra animações em todo
  `@radix-ui/react-tabs` e no sidebar.
- O engine Oxide é atrativo mas o ganho de performance é irrelevante para
  uma SPA de ~5k módulos; mudança sem ROI imediato.

### Por que adiar TypeScript 6

- Ainda em alpha; `vite@6` + `@types/react@18` não listam 6.x como target.

## Guard-rails

1. **Dependabot / Renovate** devem continuar abrindo PRs para `minor` e
   `patch` em todas as 4 libs. Esse ADR não bloqueia updates não-breaking.
2. **`scripts/check-arch-contract.sh`** não depende desses majors; o gate
   de licenças (`cargo deny check`) também não é afetado (frontend é npm).
3. **`npm audit --audit-level=high`** continua rodando em CI; qualquer
   CVE HIGH/CRITICAL em react / tailwind / router força revisão do ADR
   independente do cronograma.
4. **Próxima revisão obrigatória:** 2026-07-23 (sunset alinhado com a
   Phase-4 refactor window). Se neste ADR não houver update até lá, o
   arquivo é reaberto e cada linha avaliada de novo.

## Consequências

- 4 atualizações majors ficam explicitamente bloqueadas com gatilhos
  documentados — sem decisão silenciosa, sem drift.
- T3.4 do plano fica **fechada** neste ADR. Qualquer PR que tente bumpar
  um desses majors deve primeiro mover a linha correspondente aqui de
  "Adiar" para "Migrar" + referenciar o PR de migração.
