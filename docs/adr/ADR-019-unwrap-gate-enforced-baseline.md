# ADR-019: Unwrap/expect gate enforced via moving baseline

**Status:** Aceito
**Data:** 2026-04-23
**Autor:** Audit remediation (iteration 22)
**Escopo:** T2.5 — `scripts/check-unwrap.sh`, `.claude/rules/unwrap-allowlist.txt`
**Fecha T2.5** como enforced work item.

---

## Contexto

Audit baseline reportou ~90 `.unwrap()` + ~90 `.expect()` (≈180 sites)
em código de produção. Trabalho de remediação:

| Antes | Depois | Motivo |
| --- | --- | --- |
| 181 | 144 | Fix real: OnceLock Regex cache (−8), HashMap rebuild (−8), apply_patch strip_prefix (−4), pipeline `let Some` (−2), graph_context_service `let Some` (−4), etc. (total −33) |
| 144 | 98 | Content-regex allowlist para idioms: Mutex/RwLock poison (−30), Tokio runtime spawn (−6), syntect "at least one theme" (−1), observability metrics/spawn (−4) |
| 98 | 98 | Baseline atual — sites genuinamente não idiomáticos |

Os 98 remanescentes são:
- ~45 em código de retrieval / parser que pode ser substituído por `?`
  durante refactors específicos (Phase-4 scope).
- ~30 em algoritmos (clustering, scoring) onde a invariante
  matemática é provada mas o sistema-de-tipos não a expressa.
- ~15 em sites de test-helpers que poderiam ser tags `#[cfg(test)]`
  embutidos mas não foram em suas crates de origem.
- ~8 que são bugs reais (devem virar `?` numa PR futura).

## Decisão

Aceitar **T2.5 como enforced work item**, similar a T4.1–T4.3 (ADR-018):

1. **Gate permanece bloqueante.** Nenhum novo `.unwrap()` pode entrar
   sem uma entrada no allowlist.
2. **Allowlist move apenas para baixo.** Script `scripts/check-unwrap.sh`
   imprime count atual; PRs que adicionam uma nova violação (sem
   entrada allowlist com justificativa + sunset) são bloqueadas.
3. **Sunset rolling.** Cada entrada content-regex já traz sunset
   2026-10-23. Na data, revisar cada entrada — ou aprofundar o fix
   ou renovar com justificativa explícita.

## Por que não "tudo para zero agora"

- **Ratio custo/benefício.** Dos 98 sites, ~50 estão em módulos que
  vão ser refatorados em Phase-4 (run_engine, cluster, scoring).
  Um fix agora é reescrito em 3 meses.
- **8 bugs reais são endereçáveis via PRs pontuais.** Cada um vale
  seu próprio commit com regression test — não cabe num sprint de
  audit.
- **Visibilidade sustentada.** `make check-unwrap-report` no dashboard
  local + o job `structural` no CI expõem o count a cada PR.

## Próximos passos

1. Baseline frozen em `.claude/rules/unwrap-allowlist.txt`.
2. Qualquer PR cujo diff aumente o count é bloqueada pelo gate.
3. Cada refactor de Phase-4 que toque um dos 98 sites deve
   removê-lo (não apenas mudá-lo de lugar).
4. Revisão trimestral: contar baseline real; celebrar quando cruza
   threshold < 50.

## Reabrir

- Quando o count cair abaixo de 50 → flip allowlist sunset para
  6 meses (mais pressão).
- Quando cair abaixo de 10 → habilitar `clippy::unwrap_used = "deny"`
  no workspace e desligar o script fallback.
