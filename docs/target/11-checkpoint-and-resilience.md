# 11 — Checkpoint e Resiliencia

Mecanismos para undo, recovery, session persistence, retry seguro, e time-travel debugging.

**Depende de**: [03-decision-control-plane.md](03-decision-control-plane.md) (DecisionVersion, DecisionStatus::Revoked)

---

## Checkpoint — Undo stack

```
crates/agent/src/checkpoint/
  mod.rs                   # Checkpoint (undo stack)
  circuit_breaker.rs       # CircuitBreaker
```

Cada iteracao salva um checkpoint. Permite:
- Rollback de acoes (DecisionStatus → Revoked)
- Debug de sessoes problematicas
- Resume de sessoes interrompidas

### IterationCheckpoint

```rust
struct IterationCheckpoint {
    iteration: usize,
    phase: Phase,
    state: AgentState,
    messages_snapshot: Vec<ChatMessage>,  // Ate esta iteracao
    decisions_so_far: Vec<DecisionVersion>,
    scope_at_time: ScopedContext,
    timestamp: f64,
}

struct CheckpointStore {
    checkpoints: Vec<IterationCheckpoint>,  // Um por iteracao
}

impl CheckpointStore {
    fn save_iteration(&mut self, checkpoint: IterationCheckpoint);

    /// Time-travel: volta para iteracao N
    fn restore(&self, iteration: usize) -> Option<&IterationCheckpoint>;

    /// Debug: "o que aconteceu na iteracao 7?"
    fn inspect(&self, iteration: usize) -> Option<&IterationCheckpoint>;

    /// Persistir em disco (bincode)
    fn save_to_disk(&self, path: &Path) -> Result<()>;
}
```

### Integracao com agent loop

```rust
// No final de CADA iteracao:
checkpoint_store.save_iteration(IterationCheckpoint {
    iteration: i,
    phase: state.phase,
    state: state.clone(),
    messages_snapshot: history.as_messages().to_vec(),
    decisions_so_far: decision_store.all().to_vec(),
    scope_at_time: scope.clone(),
    timestamp: now(),
});
```

---

## CircuitBreaker

Previne que o agent entre em loop infinito de retries:

```rust
struct CircuitBreaker {
    max_failures: usize,
    current_failures: usize,
    state: CircuitState,
}

enum CircuitState {
    Closed,     // Normal — permite requests
    Open,       // Tripped — bloqueia tudo
    HalfOpen,   // Teste — permite um request
}
```

Quando o circuit breaker abre:
1. ValidationPipeline retorna `MAX_ATTEMPTS` → DENY
2. GovernanceLayer reduz escopo (`ScopedContext::reduce()`)
3. Agent tenta com escopo menor (sub-flow)

---

## Session — Save/Load

```
crates/agent/src/session/
  mod.rs                   # SessionSnapshot (save/load)
  state.rs                 # Estado serializavel
```

```rust
struct SessionSnapshot {
    session_id: String,
    agent_state: AgentState,
    decision_store: DecisionStore,
    audit_log: AuditLog,
    checkpoints: CheckpointStore,
    message_history: Vec<ChatMessage>,
    created_at: f64,
    last_updated: f64,
}

impl SessionSnapshot {
    fn save(&self, path: &Path) -> Result<()>;   // bincode
    fn load(path: &Path) -> Result<Self>;
}
```

---

## Idempotency Records — Dedupe de mutacoes

> "APIs de controle devem ser seguras sob retry." (AWS Builders' Library)

Se o agent tenta `edit_file` duas vezes com os mesmos args (retry por timeout), nao deve duplicar o efeito.

```rust
struct IdempotencyRecord {
    scope_key: String,           // "session:abc:task:fix"
    idempotency_key: String,     // SHA256(tool_name + args)
    first_seen_at: f64,
    response: ToolOutput,        // Resultado do primeiro request
}

struct IdempotencyStore {
    records: HashMap<(String, String), IdempotencyRecord>,
}

impl IdempotencyStore {
    /// Retorna resultado anterior se ja executou, ou None para executar pela primeira vez
    fn check_or_insert(&mut self, scope: &str, key: &str, response: ToolOutput) -> Option<&ToolOutput>;
}
```

**Integracao**: Antes de executar qualquer tool de mutacao (edit_file, create_file, run_command), o GovernanceLayer verifica o IdempotencyStore. Se ja executou → retorna resultado anterior sem re-executar.

---

## Retry Policy com backoff + jitter

> "Backoff exponencial com jitter evita retry storms e falhas em cascata." (AWS Builders' Library)

```rust
struct RetryPolicy {
    max_retries: usize,       // 3
    base_delay_ms: u64,       // 100
    max_delay_ms: u64,        // 5000
    jitter: bool,             // true — adiciona randomness para evitar thundering herd
}

impl RetryPolicy {
    fn delay_for_attempt(&self, attempt: usize) -> Duration {
        let base = self.base_delay_ms * 2u64.pow(attempt as u32);
        let capped = base.min(self.max_delay_ms);
        if self.jitter {
            Duration::from_millis(rand::thread_rng().gen_range(0..=capped))
        } else {
            Duration::from_millis(capped)
        }
    }
}
```

Aplicado a:
- LLM calls (timeout → retry com backoff)
- Tool execution (subprocess timeout)
- Git operations

---

## CLI de debug com time-travel

```bash
# Ver todas as iteracoes de uma sessao
theo-code debug session <session-id> --list

# Inspecionar iteracao especifica
theo-code debug session <session-id> --iteration 7

# Ver decisoes da sessao
theo-code debug session <session-id> --decisions

# Replay a partir de iteracao N (para debug)
theo-code debug session <session-id> --replay-from 5
```

---

## Testes esperados

| Tipo | Quantidade | O que testa |
|---|---|---|
| Unit (checkpoint) | ~10 | Save/restore, rollback, circuit breaker |
| Unit (idempotency) | ~5 | Dedupe, primeiro request vs retry |
| Unit (retry) | ~5 | Backoff calculo, jitter bounds |
| Integration (session) | ~3 | Save → load → resume, decision store preservado |
