# 09 — Promise System

O Promise System garante que o agent so pode declarar `done()` quando existe **prova concreta** de resultado. Fail-closed: sem proof, nao completa.

**Depende de**: [03-decision-control-plane.md](03-decision-control-plane.md) (DecisionStatus)

---

## Estrutura

```
crates/agent/src/promise/
  mod.rs                   # Promise trait + PromiseGate
  git_diff.rs              # GitDiffPromise
  decision_active.rs       # DecisionActivePromise
  combinators.rs           # AllOf, AnyOf
```

---

## Promise trait

```rust
trait Promise: Send + Sync {
    fn name(&self) -> &str;

    /// Verifica se a promise esta cumprida.
    /// Retorna Ok(()) se cumprida, Err(reason) se nao.
    fn check(&self, state: &AgentState, repo: &Path) -> Result<(), String>;
}
```

---

## PromiseGate

Agrega multiplas promises e decide se `done()` e permitido:

```rust
struct PromiseGate {
    promises: Vec<Box<dyn Promise>>,
}

impl PromiseGate {
    /// Verifica todas as promises.
    /// Retorna Ok se TODAS passam, Err com lista de falhas caso contrario.
    fn check_all(&self, state: &AgentState, repo: &Path) -> Result<(), Vec<String>>;
}
```

---

## Promises built-in

### GitDiffPromise

> "done() so e permitido se existe git diff nao-vazio no repo."

Verifica que o agent realmente modificou arquivos. Previne que o agent declare sucesso sem ter feito nada.

```rust
struct GitDiffPromise;

impl Promise for GitDiffPromise {
    fn check(&self, _state: &AgentState, repo: &Path) -> Result<(), String> {
        // git diff --stat no repo
        // Se vazio → Err("Nenhuma modificacao detectada")
        // Se tem diff → Ok(())
    }
}
```

### DecisionActivePromise

> "done() so e permitido se existe pelo menos uma Decision com status ACTIVE ou COMPLETED."

Previne que o agent pule direto para done() sem executar nenhuma acao governada.

```rust
struct DecisionActivePromise {
    decision_store: Arc<DecisionStore>,
}

impl Promise for DecisionActivePromise {
    fn check(&self, state: &AgentState, _repo: &Path) -> Result<(), String> {
        // Verifica se existe Decision com status Completed no store
        // Se nao → Err("Nenhuma decisao ativa/completada")
    }
}
```

---

## Combinators — AllOf, AnyOf

```rust
struct AllOf { promises: Vec<Box<dyn Promise>> }
struct AnyOf { promises: Vec<Box<dyn Promise>> }

impl Promise for AllOf {
    fn check(&self, state: &AgentState, repo: &Path) -> Result<(), String> {
        // TODAS devem passar
    }
}

impl Promise for AnyOf {
    fn check(&self, state: &AgentState, repo: &Path) -> Result<(), String> {
        // PELO MENOS UMA deve passar
    }
}
```

Uso tipico: `PromiseGate` usa `AllOf(GitDiffPromise, DecisionActivePromise)` por padrao.

---

## Integracao com Validation Pipeline

Quando o agent chama `done()`:
1. DoneTool propoe `Decision::Done`
2. ValidationPipeline roda todos os steps normais (scope, time, policies)
3. **Adicionalmente**, PromiseGate.check_all() e chamado
4. Se promise falha → `PROMISE_UNMET` → DENY → loop continua

---

## Testes esperados

| Tipo | Quantidade | O que testa |
|---|---|---|
| Unit (promise) | ~10 | GitDiff com/sem diff, DecisionActive com/sem decisao, combinators AllOf/AnyOf |
