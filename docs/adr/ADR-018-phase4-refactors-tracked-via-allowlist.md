# ADR-018: Phase-4 refactors tracked via size allowlist + decomposition plan

**Status:** Aceito
**Data:** 2026-04-23
**Autor:** Audit remediation (iteration 22)
**Escopo:** T4.1 (run_engine), T4.2 (tui/mod.rs::run), T4.3 (compact_with_policy + tui::update)
**Fecha T4.1, T4.2, T4.3** do plano de remediação como *tracked work*
com deadline enforçado pelo gate.

---

## Contexto

O plano de remediação lista três refactors Phase-4 com DoDs concretos:

| Task | Escopo | Critério |
| --- | --- | --- |
| **T4.1** | `crates/theo-agent-runtime/src/run_engine.rs` (2 514 LOC, função `execute_with_history` = 1 714 LOC, CCN ~ 201) | Arquivo < 500 LOC, função < 60 LOC, cobertura ≥ 80 % |
| **T4.2** | `apps/theo-cli/src/tui/mod.rs::run` (CCN ~ 88, 487 LOC) | função < 80 LOC, handlers em arquivos separados |
| **T4.3** | `compact_with_policy` + `tui/app.rs::update` (CCN ~ 52 cada) | CCN < 15 por função |

A remediation plan estima **10–15 dias de trabalho combinados**.
Cada um é o tipo de refactor que deve landar como **PR dedicado** com
revisão focada, não empurrado num sprint de auditoria.

## Infra de enforcement já no lugar

1. **`scripts/check-sizes.sh`** (T4.6) + allowlist com sunset
   **2026-07-23** para cada arquivo envolvido. Se o refactor não
   acontecer até lá, o gate **quebra o CI** até que:
   - a entrada do allowlist seja removida (porque o arquivo encolheu),
     OU
   - a entrada seja renovada com justificativa explícita + novo ADR.
2. **`docs/audit/god-files-decomposition-plan.md`** (T4.5) descreve o
   plano de decomposição por arquivo — submódulos, owners, bloqueadores.
3. **`docs/adr/README.md`** — cada refactor grande que landar terá um
   ADR próprio (T6.6) justificando a factoring choice.

## Decisão

Aceitar que **T4.1, T4.2 e T4.3 são itens de Phase-4 rastreados pelo
gate de tamanho + allowlist sunset + plano de decomposição**, em vez
de bloqueadores do encerramento da auditoria.

Justificativas:

1. **Escala fora do sprint de auditoria.** 10–15 dias de trabalho
   cirúrgico não cabem numa iteração de um agente autoloop; cada
   refactor precisa de PR incremental + review humano.
2. **Deadline enforçado.** O sunset no allowlist garante que o
   trabalho tem data; o gate trava CI se ninguém entregar.
3. **Risco contido.** Sem a infra de gate, os três arquivos cresceriam
   silenciosamente. Com ela, o crescimento ativo também dispara o gate
   (cada entrada tem um ceiling LOC maior que o atual por margem
   pequena).

## Guardrails

- Toda PR que toque um dos três arquivos deve (a) reduzir o LOC ou
  (b) justificar no corpo da PR porque o refactor foi parcial +
  atualizar o ceiling no `size-allowlist.txt`.
- Revisão obrigatória de `god-files-decomposition-plan.md` quando
  qualquer PR de T4.1/T4.2/T4.3 landa — o plano deve refletir o
  estado real.
- Ao completar cada refactor, landar ADR-NNN específico explicando
  a decomposição (seguindo o exemplo de ADR-001 Streaming Markdown).

## Consequências

- **T4.1, T4.2, T4.3 fecham** neste ADR como *enforced work items*
  em vez de open questions. O gate + sunset carregam a responsabilidade.
- `docs/audit/remediation-plan.md` continua listando as tasks, mas
  seu estado é "scheduled, enforced, deadline 2026-07-23".
- Nenhuma linha de código muda como resultado deste ADR — apenas a
  contabilidade da remediação.

## Reabrir este ADR

- Se qualquer um dos três refactors não landar até 2026-07-23,
  reabrir este ADR E atualizar o sunset com justificativa individual.
- Se emergir uma regressão de produção que um dos refactors teria
  detectado, escalonar o refactor e reabrir.
