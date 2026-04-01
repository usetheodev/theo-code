# 02 — Arquitetura

## Estrutura de Crates

```
crates/agent/src/
  lib.rs                     # Re-exports publicos
  agent.rs                   # Agent + AgentBuilder (main loop)
  config.rs                  # AgentConfig
  error.rs                   # AgentError
  phase.rs                   # Phase enum + AgentState + transicoes

  llm/
    mod.rs                   # LlmClient trait + tipos
    openai.rs                # Client OpenAI-compatible (vLLM, OpenAI, Anthropic)
    hermes.rs                # Parser fallback Hermes XML
    history.rs               # MessageHistory com compactacao

  decision/                  # ← Decision Control Plane (PDP)
    mod.rs                   # Decision types + DecisionStore
    lifecycle.rs             # State machine: PROPOSED→APPROVED→ACTIVE→COMPLETED/FAILED/REVOKED
    version.rs               # DecisionVersion — payload imutavel versionado
    reuse.rs                 # DecisionReuse — "ja resolvemos isso antes?"
    validate.rs              # ValidationPipeline — deterministico, < 50ms

  policy/                    # ← Policy Engine
    mod.rs                   # Policy trait + PolicyEngine
    dsl.rs                   # Mini-DSL: deny_if, allow_if com AST compilado
    builtin.rs               # Policies built-in (scope_match, time_validity, etc)

  governance/                # ← Orquestracao do Control Plane
    mod.rs                   # GovernanceLayer — media TODAS as interacoes
    agent_identity.rs        # AgentIdentity — agent como no no grafo
    scoped_context.rs        # ScopedContext — delegation chain
    audit.rs                 # AuditLog — trail append-only

  promise/
    mod.rs                   # Promise trait + PromiseGate
    git_diff.rs              # GitDiffPromise
    decision_active.rs       # DecisionActivePromise — decisao deve estar ACTIVE/COMPLETED
    combinators.rs           # AllOf, AnyOf

  context_loop/
    mod.rs                   # ContextLoopEngine
    diagnostics.rs           # Deteccao de problemas + prescricao

  decomposer/
    mod.rs                   # HybridDecomposer
    intent.rs                # Classificacao por keywords
    templates.rs             # Templates por intent

  checkpoint/
    mod.rs                   # Checkpoint (undo stack)
    circuit_breaker.rs       # CircuitBreaker

  session/
    mod.rs                   # SessionSnapshot (save/load)
    state.rs                 # Estado serializavel

  tools/
    mod.rs                   # Registry do agent
    done.rs                  # DoneTool (wraps promise gate + decision lifecycle)
    search_code.rs           # SearchCodeTool (wraps Pipeline + scoped context)
```

---

## Pre-requisito: Extrair Pipeline para crate

**O que**: Mover `src/pipeline.rs` e `src/extract.rs` para `crates/pipeline/`

**Por que**: Rust nao permite crate de library depender de crate binary.

**Como**:
1. Criar `crates/pipeline/Cargo.toml`
2. Mover `pipeline.rs` → `crates/pipeline/src/lib.rs`
3. Mover `extract.rs` → `crates/pipeline/src/extract.rs`
4. `src/main.rs` depende de `theo-code-pipeline`
5. 812 testes passando

---

## Integracao com crates existentes

| Componente | Crate existente | Integracao |
|---|---|---|
| Tool execution | `theo-code-tools` ToolRegistry | Reutiliza, adiciona DoneTool + SearchCodeTool |
| Tool trait | `theo-code-core` Tool | Agent tools implementam mesmo trait |
| GRAPHCTX search | `theo-code-pipeline` Pipeline | SearchCodeTool com `Arc<Mutex<Pipeline>>`, filtrado por ScopedContext |
| Impact analysis | `theo-code-governance` | Pos-edit validation |
| Graph model | `theo-code-graph` CodeGraph | Novos NodeTypes: Decision, AgentIdentity |
| Edge types | `theo-code-graph` | Novos: Affects, Follows, MadeBy, OwnedBy |

### Mudancas no `theo-code-graph`

```rust
enum NodeType {
    File, Symbol, Import, Type, Test,
    Decision,       // ← NOVO
    AgentIdentity,  // ← NOVO
}

enum EdgeType {
    Contains, Calls, Imports, Inherits, TypeDepends, Tests, CoChanges, References,
    Affects,   // Decision --AFFECTS--> File
    Follows,   // Decision --FOLLOWS--> Decision
    MadeBy,    // Decision --MADE_BY--> AgentIdentity
    OwnedBy,   // AgentIdentity --OWNED_BY--> User (string node)
}
```

---

## Data Flow Completo

```
User Task
    |
    v
GovernanceLayer::register_agent()         ← AgentIdentity no grafo
    |
    v
TaskDecomposer::decompose()
    |-- classify_intent()                 [keyword, zero LLM]
    |-- Pipeline::assemble()              [GRAPHCTX]
    |-- DecisionStore::find_reusable()    [decisao passada reutilizavel?]
    |-- template_match()                  [SubTask list]
    |
    v
ScopedContext::restrict_writable()        ← Escopo reduzido
    |
    v
Agent::run_loop()  <─── MAIN ASYNC LOOP
    |
    +──► ContextLoopEngine::maybe_emit()
    |        + SCOPE, DECISIONS, REUSE hints
    |
    +──► AgentState::should_transition()
    |        + DecisionType::PhaseTransition → validate()
    |
    +──► LlmClient::complete()
    |
    +──► Para cada ToolCall:
    |    |
    |    +── GovernanceLayer::propose_decision()
    |    |
    |    +── ValidationPipeline::validate()           ← DETERMINISTICO < 50ms
    |    |       |── Scope match
    |    |       |── Decision reuse check
    |    |       |── Time validity
    |    |       |── Circuit breaker
    |    |       |── Policy evaluation (AST)
    |    |       |── Promise gate (se done())
    |    |       └── → ALLOW ou DENY + reasons
    |    |
    |    +── if DENY: inject reasons, record blocked
    |    +── if ALLOW: execute → record outcome
    |    |
    |    +── AuditLog::append()                       ← Append-only trail
    |
    +──► On failure:
         +── CircuitBreaker → ScopedContext::reduce() → sub-flow
         +── DecisionVersion: version++ (nova tentativa, nao overwrite)
         |
         v
    AgentResult + AuditLog + DecisionChain
```

---

## Design Patterns

| Pattern | Onde | Por que |
|---|---|---|
| **State** | DecisionStatus + Phase | Lifecycle com transicoes tipadas |
| **Strategy** | LlmClient, Promise, Policy | Backends trocaveis sem rewrite |
| **Observer** | ContextLoopEngine + GovernanceLayer | Observam e reagem a acoes |
| **Command** | DecisionVersion + EditRecord | Acoes como objetos reversiveis |
| **Builder** | AgentBuilder | Config fluente |
| **Circuit Breaker** | CircuitBreaker | Previne explosao |
| **Chain of Responsibility** | ValidationPipeline | Steps fail-fast em ordem |
| **Memento** | SessionSnapshot + DecisionStore | Estado capturado para resume/reuse |
| **Mediator** | GovernanceLayer | Media agent↔tools↔graph↔policies |
| **Template Method** | decomposer/templates.rs | Templates por intent |
| **Flyweight** | CompiledRule (AST cacheado) | Parse uma vez, evaluate N vezes |
