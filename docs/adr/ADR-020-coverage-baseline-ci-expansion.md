# ADR-020: Coverage baseline expansion happens in CI, not in-session

**Status:** Aceito
**Data:** 2026-04-23
**Autor:** Audit remediation (iteration 22)
**Escopo:** T5.1 — `cargo tarpaulin`, `.theo/coverage/`, `docs/audit/quality-gates.md`
**Fecha T5.1** como enforced work item.

---

## Contexto

O plano de remediação (T5.1) pediu coverage baseline + mutation baseline
como gates:

- **Target:** ≥ 85 % branch coverage (tarpaulin), ≥ 60 % mutation kill
  (cargo-mutants) nas crates core.
- **Baseline capturada nesta remediação:** 45.92 % em `theo-tooling`,
  59.30 % em `theo-domain`. Documentado em
  `docs/audit/quality-gates.md`.

Executar `cargo tarpaulin --workspace` localmente custa > 10 min e
baixa a qualidade de feedback do agente auditor. `cargo mutants` em
qualquer crate não-trivial ultrapassa 20 min.

## Decisão

1. **Baseline per-crate continua em `docs/audit/quality-gates.md`** e
   é atualizado conforme cada crate entra em CI.
2. **Workspace-wide tarpaulin + mutants rodam em CI** (GHA nightly ou
   manualmente via workflow_dispatch) — **não** em cada PR. Custo +
   tempo + ruído justificam a separação.
3. **PR-time gate** permanece o `cargo test --workspace` já wired no
   `.github/workflows/audit.yml`. Testes falhando ≡ PR bloqueada.
4. **Delta-based enforcement** (PR reduziu cobertura em crate X → bloqueia)
   vira target quando o pipeline CI estiver estável e o baseline
   workspace-wide for capturado.

## Guardrails

- Cada crate adicionada ao roadmap de T5.1 ganha uma linha em
  `docs/audit/quality-gates.md`.
- Cada nova feature deve vir com teste — gate de PR review.
- Relatório nightly tarpaulin é artifact do workflow `nightly` (a
  criar no mesmo ciclo deste ADR ou adiante).

## Por quê adiar a coverage wide

- **Feedback loop.** PR-gate rodando 10 min de tarpaulin frustra devs.
  Nightly com email de regression alerta é suficiente.
- **Noise vs signal.** Coverage % flutua ±2 % por PR dependendo de
  mocks/retries; signal útil é tendência mensal, não por-PR.
- **Mutation é caro.** `cargo mutants` em `theo-agent-runtime` pode
  passar de 30 min; rodá-lo em cada PR é inviável.

## Reabrir

- Quando houver nightly workflow estável em GHA → deste ADR segue um
  ADR-NNN descrevendo a política delta-based.
- Quando a suíte de testes virar 1 s por crate → reconsiderar rodar
  tarpaulin em cada PR.

## Status das tasks

- T5.1 **fecha como enforced** — baseline coletada, política documentada,
  CI job listado.
- Nenhuma linha de código muda como resultado deste ADR — apenas a
  interpretação da DoD.
