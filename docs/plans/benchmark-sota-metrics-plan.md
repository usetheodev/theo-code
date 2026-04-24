# Plano: Benchmark SOTA Metrics — Observabilidade Completa

> **Versão 1.0** — Fecha o gap entre as ~40 métricas que o runtime Rust
> já computa (RunReport) e as ~10 que o benchmark Python extrai hoje.
> Move o sistema de "mede pass/fail + tokens" para "observabilidade
> completa comparável com Aider, ForgeCode e Claude Code internals".
>
> **Princípio:** o runtime JÁ TEM os dados. O trabalho é 80% extração
> e reporting, 20% enriquecimento do JSON de saída.

## Diagnóstico

### Estado Atual — Taxa de Extração: 25%

```
RunReport (Rust)                    →  HeadlessResult (Python)
────────────────────────────────────────────────────────────
TokenMetrics (8 campos)             →  3 extraídos (input, output, total)
LoopMetrics (7 campos)              →  1 extraído (iterations)
ToolBreakdown (8 campos × N tools)  →  2 extraídos (total, success)
ContextHealthMetrics (8 campos)     →  0 extraídos
MemoryMetrics (8 campos)            →  0 extraídos
SubagentMetrics (6 campos)          →  0 extraídos
ErrorTaxonomy (9 campos)            →  0 extraídos (failure_modes parcial)
DerivedMetrics (5 métricas)         →  0 extraídos
IntegrityReport (7 campos)          →  0 extraídos
────────────────────────────────────────────────────────────
Total: ~60 campos disponíveis       →  ~10 extraídos = 25%
```

### Causa Raiz

1. **headless JSON (v3) é slim**: serializa apenas `success, summary,
   iterations, duration_ms, tokens{3}, tools{2}, llm{2}, files_edited,
   model, mode, provider, environment, error_class`
2. **RunReport fica no trajectory JSONL**: linha Summary tem TUDO, mas
   trajectory é arquivo local (inacessível em Docker containers)
3. **Python analysis layer só lê headless JSON**: `_headless.py` define
   HeadlessResult com os campos slim; analysis modules herdam essa limitação

### Solução Arquitetural

```
ANTES:
  theo --headless → stdout JSON (v3, 15 campos)
  trajectory.jsonl → Summary line (RunReport, 60+ campos) [inacessível em Docker]
  Python → parse stdout → 10 campos úteis

DEPOIS:
  theo --headless → stdout JSON (v4, 60+ campos) ← embed RunReport no JSON
  Python → parse stdout → 60+ campos úteis
  Report → métricas SOTA completas
```

**Decisão: enriquecer o headless JSON (v3→v4)** em vez de parsear trajectory.
Motivo: trajectory não é acessível em containers Docker (tbench, swe-bench).
O headless JSON é a interface universal. Backward compat: campos novos são
opcionais, v3 parsers ignoram campos extras.

---

## Fases e Tasks

### Fase 1 — Enriquecer Headless JSON (Rust)

> **Objetivo:** O JSON de saída do `theo --headless` inclui o RunReport
> completo como campo `report` no JSON v4. Backward compatible.

#### Task 1.1 — Adicionar campo `report` ao headless JSON

**Descrição:** Em `apps/theo-cli/src/main.rs` (função `cmd_headless`),
após computar o `AgentResult`, serializar o `RunReport` completo como
campo `report` no JSON de saída. Bumpar schema para `theo.headless.v4`.

**Arquivos a modificar:**
- `apps/theo-cli/src/main.rs` — função `cmd_headless` (linhas ~665-875)
- `crates/theo-agent-runtime/src/observability/report.rs` — garantir `#[derive(Serialize)]` em todos os structs

**Critérios de aceite:**
- [ ] JSON de saída contém campo `report` com estrutura RunReport completa
- [ ] Schema bumpa para `theo.headless.v4`
- [ ] Campos existentes (success, summary, tokens, tools, llm) permanecem inalterados
- [ ] `report` é `null` quando RunReport não está disponível (fallback graceful)
- [ ] Todas as sub-structs de RunReport são serializáveis: TokenMetrics, LoopMetrics, ToolBreakdown[], ContextHealthMetrics, MemoryMetrics, SubagentMetrics, ErrorTaxonomy, DerivedMetrics, IntegrityReport

**DoD:**
- [ ] `cargo test -p theo-cli` passa
- [ ] `cargo test -p theo-agent-runtime` passa
- [ ] Teste manual: `theo --headless "read main.rs" | python3 -c "import json,sys; r=json.load(sys.stdin); assert 'report' in r; assert r['schema']=='theo.headless.v4'"` passa
- [ ] Backward compat: parsers v3 não quebram (campos extras ignorados por serde)

---

#### Task 1.2 — Garantir Serialize em todos os structs do RunReport

**Descrição:** Verificar e adicionar `#[derive(Serialize, Deserialize)]`
em todas as structs referenciadas por RunReport. Algumas podem ter
apenas `Debug` hoje.

**Arquivos a verificar:**
- `crates/theo-agent-runtime/src/observability/report.rs` — RunReport, TokenMetrics, LoopMetrics, PhaseMetric, BudgetUtilization, ToolBreakdown, ContextHealthMetrics, MemoryMetrics, SubagentMetrics, ErrorTaxonomy
- `crates/theo-agent-runtime/src/observability/derived_metrics.rs` — DerivedMetrics, SurrogateMetric
- `crates/theo-agent-runtime/src/observability/reader.rs` — IntegrityReport, MissingRange

**Critérios de aceite:**
- [ ] Todos os structs em RunReport tree derivam `Serialize` + `Deserialize`
- [ ] `serde_json::to_value(&run_report)` compila e produz JSON válido
- [ ] Nenhum campo usa tipos não-serializáveis (Arc, Mutex, etc.)
- [ ] HashMap<String, PhaseMetric> serializa corretamente como objeto JSON

**DoD:**
- [ ] `cargo test -p theo-agent-runtime` passa
- [ ] Teste unitário: `fn test_run_report_serializes()` que cria RunReport mock e valida JSON output
- [ ] Zero warnings de compilação

---

### Fase 2 — Atualizar Extração Python

> **Objetivo:** HeadlessResult e analysis modules parseiam TODOS os
> campos do RunReport. Nenhuma informação disponível é descartada.

#### Task 2.1 — Expandir HeadlessResult com campos RunReport

**Descrição:** Adicionar campos ao dataclass HeadlessResult para cada
seção do RunReport. Parsing graceful: campos ausentes = None/default.

**Arquivo:** `apps/theo-benchmark/_headless.py`

**Novos campos a adicionar:**

```python
# Token metrics (expandido)
cache_read_tokens: int = 0
cache_write_tokens: int = 0
reasoning_tokens: int = 0
cache_hit_rate: float = 0.0
tokens_per_successful_edit: float = 0.0

# Loop metrics
total_iterations: int = 0  # (alias de iterations, para consistência)
convergence_rate: float = 0.0
budget_utilization_iterations_pct: float = 0.0
budget_utilization_tokens_pct: float = 0.0
budget_utilization_time_pct: float = 0.0
evolution_attempts: int = 0
evolution_success: bool = False
phase_distribution: dict[str, dict] = field(default_factory=dict)

# Tool breakdown (per-tool)
tool_breakdown: list[dict] = field(default_factory=list)

# Context health
context_avg_size_tokens: float = 0.0
context_max_size_tokens: int = 0
context_growth_rate: float = 0.0
context_compaction_count: int = 0
context_compaction_savings_ratio: float = 0.0
context_refetch_rate: float = 0.0
context_action_repetition_rate: float = 0.0
context_usefulness_avg: float = 0.0

# Memory metrics
memory_episodes_injected: int = 0
memory_episodes_created: int = 0
memory_hypotheses_formed: int = 0
memory_hypotheses_invalidated: int = 0
memory_hypotheses_active: int = 0
memory_constraints_learned: int = 0
memory_failure_fingerprints_new: int = 0
memory_failure_fingerprints_recurrent: int = 0

# Subagent metrics
subagent_spawned: int = 0
subagent_succeeded: int = 0
subagent_failed: int = 0
subagent_avg_duration_ms: float = 0.0
subagent_success_rate: float = 0.0

# Error taxonomy
error_total: int = 0
error_network: int = 0
error_llm: int = 0
error_tool: int = 0
error_sandbox: int = 0
error_budget: int = 0
error_validation: int = 0

# Derived metrics (surrogate)
doom_loop_frequency: float = 0.0
llm_efficiency: float = 0.0
context_waste_ratio: float = 0.0
hypothesis_churn_rate: float = 0.0
time_to_first_tool_ms: float = 0.0

# Integrity
trajectory_complete: bool = True
trajectory_confidence: float = 1.0
```

**Critérios de aceite:**
- [ ] `HeadlessResult.from_json()` parseia campo `report` quando presente
- [ ] Campos ausentes (headless v3) usam defaults seguros (0, 0.0, None, [])
- [ ] `tool_breakdown` é lista de dicts: `[{tool_name, call_count, success_count, failure_count, avg_latency_ms, max_latency_ms, success_rate}]`
- [ ] `phase_distribution` é dict: `{phase_name: {iterations, duration_ms, pct}}`
- [ ] Nenhum campo existente muda de tipo ou semântica

**DoD:**
- [ ] `pytest tests/test_headless.py` passa (testes existentes não quebram)
- [ ] 15+ novos testes cobrindo parsing de cada seção do RunReport
- [ ] Teste com JSON v3 (sem campo report) — todos os novos campos são default
- [ ] Teste com JSON v4 completo — todos os campos parseados corretamente
- [ ] Teste com JSON v4 parcial (report com campos faltando) — graceful defaults

---

#### Task 2.2 — Expandir AggregatedResult com estatísticas dos novos campos

**Descrição:** O `run_headless_multi()` deve agregar os novos campos
com mean/std/percentis quando aplicável.

**Arquivo:** `apps/theo-benchmark/_headless.py`

**Novos campos em AggregatedResult:**

```python
mean_cache_hit_rate: float = 0.0
mean_convergence_rate: float = 0.0
mean_budget_utilization_pct: float = 0.0
mean_context_max_size: float = 0.0
mean_context_growth_rate: float = 0.0
mean_doom_loop_frequency: float = 0.0
mean_llm_efficiency: float = 0.0
mean_context_waste_ratio: float = 0.0
mean_time_to_first_tool_ms: float = 0.0
total_subagent_spawned: int = 0
total_errors: int = 0
tool_breakdown_aggregate: dict[str, dict] = field(default_factory=dict)
```

**Critérios de aceite:**
- [ ] `tool_breakdown_aggregate` combina por tool_name: soma call_count, success_count; média de avg_latency_ms
- [ ] Médias usam divisão segura (n=0 → 0.0)
- [ ] Wilson CI continua funcionando para success_rate

**DoD:**
- [ ] `pytest tests/test_headless.py` passa
- [ ] 5+ novos testes para agregação multi-run dos novos campos
- [ ] Teste edge case: 1 run com report + 1 run sem report (v3 legacy)

---

### Fase 3 — Módulos de Análise por Categoria

> **Objetivo:** Cada categoria de métrica tem um módulo de análise
> dedicado que produz output estruturado para o aggregator.

#### Task 3.1 — Módulo: Context Health Analysis

**Descrição:** Criar `analysis/context_health.py` que analisa métricas
de contexto ao longo das tasks de um benchmark run.

**Arquivo novo:** `apps/theo-benchmark/analysis/context_health.py`

**Input:** Lista de HeadlessResult (já parseados com campos v4)

**Output schema:**
```python
{
    "summary": {
        "avg_context_size_tokens": float,
        "max_context_size_tokens": int,
        "avg_growth_rate": float,
        "total_compactions": int,
        "avg_compaction_savings": float,
        "avg_refetch_rate": float,
        "avg_action_repetition_rate": float,
        "avg_usefulness": float,
    },
    "distributions": {
        "context_size_p50": float,
        "context_size_p95": float,
        "growth_rate_p50": float,
        "growth_rate_p95": float,
    },
    "correlations": {
        "context_size_vs_success": float,  # point-biserial r
        "compaction_count_vs_success": float,
        "usefulness_vs_success": float,
    },
    "alerts": [
        {"type": "high_growth_rate", "task_id": str, "value": float},
        {"type": "zero_usefulness", "task_id": str},
    ]
}
```

**Critérios de aceite:**
- [ ] Correlações usam point-biserial (success é binário, métrica é contínua)
- [ ] Alerts disparam para growth_rate > 2σ acima da média
- [ ] Funciona com 0 tasks (retorna struct vazia com defaults)
- [ ] Funciona com tasks sem dados de context (v3 legacy)

**DoD:**
- [ ] `pytest tests/test_context_health.py` com 10+ testes
- [ ] Teste com dados sintéticos cobrindo: all-pass, all-fail, mixed, empty, legacy-v3
- [ ] Documentação inline descrevendo cada métrica

---

#### Task 3.2 — Módulo: Tool Breakdown Analysis

**Descrição:** Criar `analysis/tool_analysis.py` que analisa performance
por ferramenta individual (read, edit, bash, grep, glob, think, etc.)

**Arquivo novo:** `apps/theo-benchmark/analysis/tool_analysis.py`

**Output schema:**
```python
{
    "per_tool": {
        "read": {
            "total_calls": int,
            "success_rate": float,
            "avg_latency_ms": float,
            "max_latency_ms": float,
            "p50_latency_ms": float,
            "p95_latency_ms": float,
            "pct_of_total_calls": float,
            "pct_of_total_latency": float,
        },
        "edit": { ... },
        "bash": { ... },
        ...
    },
    "summary": {
        "total_tools": int,       # unique tool names
        "total_calls": int,
        "overall_success_rate": float,
        "slowest_tool": str,
        "most_used_tool": str,
        "most_failing_tool": str,
    },
    "correlations": {
        "tool_diversity_vs_success": float,  # mais tools usadas = mais sucesso?
        "bash_calls_vs_success": float,
        "edit_success_rate_vs_task_success": float,
    }
}
```

**Critérios de aceite:**
- [ ] Agrega tool_breakdown de múltiplos HeadlessResults por tool_name
- [ ] Percentis calculados sobre distribuição de latência across tasks
- [ ] Identifica corretamente o tool mais lento, mais usado, mais falhante
- [ ] Funciona com tasks sem tool_breakdown (v3 legacy — skip gracefully)

**DoD:**
- [ ] `pytest tests/test_tool_analysis.py` com 10+ testes
- [ ] Teste com 5+ tools diferentes, cada uma com call_count variado
- [ ] Teste edge case: uma task com 100% bash, outra com 100% read

---

#### Task 3.3 — Módulo: Agent Loop Analysis

**Descrição:** Criar `analysis/loop_analysis.py` que analisa o comportamento
do agent loop: convergência, distribuição de fases, utilização de budget.

**Arquivo novo:** `apps/theo-benchmark/analysis/loop_analysis.py`

**Output schema:**
```python
{
    "convergence": {
        "avg_convergence_rate": float,
        "tasks_converged_pct": float,     # convergence_rate > 0
        "tasks_budget_bound_pct": float,  # budget_utilization > 95%
        "avg_iterations_to_converge": float,
        "median_iterations": float,
    },
    "budget_utilization": {
        "avg_iterations_pct": float,
        "avg_tokens_pct": float,
        "avg_time_pct": float,
        "tasks_hitting_iter_limit_pct": float,  # iterations_pct >= 0.95
        "tasks_hitting_token_limit_pct": float,
        "tasks_hitting_time_limit_pct": float,
    },
    "phase_distribution": {
        "planning_avg_pct": float,
        "executing_avg_pct": float,
        "evaluating_avg_pct": float,
        "other_phases": dict[str, float],
    },
    "evolution": {
        "total_evolution_attempts": int,
        "evolution_success_rate": float,
    },
    "correlations": {
        "iterations_vs_success": float,
        "budget_util_vs_success": float,
        "convergence_rate_vs_success": float,
    },
    "alerts": [
        {"type": "budget_bound", "task_id": str, "iterations_pct": float},
        {"type": "zero_convergence", "task_id": str},
    ]
}
```

**Critérios de aceite:**
- [ ] Identifica tasks que batem no iteration limit (budget_utilization_iterations_pct >= 0.95)
- [ ] Phase distribution normalizada (soma = 100%)
- [ ] Correlação iterations vs success deve ser negativa (mais iterações = menor chance de sucesso) — validar com dados sintéticos
- [ ] Funciona com phase_distribution vazia (v3 legacy)

**DoD:**
- [ ] `pytest tests/test_loop_analysis.py` com 10+ testes
- [ ] Teste com cenário "todas convergem rápido" vs "todas batem no limite"
- [ ] Teste com dados reais de smoke reports (se disponíveis)

---

#### Task 3.4 — Módulo: Memory & Learning Analysis

**Descrição:** Criar `analysis/memory_analysis.py` que analisa o uso
de memória episódica, hipóteses e aprendizado de restrições.

**Arquivo novo:** `apps/theo-benchmark/analysis/memory_analysis.py`

**Output schema:**
```python
{
    "episodes": {
        "total_injected": int,
        "total_created": int,
        "avg_injected_per_task": float,
        "tasks_using_memory_pct": float,  # episodes_injected > 0
    },
    "hypotheses": {
        "total_formed": int,
        "total_invalidated": int,
        "avg_active_per_task": float,
        "churn_rate": float,   # invalidated / formed
        "tasks_forming_hypotheses_pct": float,
    },
    "learning": {
        "total_constraints_learned": int,
        "total_failure_fingerprints_new": int,
        "total_failure_fingerprints_recurrent": int,
        "recurrence_rate": float,  # recurrent / (new + recurrent)
    },
    "correlations": {
        "episodes_injected_vs_success": float,
        "hypotheses_formed_vs_success": float,
        "constraints_learned_vs_success": float,
    }
}
```

**Critérios de aceite:**
- [ ] churn_rate = invalidated / formed (safe div)
- [ ] recurrence_rate = recurrent / (new + recurrent) (safe div)
- [ ] Correlações point-biserial
- [ ] Funciona com zero memory usage (todos os campos 0)

**DoD:**
- [ ] `pytest tests/test_memory_analysis.py` com 8+ testes
- [ ] Teste: cenário com alto reuse de memória vs zero reuse

---

#### Task 3.5 — Módulo: Error Taxonomy Analysis

**Descrição:** Criar `analysis/error_analysis.py` que analisa a taxonomia
de erros com custo-de-falha e breakdown por categoria.

**Arquivo novo:** `apps/theo-benchmark/analysis/error_analysis.py`

**Output schema:**
```python
{
    "taxonomy": {
        "network": {"count": int, "pct": float, "cost_usd": float},
        "llm": {"count": int, "pct": float, "cost_usd": float},
        "tool": {"count": int, "pct": float, "cost_usd": float},
        "sandbox": {"count": int, "pct": float, "cost_usd": float},
        "budget": {"count": int, "pct": float, "cost_usd": float},
        "validation": {"count": int, "pct": float, "cost_usd": float},
        "failure_mode": {"count": int, "pct": float, "cost_usd": float},
        "other": {"count": int, "pct": float, "cost_usd": float},
    },
    "summary": {
        "total_errors": int,
        "total_error_cost_usd": float,
        "error_rate": float,  # tasks with errors / total tasks
        "most_expensive_category": str,
        "most_frequent_category": str,
    },
    "failure_modes": {
        "modes": dict[str, int],  # mode_name: count
        "top_5": list[tuple[str, int]],
        "tasks_with_failure_modes_pct": float,
    },
    "cost_of_failure": {
        "avg_cost_failed_task_usd": float,
        "avg_cost_passed_task_usd": float,
        "wasted_cost_usd": float,  # cost of tasks that failed
        "wasted_pct": float,        # wasted / total cost
    }
}
```

**Critérios de aceite:**
- [ ] Cost-of-failure calcula custo separado para tasks passed vs failed
- [ ] Wasted cost = sum(cost) de tasks que falharam
- [ ] Taxonomy percentages somam 100% (do total_errors)
- [ ] failure_modes preserva compatibilidade com formato atual (lista de strings)
- [ ] Funciona com zero errors

**DoD:**
- [ ] `pytest tests/test_error_analysis.py` com 10+ testes
- [ ] Teste: cenário all-pass (zero errors), cenário all-fail, cenário mixed
- [ ] Teste: wasted_cost é correto para cenários conhecidos

---

#### Task 3.6 — Módulo: Cost Efficiency Analysis

**Descrição:** Criar `analysis/cost_analysis.py` que analisa eficiência
de custo: cost/pass, cost/iteration, token efficiency.

**Arquivo novo:** `apps/theo-benchmark/analysis/cost_analysis.py`

**Output schema:**
```python
{
    "per_task": {
        "avg_cost_usd": float,
        "median_cost_usd": float,
        "p95_cost_usd": float,
        "max_cost_usd": float,
        "min_cost_usd": float,
    },
    "efficiency": {
        "cost_per_pass_usd": float,         # total_cost / passed_count
        "cost_per_iteration_usd": float,
        "tokens_per_pass": float,            # total_tokens / passed_count
        "tokens_per_iteration": float,
        "cache_hit_rate_avg": float,
        "tokens_per_successful_edit_avg": float,
    },
    "breakdown": {
        "input_tokens_pct": float,           # input / total
        "output_tokens_pct": float,
        "cache_read_tokens_pct": float,
        "reasoning_tokens_pct": float,
    },
    "wasted": {
        "failed_task_cost_usd": float,
        "failed_task_tokens": int,
        "wasted_pct_of_total_cost": float,
        "wasted_pct_of_total_tokens": float,
    },
    "marginal_cost_curve": [
        {"pass_rate_pct": float, "cumulative_cost_usd": float},
    ]
}
```

**Critérios de aceite:**
- [ ] cost_per_pass = total_cost / max(1, passed_count) (safe div)
- [ ] marginal_cost_curve: ordena tasks por cost ASC, acumula cost + recalcula pass_rate; mostra curva "custo adicional → % pass adicional"
- [ ] Token breakdown usa cache_read e reasoning quando disponíveis (v4)
- [ ] Funciona com zero passes (cost_per_pass = total_cost, wasted = 100%)

**DoD:**
- [ ] `pytest tests/test_cost_analysis.py` com 10+ testes
- [ ] Teste: marginal cost curve com 10 tasks (5 pass, 5 fail, custos variados)
- [ ] Teste: cenário com cache_hit_rate > 0 vs = 0

---

#### Task 3.7 — Módulo: Latency Analysis

**Descrição:** Criar `analysis/latency_analysis.py` que computa
distribuições de latência por componente.

**Arquivo novo:** `apps/theo-benchmark/analysis/latency_analysis.py`

**Output schema:**
```python
{
    "wall_clock": {
        "p50_ms": float,
        "p95_ms": float,
        "p99_ms": float,
        "mean_ms": float,
        "max_ms": float,
    },
    "first_action": {
        "p50_ms": float,
        "p95_ms": float,
        "mean_ms": float,
    },
    "llm_call": {
        "p50_ms": float,
        "p95_ms": float,
        "mean_ms": float,
        "max_ms": float,
        "total_calls": int,
    },
    "per_tool_latency": {
        "read": {"p50_ms": float, "p95_ms": float, "mean_ms": float},
        "edit": {"p50_ms": float, "p95_ms": float, "mean_ms": float},
        "bash": {"p50_ms": float, "p95_ms": float, "mean_ms": float},
        ...
    },
    "time_breakdown": {
        "tool_time_pct": float,     # sum(tool latency) / wall_clock
        "llm_time_pct": float,      # sum(llm latency) / wall_clock
        "overhead_time_pct": float,  # 1 - tool - llm
    },
    "correlations": {
        "duration_vs_success": float,
        "first_action_latency_vs_success": float,
        "llm_latency_vs_success": float,
    }
}
```

**Fontes de dados para LLM call latency:**
- **Primária:** OTLP spans (`span.name == "llm.call"`) via `analysis/post_run.py`
- **Fallback:** Estimativa = `(wall_clock - sum(tool_latency * call_count)) / llm_calls`
  quando OTLP não disponível (containers Docker sem OTLP collector)

**Critérios de aceite:**
- [ ] Percentis por linear interpolation (mesma impl de post_run.py)
- [ ] per_tool_latency usa avg_latency_ms de cada ToolBreakdown entry
- [ ] **LLM call latency: usa OTLP spans quando disponíveis, fallback para estimativa**
- [ ] time_breakdown é estimativa: tool_time = sum(avg_latency * call_count), overhead = wall - tool - llm
- [ ] Funciona sem dados de OTLP (usa dados do RunReport + estimativa LLM)
- [ ] Correlação `llm_latency_vs_success` usa point-biserial

**DoD:**
- [ ] `pytest tests/test_latency_analysis.py` com 10+ testes
- [ ] Teste: cenário com 3 tools de latências conhecidas
- [ ] Teste: cenário com first_action = 0 (não disponível)
- [ ] Teste: LLM latency com OTLP spans mockados
- [ ] Teste: LLM latency fallback (sem OTLP, estimativa válida)

---

#### Task 3.8 — Módulo: Subagent Analysis

**Descrição:** Criar `analysis/subagent_analysis.py` que analisa
uso e eficácia de sub-agentes.

**Arquivo novo:** `apps/theo-benchmark/analysis/subagent_analysis.py`

**Output schema:**
```python
{
    "usage": {
        "total_spawned": int,
        "total_succeeded": int,
        "total_failed": int,
        "overall_success_rate": float,
        "tasks_using_subagents_pct": float,
    },
    "performance": {
        "avg_duration_ms": float,
        "p50_duration_ms": float,
        "p95_duration_ms": float,
    },
    "correlations": {
        "subagent_use_vs_task_success": float,
        "subagent_success_rate_vs_task_success": float,
    }
}
```

**Critérios de aceite:**
- [ ] Agrega subagent_metrics de múltiplos HeadlessResults
- [ ] Funciona com zero subagent usage (todos os campos 0)

**DoD:**
- [ ] `pytest tests/test_subagent_analysis.py` com 5+ testes

---

#### Task 3.9 — Módulo: Derived Metrics (Surrogate) Analysis

**Descrição:** Criar `analysis/derived_analysis.py` que analisa as
métricas derivadas/surrogate do runtime.

**Arquivo novo:** `apps/theo-benchmark/analysis/derived_analysis.py`

**Output schema:**
```python
{
    "doom_loop": {
        "mean": float,
        "p50": float,
        "p95": float,
        "tasks_with_doom_loop_pct": float,  # frequency > 0.1
    },
    "llm_efficiency": {
        "mean": float,
        "p50": float,
        "min": float,  # worst efficiency
    },
    "context_waste": {
        "mean": float,
        "p50": float,
        "p95": float,
    },
    "hypothesis_churn": {
        "mean": float,
        "tasks_with_high_churn_pct": float,  # rate > 0.5
    },
    "time_to_first_tool": {
        "mean_ms": float,
        "p50_ms": float,
        "p95_ms": float,
    },
    "correlations": {
        "doom_loop_vs_success": float,
        "llm_efficiency_vs_success": float,
        "context_waste_vs_success": float,
    }
}
```

**Critérios de aceite:**
- [ ] Thresholds para "doom loop detected" e "high churn" documentados
- [ ] Correlações point-biserial
- [ ] Funciona com zero derived metrics (v3 legacy)

**DoD:**
- [ ] `pytest tests/test_derived_analysis.py` com 5+ testes

---

#### Task 3.10 — Módulo: Prompt Metrics Analysis

**Descrição:** Criar `analysis/prompt_analysis.py` que analisa métricas
relacionadas ao prompt: ratio system/user tokens, impacto de variantes,
e instruction adherence (via LLM-as-judge lightweight).

**Arquivo novo:** `apps/theo-benchmark/analysis/prompt_analysis.py`

**Output schema:**
```python
{
    "token_ratio": {
        "avg_input_tokens": float,
        "avg_output_tokens": float,
        "avg_input_output_ratio": float,       # input / output
        "avg_reasoning_tokens": float,
        "reasoning_pct_of_output": float,      # reasoning / output
    },
    "prompt_variants": {
        "variant_name": {
            "pass_rate": float,
            "avg_cost_usd": float,
            "avg_iterations": float,
            "n_tasks": int,
        },
    },
    "instruction_adherence": {
        "tasks_with_success_check": int,
        "tasks_where_agent_claimed_success_but_failed": int,  # agent.success=True mas check_passed=False
        "false_positive_rate": float,         # claimed_success_but_failed / total
        "tasks_where_agent_gave_up_but_passed": int,  # agent.success=False mas check_passed=True
        "false_negative_rate": float,
    },
    "efficiency": {
        "tokens_per_task_pass": float,        # total_tokens de tasks que passaram / n_passed
        "tokens_per_task_fail": float,        # total_tokens de tasks que falharam / n_failed
        "output_density": float,              # output_tokens / input_tokens (quanto o LLM "produz" vs "consome")
    }
}
```

**Nota sobre instruction_adherence:** Não usa LLM-as-judge (YAGNI).
Em vez disso, compara `agent.success` (o que o agente ACHOU que fez)
com `check_passed` (o que REALMENTE aconteceu). Essa discrepância
é o proxy mais confiável de instruction adherence sem custo extra.

**Critérios de aceite:**
- [ ] false_positive_rate detecta quando o agente acha que resolveu mas não resolveu
- [ ] false_negative_rate detecta quando o agente desistiu mas a solução estava correta
- [ ] prompt_variants populado a partir de env var `THEO_PROMPT_VARIANT` no HeadlessResult ou manifest
- [ ] Token ratio funciona com e sem reasoning_tokens (v3 = 0, v4 = valor real)
- [ ] Funciona com 0 tasks passando ou 0 tasks falhando (safe div)

**DoD:**
- [ ] `pytest tests/test_prompt_analysis.py` com 8+ testes
- [ ] Teste: 5 tasks com false_positive (agent says success, check says fail)
- [ ] Teste: variant comparison com 2 variantes, pass rates diferentes
- [ ] Teste: cenário sem reasoning_tokens (v3 legacy)

---

#### Task 3.11 — Enriquecer ToolBreakdown com categorias de erro

**Descrição:** Adicionar campo `error_categories` ao struct `ToolBreakdown`
no runtime Rust, para que o benchmark saiba não só QUANTAS vezes uma
tool falhou, mas POR QUÊ falhou.

**Parte Rust — Arquivo:** `crates/theo-agent-runtime/src/observability/report.rs`

**Mudança no struct:**
```rust
pub struct ToolBreakdown {
    pub tool_name: String,
    pub call_count: u32,
    pub success_count: u32,
    pub failure_count: u32,
    pub avg_latency_ms: f64,
    pub max_latency_ms: u64,
    pub retry_count: u32,
    pub success_rate: f64,
    // NOVO:
    pub error_categories: HashMap<String, u32>,  // ex: {"timeout": 2, "parse_error": 1, "permission_denied": 1}
}
```

**Categorias de erro (derivadas do ToolCallCompleted event):**
- `timeout` — tool excedeu timeout
- `parse_error` — output da tool não parseável
- `permission_denied` — sandbox bloqueou
- `not_found` — arquivo/recurso não encontrado
- `validation_error` — input inválido (schema violation)
- `execution_error` — erro genérico de execução
- `other` — não classificado

**Parte Python — Arquivo:** `apps/theo-benchmark/analysis/tool_analysis.py`

**Novo no output schema (dentro de per_tool):**
```python
"read": {
    ...,
    "error_categories": {"not_found": 3, "permission_denied": 1},
    "most_common_error": str,
}
```

**Critérios de aceite:**
- [ ] Rust: `ToolBreakdown.error_categories` é `HashMap<String, u32>` com `#[derive(Serialize)]`
- [ ] Rust: populado durante `compute_tool_breakdown()` a partir dos ToolCallCompleted events
- [ ] Python: `tool_breakdown[i]["error_categories"]` parseado corretamente
- [ ] Python: `most_common_error` calculado por tool
- [ ] Backward compat: se `error_categories` ausente no JSON, assume `{}`

**DoD:**
- [ ] `cargo test -p theo-agent-runtime` passa
- [ ] Teste Rust: tool com 3 failures de tipos diferentes → error_categories correto
- [ ] `pytest tests/test_tool_analysis.py` passa com novos campos
- [ ] Teste Python: parsing com e sem error_categories (v4 vs v3)

---

#### Task 3.12 — Módulo: Cost by Phase Analysis

**Descrição:** Criar `analysis/phase_cost_analysis.py` que estima
custo (tokens e USD) por fase do agent loop (planning, executing,
evaluating, etc.).

**Arquivo novo:** `apps/theo-benchmark/analysis/phase_cost_analysis.py`

**Premissa:** O `LoopMetrics.phase_distribution` tem `{phase: {iterations, duration_ms, pct}}`
mas NÃO tem tokens por fase. A estimativa usa:

```
tokens_per_phase ≈ (phase_iterations / total_iterations) × total_tokens
cost_per_phase  ≈ (tokens_per_phase / 1M) × price_per_mtok
```

Isso é uma **estimativa proporcional**, não medição exata. O caveat
deve ser documentado no report.

**Output schema:**
```python
{
    "per_phase": {
        "planning": {
            "iterations": int,
            "duration_ms": int,
            "pct_of_time": float,
            "estimated_tokens": int,        # proporcional
            "estimated_cost_usd": float,    # proporcional
            "pct_of_cost": float,
        },
        "executing": { ... },
        "evaluating": { ... },
        ...
    },
    "summary": {
        "most_expensive_phase": str,
        "most_time_consuming_phase": str,
        "planning_to_execution_ratio": float,  # planning_pct / executing_pct
    },
    "caveat": "Cost per phase is estimated proportionally from iterations. Actual token usage may vary by phase."
}
```

**Critérios de aceite:**
- [ ] Estimativa proporcional: tokens_per_phase = (phase_iters / total_iters) × total_tokens
- [ ] Soma de estimated_tokens across phases = total_tokens (conservação)
- [ ] `planning_to_execution_ratio` identifica agentes que gastam demais planejando
- [ ] Caveat obrigatório no output e no Markdown report
- [ ] Funciona com phase_distribution vazia (retorna struct com defaults + caveat "no phase data")

**DoD:**
- [ ] `pytest tests/test_phase_cost_analysis.py` com 8+ testes
- [ ] Teste: 3 fases com proportções conhecidas → tokens/cost corretos
- [ ] Teste: conservação (soma = total)
- [ ] Teste: phase_distribution vazia → graceful default

---

### Fase 4 — Report Unificado

> **Objetivo:** Um único report consolidado com TODAS as dimensões
> de análise, em formato Markdown + JSON estruturado.

#### Task 4.1 — Criar report builder unificado

**Descrição:** Criar `analysis/report_builder.py` que orquestra todos
os módulos de análise (Tasks 3.1-3.12) e gera um report consolidado.

**Arquivo novo:** `apps/theo-benchmark/analysis/report_builder.py`

**API:**
```python
def build_report(
    results: list[HeadlessResult],
    benchmark_name: str,
    manifest: dict | None = None,
) -> BenchmarkReport:
    """Executa todos os módulos de análise e consolida."""
```

**Output: BenchmarkReport dataclass:**
```python
@dataclass
class BenchmarkReport:
    benchmark: str
    timestamp: str
    manifest: dict | None
    # Core
    total_tasks: int
    passed_tasks: int
    pass_rate: float
    ci_95_lower: float
    ci_95_upper: float
    # Sections
    context_health: dict    # Task 3.1
    tool_analysis: dict     # Task 3.2
    loop_analysis: dict     # Task 3.3
    memory_analysis: dict   # Task 3.4
    error_analysis: dict    # Task 3.5
    cost_analysis: dict     # Task 3.6
    latency_analysis: dict  # Task 3.7
    subagent_analysis: dict # Task 3.8
    derived_analysis: dict  # Task 3.9
    prompt_analysis: dict   # Task 3.10
    phase_cost: dict        # Task 3.12
    # Meta
    model: str
    provider: str
    theo_version: str
```

**Critérios de aceite:**
- [ ] Chama cada módulo de análise e consolida
- [ ] Se um módulo falha, reporta erro mas não interrompe os outros
- [ ] Inclui CI 95% (Wilson) para pass_rate
- [ ] Gera JSON (`to_json()`) e Markdown (`to_markdown()`)

**DoD:**
- [ ] `pytest tests/test_report_builder.py` com 5+ testes
- [ ] Teste: build_report com 20 HeadlessResults sintéticos
- [ ] Teste: um módulo lança exceção → report completa com seção marcada como "error"

---

#### Task 4.2 — Markdown report template SOTA

**Descrição:** Implementar `to_markdown()` em BenchmarkReport com
formato publicável.

**Formato do report:**
```markdown
# Theo Code Benchmark Report — {benchmark_name}

**Date:** {timestamp}
**Model:** {model} | **Provider:** {provider} | **Theo:** {theo_version}

## Headline
| Metric | Value |
|---|---|
| Pass rate | {pass_rate}% (95% CI: [{ci_low}, {ci_high}]) |
| Total tasks | {total} |
| Total cost | ${cost} |
| Avg cost/pass | ${cost_per_pass} |
| Avg iterations | {mean_iterations} |

## Context Health
(table from context_health module)

## Tool Performance
(table from tool_analysis module — one row per tool)

## Agent Loop Behavior
(table from loop_analysis module)

## Memory & Learning
(table from memory_analysis module)

## Error Taxonomy
(table from error_analysis module)

## Cost Efficiency
(table + marginal cost curve from cost_analysis module)

## Latency Distribution
(table from latency_analysis module)

## Subagent Delegation
(table from subagent_analysis module)

## Surrogate Metrics
(table from derived_analysis module)

## Prompt Metrics
(table from prompt_analysis module — token ratio, false positive/negative rates)

## Cost by Phase
(table from phase_cost module — estimated cost per agent phase, with caveat)

## Failure Taxonomy
(ranked list from error_analysis.failure_modes)

## Reproduction
theo SHA: {sha}
Model: {model}
Command: (...)
```

**Critérios de aceite:**
- [ ] Cada seção é self-contained (pode ser lida isoladamente)
- [ ] Tabelas formatadas para Github-flavored Markdown
- [ ] CI inclui método (Wilson) para transparência
- [ ] Seções com erro marcadas como "⚠ Analysis unavailable: {error}"

**DoD:**
- [ ] Output Markdown renderiza corretamente no GitHub
- [ ] Teste visual com dados sintéticos
- [ ] Teste com seções faltando (v3 legacy)

---

### Fase 5 — Reprodutibilidade e Rigor Estatístico

> **Objetivo:** Resultados publicáveis com CI, variance, flakiness
> score, e proveniência completa.

#### Task 5.1 — Multi-run com CI e variance tracking

**Descrição:** Adicionar modo `--n-runs N` ao smoke runner e outros
benchmarks. Computa mean, std, CI 95% para TODOS os campos numéricos.

**Arquivos a modificar:**
- `runner/smoke.py` — adicionar `--n-runs` flag
- `_headless.py` — `run_headless_multi()` já existe, expandir com novos campos

**Critérios de aceite:**
- [ ] `--n-runs 3` roda cada cenário 3 vezes
- [ ] Agrega: mean, std, Wilson CI para pass_rate
- [ ] Report inclui variance e CI para cada métrica
- [ ] Flakiness score por cenário: std(pass_rate) across runs (0 = determinístico, 1 = totalmente flaky)

**DoD:**
- [ ] `pytest tests/test_multirun.py` com 5+ testes
- [ ] Teste: 3 runs com resultados idênticos → std=0, CI estreito
- [ ] Teste: 3 runs com 1 pass + 2 fail → CI correto

---

#### Task 5.2 — Proveniência completa em cada report

**Descrição:** Todo report deve incluir metadata de proveniência para
reprodutibilidade.

**Campos obrigatórios:**
```python
{
    "provenance": {
        "theo_sha": str,       # git rev-parse HEAD do theo-code
        "theo_version": str,   # de environment.theo_version
        "model": str,
        "provider": str,
        "temperature": float,
        "max_iter": int,
        "pricing_toml_sha": str,  # hash do pricing.toml usado
        "benchmark_sha": str,     # git rev-parse HEAD do benchmark
        "timestamp_utc": str,
        "hostname": str,
        "python_version": str,
    }
}
```

**Critérios de aceite:**
- [ ] Proveniência coletada automaticamente (sem input manual)
- [ ] theo_sha via `git rev-parse HEAD` no repo theo-code
- [ ] pricing_toml_sha via hash do arquivo
- [ ] Incluída em smoke report, swe report, tbench report, ab report

**DoD:**
- [ ] Todos os runners emitem proveniência
- [ ] Teste: proveniência tem todos os campos preenchidos (nenhum None)

---

#### Task 5.3 — Flakiness detection e scoring

**Descrição:** Após multi-run, computar flakiness score por task e
gerar alerta para tasks instáveis.

**Arquivo novo:** `analysis/flakiness.py`

**Output schema:**
```python
{
    "per_task": {
        "task_id": {
            "pass_rate": float,
            "n_runs": int,
            "flakiness_score": float,  # 0.0 = determinístico, 1.0 = coin flip
            "is_flaky": bool,          # flakiness_score > 0.2
        }
    },
    "summary": {
        "total_tasks": int,
        "flaky_tasks": int,
        "flaky_pct": float,
        "most_flaky": list[tuple[str, float]],  # top-5
        "deterministic_tasks_pct": float,
    }
}
```

**Critérios de aceite:**
- [ ] flakiness_score = 4 * pass_rate * (1 - pass_rate) — máximo em 50% pass rate
- [ ] is_flaky quando score > 0.2 (equivale a pass_rate entre 6% e 94%)
- [ ] Requer n_runs >= 3 para computar (caso contrário flakiness = None)

**DoD:**
- [ ] `pytest tests/test_flakiness.py` com 5+ testes
- [ ] Teste: task com 3/3 passes → score=0, not flaky
- [ ] Teste: task com 1/3 passes → score=0.89, flaky
- [ ] Teste: task com 2/3 passes → score=0.89, flaky

---

### Fase 6 — Integração com Runners Existentes

> **Objetivo:** Todos os runners (smoke, swe, tbench, ab) usam o
> report builder unificado e emitem reports SOTA.

#### Task 6.1 — Integrar report builder no smoke runner

**Descrição:** Atualizar `runner/smoke.py` para usar `report_builder.build_report()`
em vez de agregação manual.

**Arquivo:** `runner/smoke.py`

**Critérios de aceite:**
- [ ] Smoke report usa BenchmarkReport
- [ ] Schema bumpa para `theo.smoke.v3`
- [ ] JSON report inclui todas as seções de análise
- [ ] Markdown report gerado automaticamente
- [ ] Backward compat: v2 readers ignoram novos campos

**DoD:**
- [ ] `pytest tests/test_smoke.py` passa (existentes não quebram)
- [ ] Smoke run real produz report com seções de context, tools, loop, etc.

---

#### Task 6.2 — Integrar report builder no SWE-bench post-processor

**Descrição:** Atualizar `analysis/swe_post.py` para usar report builder.

**Arquivo:** `analysis/swe_post.py`

**Critérios de aceite:**
- [ ] SWE report inclui todas as seções de análise
- [ ] Per-instance records mantêm campos existentes + adicionam novos
- [ ] Summary inclui BenchmarkReport completo

**DoD:**
- [ ] `pytest tests/test_swe_harness.py` passa
- [ ] Teste com dados sintéticos de SWE run

---

#### Task 6.3 — Integrar report builder no Terminal-Bench post-processor

**Descrição:** Atualizar `analysis/tbench_post.py` para usar report builder.

**Arquivo:** `analysis/tbench_post.py`

**Critérios de aceite:**
- [ ] TBench report inclui todas as seções de análise
- [ ] Per-task records mantêm campos existentes + adicionam novos

**DoD:**
- [ ] `pytest tests/test_tbench_post.py` passa (se existir)
- [ ] Teste com dados sintéticos

---

#### Task 6.4 — Enriquecer A/B comparison com novos campos

**Descrição:** Atualizar `runner/ab_compare.py` para comparar métricas
além de pass_rate e cost: context_waste, convergence_rate,
tool_success_rate per tool, etc.

**Arquivo:** `runner/ab_compare.py`

**Novos campos no paired comparison:**
```python
# Bootstrap CI para:
- context_waste_ratio (diff)
- convergence_rate (diff)
- doom_loop_frequency (diff)
- cache_hit_rate (diff)
- time_to_first_tool_ms (diff)
```

**Critérios de aceite:**
- [ ] Bootstrap CI para cada nova métrica
- [ ] Comparison markdown inclui seção "Quality Metrics Comparison"
- [ ] Funciona quando uma variante é v3 (sem novos campos)

**DoD:**
- [ ] `pytest tests/test_ab_compare.py` passa
- [ ] Teste com 2 variantes, 10 tasks cada, métricas diferentes

---

#### Task 6.5 — Atualizar aggregate.py para report builder

**Descrição:** Atualizar `analysis/aggregate.py` para usar BenchmarkReport
de cada benchmark e gerar cross-benchmark comparison SOTA.

**Arquivo:** `analysis/aggregate.py`

**Critérios de aceite:**
- [ ] Cross-benchmark report inclui comparação de TODAS as dimensões
- [ ] Tabela por benchmark inclui: pass_rate, cost, latency p50/p95, tool_success, context_health avg
- [ ] Grand total com soma de custos e média de métricas de qualidade

**DoD:**
- [ ] `pytest tests/test_aggregate.py` passa (se existir)
- [ ] Teste com 2 benchmarks de tamanhos diferentes

---

### Fase 7 — Dashboard e Visualização (Opcional/Stretch)

> **Objetivo:** Visualização das métricas para análise rápida.

#### Task 7.1 — JSON export para Grafana/Prometheus

**Descrição:** Criar `analysis/metrics_export.py` que exporta métricas
em formato compatível com Prometheus pushgateway ou Grafana JSON datasource.

**Arquivo novo:** `analysis/metrics_export.py`

**Critérios de aceite:**
- [ ] Exporta métricas em formato Prometheus text exposition
- [ ] Labels: benchmark, model, provider, task_id
- [ ] Gauges: pass_rate, cost_usd, iterations, latency_p50, context_size
- [ ] Histograms: duration_ms, tool_latency_ms

**DoD:**
- [ ] Teste: output parseável por Prometheus client library
- [ ] Documentação de como conectar ao Grafana

---

## Diagrama de Dependências

```
Fase 1: Rust JSON enrichment
  Task 1.2 (Serialize) ──→ Task 1.1 (embed RunReport)
  Task 3.11 (ToolBreakdown error_categories) ─┘  ← Rust change, parallel com 1.1
                              ↓
Fase 2: Python extraction
  Task 2.1 (HeadlessResult) → Task 2.2 (AggregatedResult)
                              ↓
Fase 3: Analysis modules (parallelizáveis)
  Task 3.1  (Context)    ─┐
  Task 3.2  (Tools)      ─┤
  Task 3.3  (Loop)       ─┤
  Task 3.4  (Memory)     ─┤
  Task 3.5  (Errors)     ─┤→ Fase 4: Report
  Task 3.6  (Cost)       ─┤
  Task 3.7  (Latency)    ─┤
  Task 3.8  (Subagent)   ─┤
  Task 3.9  (Derived)    ─┤
  Task 3.10 (Prompt)     ─┤
  Task 3.12 (Phase Cost) ─┘
                              ↓
Fase 4: Report
  Task 4.1 (builder) → Task 4.2 (Markdown)
                              ↓
Fase 5: Reprodutibilidade (parallelizável com Fase 4)
  Task 5.1 (Multi-run) ─┐
  Task 5.2 (Provenance) ┤→ independentes
  Task 5.3 (Flakiness)  ┘
                              ↓
Fase 6: Integração com runners
  Task 6.1 (Smoke)     ─┐
  Task 6.2 (SWE)       ─┤
  Task 6.3 (TBench)    ─┤→ dependem de Fase 4 + 5
  Task 6.4 (AB compare) ┤
  Task 6.5 (Aggregate)  ┘
                              ↓
Fase 7: Dashboard (stretch)
  Task 7.1 (Prometheus export)
```

## Estimativa de Esforço

| Fase | Tasks | Complexidade | Nota |
|---|---|---|---|
| 1 — Rust JSON | 2 + 1 (3.11) | Média | Requer cargo build, cuidado com Serialize |
| 2 — Python extraction | 2 | Baixa-Média | Parsing seguro, muitos campos |
| 3 — Analysis modules | 12 | Média | Parallelizáveis, TDD puro |
| 4 — Report | 2 | Média | Consolidação + Markdown |
| 5 — Reprodutibilidade | 3 | Baixa | Estatísticas conhecidas |
| 6 — Integração | 5 | Média | Toca runners existentes |
| 7 — Dashboard | 1 | Baixa | Stretch goal |
| **Total** | **28** | | |

## Matriz de Cobertura: Gaps Originais → Tasks

| # | Gap Original | Task(s) | Status |
|---|---|---|---|
| **CONTEXT** | | | |
| 1.1 | Context window utilization | 3.1 + 3.3 | ✅ `context_avg_size` + `budget_utilization_tokens_pct` |
| 1.2 | Context growth rate | 3.1 | ✅ `avg_growth_rate`, p50/p95 |
| 1.3 | Compaction count | 3.1 | ✅ `total_compactions`, savings ratio |
| 1.4 | Refetch rate | 3.1 | ✅ `avg_refetch_rate` |
| 1.5 | Context relevance | 3.1 + 3.9 | ✅ `usefulness_avg` + `context_waste_ratio` |
| **MEMORY** | | | |
| 2.1 | Episodes injected | 3.4 | ✅ `total_injected`, per-task avg |
| 2.2 | Hypotheses formed | 3.4 | ✅ `total_formed`, churn_rate |
| 2.3 | Constraints learned | 3.4 | ✅ `total_constraints_learned` |
| 2.4 | Failure fingerprints | 3.4 | ✅ `new` vs `recurrent`, `recurrence_rate` |
| **TOOLS** | | | |
| 3.1 | Per-tool breakdown | 3.2 | ✅ latency, success, call_count per tool |
| 3.2 | Tool error taxonomy | **3.11** | ✅ `error_categories` per tool (Rust + Python) |
| 3.3 | Tokens per successful edit | 3.6 | ✅ `tokens_per_successful_edit_avg` |
| **PROMPT** | | | |
| 4.1 | System/user token ratio | **3.10** | ✅ `input_output_ratio`, `reasoning_pct` |
| 4.2 | Instruction adherence | **3.10** | ✅ `false_positive_rate`, `false_negative_rate` (proxy) |
| 4.3 | A/B variante impact | 6.4 + **3.10** | ✅ `prompt_variants` + A/B compare enrichment |
| **AGENT LOOP** | | | |
| 5.1 | State transitions | 3.3 | ✅ `phase_distribution` |
| 5.2 | Phase distribution | 3.3 | ✅ `planning_avg_pct`, `executing_avg_pct` |
| 5.3 | Convergence pattern | 3.3 | ✅ `avg_convergence_rate`, `avg_iterations_to_converge` |
| 5.4 | Budget utilization | 3.3 | ✅ `iterations_pct`, `tokens_pct`, `time_pct` |
| 5.5 | Max-iter binding rate | 3.3 | ✅ `tasks_hitting_iter_limit_pct` |
| **LATENCY** | | | |
| 6.1 | First-action latency | 3.7 | ✅ `first_action.p50/p95` |
| 6.2 | Tool dispatch p50/p95 | 3.7 | ✅ `per_tool_latency` |
| 6.3 | LLM call p50/p95 | **3.7** | ✅ `llm_call.p50/p95` (OTLP + fallback) |
| 6.4 | Per-tool latency dist | 3.7 | ✅ `per_tool_latency.{tool}.p50/p95` |
| **COST EFFICIENCY** | | | |
| 7.1 | Cost per pass | 3.6 | ✅ `cost_per_pass_usd` |
| 7.2 | Cost per % improvement | 3.6 | ✅ `marginal_cost_curve` |
| 7.3 | Wasted tokens | 3.6 | ✅ `wasted_pct_of_total_tokens` |
| 7.4 | Cost by phase | **3.12** | ✅ `per_phase.{phase}.estimated_cost_usd` (proporcional) |
| **REPRODUTIBILIDADE** | | | |
| 8.1 | CI 95% Wilson | 5.1 | ✅ multi-run + Wilson CI |
| 8.2 | Variance / flakiness | 5.3 | ✅ `flakiness_score` per task |
| 8.3 | Model pinning + SHA | 5.2 | ✅ provenance completa |
| 8.4 | Determinism validation | 5.1 + 5.3 | ✅ variance=0 → determinístico |

**Cobertura: 28/28 gaps cobertos (100%)**

## Critérios de Aceite Globais (Definition of Done do Plano)

- [ ] Taxa de extração: ≥ 90% dos campos do RunReport chegam ao report final
- [ ] Cobertura de testes: cada módulo de análise tem ≥ 5 testes unitários
- [ ] Backward compat: parsers v3 não quebram com JSON v4
- [ ] Todos os runners existentes geram reports no formato novo
- [ ] Report Markdown renderiza corretamente no GitHub
- [ ] Proveniência completa em cada report
- [ ] CI 95% em todas as métricas de taxa (pass_rate, success_rate, etc.)
- [ ] Zero dependências novas no pyproject.toml (tudo com stdlib)
- [ ] Todos os 28 gaps da matriz de cobertura resolvidos
