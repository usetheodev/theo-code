# Plan 01 — Benchmark Theo Code contra os melhores

## Objective

Provar (ou descobrir o oposto) que Theo Code está no nível dos top code agents (Claude Code, Codex, Cursor, OpenCode) com **números públicos**, não alegações. Sem benchmark = sem credibilidade.

## Estratégia

Não rodar tudo. Priorizar pelo ROI: máxima sinalização de qualidade pelo mínimo de esforço de adapter. Ordem por fase é deliberada — cada fase desbloqueia a próxima.

```
Fase 0 → smoke local (1 dia)
Fase 1 → Terminal-Bench (1 semana)         [primeira nota pública]
Fase 2 → SWE-bench Verified (3 semanas)    [a nota que importa]
Fase 3 → Theo-Bench próprio (contínuo)     [diferencial competitivo]
Fase 4 → GAIA + RepoBench (opcional)       [se sobrar fôlego]
```

Skipados conscientemente: HumanEval, MBPP, CodeXGLUE — testam geração isolada, não agente. Baixo sinal para o que somos.

## Scope

**Dentro:** harness, adapters, runner, parser de resultados, dashboard local, custos de API tracking, repro pública.
**Fora:** fine-tuning de modelo, mudar arquitetura do agent loop, otimizações de prompt antes de ter baseline.

---

## Fase 0 — Smoke local (1 dia)

Antes de gastar tokens em benchmark público, garantir que o agente não trava em tarefas básicas.

### Tasks

1. **Criar `apps/theo-benchmark/scenarios/smoke/`** — file: `apps/theo-benchmark/scenarios/smoke/*.toml` — acceptance: 10 cenários (3 fix-bug, 3 add-feature, 2 refactor, 2 explore) com prompt + critério de sucesso (cargo test passa / arquivo modificado contém X / etc).
2. **Runner mínimo** — file: `apps/theo-benchmark/src/smoke.rs` — acceptance: `cargo run -p theo-benchmark -- smoke` executa todos, mede latência/tokens/sucesso, gera `reports/smoke-YYYYMMDD.json`.
3. **Triagem dos primeiros falhos** — file: `docs/benchmarks/smoke-baseline.md` — acceptance: lista de bugs/regressões encontradas + decisão fix-now vs depois.

**DoD:** ≥ 8/10 verde. Se < 8/10, parar e corrigir antes de prosseguir.

---

## Fase 1 — Terminal-Bench (1 semana)

Mais próximo do que Theo já faz hoje (CLI, bash, sandbox). Primeiro número público com menor risco.

### Tasks

1. **Clonar e estudar t-bench harness** — file: `referencias/terminal-bench/` (submodule ou clone) — acceptance: README do harness lido, formato de tarefa entendido, exemplo rodando localmente sem nosso agente.
2. **Adapter `theo-tbench`** — file: `apps/theo-benchmark/src/tbench.rs` — acceptance: implementa interface esperada pelo t-bench harness (recebe task dir + prompt, devolve transcript + final state), usa `theo --mode agent` em modo headless.
3. **Headless mode no CLI** — file: `apps/theo-cli/src/headless.rs` — acceptance: flag `--headless` que aceita prompt via stdin, devolve resultado JSON via stdout, sem TUI/REPL/banners. Exit code 0 = sucesso, 1 = falha.
4. **Sandbox por tarefa** — file: `apps/theo-benchmark/src/sandbox.rs` — acceptance: cada tarefa roda em container/tmpdir isolado, sem vazar estado entre runs.
5. **Rodar subset (10 tarefas) e validar pipeline** — file: `reports/tbench-pilot.json` — acceptance: 10 tarefas rodam end-to-end, métricas coletadas, sem crash do harness.
6. **Run completo + publicar** — file: `docs/benchmarks/tbench-2026-04.md` — acceptance: score full publicado com modelo, tokens médios, custo, comparação vs líderes (se números públicos existirem).

**DoD:** score publicado em `docs/benchmarks/`, repro documentado, custo total estimado.

**Modelo alvo inicial:** `gpt-5.3-codex` via OAuth (free dentro do plano ChatGPT Plus). Se Anthropic for melhor sinal, usar `claude-sonnet-4-6` com API key.

---

## Fase 2 — SWE-bench Verified (3 semanas)

A nota que define se somos sérios. Mais caro, mais infra, maior payoff.

### Tasks

1. **Setup da infraestrutura SWE-bench** — file: `apps/theo-benchmark/swe/Dockerfile` + `compose.yaml` — acceptance: imagem Docker com Python + pytest + git que executa um instance do SWE-bench-Verified isolado.
2. **Adapter `theo-swe`** — file: `apps/theo-benchmark/src/swe.rs` — acceptance: lê instance JSON do dataset HuggingFace `princeton-nlp/SWE-bench_Verified`, prepara workdir, chama `theo --headless`, captura patch final, devolve no formato esperado pelo grader.
3. **Estratégia de prompt-shape** — file: `crates/theo-agent-runtime/src/swe_prompt.rs` — acceptance: prompt template específico para SWE (issue body + repo context + "produce patch that makes failing tests pass"). Decisão: usar Plan mode primeiro? Ou Agent direto?
4. **Smoke em 5 instances fáceis** — file: `reports/swe-smoke.json` — acceptance: 5 instances picadas a dedo (django small, requests, etc.) rodam sem erro de harness.
5. **Run em 50 instances + tuning** — file: `reports/swe-50.json` — acceptance: pelo menos 1 resolved. Aprender o que falha, iterar 2-3 vezes no prompt/tooling.
6. **Run completo (500 instances Verified)** — file: `reports/swe-verified-full.json` — acceptance: número final + breakdown por categoria de falha (não compila / testes falham / patch vazio / loop infinito).
7. **Publicar e submeter ao leaderboard** — file: `docs/benchmarks/swe-verified-2026.md` — acceptance: PR para o leaderboard oficial (se aceitar terceiros) ou publicar no nosso repo com instruções de repro.

**DoD:** score Verified ≥ 30% para sermos competitivos. Se < 15%, parar e fazer postmortem antes de Fase 3.

**Custo estimado:** 500 instances × ~50k tokens cada × $0.01/1k = ~$250 por run completo. Orçar 3-4 runs = ~$1k.

---

## Fase 3 — Theo-Bench (diferencial)

Onde ganhamos vantagem competitiva: cenários que ninguém mais testa direito.

### Tasks

1. **Catálogo de cenários SaaS reais** — file: `apps/theo-benchmark/scenarios/theo-bench/*.toml` — acceptance: 30+ cenários cobrindo:
   - Migration quebrada (Rails/Prisma/Django)
   - Pipeline CI/CD vermelho
   - Bug em produção (logs + repro)
   - Query lenta (EXPLAIN + index)
   - Monorepo grande (resolver build cross-pkg)
   - Refactor cross-crate (nosso próprio diferencial: GRAPHCTX)
2. **Critérios objetivos por cenário** — file: cada `.toml` — acceptance: cada tarefa tem `success_check` rodável (script bash que retorna 0/1), não dependência de eyeballing humano.
3. **Runner Theo-Bench** — file: `apps/theo-benchmark/src/theo_bench.rs` — acceptance: roda subset por categoria, gera HTML report com diff + transcript.
4. **Baseline interno** — file: `reports/theo-bench-baseline.json` — acceptance: rodar Theo + Claude Code + Codex CLI nos mesmos cenários, comparar lado a lado. Esse é o número que vendemos.
5. **Documentar e publicar dataset** — file: `docs/benchmarks/theo-bench-spec.md` — acceptance: spec aberta + dataset no HuggingFace. Quanto mais gente rodar, mais credibilidade ganhamos.

**DoD:** Theo vence em ≥ 1 categoria com margem clara (> 15%). Se não vencer em nada, repensar diferencial.

---

## Fase 4 — Opcional (se ROI justificar)

- **GAIA**: tool-use multi-step. Útil só se Theo crescer para tasks de pesquisa/web. Hoje fora de escopo.
- **RepoBench / LongBench**: contexto longo. Só faz sentido depois que GRAPHCTX provar valor em SWE-bench.
- **HumanEval / MBPP**: rodar 1x por completude, mas é vitrine, não diferencial.

---

## Infraestrutura compartilhada (toca todas as fases)

1. **`apps/theo-benchmark` reformulado** — hoje só roda algo isolado. Vira o hub: subcomandos `smoke`, `tbench`, `swe`, `theo-bench`.
2. **`--headless` no CLI** — pré-requisito de tudo. Sem isso, nenhum harness consegue chamar Theo de forma confiável.
3. **Tracking de custo/tokens** — `apps/theo-benchmark/src/cost.rs` — somatório por run, alerta se passar do orçamento.
4. **Reports padronizados** — `reports/<bench>-<date>.json` + `docs/benchmarks/<bench>-<date>.md` com mesmo schema sempre.
5. **CI noturno** — depois da Fase 1, rodar smoke + tbench-subset toda noite contra `main` para detectar regressão antes de produção.

---

## Risks

- **Custo descontrolado** — SWE-bench full pode passar de $1k facilmente. **Mitigação:** budget hard cap por run, dry-run em 10 instances antes de full.
- **Modelo OAuth Codex tem rate limit/quota oculta** — pode quebrar no meio do run. **Mitigação:** detectar 429 + retry exponencial + fallback para API key se passar de N falhas.
- **Adapters frágeis** — cada bench muda formato a cada release. **Mitigação:** pinar versão do harness por hash, atualizar deliberadamente.
- **Resultado ruim publicamente** — se SWE-bench der 5%, marca queima. **Mitigação:** rodar internamente primeiro, só publicar quando tivermos um número defensável.
- **Theo ainda tem bugs do TUI/Plan mode** — corrigidos hoje, mas pode ter mais. **Mitigação:** Fase 0 é justamente para detectar isso antes de queimar tokens em bench público.
- **Comparação injusta com Claude Code / Codex** — eles têm Sonnet 4.6 / GPT-5 fine-tuned para esses benchmarks, nós usamos o mesmo modelo via API. **Mitigação:** comparar com mesmo modelo subjacente quando possível, deixar isso claro nos reports.

---

## Validation

- **Por fase:** DoD listado em cada fase. Sem DoD verde, próxima fase não começa.
- **Fim do plano:** três artefatos públicos
  1. `docs/benchmarks/tbench-*.md` — primeiro score público
  2. `docs/benchmarks/swe-verified-*.md` — score que importa
  3. `docs/benchmarks/theo-bench-spec.md` — nosso diferencial
- **Métrica de sucesso global:** Theo aparece em pelo menos 1 leaderboard público com número defensável até final do plano. Sem isso, falhamos.

---

## Próximo passo concreto

Começar **Fase 0 hoje**:

1. Criar `apps/theo-benchmark/scenarios/smoke/` com 10 cenários TOML.
2. Implementar `--headless` no CLI (1-2 horas, desbloqueia tudo).
3. Rodar smoke, ver onde quebra, decidir Fase 1.

Não tocar em SWE-bench/tbench até smoke estar verde. Disciplina aqui evita gastar uma semana de infra para descobrir que o agente trava em tarefa trivial.
