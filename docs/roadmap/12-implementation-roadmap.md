# 12 — Roadmap de Implementacao

Ordem de implementacao, estrategia de testes, criterios de verificacao, e anti-patterns a evitar.

---

## Fases de Implementacao

### Fase 0: Extrair Pipeline (1 dia)
- `crates/pipeline/` com pipeline.rs + extract.rs
- Atualizar main.rs, 812 testes passando
- **Documento**: [02-architecture.md](02-architecture.md) (Pre-requisito)

### Fase 1: Graph Extensions (1 dia)
- NodeType::Decision, NodeType::AgentIdentity
- EdgeType::Affects, Follows, MadeBy, OwnedBy
- Testes de criacao e query
- **Documento**: [02-architecture.md](02-architecture.md) (Mudancas no graph)

### Fase 2: Decision Types + Lifecycle (2 dias)
- DecisionStatus (8 estados) + transicoes
- DecisionVersion (payload imutavel, versionado, com hash)
- DecisionStore (append-only, indexado, persistente)
- DecisionReuse (find_reusable por type + scope + time)
- Testes: lifecycle completo, reuse match/mismatch
- **Documento**: [03-decision-control-plane.md](03-decision-control-plane.md)

### Fase 3: Policy Engine (1-2 dias)
- Policy trait + DslPolicy
- Mini-DSL: parse → AST → evaluate (sem eval/LLM)
- Policies built-in (scope_match, time_validity, max_attempts)
- Testes: policy evaluation, AST parsing
- **Documento**: [04-policy-engine.md](04-policy-engine.md)

### Fase 4: ValidationPipeline (1 dia)
- Pipeline deterministico: scope → reuse → time → circuit → policy → ALLOW/DENY
- Reason codes padronizados
- Benchmark: < 50ms
- Testes: cada step, fail-fast order
- **Documento**: [05-validation-pipeline.md](05-validation-pipeline.md)

### Fase 5: Governance Layer (1-2 dias)
- GovernanceLayer (media todas interacoes)
- AgentIdentity + ScopedContext + delegation chain
- AuditLog append-only
- Testes: scope reduction, delegation, audit queries
- **Documento**: [06-governance-layer.md](06-governance-layer.md)

### Fase 6: LLM Client (2 dias)
- LlmClient trait + OpenAiClient (reqwest)
- Hermes XML parser + MessageHistory
- Testes com wiremock
- **Documento**: [08-llm-client.md](08-llm-client.md)

### Fase 7: Promise System (1 dia)
- Promise trait + PromiseGate
- GitDiffPromise + DecisionActivePromise
- Combinators AllOf/AnyOf
- **Documento**: [09-promise-system.md](09-promise-system.md)

### Fase 8: Context Loop (1 dia)
- ContextLoopEngine + diagnostics
- Inclui SCOPE, DECISIONS, REUSE hints
- Testes diagnosticos
- **Documento**: [10-context-loop-and-decomposer.md](10-context-loop-and-decomposer.md)

### Fase 9: Agent Tools + Inner Loop (2 dias)
- DoneTool, SearchCodeTool
- `run_loop()` completo com governance + validation + decisions
- Teste integracao com mock LLM
- **Documento**: [07-agent-loop.md](07-agent-loop.md)

### Fase 10: Decomposer + Outer Loop (1 dia)
- Intent classification + templates + reuse check
- `run()` com decompose → validate → execute → correct
- **Documento**: [10-context-loop-and-decomposer.md](10-context-loop-and-decomposer.md)

### Fase 11: Checkpoint + Session (1 dia)
- Undo stack (Revoked status), CircuitBreaker
- Session save/load (inclui DecisionStore + AuditLog)
- Idempotency + Retry policy
- **Documento**: [11-checkpoint-and-resilience.md](11-checkpoint-and-resilience.md)

### Fase 12: CLI + Benchmark (1 dia)
- `theo-code agent <repo> <task>` — roda agent
- `theo-code audit <session>` — query decision chain
- `theo-code decisions <repo>` — lista decisoes reutilizaveis
- `theo-code debug session <id>` — time-travel debugging
- Benchmark vs Python agent (SWE-bench, target >=50%)

---

## Estrategia de Testes

| Tipo | Quantidade | O que testa |
|---|---|---|
| Unit (phase) | ~15 | Transicoes validas/invalidas |
| Unit (graph ext) | ~10 | Decision/AgentIdentity nodes, novos edges |
| Unit (decision lifecycle) | ~15 | 8 estados, transicoes, versioning |
| Unit (decision reuse) | ~10 | Match por type+scope+time, expirados, revogados |
| Unit (policy DSL) | ~15 | Parse→AST, evaluate, deny_if/allow_if |
| Unit (validation pipeline) | ~10 | Cada step, fail-fast, reason codes |
| Unit (governance) | ~15 | Scope reduction, delegation, audit |
| Unit (hermes) | ~10 | Parser XML |
| Unit (promise) | ~10 | GitDiff, DecisionActive, combinators |
| Unit (context loop) | ~10 | Diagnostics + scope/decision info |
| Unit (checkpoint) | ~10 | Rollback, circuit breaker |
| Integration (mock LLM) | ~5 | Loop com governance + decisions |
| Integration (reuse) | ~3 | Sessao 1 → sessao 2 reutiliza decisao |
| Integration (audit) | ~3 | Session → query chain completa |
| E2E | ~2 | Agent resolve bug + decision trace verificavel |
| **Total** | **~143** | |

---

## Criterios de Verificacao

1. `cargo test --workspace` — 812 existentes + ~143 novos = ~955
2. `theo-code agent <test-repo> <task>` resolve bug simples
3. `theo-code audit <session>` mostra decision chain completa
4. `theo-code decisions <repo>` lista decisoes reutilizaveis
5. SWE-bench (10 tasks Django): manter >=50%
6. ValidationPipeline benchmark: P99 < 50ms
7. Decision reuse: sessao 2 reutiliza decisao da sessao 1, reduz iteracoes

---

## Anti-patterns a Evitar

> Do Deep Research Report + versao revisada:

| Anti-pattern | Risco | Alternativa correta |
|---|---|---|
| LLM no hot path de validacao | Latencia, nao-determinismo | Regras deterministicas (AST) |
| "Memoria" do agent como fonte de autorizacao | Bypass do PDP | Decisoes so no DecisionStore |
| Mutacao in-place de decisao | Perda de historico | Versioning: nova tentativa = nova versao |
| Cache sem TTL | Decisoes stale | DecisionStore tem valid_to |
| done() sem proof | Falso sucesso | PromiseGate + fail-closed |
| Retry sem backoff | Retry storms, cascading failures | Backoff exponencial + jitter |
| Mutacao sem idempotency | Efeito duplicado em retry | IdempotencyStore |
| Decision reuse sem limite | Reuso infinito de decisao antiga | max_reuse_count |
| Alta cardinalidade em metricas | Explosao de labels | Usar decision_type, nao decision_id |
| Checkpoints so no final | Sem debug granular | Checkpoint por iteracao |
