# 05 — Validation Pipeline

Pipeline deterministico que decide ALLOW/DENY em < 50ms, sem LLM. Chain of Responsibility com fail-fast.

**Depende de**: [03-decision-control-plane.md](03-decision-control-plane.md) (DecisionStore, DecisionScope), [04-policy-engine.md](04-policy-engine.md) (Policy trait)

---

## Estrutura

```rust
struct ValidationPipeline {
    policies: Vec<Box<dyn Policy>>,
    decision_store: Arc<DecisionStore>,
}

#[derive(Debug)]
struct ValidationResult {
    verdict: Verdict,             // Allow, Deny
    reasons: Vec<DenyReason>,     // Por que negou
    reused_decision: Option<String>, // Se reutilizou decisao passada
    eval_ms: u64,                 // Tempo de avaliacao
}

enum Verdict { Allow, Deny }

struct DenyReason {
    code: &'static str,           // "SCOPE_MISMATCH", "DECISION_EXPIRED", etc
    message: String,
}
```

---

## Pipeline — Ordem canonica (fail-fast)

```rust
impl ValidationPipeline {
    fn validate(&self, request: &ValidateRequest) -> ValidationResult {
        let start = Instant::now();

        // 1. Check scope: agent pode acessar estes arquivos?
        if !request.scope.is_subset_of(&request.agent_scope) {
            return deny("SCOPE_MISMATCH", "Agent nao tem acesso a estes arquivos");
        }

        // 2. Decision reuse: ja resolvemos isso antes?
        if let Some(reusable) = self.decision_store.find_reusable(
            &request.decision_type, &request.scope, request.now
        ) {
            return allow_with_reuse(reusable);
        }

        // 3. Time validity: dentro da janela?
        if request.now > request.deadline {
            return deny("SESSION_EXPIRED", "Sessao expirou");
        }

        // 4. Circuit breaker: muitas tentativas?
        if request.attempt_count > request.max_attempts {
            return deny("MAX_ATTEMPTS", "Excedeu tentativas maximas");
        }

        // 5. Policy evaluation: regras configuraveis
        for policy in &self.policies {
            if let Some(reason) = policy.evaluate(request) {
                return deny(&reason.code, &reason.message);
            }
        }

        // 6. Default: ALLOW (se passou todos os checks)
        ValidationResult {
            verdict: Verdict::Allow,
            reasons: vec![],
            reused_decision: None,
            eval_ms: start.elapsed().as_millis() as u64,
        }
    }
}
```

**Ordem importa**: os checks mais baratos e mais provavies de negar vem primeiro (fail-fast).

---

## Reason Codes Padronizados

| Codigo | Significado |
|---|---|
| `SCOPE_MISMATCH` | Agent fora do escopo permitido |
| `SESSION_EXPIRED` | Tempo expirou |
| `MAX_ATTEMPTS` | Circuit breaker ativado |
| `DECISION_NOT_ACTIVE` | Decisao nao esta em estado valido |
| `POLICY_DENY` | Policy rule negou |
| `PROMISE_UNMET` | Promise gate falhou (ex: git diff vazio) |
| `AUTHORITY_MISSING` | Principal nao tem autoridade |
| `TESTS_NOT_PASSING` | Testes nao passaram (policy built-in) |
| `EDIT_TOO_LARGE` | Edit excede limite de linhas (policy built-in) |
| `NO_TEST_EDITS` | Tentativa de editar arquivo de teste (policy built-in) |

---

## Benchmark target

- **P99 < 50ms** — sem LLM, sem I/O de rede
- Apenas acesso a memoria (DecisionStore, policies compiladas)

---

## Testes esperados

| Tipo | Quantidade | O que testa |
|---|---|---|
| Unit (validation pipeline) | ~10 | Cada step, fail-fast order, reason codes, benchmark < 50ms |
