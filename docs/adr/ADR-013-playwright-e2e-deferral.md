# ADR-013: Defer Playwright browser E2E suite

**Status:** Aceito
**Data:** 2026-04-23
**Autor:** Audit remediation (iteration 10)
**Escopo:** `apps/theo-ui`, `docs/audit/remediation-plan.md` T5.7
**Fecha T5.7** do plano de remediação.

---

## Contexto

O plano de remediação inclui T5.7:

> Avaliar Playwright para 3 fluxos críticos da UI (login, chat, settings).
> Se sim, 3 testes rodando em CI headless.

Playwright resolveria o gap "UI não tem E2E". Hoje `apps/theo-ui` tem:

- **Vitest + Testing Library** (unit + component tests).
- Nenhum teste end-to-end.

Custo estimado de adotar Playwright:

| Item | Custo |
| --- | --- |
| Nova devDep (`@playwright/test`) | ~80 MiB do browser bundle |
| CI runner capaz de rodar browser headless | +2–3 min / PR |
| Test infra (page-objects, fixtures, auth stub) | ~3 dias iniciais + manutenção |
| Flakiness inerente a E2E | +P0 bug budget |

Benefício imediato:

- 3 fluxos críticos cobertos.
- Regressões visuais detectáveis.

## Decisão

**Adiar** a adoção de Playwright em `apps/theo-ui` para **após** os
blockers de arquitetura T1.2/T1.3 serem resolvidos.

### Por quê adiar agora

1. **`apps/theo-ui` ainda é Tauri-driven.** Playwright roda num browser
   real; quase toda chamada IPC ao backend Rust é stubbed — o que o
   teste realmente exercita é o mock, não o sistema integrado.
   **T5.6** (Desktop Tauri IPC tests) dá cobertura do caminho real IPC
   → React, e entrega muito mais sinal do que Playwright isolado.

2. **Superfícies em fluxo.** `T4.4` planeja decompor `SettingsPage.tsx`
   (466 LOC). Testes E2E escritos agora seriam reescritos em Phase-4.

3. **`apps/theo-ui` tem Vitest + Testing Library.** A pirâmide ainda
   não tem cobertura de unitários suficiente para justificar E2E (a
   ordem canônica é unit → component → integration → E2E).

4. **CI budget.** Adicionar 2–3 min de Playwright empurra o pipeline
   para fora da janela de 8 min em que desenvolvedores esperam o
   feedback fica "no mesmo café". O custo é real.

### Gatilho para revisitar

Re-abrimos este ADR quando qualquer uma das condições abaixo for
verdade:

1. **T1.2 + T1.3 + T5.6 concluídas**, e ainda existe bug-escape em UI
   que teste IPC não detectaria (ex.: bug visual puro).
2. **Uma regressão de front-end** chega em produção por falta de E2E.
3. **UI ganha fluxos fora do Tauri** (web app pública, por exemplo) —
   nesse caso Playwright passa a cobrir terreno novo, não terreno
   stubbed.

## Guard-rails até lá

- `apps/theo-ui` mantém `vitest` como gate mínimo.
- `apps/theo-ui` ganha `madge --circular` via `npm run audit:circ`
  (já entregue em T0.2) para proteger a saúde do módulo gráfico.
- Todo PR de UI com mudança em `features/**` ou `pages/**` deve
  acrescentar pelo menos um teste Vitest.
- Revisão obrigatória deste ADR em **2026-10-23** (6 meses).

## Consequências

- **T5.7 fecha** como decisão explícita "não agora" — sem Playwright
  adicionado, sem CI afetada.
- O item permanece na remediation-plan mas em estado "resolved by
  ADR-013".
- Custo zero imediato; dívida visível e rastreável.
