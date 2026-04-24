# Plano: Prompt A/B Testing — decisão data-driven sobre variantes SOTA

> **Versão 1.0** — fecha a lacuna entre "reescrevemos o prompt baseado em
> deep research" e "temos evidência empírica de qual variante é melhor".
> Sem A/B real, o prompt SOTA atual (commit `986768f`) é hipótese sem
> validação. Esta plano roda 3 variantes contra as MESMAS tarefas de
> Terminal-Bench Core e produz uma decisão estatisticamente defensável.

## Contexto

### O que motivou este plano

Após o run inicial de tb-core (n=39, pass rate 20.5%), identificamos 7
bugs nos dados reais e reescrevemos o `default_system_prompt` aplicando
doutrinas dos 4 scaffolds frontier (Codex 5.4, Claude Code 2.1, Gemini
CLI, pi-mono). O novo prompt tem ~2885 tokens vs ~2000 do anterior.

**O problema**: nenhum dado prova que o novo prompt é melhor. Estamos
reescrevendo baseado em teoria. O run anterior usou o prompt antigo, e
qualquer comparação com um run novo do prompt SOTA mistura múltiplas
mudanças (7 bug fixes + prompt rewrite).

**Decisão correta**: isolar o efeito do prompt sozinho via A/B controlado.

### Por que agora

- Antes de relançar tb-core completo (~$80, ~3h), queremos saber qual
  variante usar
- Antes de submeter resultados públicos, queremos defender escolhas
- Antes de aplicar este prompt em produção (interactive mode), queremos
  evidência além de "parece melhor pelas referências"

### Por que NÃO incluímos legacy

O prompt legacy (anterior ao commit `986768f`) tinha 2 anti-padrões já
diagnosticados nos dados:

1. **"VERIFY+DONE same response — Do not waste an iteration just to
   verify"** — causa direta dos 22% `tests_disagree` observados
2. **"Aim for 3-4 iterations"** — desencoraja persistência (oposto da
   doutrina dos 4 scaffolds frontier)

Re-rodar legacy seria gastar ~$20 para confirmar o que já sabemos:
ele perde. Foco do A/B é entre variantes do SOTA.

## Variantes a testar

| Variant | Conteúdo | Tokens (~) | Hipótese isolada |
|---|---|---|---|
| **`sota`** | Prompt de `default_system_prompt()` + bench-mode addendum (safety relax + execute-before-done) | 2885 + 280 = 3165 | Baseline do novo approach completo |
| **`sota-lean`** | Mesmo SOTA mas trimado a ~1500 tokens via remoção de seções verbosas | 1500 + 280 = 1780 | **H1**: tamanho do prompt importa? (pi-mono é ~150 tokens e funciona) |
| **`sota-no-bench`** | SOTA core sem o bench-mode addendum | 2885 | **H2**: o bench-mode block (safety relax + execute-before-done) gera ganho marginal sobre o SOTA core ou é redundante? |

### Comparações que cada par responde

| Comparação | Pergunta respondida | Decisão consequente |
|---|---|---|
| `sota` vs `sota-lean` | Tamanho importa? | Se lean ganha → enxugar TODOS os prompts. Se sota ganha → o tax do tamanho vale |
| `sota` vs `sota-no-bench` | Bench-mode addendum gera ganho? | Se sota ganha → manter bench-mode em runs de eval. Se igual → simplificar (remover addendum) |
| `sota-lean` vs `sota-no-bench` | Lean sem bench-mode é viável? | Identifica Pareto-front: melhor pass rate vs custo |

## Decisões de arquitetura

### D1: Variantes vivem em ARQUIVO, não em código

Hard-coded em `config.rs` requer rebuild Rust (~10min) por iteração.
Iteração de prompt deve ser instantânea.

**Implementação**: env var `THEO_SYSTEM_PROMPT_FILE=/path/to/variant.md`.
Quando setada, theo carrega o conteúdo do arquivo no startup como
`config.system_prompt`. Quando ausente, fallback para
`default_system_prompt()` hard-coded (zero breaking change).

Variantes vivem em `apps/theo-benchmark/prompts/` versionadas no git.

### D2: Paired comparison (não amostragem aleatória)

TODAS as variantes rodam as MESMAS N tasks. Isto elimina a maior
fonte de variância (qual task pegou) e permite McNemar test (mais
sensível que comparação de proporções independentes).

Sample size: N=20 tasks/variante. Detecta diferença de pass rate ≥
20pp com p<0.05 e power 0.8. Se queremos detectar diferenças menores
(10pp), precisaríamos N=80 — fora do orçamento desta iteração.

### D3: Mesmo modelo, mesma droplet, mesmo dia

Todos os confounders externos travados:
- Modelo: `gpt-5.4` (OAuth Codex)
- Droplet: a mesma já provisionada (`67.205.174.95`)
- Tempo: runs sequenciais no mesmo dia (variabilidade da API OpenAI)
- Theo SHA: pin único para todos os runs (commit do A/B)

### D4: Tasks selection — primeiras N alfabéticas (determinístico)

Em vez de selecionar tasks aleatoriamente (ruído entre execuções), usamos
as primeiras N tasks ordenadas alfabeticamente do dataset
`terminal-bench-core==0.1.1`. Isto garante:
- Mesmo set entre variantes ✓
- Reprodutibilidade entre executores ✓
- Cobertura razoável (alfabética é proxy para "diversidade de domínios")

Alternativa rejeitada: stratified sampling por categoria — overhead alto
para benefício marginal em N=20.

### D5: Statistical methodology — McNemar + bootstrap

- **Pass rate**: McNemar test (paired binary outcomes), reportar p-value
  e effect size. Significância threshold: p<0.05.
- **Cost/iter/duration**: paired diffs com bootstrap CI 95% (não assume
  normalidade). Reportar mediana e IQR.
- **Failure modes**: tabela de transições por task (variant_A_outcome,
  variant_B_outcome) — qual variante "consertou" tasks que a outra falhou.

### D6: Output em formato decision-ready

`reports/<date>/ab/comparison.md` deve responder em ≤30s de leitura:
1. Qual variante VENCE (ou "estatisticamente empate")?
2. Quanto o vencedor ganha (em pp de pass rate, em $/task)?
3. Quais tasks o vencedor desbloqueou que outras falharam?
4. Recomendação concreta para próxima decisão (qual variante adotar como
   default? continuar testando? testar mais N?)

## Fases

### Fase 52 — Theo carrega prompt de arquivo

**Objetivo**: `THEO_SYSTEM_PROMPT_FILE=/path/to/file.md` faz theo usar o
conteúdo do arquivo como `system_prompt`, sobrepondo o
`default_system_prompt()` hard-coded.

**Arquitetura**:
```rust
// apps/theo-cli/src/main.rs (modificar fn main após config.system_prompt = ...)

// Phase 52 (prompt-ab): operator can override the default system prompt
// via THEO_SYSTEM_PROMPT_FILE — used by A/B testing infrastructure
// to compare variants without rebuilding the binary.
if let Ok(path) = std::env::var("THEO_SYSTEM_PROMPT_FILE")
    && !path.is_empty()
{
    match std::fs::read_to_string(&path) {
        Ok(contents) => {
            config.system_prompt = contents;
            eprintln!("[theo] using prompt from {}", path);
        }
        Err(e) => {
            eprintln!("[theo] WARN: THEO_SYSTEM_PROMPT_FILE={} unreadable: {}; \
                       falling back to default", path, e);
        }
    }
}
```

**TDD Sequence**:
```
RED:
  prompt_file_env_overrides_default_when_set
  prompt_file_env_falls_back_when_unreadable
  prompt_file_env_falls_back_when_empty
  prompt_file_env_no_op_when_unset
  prompt_file_loaded_content_is_used_verbatim_no_processing

GREEN:
  - Add the env-check block in cmd_headless and cmd_agent
  - 5 unittest cases in apps/theo-cli (or extracted helper)

INTEGRATION:
  - target/debug/theo --version  (no env, default behavior)
  - THEO_SYSTEM_PROMPT_FILE=/tmp/empty.md theo --headless 'echo hi'
    (empty file → fallback)
```

**Verify**:
```bash
cargo build -p theo --bin theo
echo "Custom prompt for testing" > /tmp/test-prompt.md
THEO_SYSTEM_PROMPT_FILE=/tmp/test-prompt.md THEO_SKIP_ONBOARDING=1 \
  target/debug/theo --headless --max-iter 1 'noop' 2>&1 | grep "using prompt"
```

---

### Fase 53 — Criar 3 prompt variants em `apps/theo-benchmark/prompts/`

**Objetivo**: 3 arquivos markdown versionados.

**Arquivos**:
- `apps/theo-benchmark/prompts/sota.md` — copiado de `default_system_prompt()` + bench-mode addendum (string concatenada)
- `apps/theo-benchmark/prompts/sota-no-bench.md` — copiado de `default_system_prompt()` SEM addendum
- `apps/theo-benchmark/prompts/sota-lean.md` — versão trimada (~1500 tokens) — trabalho de redação

**Trabalho real está no `sota-lean`**: identificar quais seções do SOTA
podem ser cortadas sem perder doutrina. Candidatos primários a corte:
- Seção "Common pitfalls" inteira (10 linhas → 1 linha)
- Seção "When stuck" inteira
- Tool catalog: agrupar por categoria, 1 linha por categoria
- Reduzir editing rules a 3 bullets
- Manter intactos: workflow doctrine, persistência, git safety

Target: ~1500 tokens (vs 2885 atual).

**TDD Sequence**:
```
RED (per-file):
  sota_md_exists_and_has_persistence_doctrine
  sota_lean_md_exists_and_under_1700_tokens
  sota_no_bench_md_exists_and_lacks_benchmark_addendum

GREEN:
  - Write the 3 .md files
  - Optional: helper script to extract default_system_prompt() to .md
    (avoids drift between code and file)
```

**Verify**:
```bash
wc -c apps/theo-benchmark/prompts/*.md
# rough token estimate (chars/4):
for f in apps/theo-benchmark/prompts/*.md; do
  echo "$f: ~$(($(wc -c < $f) / 4)) tokens"
done
```

---

### Fase 54 — TheoAgent forwarda + resolve path no container

**Objetivo**: harness Python passa `THEO_SYSTEM_PROMPT_FILE` para dentro
do container, mas com path resolvido (o arquivo está no host).

**Estratégia**: o HTTP server da droplet (já serving `theo` binary +
`auth.json`) também serve as variantes:
- `http://172.17.0.1:8080/prompts/sota.md`
- `http://172.17.0.1:8080/prompts/sota-lean.md`
- `http://172.17.0.1:8080/prompts/sota-no-bench.md`

`setup.sh` (já modificado para bug #7 retry) também baixa o variant
selecionado para `/installed-agent/prompt.md` antes de invocar theo.

**Arquitetura**:
```python
# apps/theo-benchmark/tbench/agent.py (modificar _env)

# Phase 54: forward variant name; setup.sh resolves to URL
"THEO_PROMPT_VARIANT",  # values: "sota" | "sota-lean" | "sota-no-bench"
```

```bash
# apps/theo-benchmark/tbench/setup.sh (add after binary download)

# Phase 54: download A/B prompt variant if requested
if [ -n "${THEO_PROMPT_VARIANT:-}" ]; then
    local variant_url="http://172.17.0.1:8080/prompts/${THEO_PROMPT_VARIANT}.md"
    if curl -fsSL --max-time 10 "$variant_url" -o /installed-agent/prompt.md; then
        export THEO_SYSTEM_PROMPT_FILE=/installed-agent/prompt.md
        echo "[theo-setup] prompt variant loaded: ${THEO_PROMPT_VARIANT}"
    else
        echo "[theo-setup] WARN: prompt variant ${THEO_PROMPT_VARIANT} not available"
    fi
fi
```

**TDD Sequence**:
```
RED:
  agent_env_includes_theo_prompt_variant_when_set
  agent_env_omits_theo_prompt_variant_when_unset

GREEN:
  - Update _OTLP_ENV_KEYS / _env loop in agent.py to include the var
  - Update setup.sh to download + export THEO_SYSTEM_PROMPT_FILE
```

---

### Fase 55 — `runner/ab_test.py` — orquestração

**Objetivo**: rodar TODAS as 3 variantes sobre as MESMAS N tasks,
sequencial (não paralelo, para evitar contenção de containers).

**Arquitetura**:
```python
# apps/theo-benchmark/runner/ab_test.py
"""
A/B test orchestrator — Phase 55 (prompt-ab-testing).

Executes the same N tasks across multiple prompt variants, producing
paired data for statistical comparison.

Usage on the droplet:
    python3 runner/ab_test.py \\
      --variants sota,sota-lean,sota-no-bench \\
      --n-tasks 20 \\
      --output-dir reports/<date>/ab

Output:
    <output-dir>/<variant>/raw/   # tb run output per variant
    <output-dir>/manifest.json    # variants, tasks selected, theo SHA
"""
# Pseudocode:
# 1. Load tb dataset, take first N task ids alphabetically
# 2. For each variant in --variants:
#    a. Set THEO_PROMPT_VARIANT=<variant>
#    b. Invoke `tb run` with --task-id flags for the selected N tasks
#    c. Wait for completion
#    d. Move raw output to <output-dir>/<variant>/raw/
# 3. Write manifest.json with provenance pin (SHA, model, date, task list)
```

**TDD Sequence**:
```
RED:
  load_first_n_tasks_alphabetically_returns_n
  load_first_n_tasks_alphabetically_is_deterministic
  ab_test_writes_manifest_with_provenance

GREEN:
  - Implement ab_test.py
  - 3 unittests with fixture dataset
```

---

### Fase 56 — `runner/ab_compare.py` — paired statistical analysis

**Objetivo**: ler outputs das 3 variantes + produzir `comparison.md`
com decisão clara.

**Arquitetura**:
```python
# apps/theo-benchmark/runner/ab_compare.py
"""
Paired-comparison analysis — Phase 56 (prompt-ab-testing).

Reads <ab-dir>/<variant>/raw/ for all variants and produces:
- comparison.md (decision-ready summary)
- per_task_matrix.csv (rows=tasks, cols=variants, cells=resolved)
- mcnemar_results.json (paired binary test for each variant pair)
- cost_analysis.json (paired cost diffs with bootstrap CI 95%)

Statistical methods:
- McNemar test for paired binary outcomes (resolved/unresolved)
- Bootstrap CI for paired continuous metrics (cost, iters, duration)
- Categorical χ² for failure_mode distribution shifts
"""
# Pseudocode:
# 1. Walk each variant subdir, extract per-task resolution + sidecar metrics
# 2. Build matrix: tasks × variants
# 3. For each variant pair:
#    - McNemar on (resolved_A, resolved_B)
#    - Bootstrap on cost_diff, iter_diff, duration_diff
# 4. Render comparison.md with:
#    - Headline table (variant × pass_rate × $/task × p50 dur)
#    - Significance table (pair × p-value × effect direction)
#    - Per-task win/loss heatmap (which variant unblocked which task)
#    - Pareto front analysis
#    - Recommendation
```

**TDD Sequence**:
```
RED:
  mcnemar_returns_significant_when_clear_winner
  mcnemar_returns_nonsignificant_when_tied
  bootstrap_ci_brackets_observed_diff
  per_task_matrix_handles_missing_runs
  recommendation_chosen_when_statistically_significant
  recommendation_says_inconclusive_when_no_significance

GREEN:
  - Implement ab_compare.py with stdlib (statistics module + manual McNemar)
  - 6 unittests with synthetic data
```

---

### Fase 57 — Execução real + report

**Pré-req**: Fases 52-56 verdes localmente.

**Steps**:
1. Push variantes + scripts para droplet
2. Rebuild theo portable (com prompt-from-file capability)
3. Restart HTTP server para servir as 3 variantes
4. `bash scripts/bench/run-ab.sh` (wrapper invocando ab_test.py com defaults)
5. ~3h wait + monitor periódico
6. `python3 runner/ab_compare.py` localmente após `collect-everything.sh`
7. Commit `docs/benchmarks/2026-04-XX-prompt-ab.md` com findings

**Verify**:
```bash
# On droplet
bash scripts/bench/run-ab.sh

# Local (after run completes)
bash scripts/bench/collect-everything.sh
python3 apps/theo-benchmark/runner/ab_compare.py \
  --ab-dir .theo/bench-data/<date>/reports/ab \
  --output docs/benchmarks/<date>-prompt-ab.md
```

---

## Riscos e mitigações

| Risco | Mitigação |
|---|---|
| N=20 não detecta efeito pequeno (5pp) | Documentar limitação no report; sugerir N=80 follow-up se inconclusivo |
| Variantes não trimadas corretamente (sota-lean perde doutrina chave) | Test `sota_lean_md_includes_persistence_doctrine` lock |
| HTTP server morre mid-run (perda de download de prompt) | setup.sh já tem retry (bug #7 fix); fallback para default se falhar |
| OAuth token expira durante 3h de runs | Token expira 2026-05-02; runs em abril estão seguros. Documentar para refresh |
| Docker contention entre variantes (3 runs sequenciais usam mesmo Docker daemon) | Sequencial OK; paralelo seria problemático mas não estamos fazendo |
| Custo estoura $60 por LLM retries | Hard cap via `THEO_MAX_ITER=20` mantido; sidecar telemetry monitora |
| McNemar exige discordância — se variantes empatam em quase todas tasks, p-value não é significativo | Documentar como "no significant difference"; reportar effect size mesmo assim |

## Verificação final agregada

```bash
# Phase 52 — prompt loading
cargo test -p theo --bin theo -- prompt_file_env

# Phase 53 — variants exist
test -f apps/theo-benchmark/prompts/sota.md
test -f apps/theo-benchmark/prompts/sota-lean.md
test -f apps/theo-benchmark/prompts/sota-no-bench.md

# Phase 54 — agent forwarding
cd apps/theo-benchmark && python3 -m unittest tests.test_theo_agent

# Phase 55-56 — orchestration + analysis
cd apps/theo-benchmark && python3 -m unittest tests.test_ab_test tests.test_ab_compare

# Phase 57 — full run (real)
bash scripts/bench/run-ab.sh
```

## Cronograma

```
Sprint sequencial:
  Fase 52 (prompt-from-file)        ~30min + 5 RED tests
  Fase 53 (3 prompt variants)        ~20min (sota-lean é o trabalho real)
  Fase 54 (agent forward + setup.sh) ~15min
  Fase 55 (ab_test.py)               ~45min + 3 RED tests
  Fase 56 (ab_compare.py)            ~1h + 6 RED tests
  Fase 57 (rebuild + run + report)   ~3h CI + ~$60 LLM + 30min análise

Total work: ~3h
Total CI:   ~3h
Total $:    ~$60-80
Total wall: ~6h se sequencial
```

## Compromisso de cobertura final

Após este plano: **decisão data-driven sobre qual prompt variant adotar
como default em produção (interactive + benchmark)**.

| Item | Status pós-plano |
|---|---|
| Theo carrega prompt de arquivo via env | ✓ Fase 52 |
| 3 variantes versionadas em git | ✓ Fase 53 |
| Harness forwarda variante para containers | ✓ Fase 54 |
| Orchestrator A/B com paired tasks | ✓ Fase 55 |
| Statistical analysis (McNemar + bootstrap) | ✓ Fase 56 |
| Real run + report decisão | ✓ Fase 57 |

Plus:
- 14+ novos tests (TDD obrigatório por fase)
- Reprodutibilidade total (manifest.json + variant md files versionados)
- Decisão **defensável publicamente** (p-values, effect sizes, paired stats)

## Trabalho fora deste plano

Confirmados como épicos separados, **NÃO** parte deste escopo:

- **Test legacy prompt** — sabemos que perde, não vale gastar $20 confirmar
- **N=80 follow-up** — só se este plano der inconclusivo (effect <10pp)
- **Variantes não-SOTA** (Codex literal, Claude literal, etc.) — interesse
  acadêmico, não prático
- **A/B em outros benchmarks** (SWE-bench Lite, tbench Pro) — após decidir
  prompt em tb-core, expandir
- **A/B em modelos diferentes** (Opus 4.7 vs gpt-5.4) — controlar 1 variável
  por vez

## Referências

- Padrões SOTA: `referencias/system_prompts_examples/{Anthropic,OpenAI,Google}/`
- Findings que motivaram este plano:
  `docs/benchmarks/2026-04-24-tbench-core-partial.md`
- Commit do prompt SOTA atual: `986768f`
- Plano antecedente: `docs/plans/benchmark-validation-plan.md`
- McNemar test: standard paired-binary statistical test (Wikipedia ok)
- TDD: RED → GREEN → REFACTOR (sem exceções)
