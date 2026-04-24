# Plano: Headless Error Classification + Rate-Limit Retry Hardening

> **Versão 1.0** — fecha duas dores P0/P1 detectadas durante o smoke A/B do
> `prompt-ab-testing-plan.md`: (1) o schema headless atual confunde "agent
> falhou" com "infra falhou" sob o mesmo `success=false`; (2) `LlmError::
> RateLimited { retry_after: None }` derruba trials inteiros com `iter=1,
> calls=0, retries=0` quando o provider devolve 429 sem header
> `Retry-After`. Ambos são bugs de produção, não só do bench.

## Contexto

### Evidências dos dados reais

`smoke3` (commit `65d0b69`, droplet 67.205.174.95, 2026-04-24):
- Variante `sota`: 3 trials reais (10/10/15 iterações, 9/15/19 tools, 81-150k
  tokens) — 0/3 passados (mazes são hard)
- Variantes `sota-lean` e `sota-no-bench`: 6 trials TODOS reportaram
  `iter=1, llm.calls=0, llm.retries=0, tokens.input=0, tokens.output=0`
  com `summary: "LLM error: rate limited (retry after Nones)"`

A variante anterior queimou 314k tokens em 5 min, exauriu o TPM do OAuth
Codex (~150k TPM), e os 6 trials seguintes nunca conseguiram fazer nem a
primeira chamada. O retry loop existente (`RetryPolicy::benchmark()` com
5 tentativas, 10s→120s) também falhou — porque o Codex devolveu 429 em
TODAS as 5 tentativas dentro da janela exponencial.

### Por que isso é crítico

**P1 — Schema confunde error vs failure**

O `comparison.md` da Phase 56 vai computar McNemar tratando esses 6 trials
zerados como "agente fracassou", quando na verdade **a infra falhou**. O
A/B inteiro fica enviesado contra qualquer variante que rode depois de
uma variante "cara".

Não é só bench. Em produção, qualquer dashboard que monitore "% de runs
bem-sucedidos" também conta rate-limit como falha do agent. Engineers vão
caçar bug no agent quando o problema é a OpenAI/Anthropic.

**P0 — Retry exponencial é a estratégia errada para 429 sem header**

`RetryPolicy::benchmark()` faz `10, 20, 40, 80, 120` segundos. Total
até desistir: 270s. Em janelas TPM de 60s renovadas a cada minuto, esse
pacing pode pegar 5/5 dentro da MESMA janela apertada. Estratégia melhor
quando provider não diz quando voltar: backoff de duração FIXA progressiva
(60s → 120s → 300s) que cobre múltiplas janelas TPM.

Plus: o counter `retries` no `AgentResult` está 0 mesmo quando retries
ocorrem — instrumentação quebrada que escondeu o problema até olharmos
métrica de tokens=0.

### Por que NÃO incluir agora

- **Auto-refresh de OAuth token** (item P2 da revisão): independente, mais
  arquitetural, merece plano dedicado quando token estiver perto de
  expirar (~7 dias atualmente)
- **Sandbox cascade observability** (P3): trivial, vira PR avulso
- **CI para Python bench** (P2): vira issue + PR avulso

## Decisões de arquitetura

### D1: `ErrorClass` é enum tipado, não string

`enum ErrorClass { Solved, Exhausted, RateLimited, AuthFailed, ContextOverflow, SandboxDenied, Cancelled, Aborted, InvalidTask }` em `theo-domain`. Serializa como snake_case. Strings dão drift, enum trava o contrato.

### D2: Mantém `success: bool` por backcompat

Nada removido do schema v2. Apenas ADICIONA `error_class` e bump para `theo.headless.v3`. Consumers v2 ignoram o novo campo. Schema v3 SÓ é declarado quando `error_class` está presente — caminho `emit_headless_error` no main.rs (pre-flight) continua emitindo v1 minimal.

### D3: `success=true` ⇔ `error_class == Some(Solved)`

Invariante: nunca temos `success=true` com `error_class != Solved`, nunca temos `success=false` com `error_class == Solved`. Test propriedade-based valida.

### D4: Rate-limit retry usa política dedicada quando `retry_after: None`

Não muda `RetryPolicy::benchmark()` (que está OK quando provider devolve `Retry-After`). Adiciona `RetryPolicy::rate_limit_no_hint()` com sleeps fixos `[60, 120, 300]` segundos. O `run_engine.rs` escolhe qual política usar baseado no tipo do erro:
- `RateLimited { retry_after: Some(s) }` → respeita o header, fim
- `RateLimited { retry_after: None }` → usa a política dedicada
- Outros erros retryable (Network, Timeout, ServiceUnavailable) → política original

### D5: Retry counter REALMENTE incrementa

Hoje `metrics.record_llm_retry()` existe mas não é chamado no loop em `run_engine.rs`. Fix: adicionar a chamada no branch `Err(ref e) if e.is_retryable()`. Test: smoke unitário valida que após 3 retries, `metrics.snapshot().total_retries == 3`.

### D6: ab_compare.py trata `RateLimited` como missing, não como fail

Quando `error_class == "rate_limited"`, a tabela paireada exclui aquela task daquela variante (NaN, não 0). McNemar usa só pares onde AMBAS variantes têm resultado real. Reporta no comparison.md: "X tasks excluídas por rate-limit".

## Variantes de erro mapeadas

| ErrorClass | Quando | Heurística |
|---|---|---|
| `Solved` | done tool + cargo test gate verde | success=true ∧ done call OK |
| `Exhausted` | iterações ou tokens esgotados sem done | budget hit |
| `RateLimited` | LLM error 429 (com ou sem Retry-After) que esgotou retries | LlmError::RateLimited |
| `AuthFailed` | LLM error 401/403 | LlmError::AuthFailed |
| `ContextOverflow` | LLM context too long | LlmError::ContextOverflow |
| `SandboxDenied` | tool error: bash/file ops bloqueados pelo sandbox cascade | sandbox error variant |
| `Cancelled` | parent ou Ctrl+C | result.cancelled == true |
| `Aborted` | erro irrecuperável (RunState::Aborted sem outra classe) | catch-all de aborto |
| `InvalidTask` | task spec não parseável (vazia, malformada) | early validation fail |

## Fases

### Fase 58 — `ErrorClass` enum em theo-domain

**Objetivo:** tipo canônico que vive em `theo-domain` (zero deps), com
serde + Display.

**Arquitetura:**
```rust
// crates/theo-domain/src/error_class.rs (NEW)
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ErrorClass {
    Solved,
    Exhausted,
    RateLimited,
    AuthFailed,
    ContextOverflow,
    SandboxDenied,
    Cancelled,
    Aborted,
    InvalidTask,
}

impl ErrorClass {
    pub fn is_terminal(&self) -> bool {
        !matches!(self, Self::Solved)
    }
    pub fn is_infra(&self) -> bool {
        matches!(self, Self::RateLimited | Self::AuthFailed | Self::ServiceUnavailable | Self::ContextOverflow)
    }
}
```

**TDD Sequence:**
```
RED:
  error_class_solved_is_not_terminal
  error_class_serializes_as_snake_case
  error_class_rate_limited_is_infra
  error_class_aborted_is_terminal_but_not_infra
  error_class_round_trips_through_serde

GREEN:
  - Create error_class.rs
  - Add to theo-domain lib.rs pub mod + re-export
```

**Verify:**
```bash
cargo test -p theo-domain error_class::
```

---

### Fase 59 — `AgentResult.error_class` field + populate sites

**Objetivo:** `AgentResult` ganha `error_class: Option<ErrorClass>`. Todos
os return sites em `run_engine.rs` populam adequadamente.

**Arquitetura:**
```rust
// crates/theo-agent-runtime/src/agent_loop.rs
pub struct AgentResult {
    pub success: bool,
    pub summary: String,
    // ... existing fields ...
    /// Phase 58 (headless-error-classification): typed reason for the
    /// outcome. `None` only on legacy paths that haven't been migrated.
    pub error_class: Option<ErrorClass>,
}
```

**Mapeamento dos return sites em run_engine.rs:**

| Site | Condição | error_class |
|---|---|---|
| done tool + gate OK | success=true | `Solved` |
| budget exceeded | success=false (bug #1 fix) | `Exhausted` |
| max iterations hit | success=false | `Exhausted` |
| LLM error retryable, exhausted retries | success=false | `RateLimited` (se 429) ou `Aborted` |
| LLM error AuthFailed | success=false | `AuthFailed` |
| LLM error ContextOverflow | success=false | `ContextOverflow` |
| Cancellation token | success=false, cancelled=true | `Cancelled` |
| Tool error: sandbox denied | propaga, agent decide | (não direto, vira Exhausted) |

**TDD Sequence:**
```
RED:
  agent_result_default_has_no_error_class
  budget_exceeded_returns_exhausted
  rate_limit_exhausted_retries_returns_rate_limited
  auth_failed_returns_auth_failed
  context_overflow_returns_context_overflow
  cancellation_returns_cancelled
  done_with_gate_pass_returns_solved
  invariant_solved_iff_success_true (proptest)

GREEN:
  - Add field to AgentResult
  - Update each return site to populate it
  - Bug #1 fix already exists; just add error_class=Some(Exhausted)
```

**Verify:**
```bash
cargo test -p theo-agent-runtime error_class
cargo test -p theo-agent-runtime --lib  # full regression
```

---

### Fase 60 — Headless v3 schema emission

**Objetivo:** `apps/theo-cli/src/main.rs` emite `error_class` quando
disponível e bump schema para v3.

**Arquitetura:**
```rust
// apps/theo-cli/src/main.rs (cmd_headless final JSON)
let json = serde_json::json!({
    "schema": "theo.headless.v3",   // bump
    "success": result.success,
    "error_class": result.error_class
        .map(|e| e.to_string()),     // optional snake_case
    "summary": result.summary,
    // ... rest unchanged
});
```

**TDD Sequence:**
```
RED:
  headless_v3_includes_error_class_when_set
  headless_v3_omits_error_class_when_none (use serde skip_serializing_if)
  headless_v3_success_true_implies_error_class_solved (e2e binary smoke)

GREEN:
  - Update json! literal
  - Update SCHEMA_VERSION constant in tbench/agent.py
    (TheoAgent.SCHEMA_VERSION = "theo.headless.v3")
  - parse_result() in agent.py: read data.get("error_class")
```

**Verify:**
```bash
echo "noop" | THEO_SKIP_ONBOARDING=1 target/debug/theo --headless | jq .error_class
# expect "solved" (or null on error)
```

---

### Fase 61 — Rate-limit retry hardening

**Objetivo:** quando `RateLimited { retry_after: None }`, usar política
dedicada com sleeps fixos longos. Plus: incrementar `retries` corretamente.

**Arquitetura:**
```rust
// crates/theo-domain/src/retry_policy.rs (extend)
impl RetryPolicy {
    /// Phase 61 (headless-error-classification): provider returned 429
    /// without Retry-After header. Use a fixed long backoff that crosses
    /// multiple TPM windows (60s, 120s, 300s).
    pub fn rate_limit_no_hint() -> Self {
        Self {
            max_retries: 3,
            base_delay_ms: 60_000,   // 60s
            max_delay_ms: 300_000,   // 5min cap
            jitter: false,           // deterministic on the slow path
        }
    }
}
```

```rust
// crates/theo-agent-runtime/src/run_engine.rs (modify retry loop)
Err(ref e) if e.is_retryable() && attempt < max_retries => {
    // Phase 61: pick policy based on error type
    let effective_policy = match e {
        LlmError::RateLimited { retry_after: None } => {
            theo_domain::retry_policy::RetryPolicy::rate_limit_no_hint()
        }
        LlmError::RateLimited { retry_after: Some(s) } => {
            // Respect provider hint
            tokio::time::sleep(std::time::Duration::from_secs(*s)).await;
            self.metrics.record_llm_retry();   // bug #5: count it
            continue;
        }
        _ => retry_policy.clone(),
    };
    let delay = effective_policy.delay_for_attempt(attempt);
    self.metrics.record_llm_retry();   // bug #5: count it
    self.event_bus.publish(/* same as before */);
    tokio::time::sleep(delay).await;
    continue;
}
```

**TDD Sequence:**
```
RED:
  rate_limit_no_hint_uses_60s_120s_300s
  rate_limit_no_hint_caps_at_300s_after_attempt_2
  rate_limit_no_hint_is_not_jittered (deterministic ensures matching against TPM windows)
  retry_loop_increments_retry_counter_on_each_attempt
  retry_loop_uses_no_hint_policy_for_429_without_header
  retry_loop_uses_provider_hint_when_present
  retry_loop_uses_default_policy_for_network_error

GREEN:
  - Add rate_limit_no_hint() in retry_policy.rs
  - Modify run_engine.rs retry branch
  - Add metrics.record_llm_retry() (already exists, just call it)
```

**Verify:**
```bash
cargo test -p theo-domain retry_policy::tests::rate_limit_no_hint
cargo test -p theo-agent-runtime run_engine::tests::retry_uses_correct_policy
cargo test -p theo-agent-runtime metrics::tests::retry_counter_increments
```

---

### Fase 62 — ab_compare.py treats RateLimited as missing

**Objetivo:** quando uma task tem `error_class == "rate_limited"` em uma
variante, ELA é excluída da comparação paireada para aquela variante.
Reportar no comparison.md.

**Arquitetura:**
```python
# apps/theo-benchmark/runner/ab_compare.py

def is_real_outcome(record: dict) -> bool:
    """Returns True when the record represents a true agent outcome,
    not an infra failure (rate-limit, auth, etc.)."""
    if record is None:
        return False
    ec = record.get("error_class")
    if ec in ("rate_limited", "auth_failed", "context_overflow"):
        return False
    return True

def compute_pair_stats(variant_a, variant_b, records):
    # filter to common task IDs WHERE both have real outcomes
    common = [
        t for t in (records[a].keys() & records[b].keys())
        if is_real_outcome(records[a][t]) and is_real_outcome(records[b][t])
    ]
    # ... rest unchanged
    return {
        ...,
        "n_excluded_a": count_infra_failures(records[a]),
        "n_excluded_b": count_infra_failures(records[b]),
    }
```

**TDD Sequence:**
```
RED:
  is_real_outcome_returns_false_for_rate_limited
  is_real_outcome_returns_true_for_solved_or_exhausted
  compute_pair_stats_excludes_infra_failures_from_paired_set
  comparison_md_lists_excluded_count_per_variant
  recommendation_warns_when_too_many_excluded (n_excluded > n_paired/3 → "data quality concern")

GREEN:
  - Add is_real_outcome helper
  - Update compute_pair_stats
  - Update render_comparison_md to surface exclusions
  - Update tests with synthetic rate-limited fixtures
```

**Verify:**
```bash
cd apps/theo-benchmark && python3 -m unittest tests.test_ab_compare
```

---

### Fase 63 — Validation smoke + docs

**Objetivo:** rodar smoke em droplet com prompt forçado a falhar (rate
limit simulado via curl), validar que ErrorClass aparece corretamente,
commitar.

**Steps:**
1. Build theo portable (rust:1.95-slim-bookworm — já automatizado)
2. Push para droplet
3. Smoke local: rodar headless task simples + validar JSON tem `error_class: "solved"`
4. Smoke droplet (1-2 tasks por variante para validar pipeline)
5. Documento: `docs/current/headless-schema.md` descrevendo v3 + ErrorClass
6. CHANGELOG entry
7. Commit + push

**Verify:**
```bash
# Local
target/debug/theo --headless 'echo hi' | jq .error_class
# expect: "solved"

# Droplet (after build + push)
ssh_d 'theo --headless "echo hi" | jq .error_class'
```

---

## Riscos e mitigações

| Risco | Mitigação |
|---|---|
| Bump schema v2→v3 quebra parsers existentes | `parse_result()` em `tbench/agent.py` aceita prefixo `theo.headless.` (já é o caso); `error_class` opcional via `data.get(...)` |
| `rate_limit_no_hint()` espera 60+120+300 = 8min total no pior caso | Documentar; usar só em benchmark mode (aggressive_retry=true). Default mode (`default_llm`) continua exponencial rápido. |
| Adicionar `metrics.record_llm_retry()` pode estourar contador em teste mock | Tests usam policy com `max_retries=0` quando não querem retry path; existing tests não tocam o branch |
| Proptest de invariante `solved iff success` pode ser caro | Usa só 100 cases (trivial), 1ms total |
| ab_compare.py mudar API quebra dashboards externos | Não há dashboards externos hoje; comparison.md é o único consumer |
| ErrorClass enum cresce (e.g., `ServiceUnavailable`) → `#[non_exhaustive]` exige match catch-all | Já marcado `#[non_exhaustive]`; tests validam padrão `_ =>` em consumers |

## Verificação final agregada

```bash
# Phase 58 — domain enum
cargo test -p theo-domain error_class::

# Phase 59 — runtime population
cargo test -p theo-agent-runtime error_class
cargo test -p theo-agent-runtime --lib  # full regression

# Phase 60 — headless v3
echo noop | THEO_SKIP_ONBOARDING=1 target/debug/theo --headless | jq .error_class
cd apps/theo-benchmark && python3 -m unittest tests.test_theo_agent

# Phase 61 — retry hardening
cargo test -p theo-domain retry_policy::tests::rate_limit_no_hint
cargo test -p theo-agent-runtime run_engine::tests::retry

# Phase 62 — ab_compare update
cd apps/theo-benchmark && python3 -m unittest tests.test_ab_compare

# Phase 63 — E2E smoke
target/debug/theo --headless 'echo hi' | jq '.schema, .error_class, .success'
# expect: "theo.headless.v3", "solved", true
```

## Cronograma

```
Fase 58 — error_class enum                ~30min + 5 RED tests
Fase 59 — populate AgentResult            ~1h + 8 RED tests
Fase 60 — headless v3 schema              ~30min + 3 RED tests
Fase 61 — rate-limit retry hardening      ~1h + 7 RED tests
Fase 62 — ab_compare exclusion logic      ~45min + 5 RED tests
Fase 63 — smoke + docs + commit           ~30min

Total work: ~4h
Total CI:   smoke <5min (no LLM cost — rate limit fix doesn't need real LLM)
```

## Compromisso de cobertura final

Após este plano: **erros classificados separadamente de falhas, retry de
rate-limit não desperdiça trial inteiro, A/B compare exclui infra-failures
do cômputo estatístico**.

| Item | Status pós-plano |
|---|---|
| ErrorClass tipado em theo-domain | ✓ Fase 58 |
| AgentResult populado em todos os return sites | ✓ Fase 59 |
| Headless schema v3 emite error_class | ✓ Fase 60 |
| Rate-limit sem Retry-After respeita janelas TPM | ✓ Fase 61 |
| Retry counter incrementa de fato | ✓ Fase 61 (D5) |
| ab_compare exclui rate_limited do McNemar | ✓ Fase 62 |
| Documentação do schema v3 | ✓ Fase 63 |

Plus:
- 28+ novos tests (TDD obrigatório por fase)
- Backcompat: v2 consumers continuam funcionando (campo opcional)
- Retroativamente conserta o smoke3 (re-rodar smoke produz dados utilizáveis)

## Trabalho fora deste plano

Confirmados como épicos separados, **NÃO** parte deste escopo:
- **Auto-refresh OAuth tokens** (P2 da revisão) — token atual válido até
  2026-05-02; fora do prazo desta entrega
- **CI Python para apps/theo-benchmark** (P2) — issue + PR avulso
- **Sandbox cascade telemetry** (P3) — PR avulso (~1h trabalho)
- **Pricing.toml monitoring** (P3) — não bloqueante
- **Rerun de smoke A/B com fixes** (segue após Fase 63) — virou Fase 57
  do plano `prompt-ab-testing-plan.md`, agora desbloqueada

## Referências

- `docs/plans/prompt-ab-testing-plan.md` — onde o problema apareceu
- `docs/benchmarks/2026-04-24-tbench-core-partial.md` — dados que motivaram
- `crates/theo-infra-llm/src/error.rs` — `LlmError::RateLimited` definition
- `crates/theo-agent-runtime/src/run_engine.rs:1112` — retry loop
- `crates/theo-domain/src/retry_policy.rs` — policy types
- `apps/theo-cli/src/main.rs:838` — schema v2 emission point
- TDD: RED → GREEN → REFACTOR (sem exceções)
