# Plano: Benchmark Validation — Provar Theo Code antes de polir UX

> **Versão 1.0** — fecha o gap "sistema funciona internamente mas não
> tem score público comparável". Move o status quo de "passa nossos
> testes E2E" para "tem número defensível em Terminal-Bench 2.0,
> SWE-bench Verified e Terminal-Bench Pro, com OTLP traces capturados
> para análise pós-run".
>
> Princípio diretor (do usuário): **"funcionar perfeitamente antes de
> parecer perfeito"** — todo investimento em UX é prematuro até termos
> baseline reproduzível e iterável.

## Contexto

### Estado atual da infraestrutura

Auditoria de `apps/theo-benchmark/`:

| Componente | Estado | Pendência |
|---|---|---|
| `tbench/agent.py` (`TheoAgent` Harbor adapter) | ✓ Implementado com `BaseInstalledAgent` + ImportError fallback para `AbstractInstalledAgent` | API Harbor 2.0 mudou — verificar compat e atualizar |
| `tbench/setup.sh` | ✓ 3 métodos de install (URL, mounted volume, build-from-source) | Adicionar OTLP env passthrough |
| `swe/adapter.py` (SWE-bench Lite/Verified) | ✓ HuggingFace dataset loader + patch-only mode (sem Docker) | Validar que patch-mode roda standalone |
| `runner/smoke.py` (20 cenários internos) | ✓ Funcional, sem Docker (temp dirs + git init) | Confirmar que ainda passa após Phase 30-45 |
| `_headless.py` wrapper | ✓ Schema `theo.headless.v1` | Adicionar OTLP_ENDPOINT injection opcional |
| Reports infrastructure | ✓ JSONL + retention | Falta agregação cross-run + cost-per-task |
| OTLP exporter (Phase 40-45) | ✓ Funcional via feature `otel` | Habilitar default para runs de benchmark |

### Estado do mercado (abril 2026)

Targets que precisamos enfrentar:

| Benchmark | Top score 2026 | Modelo + scaffold |
|---|---|---|
| **SWE-bench Verified** | 87.6% | Opus 4.7 (Anthropic-reported) |
| **SWE-bench Verified (Vals.ai independent)** | 82.0% | Opus 4.7 (standardized harness) |
| **SWE-bench Pro** | 64.3% | Opus 4.7 (Anthropic) / 56.8% GPT-5.3-Codex CLI (independent) |
| **Terminal-Bench 2.0** | 81.8% | ForgeCode + Opus 4.6 (top scaffold) |
| **Terminal-Bench 2.0 (raw model)** | 75.1% | GPT-5.4 |
| **Terminal-Bench Pro** | (sem público estabelecido) | (Alibaba só liberou Nov/2025) |

A diferença "modelo bruto vs scaffold" é 5-15 pontos. Theo Code é
**scaffold** — competimos com ForgeCode/Terminus-KIRA/Codex CLI, não
com o modelo bruto. Targets defensíveis para v1:

| Benchmark | Theo Code v1 target | Justificativa |
|---|---|---|
| Terminal-Bench Core | ≥ 70% (com Opus 4.7 / GPT-5.4) | 5pp acima do raw model, abaixo do top scaffold (gap conhecido para iterar) |
| SWE-bench Lite | ≥ 60% | Score honesto para scaffold inicial |
| SWE-bench Verified | ≥ 70% | Margem para iterar |
| Terminal-Bench Pro (público) | ≥ 40% | Conservador — primeira tentativa em domínio "Pro" |

**Falha de target = sinal de iteração**, não derrota. Cada % abaixo é
oportunidade documentada de melhoria de prompt/tool/router.

### Causa raiz por que o sistema "funciona" mas não tem score público

1. **Adapter Harbor 2.0 não validado** — código existe (`tbench/agent.py`)
   mas nunca foi rodado contra Harbor real. O fallback ImportError
   sugere que pode estar pegando o branch errado em runtime.
2. **Sem captura sistemática de telemetria por-run** — temos OTLP
   exporter (Fase 40-45) mas não está habilitado nos benchmark scripts.
3. **Sem cost-tracking agregado** — o JSON `theo.headless.v1` tem
   `tokens.input/output/total` mas ninguém calcula `$/task`.
4. **Sem baseline em models pinados** — runs anteriores usaram
   modelos diferentes em datas diferentes. Sem reprodutibilidade.
5. **Docker** — Harbor exige Docker para spin-up de containers.
   Estação de dev atual não tem Docker (validado em Phase 45).

**Objetivo:** ter `bench/run-all.sh` que executa o benchmark suite
completo de forma reproduzível, exporta OTLP para análise, e gera
report comparativo `bench/reports/<date>/comparison.md` com
score-por-fase + cost-por-task + failure taxonomy.

**Estratégia:** 6 fases sequenciais, cada uma com TDD/validação real,
pré-requisitos explícitos (sem Docker = skip + log "needs Docker").
A entrega final é um número defensável para cada benchmark + dataset
de OTLP traces para iteração de prompt.

| Fase | Pré-req | Entrega | Bloqueia |
|---|---|---|---|
| **46** | Nenhum | Adapter Harbor 2.0 verificado + 5-task smoke local (sem Docker via patch-only path) | 47 |
| **47** | OTLP exporter (Fase 40-45) | OTLP wired em todo run de benchmark + script de análise pós-run | 48 |
| **48** | Docker + Phase 47 | Terminal-Bench Core (~100 tasks) — primeiro número público | 49 |
| **49** | Docker + Phase 47 | SWE-bench Lite (300 tasks) com patch-mode (sem Docker grader) + opcional grader Docker | 50 |
| **50** | Docker + ≥ 70% no #48 | Terminal-Bench Pro (200 públicas) | 51 |
| **51** | #48-50 ≥ targets | Submissão pública + blog post + leaderboard claim | — |

---

## Decisões de arquitetura

### D1: Validação > UX (princípio do usuário)

Toda hora gasta em UX antes de termos baseline reproduzível é hora
desperdiçada. O plano respeita esta diretriz: **zero alteração de UX
durante a entrega**. Toda mudança é em adapter, harness, observability.

### D2: Real OAuth Codex como provider primário

Já temos OAuth Codex wired (`auth.json` + `theo login` + `chatgpt-account`
allowlist). Usamos ele como provider primário porque:
- Token é o mesmo do dev workflow (zero gasto API extra para validar)
- Modelo `gpt-5.4` é uma das melhores opções disponíveis
- Validação E2E com OAuth já está provada (Fase 39: 26/26 stress)

Fallback: `OPENAI_API_KEY` direto para CI sem OAuth refresh.

### D3: OTLP em TODO run de benchmark

Inverter o default: durante benchmark, `OTLP_ENDPOINT` é mandatório
(não opcional). Cada task gera trajectory JSONL local + spans OTLP no
collector. Análise pós-run cruza ambos.

Justificativa: benchmark é EXATAMENTE o cenário onde queremos máxima
visibilidade. Custo de overhead é desprezível (~5% latência) vs. valor
analítico (filtra runs por failure taxonomy, agrupa por modelo, etc).

### D4: Cost tracking é mandatório

Cada report inclui:
- `tokens.input`, `tokens.output`, `tokens.total` (já temos)
- `cost_usd` (NOVO) — multiplicado por tabela de preços do modelo
- `iterations`, `tools.{total,success,success_rate}` (já temos)

Tabela de preços vive em `apps/theo-benchmark/pricing.toml` —
fonte única de verdade, atualizada manualmente quando provider muda
preço. Cada modelo tem `(input_price_per_mtok, output_price_per_mtok)`.

### D5: Public submission só APÓS bater target interno

Não submetemos nada até termos:
- ≥ 70% Terminal-Bench Core em 3 runs consecutivos (variance < 5pp)
- ≥ 60% SWE-bench Lite em 1 run completo
- Failure taxonomy documentada (não submeter score "sortudo")

Reduz risco de manchar reputação com número não-reprodutível.

### D6: Modelos pinados por data, não "latest"

Cada report registra:
- Modelo exato (e.g. `gpt-5.4-2026-04-15`, não `gpt-5.4`)
- Provider (e.g. `openai_oauth_codex`, não `openai`)
- Timestamp do run (UTC ISO)
- Git SHA do `theo` binary

Permite re-rodar 6 meses depois e comparar com a mesma config.
`reports/<date>/manifest.json` carrega tudo isso.

### D7: Smoke-first, scale-second (5-task gate)

Nunca rodamos 100+ tasks sem antes provar que 5 tasks completam
sem erro de adapter, parsing, OTLP, cost calculation. Custo de
5 tasks = ~$2; custo de 100 tasks com adapter quebrado = ~$50 desperdiçados.

Gate explícito em `bench/run-all.sh`: se smoke 5-task falha, abort
antes de continuar para Terminal-Bench/SWE-bench.

---

## Fases

### Fase 46 — Adapter Harbor 2.0 + smoke 5-task

**Objetivo:** garantir que `tbench/agent.py:TheoAgent` é compatível
com a API Harbor 2.0 atual (`pip install harbor`), e que um smoke
local de 5 tasks roda sem erro de adapter, parsing ou OTLP.

**Arquitetura:**

```python
# apps/theo-benchmark/tbench/agent.py (verificar/atualizar)

# Import correto da API Harbor 2.0:
from harbor.agents import BaseInstalledAgent
from harbor.harness_models import TerminalCommand

class TheoAgent(BaseInstalledAgent):
    @staticmethod
    def name() -> str:
        return "theo-code"

    @staticmethod
    def version() -> str:
        return f"0.1.0+{git_sha()[:7]}"  # NOVO: pin no SHA

    async def install(self, environment) -> None:
        # ... existing setup.sh path ...

    async def run(self, instruction: str, environment, context) -> None:
        # NOVO: forward OTLP env se setado fora
        env_extra = {}
        for k in ["OTLP_ENDPOINT", "OTLP_PROTOCOL", "OTLP_HEADERS",
                  "OTLP_SERVICE_NAME", "OTLP_TIMEOUT_SECS"]:
            if v := os.environ.get(k):
                env_extra[k] = v
        # ... existing exec_as_agent with env_extra merged ...

    async def populate_context_post_run(self, context) -> None:
        # ... existing JSON parsing ...
        # NOVO: extrair cost_usd da output via pricing.toml
        context["cost_usd"] = compute_cost(
            data.get("tokens", {}),
            data.get("model"),
        )
```

**Novo arquivo:** `apps/theo-benchmark/pricing.toml` — tabela de preços
versionada.

```toml
# apps/theo-benchmark/pricing.toml
# Source: provider docs as of 2026-04-24. Update when prices change.

[models."gpt-5.4"]
input_per_mtok = 5.00
output_per_mtok = 15.00

[models."gpt-5.3-codex"]
input_per_mtok = 3.00
output_per_mtok = 9.00

[models."claude-opus-4-7"]
input_per_mtok = 15.00
output_per_mtok = 75.00

[models."claude-sonnet-4-6"]
input_per_mtok = 3.00
output_per_mtok = 15.00
```

**TDD Sequence:**
```
RED:
  test_pricing_compute_cost_for_known_model
  test_pricing_returns_none_for_unknown_model
  test_pricing_computes_total_from_input_and_output_tokens
  test_pricing_handles_zero_tokens_gracefully
  test_theo_agent_version_includes_git_sha
  test_theo_agent_run_forwards_otlp_env_vars
  test_theo_agent_populate_context_extracts_cost_usd
  test_smoke_5_tasks_completes_without_adapter_error
  test_smoke_5_tasks_emits_cost_usd_for_each
  test_smoke_5_tasks_writes_otlp_spans_when_endpoint_set

GREEN:
  - Verify Harbor 2.0 compat (pip install harbor + import test)
  - Add pricing.toml + compute_cost() helper
  - Update agent.py: version() with SHA, run() forwards OTLP env,
    populate_context_post_run() includes cost_usd
  - Add CLI flag --smoke-5 to run/smoke.py for the 5-task gate

INTEGRATION:
  - Run smoke locally: python runner/smoke.py --filter 01,02,03,04,05
  - Verify reports/smoke-<ts>.json includes cost_usd per scenario
```

**Verify:**
```bash
cd apps/theo-benchmark
python -m pytest tests/test_pricing.py tests/test_theo_agent.py -v
python runner/smoke.py --filter 01,02,03,04,05
```

**Pré-req:** nenhum (não precisa Docker).

**Saída:** `apps/theo-benchmark/reports/smoke-<ts>.json` com 5 entries,
cada uma com `cost_usd` calculado.

**Risco mitigado:** se Harbor 2.0 quebrou alguma API, falha aqui ANTES
de gastar $$ rodando 100 tasks.

---

### Fase 47 — OTLP wiring per-run + análise pós-run

**Objetivo:** todo run de benchmark sobe um collector local OTLP
(via Docker Compose ou local binary), recebe spans de cada
`theo --headless` run, e gera análise agregada cruzando
trajectory JSONL local com spans OTLP.

**Arquitetura:**

```yaml
# apps/theo-benchmark/otlp/docker-compose.yml (NOVO)
# Sobe collector OTel + Jaeger UI para inspeção visual.

version: "3.8"
services:
  otel-collector:
    image: otel/opentelemetry-collector-contrib:0.110.0
    command: ["--config=/etc/otel/config.yaml"]
    volumes:
      - ./collector-config.yaml:/etc/otel/config.yaml:ro
    ports:
      - "4317:4317"   # OTLP gRPC
      - "4318:4318"   # OTLP HTTP
      - "8889:8889"   # Prometheus metrics

  jaeger:
    image: jaegertracing/all-in-one:1.62
    ports:
      - "16686:16686" # UI
      - "14268:14268" # OTLP receiver
    environment:
      - COLLECTOR_OTLP_ENABLED=true
```

```yaml
# apps/theo-benchmark/otlp/collector-config.yaml
receivers:
  otlp:
    protocols:
      grpc: { endpoint: 0.0.0.0:4317 }
      http: { endpoint: 0.0.0.0:4318 }

processors:
  batch:
  attributes/run_label:
    actions:
      - key: bench.run_id
        action: insert
        from_context: BENCH_RUN_ID
      - key: bench.benchmark
        action: insert
        from_context: BENCH_NAME

exporters:
  jaeger:
    endpoint: jaeger:14268
  file:
    path: /var/log/spans.jsonl
  prometheus:
    endpoint: 0.0.0.0:8889

service:
  pipelines:
    traces:
      receivers: [otlp]
      processors: [batch, attributes/run_label]
      exporters: [jaeger, file]
    metrics:
      receivers: [otlp]
      processors: [batch]
      exporters: [prometheus, file]
```

```python
# apps/theo-benchmark/analysis/post_run.py (NOVO)
"""
Cross-correlate trajectory JSONL (.theo/trajectories/*.jsonl) with
OTLP spans (collector file exporter output) to produce per-run report:

  - tokens.{input,output,total}
  - cost_usd (from pricing.toml)
  - iterations
  - tools.{total,success}
  - llm.{calls,retries}
  - duration_ms (wall clock)
  - p50/p95 tool dispatch latency (from spans)
  - p50/p95 LLM call latency (from spans)
  - failure_modes detected (from failure_sensors.rs analysis)
  - first_action_latency_ms (time-to-first-tool from spans)

Output: reports/<date>/<benchmark>/<task_id>.json
"""
```

```bash
# apps/theo-benchmark/run-all.sh (NOVO — orchestration entry point)
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BENCH_DIR="$REPO_ROOT/apps/theo-benchmark"
DATE=$(date -u +%Y-%m-%dT%H-%M-%SZ)
REPORT_DIR="$BENCH_DIR/reports/$DATE"
mkdir -p "$REPORT_DIR"

# 1. Pre-flight
require_docker || abort_or_skip
require_oauth_or_api_key || abort

# 2. Smoke gate (Phase 46) — abort if it fails
echo "[bench] Running smoke 5-task gate..."
python "$BENCH_DIR/runner/smoke.py" --filter 01,02,03,04,05 \
  --report "$REPORT_DIR/smoke.json" || abort "smoke failed"

# 3. Boot OTLP collector
docker compose -f "$BENCH_DIR/otlp/docker-compose.yml" up -d
trap "docker compose -f $BENCH_DIR/otlp/docker-compose.yml down" EXIT
sleep 5

# 4. Run benchmarks (sequential, per Phase 48-50)
export OTLP_ENDPOINT=http://localhost:4317
export OTLP_SERVICE_NAME=theo-bench-$DATE

bash "$BENCH_DIR/scripts/run-tbench-core.sh" "$REPORT_DIR" || true
bash "$BENCH_DIR/scripts/run-swebench-lite.sh" "$REPORT_DIR" || true

# 5. Aggregate + comparison
python "$BENCH_DIR/analysis/aggregate.py" \
  --report-dir "$REPORT_DIR" \
  --output "$REPORT_DIR/comparison.md"

echo "[bench] Done. See $REPORT_DIR/comparison.md"
```

**TDD Sequence:**
```
RED:
  test_post_run_extracts_tokens_from_trajectory_jsonl
  test_post_run_extracts_p95_latency_from_spans
  test_post_run_correlates_tool_dispatch_with_completion_via_call_id
  test_post_run_detects_failure_modes_from_sensor_output
  test_post_run_computes_first_action_latency_from_spans
  test_aggregate_produces_comparison_md_with_all_metrics
  test_aggregate_handles_partial_runs_gracefully

GREEN:
  - Create otlp/docker-compose.yml + collector-config.yaml
  - Create analysis/post_run.py + analysis/aggregate.py
  - Create scripts/run-all.sh with smoke gate
  - All tests use captured fixtures (no live Docker for unit tests)

INTEGRATION:
  - Run smoke + verify OTLP collector receives spans
  - Verify reports/<date>/comparison.md is generated
```

**Verify:**
```bash
cd apps/theo-benchmark
python -m pytest tests/test_post_run.py tests/test_aggregate.py -v
docker compose -f otlp/docker-compose.yml up -d
THEO_MAX_ITER=10 OTLP_ENDPOINT=http://localhost:4317 \
  python runner/smoke.py --filter 01
docker compose -f otlp/docker-compose.yml logs otel-collector | grep "subagent.spawn"
docker compose -f otlp/docker-compose.yml down
```

**Pré-req:** Docker (para o collector local). Se Docker absent, gate
explícito em `run-all.sh` cai para "trajectory-only mode" (sem OTLP).

---

### Fase 48 — Terminal-Bench Core (100 tasks)

**Objetivo:** primeiro número público comparável. Roda
`terminal-bench-core==head` via `tb run` com `TheoAgent`, captura
OTLP, gera report.

**Arquitetura:**

```bash
# apps/theo-benchmark/scripts/run-tbench-core.sh (NOVO)
set -uo pipefail

REPORT_DIR="$1"
mkdir -p "$REPORT_DIR/tbench-core"

# Required: tb installed, Docker running, theo binary built
command -v tb >/dev/null || { echo "tb absent — pip install terminal-bench"; exit 1; }
command -v docker >/dev/null || { echo "Docker absent"; exit 1; }
[ -x target/release/theo ] || { echo "theo binary absent — cargo build --release --features otel -p theo"; exit 1; }

# Run via the official tb CLI
# --k 1 = single attempt per task (Pass@1). Use --k 3 for Pass@3.
# --n-concurrent = parallel tasks. 4 is conservative for 16-core machine.

tb run \
  --dataset-name terminal-bench-core --dataset-version head \
  --agent-import-path tbench.agent:TheoAgent \
  --n-concurrent 4 \
  --k 1 \
  --output-dir "$REPORT_DIR/tbench-core/raw" \
  2>&1 | tee "$REPORT_DIR/tbench-core/tb-run.log"

# Post-process
python apps/theo-benchmark/analysis/tbench_post.py \
  --raw-dir "$REPORT_DIR/tbench-core/raw" \
  --output "$REPORT_DIR/tbench-core/summary.json"
```

**TDD Sequence:**
```
RED:
  test_run_tbench_script_aborts_when_docker_missing
  test_run_tbench_script_aborts_when_theo_binary_missing
  test_tbench_post_aggregates_pass_rate_per_category
  test_tbench_post_includes_cost_usd_per_task
  test_tbench_post_emits_failure_taxonomy

GREEN:
  - Create scripts/run-tbench-core.sh
  - Create analysis/tbench_post.py
  - Tests use captured fixtures (mock tb output)

INTEGRATION (precisa Docker + OAuth válido):
  - bash apps/theo-benchmark/scripts/run-tbench-core.sh /tmp/test-bench
  - Tempo estimado: 2-4h (100 tasks × 1-3min cada com paralelismo 4)
  - Custo estimado: $30-100 dependendo do modelo
```

**Verify:**
```bash
cd apps/theo-benchmark
python -m pytest tests/test_tbench_post.py -v
# E2E (precisa Docker):
bash scripts/run-tbench-core.sh /tmp/test-bench-$(date +%s)
```

**Critério de aceite:**
- ≥ 95 das 100 tasks executam sem adapter error
- ≥ 70% pass rate em 3 runs consecutivos (variance < 5pp) → SUCCESS
- Senão: documentar failure modes em `<report>/failures.md` e iterar
  prompts/tools antes de re-rodar

**Pré-req:** Docker + OAuth válido + theo binary com `--features otel`.

---

### Fase 49 — SWE-bench Lite (300 tasks) — patch-only + opcional grader

**Objetivo:** segundo número público — SWE-bench Lite, padrão da
indústria. Patch generation roda SEM Docker (já temos
`swe/adapter.py`); grading oficial requer Docker.

**Arquitetura:** estender o `swe/adapter.py` existente, sem rebuild.

```bash
# apps/theo-benchmark/scripts/run-swebench-lite.sh (NOVO)
set -uo pipefail

REPORT_DIR="$1"
mkdir -p "$REPORT_DIR/swebench-lite"

# Phase 1: patch generation (no Docker required)
python apps/theo-benchmark/swe/adapter.py \
  --dataset lite \
  --report "$REPORT_DIR/swebench-lite/patches.json"

# Phase 2: official grading (requires Docker + swebench package)
if command -v docker >/dev/null && python -c "import swebench" 2>/dev/null; then
  python apps/theo-benchmark/swe/adapter.py \
    --dataset lite \
    --grade \
    --predictions "$REPORT_DIR/swebench-lite/patches.json" \
    --report "$REPORT_DIR/swebench-lite/graded.json"
else
  echo "[swebench-lite] Docker or swebench package missing — skipping grading"
fi

# Aggregate
python apps/theo-benchmark/analysis/swe_post.py \
  --report-dir "$REPORT_DIR/swebench-lite" \
  --output "$REPORT_DIR/swebench-lite/summary.json"
```

**TDD Sequence:**
```
RED:
  test_swe_post_aggregates_resolved_count
  test_swe_post_includes_cost_usd_per_instance
  test_swe_post_handles_no_grader_gracefully

GREEN:
  - Create scripts/run-swebench-lite.sh
  - Create analysis/swe_post.py
  - Tests use captured fixtures

INTEGRATION:
  - bash scripts/run-swebench-lite.sh /tmp/test-swe (patch-only — sem Docker)
  - Tempo: 4-8h, ~$80-200
  - Com Docker: +2-4h grading, sem custo LLM extra
```

**Critério de aceite:**
- Patches gerados para ≥ 290 das 300 instâncias (sem adapter crash)
- Resolved rate (com grader) ≥ 60% para promover para Verified
- Senão: análise de failure mode em `summary.json` + iteração

**Pré-req:**
- Patch-only: nenhum (Python + theo binary)
- Grader: Docker + `pip install swebench` (~15GB de imagens Docker)

---

### Fase 50 — Terminal-Bench Pro (200 públicas)

**Objetivo:** "boss fight" — Alibaba's harder benchmark. 200 tasks
públicas (200 privadas requerem submissão por email). Confirma que
sistema lida com tasks "Pro-tier" (debugging real, sysadmin, security).

**Arquitetura:** mesmo padrão de Phase 48, dataset diferente.

```bash
# apps/theo-benchmark/scripts/run-tbench-pro.sh (NOVO)
set -uo pipefail

REPORT_DIR="$1"
mkdir -p "$REPORT_DIR/tbench-pro"

# Pre-req gate: terminal-bench-pro adoptado pela Harbor registry?
# Se sim, usar --dataset; se não, clonar o repo e usar --path.

if harbor datasets list 2>/dev/null | grep -q terminal-bench-pro; then
  harbor run \
    --dataset terminal-bench-pro@1.0 \
    --agent-import-path tbench.agent:TheoAgent \
    --n-concurrent 4 --k 1 \
    --jobs-dir "$REPORT_DIR/tbench-pro/raw"
else
  # Fallback: clone + path mode
  CLONE_DIR=$(mktemp -d -t tbench-pro-XXXXXX)
  git clone --depth 1 https://github.com/alibaba/terminal-bench-pro "$CLONE_DIR"
  trap "rm -rf $CLONE_DIR" EXIT
  harbor run \
    --path "$CLONE_DIR" \
    --agent-import-path tbench.agent:TheoAgent \
    --n-concurrent 4 --k 1 \
    --jobs-dir "$REPORT_DIR/tbench-pro/raw"
fi

python apps/theo-benchmark/analysis/tbench_post.py \
  --raw-dir "$REPORT_DIR/tbench-pro/raw" \
  --output "$REPORT_DIR/tbench-pro/summary.json"
```

**TDD Sequence:**
```
RED:
  test_run_tbench_pro_uses_dataset_when_in_registry
  test_run_tbench_pro_falls_back_to_clone_when_dataset_absent
  test_tbench_pro_post_groups_by_domain (8 domains: data/games/debug/...)

GREEN:
  - Create scripts/run-tbench-pro.sh
  - Extend analysis/tbench_post.py to break down by 8 Pro domains
  - Tests with captured fixtures

INTEGRATION:
  - bash scripts/run-tbench-pro.sh /tmp/test-pro
  - Tempo: 6-12h
  - Custo: $200-600
```

**Critério de aceite:**
- ≥ 195 das 200 tasks executam sem crash
- ≥ 40% pass rate (conservador para v1)
- Per-domain breakdown identifica forças/fraquezas

**Pré-req:** Phase 48 ≥ 70%, Docker, OAuth válido, custo aprovado.

---

### Fase 51 — Submissão pública + blog post

**Objetivo:** publicar resultados. Sem isso, todo investimento de
Phase 46-50 fica interno.

**Entregas:**

1. **Atualizar `tbench/agent.py`** com final version + commit SHA da
   release que vai ser submetida.

2. **`docs/benchmarks/<date>-results.md`** — resultados estruturados:
   - Hardware (CPU, RAM, OS)
   - Modelo + provider
   - Theo Code commit SHA
   - Per-benchmark scores + variance over N runs
   - Cost analysis ($/task, $/% gained)
   - Failure taxonomy
   - Reproduction commands

3. **Pull request à `tbench.ai`** — submeter o `TheoAgent` adapter
   ao registry oficial.

4. **Email para `yanquan.xx@alibaba-inc.com`** — submeter
   Terminal-Bench Pro public results.

5. **Blog post** (opcional, depende do score) — narrativa de
   "Theo Code v1: scaffold benchmark results + lessons learned".

**TDD Sequence:**
```
RED (manual checks):
  - results.md tem todas as seções obrigatórias
  - submission email tem resultados verificáveis
  - PR à tbench.ai segue o template do projeto

GREEN:
  - Escrever results.md
  - Submeter
```

**Critério de aceite:** confirmação de recebimento de cada submissão.

---

## Riscos e mitigações

| Risco | Mitigação |
|---|---|
| Docker não disponível na máquina dev | Phases 46+47 funcionam sem Docker; phases 48+ explicitamente gateadas |
| Custo OAuth Codex disparar | Smoke gate (Phase 46) + cost tracking (D4) — abort se `cost_usd / task > $5` |
| Token OAuth expirar mid-run | `_headless.py` já detecta + script captura erro, não derruba run inteiro |
| Harbor 2.0 API quebrou nosso adapter | Phase 46 valida ANTES de gastar $$ em scale |
| Modelos atualizam mid-benchmark | D6: pin exato do modelo + commit SHA; report manifest |
| Variance entre runs > 10pp | --k 3 (3 attempts/task) + reportar mediana, não single run |
| Submissão pública revela resultados ruins | D5: só submeter quando ≥ target interno em 3 runs |
| OTLP collector vira gargalo | Phase 47 collector tem `batch` processor + métricas Prometheus para detectar |
| Rate limit do provider mid-run | Retry policy já existe (`RetryPolicy::default_llm`); `theo --headless` não morre |
| Disk fill por OTLP file exporter | Collector config tem rotation; report dir limpo após análise |

---

## Verificação final agregada

```bash
# Phase 46 — adapter + smoke
cd apps/theo-benchmark
python -m pytest tests/test_pricing.py tests/test_theo_agent.py -v
python runner/smoke.py --filter 01,02,03,04,05

# Phase 47 — OTLP wiring
python -m pytest tests/test_post_run.py tests/test_aggregate.py -v
docker compose -f otlp/docker-compose.yml up -d
THEO_MAX_ITER=10 OTLP_ENDPOINT=http://localhost:4317 \
  python runner/smoke.py --filter 01
docker compose down

# Phase 48 — Terminal-Bench Core (precisa Docker)
bash scripts/run-tbench-core.sh /tmp/bench-tbench

# Phase 49 — SWE-bench Lite
bash scripts/run-swebench-lite.sh /tmp/bench-swe

# Phase 50 — Terminal-Bench Pro
bash scripts/run-tbench-pro.sh /tmp/bench-pro

# End-to-end (one button)
bash scripts/run-all.sh
```

---

## Cronograma

```
Sprint sequencial — toda fase é gate da próxima:

Fase 46 (adapter + smoke 5-task)         ~3h
Fase 47 (OTLP wiring + análise)          ~4h
Fase 48 (Terminal-Bench Core)            ~6h CI ($30-100)
Fase 49 (SWE-bench Lite patches)         ~6h CI ($80-200)
        + grader (Docker, opcional)      ~3h CI (sem $$ extra)
Fase 50 (Terminal-Bench Pro)             ~10h CI ($200-600)
Fase 51 (submissão + docs)               ~6h

Total work: ~38h sequenciais
Total custo: ~$300-900 dependendo de retries/variance
Total wall-clock: 4-7 dias se 1 pessoa, ~2 dias com paralelismo
```

---

## Compromisso de cobertura final

Após este plano: **score público comparável + dataset OTLP para iteração**.

| Item | Status pós-plano |
|---|---|
| Theo Code Harbor 2.0-compatible | ✓ Phase 46 |
| Smoke 5-task gate | ✓ Phase 46 |
| OTLP collector local + análise pós-run | ✓ Phase 47 |
| Cost-per-task tracking | ✓ Phase 46 (`pricing.toml`) |
| Terminal-Bench Core score | ✓ Phase 48 (target ≥ 70%) |
| SWE-bench Lite score | ✓ Phase 49 (target ≥ 60%) |
| Terminal-Bench Pro score | ✓ Phase 50 (target ≥ 40%) |
| Failure taxonomy documentada | ✓ Phase 47 + 48-50 reports |
| Public submission | ✓ Phase 51 |
| Blog post / external comms | ✓ Phase 51 (opcional) |

Plus:
- Reprodutibilidade: cada run gera `manifest.json` com modelo + commit SHA + timestamp
- Telemetria: cada task tem trajectory JSONL + spans OTLP correlacionados
- Custo: cada task tem `cost_usd` calculado a partir de `pricing.toml`

---

## Trabalho fora deste plano

Confirmados como épicos separados, **NÃO** parte deste escopo:

- **Fine-tuning router** — só faz sentido após termos failure taxonomy
  rica de Phase 50
- **Terminal-Bench Pro privado** (200 tasks) — requer email + API
  access do Alibaba; aguardamos resposta após Phase 51
- **Outros benchmarks** (BigCodeBench, LiveCodeBench, Aider Polyglot) —
  cada um seu próprio adapter; depois de baseline em TBench/SWE
- **Cloud runner** (GCP/AWS/Modal) para escalar concorrência > 16 — só
  vale se runs locais virarem gargalo
- **A/B test scaffolds** (ForgeCode-style vs nosso) — após termos
  número estável para comparar
- **Continuous benchmarking em CI** — só após passar baseline 2x
  estável

---

## Referências

- [terminal-bench-pro (Alibaba)](https://github.com/alibaba/terminal-bench-pro)
- [Terminal-Bench (laude-institute)](https://github.com/laude-institute/terminal-bench)
- [Terminal-Bench 2.0 docs](https://www.tbench.ai/docs/installation)
- [Pi terminal-bench adapter (community example)](https://github.com/badlogic/pi-terminal-bench)
- [Terminal-Bench leaderboard](https://www.tbench.ai/leaderboard/terminal-bench/2.0)
- [SWE-Bench Verified leaderboard](https://www.swebench.com/)
- [Vals.ai independent SWE-bench](https://www.vals.ai/benchmarks/swebench)
- `apps/theo-benchmark/README.md` — infraestrutura existente
- `apps/theo-benchmark/tbench/agent.py` — adapter atual
- `apps/theo-benchmark/swe/adapter.py` — SWE-bench adapter (patch-only OK)
- `docs/plans/otlp-exporter-plan.md` — Phase 40-45 (pré-req da Fase 47)
- TDD: RED → GREEN → REFACTOR (sem exceções)
- Plano antecedente: `docs/plans/otlp-exporter-plan.md` (estrutura)
