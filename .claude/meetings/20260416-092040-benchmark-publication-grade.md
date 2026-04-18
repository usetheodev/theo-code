---
id: 20260416-092040
date: 2026-04-16
topic: "Benchmark Publication-Grade: Fix bugs críticos e plano de 10 fases"
verdict: REVISED
participants: 16
branch: develop
head_commit: 5a5c4bd
---

# Reunião: Benchmark Publication-Grade

## Pauta

**Contexto:** Análise FAANG do sistema de benchmark revelou 3 blockers fatais para publicação e comparação com Claude Code, Codex, OpenCode. Bug P0 crítico descoberto: `THEO_TEMPERATURE` env var nunca é lido pelo binário Rust — todos os benchmarks rodaram com temperature=0.1. Oracle mode como default no SWE-bench é data leakage.

**Questões a decidir:**
1. Aprovação do plano de 10 fases (~52h eng, ~$1k budget)
2. Priorização: caminho rápido vs plano completo
3. Feature toggles para ablation: onde colocar? (agent-runtime vs engine-retrieval)
4. Schema v1→v2 backward compatibility
5. Invalidação de reports históricos

**Restrições:** Budget ~$1k para API runs. Builds/testes no vast.ai. TDD obrigatório.

---

## Posições por Agente

### Estratégia

| Agente | Posição | Resumo |
|--------|---------|--------|
| chief-architect | APPROVE COM CONDIÇÕES | Bug P0 confirmado. Nenhuma violação de dependency rules. Ablation via AgentConfig fields (não feature flags de compilação). Schema v2 precisa backward compat. CI com vast.ai é frágil — não bloquear merge por benchmark. |
| evolution-agent | APPROVE COM GAPS | Confirmou que smoke.py TAMBÉM não propaga temperature. Evolve.py precisa redesign parcial (mutation bank baseado em find/replace é frágil). Baseline real sem oracle será muito pior (~10-15% SWE). Budget viável se scope controlado. |

### Conhecimento

| Agente | Posição | Resumo |
|--------|---------|--------|
| knowledge-compiler | APPROVE | Schema v2 precisa de ADR canônico antes de wiki proposals. Cada fase deve produzir doc em docs/current/. |
| ontology-manager | APPROVE | 7 novos conceitos em 2 clusters (estatística + infra). Criar namespace `benchmark-evaluation`. "Oracle mode" precisa disambiguação com retrieval oracle. |
| data-ingestor | CONCERN | Volume de dados alto. Runs com seeds diferentes são semanticamente distintas mas estruturalmente idênticas — risco de deduplicação incorreta. Precisa schema canônico com run_id/seed/model_id antes de coleta. |
| wiki-expert | APPROVE | Reports JSON são RuntimeInsights Layer 3. Fase 10 deve emitir JSON estruturado, wiki consumer faz conversão. Definir contrato mínimo RuntimeInsight. |

### Qualidade

| Agente | Posição | Resumo |
|--------|---------|--------|
| validator | CONCERN | Bug P0 parcialmente confirmado (ProjectConfig tem o reader mas nunca é chamado). Oracle default invalida publicação. Schema v1/v2 sem migration guard = risco de corrupção. Precisa validação de schema version no agregador. |
| linter | CONCERN | Cobertura de testes do benchmark: ~22%. Adicionar +2k LOC sem TDD prévio viola CLAUDE.md. Python desorganizado (18 arquivos flat). Últimos commits adicionaram testes DEPOIS de implementação. |
| retrieval-engineer | APPROVE | Ablation de GRAPHCTX é cientificamente necessário. Pipeline tem fallback limpo (BM25+dense sem graph). Implementar como feature flag no pipeline, não remoção de código. |
| memory-synthesizer | APPROVE | Dados estruturados de ablation são ouro para embeddings. Schema estável é prerequisito. Competitor data em namespace isolado. |

### Engineering

| Agente | Posição | Resumo |
|--------|---------|--------|
| code-reviewer | APPROVE COM P0 BLOQUEANTE | Bug P0 confirmado em 3 pontos. Seed em LLMs não garante determinismo entre providers. Feature toggles sem testes = espaço de estados explosivo. Fix P0 deve começar com teste RED. |
| graphctx-expert | APPROVE | Implementar via `GraphMode` enum em `RerankConfig` (Full/Disabled). Sem flag CLI, sem #[cfg]. Benchmark instancia seu próprio config. Manter enum dentro de theo-engine-retrieval. |
| arch-validator | VÁLIDO | Nenhuma violação de bounded contexts. Todas mudanças Rust ficam em theo-cli, theo-agent-runtime, theo-infra-llm. theo-domain intocado. Seed precisa validação de suporte por provider. |
| test-runner | VIÁVEL MAS INCOMPLETO | Base Python tem 82 testes passando. Faltam testes de ciclo completo temperature. Schema mismatch não testado. Compilation errors em theo-application podem bloquear suite Rust. |
| frontend-dev | ABSTAIN | Zero impacto no Tauri/React. Benchmark é isolado. matplotlib ok para Fase 10. Se dashboard futuro, dados via JSON (não imagens). |

### Pesquisa

| Agente | Posição | Resumo |
|--------|---------|--------|
| research-agent | APPROVE | SWE-bench Verified é padrão para publicação. Non-oracle obrigatório. Pass@1 média de 3 runs com Wilson CI. Ablation obrigatório (SWE-Agent, OpenHands, Agentless todos fazem). McNemar para comparações pareadas. Gap: cost-per-resolved como métrica emergente. |

---

## Conflitos Identificados

### Conflito 1: Onde colocar ablation toggles?

- **chief-architect:** `AgentConfig` fields (disable_graphctx, disable_context_loops) — runtime config
- **graphctx-expert:** `GraphMode` enum em `RerankConfig` dentro de `theo-engine-retrieval` — pipeline config

**Resolução:** AMBOS estão corretos para camadas diferentes. O `AgentConfig` controla ablation de alto nível (CLI flags → agent). O `RerankConfig.GraphMode` controla ablation do pipeline de retrieval especificamente. O agent-runtime propaga a decisão para o engine. Não há conflito — são camadas complementares.

### Conflito 2: Smoke assertions — upgrade prompts ou assertions?

- **Plano original:** Mudar prompts para pedir que agente escreva em arquivo
- **Cenário 15:** Pode checar arquivos setup diretamente sem mudar prompt

**Resolução:** Para cenários read-only (01, 02, 07, 12), mudar prompt é necessário. Para cenário 15, checar arquivos setup diretamente é melhor. Approach misto.

### Conflito 3: Schema v1→v2 transition

- **validator:** Rejeitar mistura v1+v2 com erro explícito
- **knowledge-compiler:** ADR canônico antes de implementar
- **data-ingestor:** Schema canônico com run_id antes de coleta

**Resolução:** Criar ADR do schema v2 ANTES de implementar. Parser Python suporta ambos (v1 fallback) mas agregador rejeita mistura. Reports v1 históricos marcados como `"invalidated": true`.

### Conflito 4: TDD enforcement

- **linter:** REJEITAR até TDD estar verde, exigir 70% cobertura ANTES
- **code-reviewer:** Fix P0 começa com teste RED

**Resolução:** Não bloquear por meta de cobertura arbitrária (70% é YAGNI neste momento). Mas CADA mudança segue RED-GREEN-REFACTOR. O test-runner valida viabilidade do plano TDD.

---

## Gaps Adicionais Identificados

1. **smoke.py não propaga temperature** (evolution-agent) — deve ser incluído na Fase 1
2. **evolve.py mutation bank fica obsoleto** pós-fix — precisa redesign parcial
3. **Binary fingerprint** (hash SHA do binário) — adicionar ao environment block
4. **Reports históricos invalidados** — criar REPORTS_MIGRATION.md
5. **Cost-per-resolved** como métrica (research-agent) — adicionar na Fase 10
6. **Compilation errors em theo-application** (test-runner) — verificar antes de Fase 1

---

## Decisões

1. **APROVADO: Plano de 10 fases** com revisões abaixo
2. **APROVADO: Fase 1 é P0 absoluto** — fix temperature em TODOS os entry points (headless, smoke, evolve)
3. **APROVADO: Non-oracle como default** no SWE-bench adapter
4. **APROVADO: Ablation em duas camadas** — AgentConfig (alto nível) + RerankConfig.GraphMode (pipeline)
5. **APROVADO: Schema v2 com backward compat** — ADR primeiro, parser suporta v1, agregador rejeita mistura
6. **APROVADO: Dataset primário = SWE-bench Verified**, Lite como secundário
7. **APROVADO: Pass@1 média de 3 runs** com Wilson CI como métrica primária
8. **REVISADO: Fase 1 expandida** para cobrir smoke.py e evolve.py (não só adapter)
9. **REVISADO: Adicionar REPORTS_MIGRATION.md** para invalidar reports históricos
10. **REVISADO: Evolve.py precisa de redesign parcial** — mutation bank via TOML config overrides, não find/replace

---

## Plano TDD

### Fase 1: Fix Temperature Bug

1. **RED:** Teste em `test_headless.py` que mocka subprocess e verifica que `--temperature 0.0` aparece nos args do comando
2. **RED:** Teste Rust em `config.rs` que seta `THEO_TEMPERATURE=0.7` e assert `config.temperature == 0.7`
3. **GREEN:** Adicionar `--temperature` CLI flag + env var fallback em `resolve_agent_config()`
4. **GREEN:** `_headless.py` passa `--temperature` como CLI flag
5. **GREEN:** `smoke.py` passa temperature via `_headless.py`
6. **REFACTOR:** Remover path morto `ProjectConfig::with_env_overrides()` se redundante
7. **VERIFY:** `cargo test -p theo-agent-runtime` + `python -m pytest tests/test_headless.py`

### Fase 2: Environment Fingerprinting

1. **RED:** Teste que headless JSON contém campo `"environment"` com `"theo_git_commit"`
2. **GREEN:** Adicionar build.rs para capturar git commit. Emitir bloco environment no JSON
3. **REFACTOR:** Bump schema para v2
4. **VERIFY:** `theo --headless --temperature 0.0 "test"` | jq '.environment'

### Fase 3: Smoke Assertions

1. **RED:** Rodar cenários 01, 02, 07, 12, 15 com assertion nova (vão falhar se prompt não mudou)
2. **GREEN:** Atualizar prompts e assertions nos 5 TOMLs
3. **VERIFY:** `python runner/smoke.py --filter 01` (e os outros 4)

### Fase 4: SWE-bench Overhaul

1. **RED:** Teste que `adapter.py` default é `oracle=False`
2. **RED:** Teste de pass@k calculation
3. **GREEN:** Flip default, implementar pass@k, Wilson CI
4. **VERIFY:** `python -m pytest tests/test_swe_harness.py`

### Fase 5: Ablation

1. **RED:** `test_ablation_no_graph_returns_valid_results` em theo-engine-retrieval
2. **RED:** Teste que `AgentConfig { disable_graphctx: true }` propaga para pipeline
3. **GREEN:** GraphMode enum + AgentConfig fields + CLI flags
4. **VERIFY:** `cargo test -p theo-engine-retrieval` + `cargo test -p theo-agent-runtime`

---

## Action Items

- [ ] **Fase 1** — Fix temperature bug em TODOS os entry points (headless, smoke, evolve). TDD: RED primeiro. Inclui `--temperature` CLI flag, env var fallback, `_headless.py` pass-through, `smoke.py` propagation.
- [ ] **ADR** — Criar `docs/adr/benchmark-schema-v2.md` definindo novo schema antes de implementar Fase 2.
- [ ] **REPORTS_MIGRATION.md** — Criar em `apps/theo-benchmark/` marcando todos reports v1 como invalidados (temperature bug + oracle default).
- [ ] **Fase 2** — Environment fingerprinting com build.rs + git commit + schema v2.
- [ ] **Fase 3** — Upgrade 5 smoke assertions WEAK → STRONG.
- [ ] **Fase 4** — SWE-bench: non-oracle default + pass@k + `--runs N`.
- [ ] **Fase 5** — Ablation: GraphMode enum + AgentConfig fields + CLI flags.
- [ ] **Fases 6-10** — Multi-model, competitors, statistics, CI/CD, reporting (sequenciais, após Fases 1-5).
- [ ] **Verificar** — Compilation status de theo-application antes de iniciar (test-runner reportou errors).

---

## Veredito Final

**REVISED**: Plano de 10 fases aprovado com 5 revisões: (1) Fase 1 expandida para cobrir smoke.py e evolve.py, (2) ADR de schema v2 obrigatório antes da Fase 2, (3) REPORTS_MIGRATION.md para invalidar históricos, (4) Ablation em duas camadas (AgentConfig + RerankConfig.GraphMode), (5) Evolve.py precisa redesign parcial do mutation bank. Nenhum REJECT crítico. 3 CONCERNs resolvidos (data-ingestor via schema canônico, validator via migration guard, linter via TDD enforcement por mudança). Consenso: Fase 1 é P0 absoluto, non-oracle é obrigatório para publicação, SWE-bench Verified é o dataset primário.
