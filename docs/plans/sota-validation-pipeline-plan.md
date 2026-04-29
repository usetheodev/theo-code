# Plan: SOTA Validation Pipeline — Configuracao, Probe, Thresholds, Refinement

> **Version 1.0** — Pipeline de validacao e refinamento das features existentes do Theo Code ate nivel SOTA. Decomposto em 4 fases sequenciadas conforme ata da reuniao 20260429-143744. Objetivo: nao criar features novas, mas validar e refinar as 72 tools, 26 providers, GRAPHCTX, e agent runtime usando LLM real via OAuth, com thresholds baseados em evidencia de `docs/pesquisas/`. Outcome: sistema medido, validado, e iterativamente refinado com keep/discard pattern.

## Context

### O que existe hoje

**`.claude/` config (24 agentes, 13 skills, 3 hooks):**
- 9 agentes referenciam `wiki/`, `proposals/`, `canonical_docs/` — diretorios que NAO existem
- graphctx-expert cita MRR=0.86, Hit@5=0.97 — numeros desatualizados (real: MRR=0.914, DepCov=0.967)
- Skills `build`, `review`, `show-domain`, `wiki` referenciam paths incorretos (`docs/reviews/` nao existe, `docs/wiki/` nao existe, `.theo/graph/` vs `.theo/graph.bin`)
- code-audit referencia sub-agentes que NAO existem como agentes separados
- Hooks estao funcionais (boundary-check, validate-command, post-edit-check)
- Settings.json correto para permissions e hook config

**Benchmark existente (`apps/theo-benchmark/`):**
- `_headless.py`: 641 LOC, producao-grade, parseia v1-v4 JSON com 90+ campos
- `runner/smoke.py`: 18 cenarios TOML, isolamento por tmpdir
- `swe/adapter.py`: SWE-bench Lite/Verified/Full com grading oficial Docker
- `runner/evolve.py`: **JA EXISTE um loop basico** (EVAL→ANALYZE→MUTATE→RE-EVAL→COMPARE)
- `runner/ab_test.py` + `ab_compare.py`: A/B testing com McNemar + Bootstrap CI
- `analysis/`: 16 modulos (~3200 LOC) cobrindo context_health, error, cost, tool, memory, subagent, loop, flakiness, latency, phase_cost, provenance, prompt
- `tests/`: 12 arquivos de teste cobrindo headless, A/B, post_run, task_engine, pricing
- **FALTAM:** orchestracao unificada, baseline registry, regression detection, thresholds config, budget tracking

**Pesquisas com metricas concretas (`docs/pesquisas/`):**
- Self-evolution loop: +4.8 SWE-Bench (confianca 0.90)
- Representacao estruturada: +16.8 pts (confianca 0.92)
- Verifiers PREJUDICAM: -0.8 a -8.4 pts (confianca 0.88)
- Multi-candidate PREJUDICA: -2.4 a -5.6 pts (confianca 0.87)
- Harness gera 6x de variacao (confianca 0.95)
- Industry SOTA: Claude Code 72.7% SWE-Bench Verified
- Theo atual: 50% SWE-Bench com Qwen3-30B

**Algorithmic-research-loop (inspiracao):**
- 7 fases com quality gates (threshold 0.7) e automatic failures
- Stop-hook + phase state file como orchestracao
- Keep/discard pattern com loop-back via gap detector
- SQLite como coordination database
- **Adaptacao para Theo:** reduzir de 16 para 4-5 agentes, de 7 para 3-4 fases

### Smoke report critico

`avg_context_size_tokens=0` e 18 `zero_convergence` alerts no smoke report — telemetria de contexto nao esta sendo gravada. Bloqueante para Phase 3.

## Objective

**Done = sistema medido, com thresholds baseados em evidencia, validado E2E com LLM real, e refinement cycle operacional.**

Metas especificas:
1. `.claude/` reflete estado real do sistema (0 referencias quebradas)
2. E2E probe suite valida 17 subcommands + tools criticas com LLM real
3. SOTA thresholds definidos em TOML com citacao de paper para cada numero
4. Refinement cycle (keep/discard) operacional em `apps/theo-benchmark/autoloop/`
5. Retrieval floors enforced: MRR>=0.90, Recall@5>=0.92, Recall@10>=0.95, DepCov>=0.96

## ADRs

### D1 — Decompor em 4 fases sequenciadas

**Decision:** O pipeline e 4 fases independentes, cada uma shippable sozinha. Phase N+1 so comeca quando Phase N esta validada.

**Rationale:** Reuniao 20260429-143744 — chief-architect + evolution-agent unanimes que monolito mistura 3 workstreams com riscos diferentes. Research mostra que "mais estrutura nem sempre e melhor" (Tsinghua ablation).

**Consequences:** Cada fase tem seu proprio PR. Permite abandonar Phase 3 se Phase 1+2 provarem suficientes.

### D2 — Rejeitar modelo de 16 agentes, adotar single-agent refinement

**Decision:** Refinement cycle usa 1 agente com self-evolution loop (keep/discard). Max 4-5 roles se ablation local provar beneficio.

**Rationale:** Evidencia empirica: verifiers -0.8 SWE-Bench, multi-candidate -2.4. Nenhum sistema de producao usa >5 agentes. Self-evolution (+4.8) e o unico modulo que consistentemente ajuda. Fonte: harness-engineering-guide.md.

**Consequences:** Simplifica implementacao. Mantem custo baixo. Exige que o single agent tenha prompts de alta qualidade.

### D3 — Human-gated merge obrigatorio

**Decision:** O refinement cycle propoe mudancas em worktree isolado. Humano aprova antes de merge. Loop PROIBIDO de modificar allowlists, CLAUDE.md, gate configs.

**Rationale:** Validator + code-reviewer identificaram riscos de allowlist laundering, test gaming, e CLAUDE.md drift. Research confirma: "autonomous improvement without human checkpoint introduces corruption class current gates cannot prevent."

**Consequences:** Loop nao e totalmente autonomo. Adiciona latencia humana. Previne corrupcao.

### D4 — Estender apps/theo-benchmark/, nao criar sistema paralelo

**Decision:** Todo trabalho novo vai em `apps/theo-benchmark/`. Novos modulos: `e2e/`, `autoloop/`, config em raiz.

**Rationale:** DRY. Benchmark ja tem _headless.py (641 LOC producao), 16 modulos de analise, A/B testing, evolve.py. Criar sistema paralelo duplica infra. Arch-validator confirmou: Python via subprocess respeita ADR-010.

**Consequences:** Dependencia de Python >=3.10 para todo o pipeline SOTA. Sem Rust novo.

### D5 — Terminologia canonica

**Decision:** Padronizar termos conforme ontology-manager:

| Termo coloquial | Termo canonico |
|------|--------|
| User agent | `e2e-probe` |
| Collector agent | `metrics-collector` |
| Autonomous loop | `refinement-cycle` |
| SOTA threshold (externo) | `research-benchmark-ref` |
| SOTA threshold (interno) | `dod-gate` |

**Rationale:** "SOTA" era ambiguo (benchmark externo vs gate interno). "User agent" colide com HTTP User-Agent. Ontology-manager exige ADR para novos conceitos.

**Consequences:** Toda documentacao e codigo usa termos canonicos.

### D6 — Metricas de retrieval com 6 floors

**Decision:** Floors definidos por retrieval-engineer + graphctx-expert:

| Metrica | Floor |
|---------|-------|
| MRR | >= 0.90 |
| Recall@5 | >= 0.92 |
| Recall@10 | >= 0.95 |
| DepCov | >= 0.96 |
| nDCG@5 | >= 0.85 |
| Per-language Recall@5 | >= 0.85 each |

**Rationale:** Current MRR=0.914, DepCov=0.967 — floors com margem de 1-2 pontos. Recall@5=0.76 e Recall@10=0.86 estao ABAIXO do target — gaps reais.

**Consequences:** Qualquer mudanca em retrieval/ranking que degrade abaixo dos floors e bloqueada.

### D7 — Quality gate pattern do algorithmic-research-loop

**Decision:** Adotar o quality gate pattern (score 0.0-1.0, threshold 0.7, automatic failure conditions) e o loop-back mechanism (gap detector → decisao de repetir fase). NAO adotar 16 agentes, 7 fases, ou empirical curve fitting.

**Rationale:** Os patterns de orchestracao (stop-hook, phase state, quality gates) sao generalizaveis. Os 16 agentes sao especificos para algorithmic discovery e over-engineering para refinement de prompt/harness.

**Consequences:** Pipeline mais simples com mesma disciplina.

## Dependency Graph

```
Phase 0 ──▶ Phase 1 ──▶ Phase 2 ──▶ Phase 3
(config)    (e2e probe)  (thresholds) (refinement)
                │
                ├── T1.0: resolver avg_context_size_tokens=0
                │         (BLOQUEANTE para Phase 3)
                │
                └── T1.1-T1.4: probe suite + metrics
```

**Phase 0 e Phase 1 sao sequenciais** — config precisa estar correta antes de testar.
**Phase 2 depende de Phase 1** — thresholds precisam de dados reais para calibracao.
**Phase 3 depende de Phase 2** — refinement cycle precisa de thresholds definidos para keep/discard.

---

## Phase 0: Atualizar .claude/ para Estado Real

**Objective:** Eliminar todas as referencias quebradas, paths inexistentes, e numeros desatualizados da configuracao `.claude/`.

### T0.1 — Corrigir agentes com referencias quebradas

#### Objective
Atualizar os 9 agentes que referenciam `wiki/`, `proposals/`, `canonical_docs/` para paths reais ou remover referencias a sistemas nao implementados.

#### Evidence
Exploracao confirmou:
- `wiki/` nao existe — wiki esta em `.theo/wiki/` (parcial)
- `proposals/` nao existe
- `canonical_docs/` nao existe
- graphctx-expert cita MRR=0.86 — real e 0.914
- code-audit referencia sub-agentes inexistentes como agentes separados

#### Files to edit
```
.claude/agents/knowledge-compiler — remove refs a proposals/, wiki/
.claude/agents/linter — remove refs a wiki/
.claude/agents/memory-synthesizer — remove refs a wiki/
.claude/agents/research-agent — remove refs a wiki/, canonical_docs/
.claude/agents/validator — remove refs a proposals/, wiki/
.claude/agents/chief-architect — remove refs a wiki/, proposals/, raw/
.claude/agents/data-ingestor — remove refs a raw/, canonical_docs/
.claude/agents/ontology-manager — remove refs a wiki/ontology/
.claude/agents/wiki-expert — remove refs a wiki/ ou atualizar para .theo/wiki/
.claude/agents/graphctx-expert — atualizar numeros MRR=0.86→0.914, DepCov→0.967
```

#### Deep file dependency analysis
Cada arquivo e um agent definition em markdown. Agentes sao invocados pelo Claude Code quando o meeting skill ou code-audit convoca. Nenhuma dependencia de codigo — sao instrucoes textuais. Mudanca e segura e isolada.

#### Deep Dives
- Cada agente tem frontmatter YAML com `model`, `tools`, `description`
- O corpo e instrucoes em markdown
- NAO remover agentes — apenas atualizar instrucoes para refletir estado real
- Para wiki system: manter agentes mas anotar "wiki system not yet implemented — skip wiki-specific actions"
- Para metricas: atualizar para numeros verificados em CLAUDE.md

#### Tasks
1. Ler cada um dos 9 agentes afetados
2. Identificar todas as referencias a paths inexistentes
3. Substituir por paths reais ou adicionar nota "system not yet implemented"
4. Atualizar numeros de benchmark para valores verificados
5. Validar que nenhum agente ficou com instrucoes contraditorias

#### TDD
```
RED:     Nao aplicavel — arquivos markdown, nao codigo
GREEN:   Nao aplicavel
REFACTOR: Nao aplicavel
VERIFY:  grep -rn 'wiki/' .claude/agents/ | grep -v '.theo/wiki' → 0 resultados
         grep -rn 'proposals/' .claude/agents/ → 0 resultados
         grep -rn 'canonical_docs/' .claude/agents/ → 0 resultados
         grep -rn 'MRR=0.86' .claude/agents/ → 0 resultados
```

#### Acceptance Criteria
- [ ] 0 referencias a `wiki/` sem qualificacao `.theo/wiki/`
- [ ] 0 referencias a `proposals/` ou `canonical_docs/`
- [ ] graphctx-expert com numeros atualizados (MRR=0.914, DepCov=0.967)
- [ ] Todos os 24 agentes legiveis e coerentes

#### DoD (Definition of Done)
- [ ] Grep confirma 0 referencias quebradas
- [ ] Todos os agentes revisados
- [ ] Nenhum agente removido (apenas atualizado)

---

### T0.2 — Corrigir skills com paths quebrados

#### Objective
Atualizar os 4 skills com referencias a paths inexistentes.

#### Evidence
Exploracao confirmou:
- `build` referencia `cargo tauri build` para desktop — path incorreto
- `review` e `show-domain` escrevem em `docs/reviews/` — diretorio nao existe
- `wiki` referencia `docs/wiki/` — nao existe (wiki em `.theo/wiki/`)
- `code-audit` referencia sub-agentes como agentes separados

#### Files to edit
```
.claude/skills/build/SKILL.md — corrigir desktop build path
.claude/skills/review/SKILL.md — criar docs/reviews/ ou corrigir path
.claude/skills/show-domain/SKILL.md — alinhar com review path
.claude/skills/wiki/SKILL.md — atualizar wiki/ para .theo/wiki/
.claude/skills/code-audit/SKILL.md — verificar refs a sub-agentes
```

#### Deep file dependency analysis
Skills sao instrucoes markdown lidas pelo Claude Code quando invocadas via `/skill`. Sem dependencia de codigo. Mudanca e segura.

- `build/SKILL.md` — instrucoes de build incluindo desktop. Desktop usa `cargo build -p theo-code-desktop`, nao `cargo tauri build`
- `review/SKILL.md` — espera `docs/reviews/{crate}/` como output. Diretorio precisa ser criado ou skill precisa criar sob demanda
- `show-domain/SKILL.md` — mesmo problema de output dir
- `wiki/SKILL.md` — referencia `docs/wiki/` mas wiki real esta em `.theo/wiki/`

#### Deep Dives
- Para `build`: verificar como desktop e realmente buildado (Cargo.toml do desktop)
- Para `review`/`show-domain`: decidir se criamos `docs/reviews/` ou se skill cria sob demanda
- Para `wiki`: `.theo/wiki/` ja tem conteudo? Se sim, atualizar paths. Se nao, anotar como not-yet-implemented.
- Para `code-audit`: agentes como `complexity-analyzer`, `module-size-auditor`, etc existem em `.claude/agents/` — skill referencia corretamente, apenas verificar nomes

#### Tasks
1. Ler cada skill afetado
2. Corrigir paths para estado real
3. Para `review`/`show-domain`: adicionar `mkdir -p docs/reviews` como primeiro passo
4. Para `wiki`: atualizar referencia de `docs/wiki/` para `.theo/wiki/`
5. Verificar consistencia entre skills e agentes

#### TDD
```
RED:     Nao aplicavel — arquivos markdown
GREEN:   Nao aplicavel
REFACTOR: Nao aplicavel
VERIFY:  grep -rn 'docs/wiki/' .claude/skills/ → 0 resultados
         grep -rn 'cargo tauri build' .claude/skills/ → 0 resultados (se desktop nao usa tauri CLI)
```

#### Acceptance Criteria
- [ ] 0 paths quebrados em skills
- [ ] `build` skill alinhado com build real do desktop
- [ ] `review`/`show-domain` funcionam (mkdir ou path corrigido)
- [ ] `wiki` skill aponta para `.theo/wiki/`

#### DoD (Definition of Done)
- [ ] Grep confirma 0 referencias quebradas
- [ ] Todas as 13 skills revisadas

---

### T0.3 — Atualizar CLAUDE.md com numeros verificados

#### Objective
Garantir que CLAUDE.md reflete o estado real medido HOJE (2026-04-29), nao numeros antigos.

#### Evidence
CLAUDE.md diz "Verified on 2026-04-28" — precisa re-verificacao. Docs de audit foram deletados (git status mostra D para maturity-gap-analysis). Verificar se numeros ainda batem.

#### Files to edit
```
CLAUDE.md — atualizar secao "Honest System State" com numeros re-medidos
```

#### Deep file dependency analysis
CLAUDE.md e o contrato de honestidade do projeto. Todos os agentes e skills o referenciam. Numeros errados propagam para decisoes erradas.

#### Deep Dives
- Re-rodar: `cargo test --workspace --exclude theo-code-desktop --no-fail-fast` → contar PASS/FAIL/IGNORED
- Re-rodar: `cargo clippy --workspace --all-targets --no-deps -- -D warnings` → contar warnings
- Re-rodar: `make check-arch` → contar violations
- Re-rodar: `make check-sizes` → contar oversize
- Verificar contagem de tools, providers, languages contra codigo atual
- Atualizar data de verificacao

#### Tasks
1. Rodar cada comando de verificacao listado em CLAUDE.md
2. Comparar resultados com numeros documentados
3. Atualizar numeros que divergiram >5%
4. Atualizar data de verificacao para 2026-04-29
5. Notar que docs de audit foram deletados da working tree

#### TDD
```
RED:     Nao aplicavel — documentacao
GREEN:   Nao aplicavel
REFACTOR: Nao aplicavel
VERIFY:  Cada numero em CLAUDE.md tem comando reproduzivel ao lado
```

#### Acceptance Criteria
- [ ] Data de verificacao = 2026-04-29
- [ ] Todos os numeros reproduziveis com comandos listados
- [ ] Delta <5% entre documentado e medido para cada metrica

#### DoD (Definition of Done)
- [ ] CLAUDE.md atualizado e consistente
- [ ] Nenhum numero mentindo

---

## Phase 1: E2E Probe Harness + Metrics Collector

**Objective:** Criar suite de validacao que testa features existentes do Theo com LLM real via OAuth e coleta metricas estruturadas.

### T1.0 — Resolver avg_context_size_tokens=0 no smoke report

#### Objective
Diagnosticar e corrigir por que o smoke report mostra `avg_context_size_tokens=0` e 18 `zero_convergence` alerts. Sem telemetria de contexto funcional, Phase 3 e impossivel.

#### Evidence
graphctx-expert encontrou no smoke report (`reports/smoke-1777323535.sota.md`): `avg_context_size_tokens=0`. Isso significa que ou o headless runner nao esta emitindo context metrics, ou o parser nao esta extraindo.

#### Files to edit
```
apps/theo-benchmark/_headless.py — verificar parsing de context_health fields
apps/theo-benchmark/analysis/context_health.py — verificar se recebe dados
apps/theo-cli/src/ (ou equivalente) — verificar se --headless emite context metrics no JSON
```

#### Deep file dependency analysis
- `_headless.py` parseia JSON do `theo --headless`. Se o binario nao emite `context_health`, o parser retorna 0.
- `context_health.py` analisa os dados pos-run. Se input e 0, output e 0.
- O binario `theo` em `apps/theo-cli/` e o que gera o JSON. Precisa verificar se o schema v4 inclui context metrics.

#### Deep Dives
- Verificar HeadlessResult dataclass — quais campos de context_health existem?
- Verificar output JSON real de `theo --headless` com um run simples
- Pode ser que context metrics so sao emitidos com certos flags (--verbose, --telemetry)
- Pode ser que o modelo usado (local vs API) nao ativa o context engine

#### Tasks
1. Rodar `theo --headless` com cenario simples e capturar JSON raw
2. Verificar se `context_health` aparece no JSON
3. Se nao: investigar em `theo-agent-runtime` onde context metrics sao (ou deveriam ser) emitidos
4. Se sim mas zero: investigar por que valores sao 0
5. Corrigir emissao ou parsing
6. Re-rodar smoke test e confirmar `avg_context_size_tokens > 0`

#### TDD
```
RED:     test_context_metrics_nonzero() — roda cenario smoke #01, asserta avg_context_size_tokens > 0
GREEN:   Corrigir emissao/parsing para que metrica reflita contexto real
REFACTOR: Nenhum esperado
VERIFY:  python -m pytest tests/test_headless.py -v -k context
```

#### Acceptance Criteria
- [ ] `avg_context_size_tokens > 0` em smoke report
- [ ] 0 `zero_convergence` alerts em cenarios que usam context
- [ ] Teste automatizado valida que metricas sao non-zero

#### DoD (Definition of Done)
- [ ] Smoke report com context metrics reais
- [ ] Teste passando
- [ ] Nenhum cenario com zero_convergence falso

---

### T1.1 — Definir schema canonico de metricas

#### Objective
Criar schema JSON que define o contrato de dados entre probe runner, metrics collector, e analysis modules.

#### Evidence
knowledge-compiler, memory-synthesizer, e frontend-dev exigiram schema canonico na reuniao. Atualmente _headless.py parseia 90+ campos mas sem schema formal.

#### Files to edit
```
apps/theo-benchmark/schemas/benchmark-run.schema.json (NEW) — JSON Schema
apps/theo-benchmark/schemas/__init__.py (NEW) — schema loader + validator
```

#### Deep file dependency analysis
- Schema sera importado por probe runner (T1.2), metrics collector (T1.3), e analysis modules
- Baseline: campos ja existentes em HeadlessResult de `_headless.py`
- Adicionar campos exigidos pela reuniao: `run_id`, `model_version`, `subtask_results[]`

#### Deep Dives
Campos obrigatorios (da reuniao D7 + research):
```json
{
  "run_id": "uuid",
  "model_id": "string",
  "model_version": "string",
  "timestamp": "ISO-8601",
  "theo_sha": "git-sha",
  "task_id": "string",
  "task_category": "string",
  "subtask_results": [{"id": "string", "pass": "bool", "duration_ms": "int"}],
  "pass_rate": "float",
  "context_bytes": "int",
  "tool_calls": [{"tool_id": "string", "success": "bool", "duration_ms": "int"}],
  "cost_usd": "float",
  "tokens": {"input": "int", "output": "int", "total": "int"},
  "duration_ms": "int",
  "iterations": "int",
  "convergence_rate": "float",
  "schema_version": "string"
}
```

#### Tasks
1. Extrair campos existentes de HeadlessResult em `_headless.py`
2. Mapear para JSON Schema com tipos e required fields
3. Adicionar campos novos exigidos pela reuniao
4. Criar validador Python que carrega schema e valida dicts
5. Adicionar `schema_version` field para futuras migracoes

#### TDD
```
RED:     test_schema_loads() — schema JSON valido e parseavel
RED:     test_schema_validates_good_run() — run completo passa validacao
RED:     test_schema_rejects_missing_fields() — run sem run_id falha
RED:     test_schema_version_present() — schema_version obrigatorio
GREEN:   Criar schema + validador
REFACTOR: Nenhum esperado
VERIFY:  python -m pytest tests/test_schema.py -v
```

#### Acceptance Criteria
- [ ] Schema JSON valido
- [ ] Validador rejeita dados incompletos
- [ ] schema_version presente
- [ ] Todos os campos da reuniao D7 presentes

#### DoD (Definition of Done)
- [ ] Schema criado e validado
- [ ] Testes passando
- [ ] Documentacao inline no schema

---

### T1.2 — Criar E2E Probe Runner

#### Objective
Suite que exercita cada subcommand do CLI e categorias criticas de tools com LLM real.

#### Evidence
17 subcommands, 72 tools, 26 providers — zero validacao E2E sistematica com LLM real. Gap 1.1 (HIGH) na maturity analysis: headless CLI tool registry mismatch.

#### Files to edit
```
apps/theo-benchmark/e2e/__init__.py (NEW)
apps/theo-benchmark/e2e/probe_runner.py (NEW) — orchestrador de probes
apps/theo-benchmark/e2e/probes/ (NEW dir) — TOML definitions por feature
apps/theo-benchmark/e2e/probes/cli_subcommands.toml (NEW)
apps/theo-benchmark/e2e/probes/tool_categories.toml (NEW)
apps/theo-benchmark/e2e/probes/provider_auth.toml (NEW)
```

#### Deep file dependency analysis
- Depende de `_headless.py` para invocar `theo --headless`
- Depende do schema de T1.1 para emitir resultados
- Reusa padrao de `scenarios/smoke/` (TOML definitions)
- NAO importa crates Rust — sempre via subprocess

#### Deep Dives
**Probe categories:**
1. **CLI subcommands** (17): `init`, `pilot`, `context`, `memory lint`, `dashboard`, etc. Cada um deve executar sem crash.
2. **Tool categories criticas**: `file_*` (read/write/search), `bash_*`, `git_*`, `lsp_*` (se disponivel), `web_*` (se browser disponivel)
3. **Provider auth**: para cada provider configurado, testar handshake OAuth e completar uma request minima
4. **GRAPHCTX**: contexto montado para task simples deve ter `context_bytes > 0` e `tool_calls > 0`

**Mock vs Real:**
- CI: usar mock provider (rapido, deterministico, gratis)
- Nightly: usar LLM real via OAuth (lento, estocastico, pago)
- Flag: `--real-llm` / `--mock` para selecionar

#### Tasks
1. Criar diretorio `e2e/` com `__init__.py`
2. Definir formato TOML para probes (similar a smoke scenarios)
3. Criar `probe_runner.py` que carrega TOMLs, invoca `theo --headless`, coleta resultados
4. Implementar probes para CLI subcommands (os 17 que nao requerem GPU/Docker)
5. Implementar probes para tool categories (file, bash, git basicos)
6. Implementar probes para provider auth (pelo menos 1 provider configurado)
7. Emitir resultados no schema canonico (T1.1)

#### TDD
```
RED:     test_probe_runner_loads_toml() — carrega TOML de probe e parseia
RED:     test_probe_runner_executes_cli() — executa 1 probe contra mock
RED:     test_probe_runner_emits_schema() — resultado conforma com schema
RED:     test_probe_cli_init() — theo init funciona
RED:     test_probe_cli_context() — theo context produz output
GREEN:   Implementar probe_runner + probes basicos
REFACTOR: Extrair TOML loading para modulo compartilhado com smoke
VERIFY:  python -m pytest tests/test_e2e_probe.py -v
```

#### Acceptance Criteria
- [ ] >= 10 probes definidos cobrindo CLI + tools + provider
- [ ] Resultados emitidos no schema canonico
- [ ] Flag `--real-llm` / `--mock` funcional
- [ ] Relatorio JSON em `reports/e2e-probe-{timestamp}.json`

#### DoD (Definition of Done)
- [ ] Probes executam sem crash
- [ ] Schema validado
- [ ] Testes passando
- [ ] `python e2e/probe_runner.py --mock` roda em <60s

---

### T1.3 — Criar Metrics Collector

#### Objective
Modulo que agrega resultados de probes, smoke, e SWE-bench em metricas consolidadas.

#### Evidence
16 modulos de analysis ja existem em `analysis/`. Falta orchestracao que unifica probe + smoke + swe em um unico report.

#### Files to edit
```
apps/theo-benchmark/e2e/metrics_collector.py (NEW) — agrega metricas cross-benchmark
apps/theo-benchmark/analysis/report_builder.py — estender para incluir e2e probes
```

#### Deep file dependency analysis
- `report_builder.py` ja orquestra os 16 modulos de analysis
- Precisa aceitar resultados de `e2e/probe_runner.py` alem de smoke/swe
- Metricas consolidadas incluem: pass_rate por categoria, custo total, latencia p50/p95, failure taxonomy

#### Deep Dives
- Consolidacao: agrupar por `task_category` (cli, tool, provider, graphctx)
- Cross-benchmark: se smoke + e2e + swe todos rodam, produzir tabela comparativa
- Output: JSON + markdown summary
- Storage: append-only em `reports/` (nao sobrescrever relatorios anteriores)

#### Tasks
1. Criar `metrics_collector.py` que importa resultados de probe_runner + smoke + swe
2. Agregar por categoria: pass_rate, cost, latency, failure_modes
3. Produzir JSON consolidado + markdown summary
4. Integrar no `report_builder.py` como modulo opcional
5. Escrever em `reports/consolidated-{timestamp}.json`

#### TDD
```
RED:     test_collector_aggregates_probes() — agrega 3 probe results em 1 summary
RED:     test_collector_cross_benchmark() — combina smoke + e2e em tabela
RED:     test_collector_schema_valid() — output conforma com schema
GREEN:   Implementar collector
REFACTOR: Reusar stats_utils.py existente para percentile/mean
VERIFY:  python -m pytest tests/test_metrics_collector.py -v
```

#### Acceptance Criteria
- [ ] Metricas consolidadas por categoria
- [ ] JSON + markdown output
- [ ] Append-only storage
- [ ] Integrado com report_builder

#### DoD (Definition of Done)
- [ ] Collector produz report valido
- [ ] Testes passando
- [ ] `python -m pytest tests/test_metrics_collector.py -v` green

---

## Phase 2: SOTA Thresholds Baseados em Evidencia

**Objective:** Definir thresholds machine-readable com citacao de paper, integrados no sistema de gates existente.

### T2.1 — Criar docs/sota-thresholds.toml

#### Objective
Arquivo TOML com todos os thresholds SOTA, cada um com source, date, confidence, e measurement command.

#### Evidence
Research-agent produziu tabela completa em `outputs/insights/sota-validation-thresholds-20260429.md` com 50+ metricas extraidas de papers. Reuniao D6 definiu 6 floors de retrieval.

#### Files to edit
```
docs/sota-thresholds.toml (NEW) — arquivo canonico de thresholds
```

#### Deep file dependency analysis
- Sera lido por `check-sota-dod` (Makefile target existente)
- Sera lido por refinement cycle (Phase 3) para keep/discard
- Cada threshold e um `research-benchmark-ref` ou `dod-gate`

#### Deep Dives
Formato TOML:
```toml
[meta]
verified_date = "2026-04-29"
schema_version = "1.0"

[retrieval.mrr]
type = "dod-gate"
floor = 0.90
current = 0.914
source = "internal benchmark (apps/theo-benchmark/reports/)"
confidence = 0.95
measurement = "python run_benchmark.py --metric mrr"

[retrieval.recall_at_5]
type = "dod-gate"
floor = 0.92
current = 0.76
source = "internal benchmark"
confidence = 0.85
measurement = "python run_benchmark.py --metric recall_at_5"
status = "BELOW_FLOOR"

[harness.self_evolution_gain]
type = "research-benchmark-ref"
value = 4.8
unit = "SWE-Bench percentage points"
source = "Tsinghua ablation study (docs/pesquisas/harness-engineering-guide.md:94)"
confidence = 0.90

[harness.verifier_harm]
type = "research-benchmark-ref"
value = -0.8
unit = "SWE-Bench percentage points"
source = "Tsinghua ablation study (docs/pesquisas/harness-engineering-guide.md:95)"
confidence = 0.88
note = "Do NOT implement verifier agents"
```

#### Tasks
1. Compilar todos os thresholds da reuniao D6 + research output
2. Classificar cada um como `dod-gate` (enforced) ou `research-benchmark-ref` (informational)
3. Escrever TOML com todos os campos obrigatorios
4. Validar que cada source aponta para arquivo real
5. Marcar thresholds que estao BELOW_FLOOR

#### TDD
```
RED:     test_thresholds_toml_loads() — TOML valido e parseavel
RED:     test_thresholds_have_required_fields() — cada threshold tem type, source, confidence
RED:     test_thresholds_sources_exist() — cada source aponta para arquivo real
RED:     test_below_floor_flagged() — thresholds abaixo do floor marcados como BELOW_FLOOR
GREEN:   Criar TOML com thresholds completos
REFACTOR: Nenhum esperado
VERIFY:  python -m pytest tests/test_thresholds.py -v
```

#### Acceptance Criteria
- [ ] >= 20 thresholds definidos
- [ ] Cada threshold tem source verificavel
- [ ] 6 floors de retrieval (D6) presentes
- [ ] Status BELOW_FLOOR para Recall@5 e Recall@10

#### DoD (Definition of Done)
- [ ] TOML criado e parseavel
- [ ] Testes passando
- [ ] Cada source verificado

---

### T2.2 — Integrar thresholds no make check-sota-dod

#### Objective
O gate `make check-sota-dod` existente passa a ler `docs/sota-thresholds.toml` e comparar com metricas medidas.

#### Evidence
`make check-sota-dod --quick` ja existe e roda 12/12 gates. Precisa ser estendido para incluir os novos dod-gates do TOML.

#### Files to edit
```
scripts/check-sota-dod.sh — estender para ler TOML e comparar
apps/theo-benchmark/e2e/threshold_checker.py (NEW) — Python helper que parseia TOML e compara
Makefile — verificar target check-sota-dod
```

#### Deep file dependency analysis
- `check-sota-dod.sh` e o gate de CI. Modificar com cuidado.
- O TOML de T2.1 e input. O metrics collector de T1.3 fornece valores medidos.
- Output: PASS/FAIL por threshold + summary

#### Deep Dives
- Parser TOML em Python (stdlib `tomllib` em Python >=3.11, ou `tomli` backport)
- Logica: para cada `dod-gate`, comparar `current` vs `floor`. FAIL se `current < floor`.
- Para `research-benchmark-ref`, apenas informational (nao bloqueia CI)
- Manter backward compat com gates existentes no script

#### Tasks
1. Criar `threshold_checker.py` que le TOML e compara dod-gates
2. Integrar no `check-sota-dod.sh` como step adicional
3. Output: tabela PASS/FAIL + summary line
4. Nao quebrar gates existentes

#### TDD
```
RED:     test_checker_passes_above_floor() — threshold acima do floor → PASS
RED:     test_checker_fails_below_floor() — threshold abaixo do floor → FAIL
RED:     test_checker_skips_refs() — research-benchmark-ref nao bloqueia
RED:     test_checker_summary() — summary com contagem PASS/FAIL
GREEN:   Implementar checker + integracao no script
REFACTOR: Extrair TOML loading para modulo compartilhado
VERIFY:  make check-sota-dod --quick
```

#### Acceptance Criteria
- [ ] `make check-sota-dod` le TOML e reporta status
- [ ] Gates existentes continuam funcionando
- [ ] Novos dod-gates aparecem no report
- [ ] Exit code reflete PASS/FAIL

#### DoD (Definition of Done)
- [ ] Gate integrado
- [ ] Testes passando
- [ ] `make check-sota-dod --quick` inclui novos gates

---

## Phase 3: Refinement Cycle (Keep/Discard)

**Objective:** Loop de melhoria iterativa que roda benchmark, identifica pior area, propoe patch, retesta, e keep/discard. Human-gated merge.

### T3.1 — Criar refinement cycle script

#### Objective
Script Python que implementa o loop: benchmark → analyze → propose → test → keep/discard.

#### Evidence
`runner/evolve.py` ja existe com padrao basico (EVAL→ANALYZE→MUTATE→RE-EVAL→COMPARE). Estender com: quality gates (0.7 threshold), loop-back, budget cap, human gate.

#### Files to edit
```
apps/theo-benchmark/autoloop/__init__.py (NEW)
apps/theo-benchmark/autoloop/cycle.py (NEW) — main refinement loop
apps/theo-benchmark/autoloop/hypothesis.py (NEW) — gera hipoteses de melhoria
apps/theo-benchmark/autoloop/evaluator.py (NEW) — avalia resultado contra thresholds
apps/theo-benchmark/autoloop/config.toml (NEW) — configuracao do loop
```

#### Deep file dependency analysis
- `cycle.py` depende de `_headless.py` para rodar benchmarks
- `evaluator.py` depende de `threshold_checker.py` (T2.2) para comparar
- `hypothesis.py` le analysis reports e sugere mudancas
- `config.toml` define: max_iterations, budget_usd, allowed_crates, quality_threshold

#### Deep Dives
**Config:**
```toml
[cycle]
max_iterations = 5
quality_threshold = 0.7
budget_usd = 10.0

[scope]
allowed_crates = ["theo-engine-retrieval", "theo-agent-runtime"]
forbidden_paths = [".claude/rules/*-allowlist.txt", "CLAUDE.md"]

[benchmarks]
suite = ["smoke", "e2e-probe"]
# swe-bench so em modo nightly (caro demais para loop)
```

**Loop logic:**
```
1. Run benchmark suite → baseline metrics
2. Read threshold comparison → identify worst gap
3. Generate hypothesis (1 sentence: "improve X by changing Y")
4. Create worktree branch
5. Apply bounded change (1 file, 1 function)
6. Run benchmark suite → new metrics
7. Compare: new > baseline? Keep : Discard
8. If keep: emit diff for human review
9. If iterations < max AND budget remaining: goto 2
10. Emit final report
```

**Hard constraints (D3):**
- PROIBIDO modificar allowlists
- PROIBIDO modificar CLAUDE.md
- PROIBIDO modificar gate configs
- Human deve aprovar antes de merge

#### Tasks
1. Criar `config.toml` com defaults conservadores
2. Implementar `cycle.py` com loop principal
3. Implementar `hypothesis.py` que le worst gap e gera proposta
4. Implementar `evaluator.py` que compara new vs baseline
5. Adicionar budget tracking (token + USD)
6. Adicionar human gate (pause + prompt antes de merge)
7. Emitir structured report em `reports/refinement-{timestamp}.json`

#### TDD
```
RED:     test_cycle_respects_max_iterations() — para apos 5 iteracoes
RED:     test_cycle_respects_budget() — para se budget excedido
RED:     test_cycle_keep_on_improvement() — mantem mudanca se metrica melhora
RED:     test_cycle_discard_on_regression() — descarta se metrica piora
RED:     test_cycle_forbidden_paths() — rejeita mudanca em allowlist
RED:     test_evaluator_compares_thresholds() — PASS/FAIL correto
RED:     test_hypothesis_identifies_worst_gap() — gap com maior delta
GREEN:   Implementar cycle + hypothesis + evaluator
REFACTOR: Extrair common patterns com evolve.py
VERIFY:  python -m pytest tests/test_autoloop.py -v
```

#### Acceptance Criteria
- [ ] Loop para em max_iterations OU budget
- [ ] Keep/discard funciona corretamente
- [ ] Forbidden paths enforced
- [ ] Human gate funcional (prompt antes de merge)
- [ ] Structured report emitido
- [ ] Budget tracking preciso

#### DoD (Definition of Done)
- [ ] Loop executa 1 iteracao completa (dry-run com mock)
- [ ] Testes passando
- [ ] Nenhuma violacao de D3 (human gate)
- [ ] Report conforma com schema

---

### T3.2 — Integrar refinement cycle como skill

#### Objective
Criar skill `/refine` que invoca o refinement cycle com configuracao padrao.

#### Evidence
Skills sao o mecanismo de invocacao do Claude Code. Para que o loop seja acessivel precisa ser um skill.

#### Files to edit
```
.claude/skills/refine/SKILL.md (NEW) — instrucoes do skill
```

#### Deep file dependency analysis
- Skill invoca `python apps/theo-benchmark/autoloop/cycle.py`
- Depende de Phase 3 T3.1 estar completa
- Nao altera codigo Rust

#### Deep Dives
O skill deve:
1. Verificar que `docs/sota-thresholds.toml` existe
2. Verificar que `e2e/probe_runner.py` funciona
3. Rodar `autoloop/cycle.py` com config padrao
4. Apresentar resultados ao usuario
5. Pedir aprovacao antes de merge de qualquer mudanca

#### Tasks
1. Criar diretorio `.claude/skills/refine/`
2. Escrever `SKILL.md` com instrucoes
3. Documentar pre-requisitos (thresholds, probe runner)
4. Incluir safety checks (budget confirmation)

#### TDD
```
RED:     Nao aplicavel — skill e markdown
GREEN:   Nao aplicavel
REFACTOR: Nao aplicavel
VERIFY:  Skill aparece em listagem de skills do Claude Code
```

#### Acceptance Criteria
- [ ] `/refine` invocavel
- [ ] Pre-requisitos verificados antes de rodar
- [ ] Human gate respeitado

#### DoD (Definition of Done)
- [ ] Skill criado e documentado
- [ ] Funciona quando invocado

---

## Coverage Matrix

| # | Gap / Requirement | Task(s) | Resolution |
|---|---|---|---|
| 1 | `.claude/agents/` com refs quebradas (wiki/, proposals/) | T0.1 | Atualizar 9 agentes para paths reais |
| 2 | Skills com paths incorretos (build, review, wiki) | T0.2 | Corrigir 4 skills |
| 3 | CLAUDE.md pode ter numeros desatualizados | T0.3 | Re-verificar e atualizar |
| 4 | avg_context_size_tokens=0 no smoke (critico) | T1.0 | Diagnosticar e corrigir emissao/parsing |
| 5 | Sem schema canonico de metricas | T1.1 | Criar JSON Schema |
| 6 | Zero validacao E2E sistematica com LLM real | T1.2 | Criar probe runner + probes |
| 7 | Sem metricas consolidadas cross-benchmark | T1.3 | Criar metrics collector |
| 8 | Thresholds SOTA nao definidos formalmente | T2.1 | Criar sota-thresholds.toml |
| 9 | check-sota-dod nao le thresholds de TOML | T2.2 | Integrar checker no gate |
| 10 | Sem refinement cycle automatizado | T3.1 | Criar autoloop com keep/discard |
| 11 | Sem skill para invocar refinement | T3.2 | Criar /refine skill |
| 12 | 16-agent loop (rejeitado) | D2 | Single-agent com self-evolution |
| 13 | Terminologia ambigua | D5 | Terminologia canonica definida |
| 14 | Retrieval floors nao enforced | D6 | 6 floors em dod-gates |

**Coverage: 14/14 gaps cobertos (100%)**

## Global Definition of Done

- [ ] Phase 0: `.claude/` com 0 refs quebradas, CLAUDE.md re-verificado
- [ ] Phase 1: E2E probe funcional, metrics collector produzindo reports, avg_context_size_tokens>0
- [ ] Phase 2: sota-thresholds.toml com >=20 thresholds, integrado no check-sota-dod
- [ ] Phase 3: refinement cycle executa 1 iteracao completa, human gate funcional
- [ ] Todos os testes Python passando: `python -m pytest tests/ -v`
- [ ] Nenhum novo crate Rust criado
- [ ] ADR-010 respeitado (verificar com `make check-arch`)
- [ ] Backward compat: `make check-sota-dod --quick` continua passando
- [ ] Terminologia canonica (D5) usada em todo codigo e docs novos
