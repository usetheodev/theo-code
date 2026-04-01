# Plano: `theo-code-agent` — Agent Autonomo em Rust

## Context

O agent Python (theo_agent.py) provou 50% no SWE-bench Lite com Qwen3-30B via 3 mecanismos: State Machine, Promise Gate, Context Loops. Agora precisamos portar para Rust como crate de producao, integrado ao engine existente (812 testes, 7 crates).

**Problema**: O agent Python e um prototipo — subprocess calls, string parsing fragil, sem tipagem, sem session persistence. Cada chamada a `theo-code context` rebuilda o scorer (30s). Como library Rust, o scorer vive em memoria (~2s/query).

**Resultado esperado**: Crate `theo-code-agent` com agent loop async, decision control plane deterministico, context loops, integracao direta com Pipeline (zero subprocess).

---

## Principios Fundamentais

> Inspirado em "Context Graphs as the Control Plane for the Agentic Enterprise" (IndyKite, Dave Bennett)
> e "Roadmap tecnico para Context Graph como Control Plane de Decisao" (Deep Research Report)

### 1. Decouple Agent Intelligence from Agent Governance

O LLM decide O QUE fazer. O Theo Code governa SE pode fazer, COMO registrar, e QUANDO parar.

```
  LLM (Intelligence)          Theo Code (Governance / PDP)
  ─────────────────           ──────────────────────────────
  "Quero editar X"    ──►     ValidationPipeline: scope ok? policy ok? → ALLOW/DENY
  "Done!"             ──►     PromiseGate: git diff existe? decisao ACTIVE?
  "search Y"          ──►     ScopedContext: pode acessar Y nesta delegation chain?
  "Tentar de novo"    ──►     DecisionReuse: ja resolvemos algo similar? → reutiliza
```

### 2. Context Graph = Control Plane (PDP)

> "It's not just enough to have data. We need to operationalize it." — Dave Bennett

O CodeGraph NAO e apenas um indice de busca. E o **Policy Decision Point (PDP)**:

- **Provenance**: de onde veio cada informacao (git commit, arquivo, funcao)
- **Temporal Validity**: quando a informacao e valida (recency scores, co-change decay, valid_from/valid_to)
- **Decision Traces**: cada decisao materializada com lifecycle completo (PROPOSED→ACTIVE→COMPLETED)
- **Decision Reuse**: decisoes passadas reutilizaveis se contexto e compativel
- **Agent Identity**: agent como no no grafo com ownership, capabilities e trust level
- **Scoped Access**: delegation chain determina o que o agent pode ver/editar
- **Policy Enforcement**: regras deterministicas avaliadas sem LLM (P99 < 50ms)

### 3. No Standing Privilege + Fail-Closed

> "Na ausencia de autorizacao explicita e consistente, o sistema deve negar."

```
User("Fix bug X") → Agent(task="bug_fix") → SubFlow(scope=reduced)
     subject           actor                   actor_delegated

- Sem autorizacao explicita → DENY (fail-closed)
- Cada nivel REDUZ o escopo
- done() bloqueado ate promise cumprida
- LLM NUNCA no hot path de validacao (deterministico)
```

### 4. Determinismo no Hot Path

> "A decisao ALLOW/BLOCK deve ser computada sem LLM, com regras e politicas auditaveis."

O agent loop chama o LLM para raciocinar. Mas TODA validacao (pode editar? pode completar? escopo ok?) e deterministica:

```
ValidationPipeline (< 50ms, sem LLM):
  Parse → Fetch Decision → Check Status → Time Validity → Scope Match → Conditions → Policy → ALLOW/DENY
```

---

## Arquitetura

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

## Decision Control Plane

> Baseado no Deep Research Report: "Roadmap tecnico para Context Graph como Control Plane de Decisao"

### Decision Lifecycle — Maquina de Estados

```
         ┌──────────┐
         │ PROPOSED │ ← Agent quer fazer algo
         └────┬─────┘
              │ validate() → ALLOW
              v
         ┌──────────┐
         │ APPROVED │ ← Governance permitiu
         └────┬─────┘
              │ execute()
              v
         ┌──────────┐
    ┌───►│  ACTIVE  │ ← Em execucao
    │    └────┬─────┘
    │         │
    │    ┌────┴────┬──────────┬──────────┐
    │    │         │          │          │
    │    v         v          v          v
    │ ┌─────┐ ┌────────┐ ┌───────┐ ┌───────────┐
    │ │DONE │ │FAILED  │ │REVOKED│ │SUPERSEDED │
    │ └─────┘ └────┬───┘ └───────┘ └───────────┘
    │              │
    │              │ retry (nova versao)
    └──────────────┘
```

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum DecisionStatus {
    Proposed,     // Agent quer executar uma acao
    Approved,     // ValidationPipeline retornou ALLOW
    Active,       // Acao em andamento
    Completed,    // Acao finalizada com sucesso
    Failed,       // Acao falhou
    Blocked,      // PromiseGate negou done()
    Revoked,      // Rollback/undo executado
    Superseded,   // Nova versao substituiu esta
}

impl DecisionStatus {
    fn can_transition_to(&self, target: &DecisionStatus) -> bool {
        matches!((self, target),
            (Proposed, Approved) | (Proposed, Blocked) |
            (Approved, Active) |
            (Active, Completed) | (Active, Failed) | (Active, Revoked) |
            (Failed, Superseded) |  // retry cria nova versao
            (Blocked, Superseded) | // re-tentativa
        )
    }
}
```

### Decision Version — Payload imutavel

> "Mudancas devem ser registradas como eventos append-only para permitir auditoria e reconstrucao."

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DecisionVersion {
    decision_id: String,          // "decision:session_abc:task_fix"
    version: u32,                 // 1, 2, 3... (nova tentativa = nova versao)
    status: DecisionStatus,
    decision_type: DecisionType,  // Edit, Search, Verify, Done

    // Temporal Validity
    created_at: f64,
    valid_from: f64,
    valid_to: f64,                // Expira apos N segundos (session timeout)

    // Scope (ABAC)
    scope: DecisionScope,

    // Authority
    created_by: Principal,        // Quem criou (user, agent, sub-flow)
    approved_by: Option<String>,  // Quem aprovou (governance layer)

    // Payload
    payload: DecisionPayload,     // O que foi feito (edit details, search query, etc)
    payload_hash: String,         // SHA256 do payload (integridade)

    // Lineage
    parent_decision: Option<String>,  // Decisao anterior na cadeia
    supersedes: Option<String>,       // Qual versao esta substituiu
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DecisionScope {
    files: HashSet<String>,           // Arquivos afetados
    communities: HashSet<String>,     // Communities no escopo
    task_type: String,                // "bug_fix", "refactor", etc
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum DecisionPayload {
    Edit { path: String, old_text: String, new_text: String },
    Search { query: String, results_count: usize },
    Verify { tests_passed: bool, tests_output: String },
    Done { summary: String, files_changed: Vec<String> },
    Rollback { reverted_decision: String },
    PhaseTransition { from: Phase, to: Phase },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Principal {
    id: String,                       // "user:paulo" ou "agent:theo:session_abc"
    principal_type: PrincipalType,    // Human, Agent, SubFlow
}

enum PrincipalType { Human, Agent, SubFlow }
```

### Decision Reuse — "Ja resolvemos isso antes?"

> O conceito central do Deep Research Report: reutilizar decisoes passadas sem re-pensar com LLM.

```rust
struct DecisionStore {
    decisions: Vec<DecisionVersion>,  // Append-only
    index_by_type: HashMap<String, Vec<usize>>,   // type → indices
    index_by_file: HashMap<String, Vec<usize>>,   // file → indices
    index_by_scope: HashMap<String, Vec<usize>>,  // scope key → indices
}

impl DecisionStore {
    /// Busca decisoes passadas que podem ser reutilizadas no contexto atual.
    /// Retorna ALLOW + decisao reutilizada, ou DENY + razoes.
    fn find_reusable(
        &self,
        decision_type: &DecisionType,
        current_scope: &DecisionScope,
        now: f64,
    ) -> Option<&DecisionVersion> {
        self.decisions.iter()
            .filter(|d| d.status == DecisionStatus::Completed)     // So reutiliza decisoes que deram certo
            .filter(|d| d.decision_type == *decision_type)         // Mesmo tipo
            .filter(|d| d.valid_to > now)                          // Nao expirou
            .filter(|d| d.scope.is_superset_of(current_scope))    // Escopo compativel
            .max_by_key(|d| d.version)                             // Versao mais recente
    }

    /// Persiste em disco (bincode, append-only)
    fn save(&self, path: &Path) -> Result<()>;
    fn load(path: &Path) -> Result<Self>;
}
```

**Exemplo de reuse**:
```
Sessao 1: Agent editou django/db/models/sql/compiler.py (regex fix) → Completed
Sessao 2: Nova task similar sobre regex no compiler.py
  → DecisionStore::find_reusable() encontra decisao da sessao 1
  → Inject no context: "Decisao similar encontrada: [edit details]. Reutilizar abordagem."
  → Agent nao precisa re-descobrir — vai direto ao fix
```

### Validation Pipeline — Deterministico, < 50ms

> "A decisao ALLOW/BLOCK deve ser computada sem LLM."

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

impl ValidationPipeline {
    /// Pipeline deterministico em ordem canonica (fail-fast):
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

**Reason codes padronizados**:
- `SCOPE_MISMATCH` — Agent fora do escopo permitido
- `SESSION_EXPIRED` — Tempo expirou
- `MAX_ATTEMPTS` — Circuit breaker
- `DECISION_NOT_ACTIVE` — Decisao nao esta em estado valido
- `POLICY_DENY` — Policy rule negou
- `PROMISE_UNMET` — Promise gate falhou (git diff vazio)
- `AUTHORITY_MISSING` — Principal nao tem autoridade

---

## Policy Engine — Regras configuraveis sem LLM

```rust
trait Policy: Send + Sync {
    fn name(&self) -> &str;
    fn evaluate(&self, request: &ValidateRequest) -> Option<DenyReason>;
}

/// Mini-DSL para regras: deny_if expr
struct DslPolicy {
    name: String,
    applies_to: String,           // decision_type filter
    rules: Vec<CompiledRule>,     // AST pre-compilado
}

struct CompiledRule {
    expr: Expr,                   // AST da expressao
    reason_code: String,
    message: String,
}

/// AST simples para expressoes deterministicas
enum Expr {
    // Literals
    Bool(bool),
    Int(i64),
    Str(String),
    // Access
    Fact(String),                 // facts.tests_passed
    Scope(String),                // scope.env
    // Operators
    Eq(Box<Expr>, Box<Expr>),
    NotEq(Box<Expr>, Box<Expr>),
    Lt(Box<Expr>, Box<Expr>),
    Gt(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    In(Box<Expr>, Vec<Expr>),
}

impl Expr {
    /// Parse string → AST (feito uma vez, cacheado)
    fn parse(input: &str) -> Result<Expr>;

    /// Evaluate contra facts/scope (deterministico, < 1ms)
    fn evaluate(&self, facts: &HashMap<String, Value>, scope: &HashMap<String, Value>) -> bool;
}
```

**Exemplo de policies**:
```json
[
    {
        "name": "require_tests_before_done",
        "applies_to": "Done",
        "deny_if": [
            { "expr": "facts.tests_passed != true", "reason": "TESTS_NOT_PASSING" }
        ]
    },
    {
        "name": "max_edit_size",
        "applies_to": "Edit",
        "deny_if": [
            { "expr": "facts.lines_changed > 200", "reason": "EDIT_TOO_LARGE" }
        ]
    },
    {
        "name": "no_test_file_edits",
        "applies_to": "Edit",
        "deny_if": [
            { "expr": "scope.file in ['test_*.py', '*_test.py']", "reason": "NO_TEST_EDITS" }
        ]
    }
]
```

---

## Agent Loop com Decision Control Plane

```rust
impl Agent {
    async fn run_loop(&self, repo: &Path, task: &str, scope: &ScopedContext) -> Result<LoopResult> {
        let mut state = AgentState::new(scope.clone());
        let ctx_engine = ContextLoopEngine::new(self.config.max_iterations);

        for i in 1..=self.config.max_iterations {
            // ── Context Loop injection ──
            if let Some(ctx_msg) = ctx_engine.maybe_emit(&state, task) {
                self.history.push(user_msg(&ctx_msg));
            }

            // ── Phase transition ──
            if let Some(transition) = state.should_transition(&self.config.phase_config) {
                let decision = self.propose_decision(DecisionType::PhaseTransition, &state);
                let result = self.governance.validate(&decision);
                if result.verdict == Verdict::Allow {
                    state.transition_to(transition);
                    self.governance.record(decision.approve().activate().complete());
                }
            }

            // ── LLM call ──
            let response = self.llm.complete(self.build_request(&state)).await?;

            // ── Process tool calls ──
            for tool_call in response.tool_calls() {

                // 1. PROPOSE decision
                let decision = self.propose_decision_from_tool(&tool_call, &state);

                // 2. VALIDATE (deterministico, < 50ms, sem LLM)
                let validation = self.governance.validate(&decision);
                self.audit.append(AuditEntry::validation(&validation));

                match validation.verdict {
                    Verdict::Deny => {
                        // BLOCKED — inject razoes e continua loop
                        let msg = format!("BLOCKED: {}", validation.reasons_text());
                        self.history.push(user_msg(&msg));
                        self.governance.record(decision.block(&validation));
                        state.record_blocked();
                        continue;
                    }
                    Verdict::Allow => {
                        // 3. APPROVE + ACTIVATE
                        let decision = decision.approve().activate();

                        // 4. EXECUTE tool
                        let result = self.registry.execute(&tool_call, repo).await;
                        state.record_tool_call(&tool_call, &result);

                        // 5. Decision outcome
                        match &result {
                            Ok(output) => {
                                self.governance.record(decision.complete(&output));
                                self.history.push(user_msg(&output.text));

                                // Check reuse hint
                                if let Some(reuse) = validation.reused_decision {
                                    self.history.push(user_msg(
                                        &format!("HINT: Decisao similar encontrada: {}. Abordagem pode ser reutilizada.", reuse)
                                    ));
                                }
                            }
                            Err(e) => {
                                self.governance.record(decision.fail(&e.to_string()));
                                self.history.push(user_msg(&format!("ERROR: {}", e)));
                            }
                        }
                    }
                }
            }

            // ── done() handling via PromiseGate ──
            // (DoneTool propoe Decision::Done → validate inclui PromiseGate check)
        }

        state.finalize()
    }
}
```

---

## Data Flow completo

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

## Ordem de implementacao

### Fase 0: Extrair Pipeline (1 dia)
- `crates/pipeline/` com pipeline.rs + extract.rs
- Atualizar main.rs, 812 testes passando

### Fase 1: Graph Extensions (1 dia)
- NodeType::Decision, NodeType::AgentIdentity
- EdgeType::Affects, Follows, MadeBy, OwnedBy
- Testes de criacao e query

### Fase 2: Decision Types + Lifecycle (2 dias)
- DecisionStatus (8 estados) + transicoes
- DecisionVersion (payload imutavel, versionado, com hash)
- DecisionStore (append-only, indexado, persistente)
- DecisionReuse (find_reusable por type + scope + time)
- Testes: lifecycle completo, reuse match/mismatch

### Fase 3: Policy Engine (1-2 dias)
- Policy trait + DslPolicy
- Mini-DSL: parse → AST → evaluate (sem eval/LLM)
- Policies built-in (scope_match, time_validity, max_attempts)
- Testes: policy evaluation, AST parsing

### Fase 4: ValidationPipeline (1 dia)
- Pipeline deterministico: scope → reuse → time → circuit → policy → ALLOW/DENY
- Reason codes padronizados
- Benchmark: < 50ms
- Testes: cada step, fail-fast order

### Fase 5: Governance Layer (1-2 dias)
- GovernanceLayer (media todas interacoes)
- AgentIdentity + ScopedContext + delegation chain
- AuditLog append-only
- Testes: scope reduction, delegation, audit queries

### Fase 6: LLM Client (2 dias)
- LlmClient trait + OpenAiClient (reqwest)
- Hermes XML parser + MessageHistory
- Testes com wiremock

### Fase 7: Promise System (1 dia)
- Promise trait + PromiseGate
- GitDiffPromise + DecisionActivePromise
- Combinators AllOf/AnyOf

### Fase 8: Context Loop (1 dia)
- ContextLoopEngine + diagnostics
- Inclui SCOPE, DECISIONS, REUSE hints
- Testes diagnosticos

### Fase 9: Agent Tools + Inner Loop (2 dias)
- DoneTool, SearchCodeTool
- `run_loop()` completo com governance + validation + decisions
- Teste integracao com mock LLM

### Fase 10: Decomposer + Outer Loop (1 dia)
- Intent classification + templates + reuse check
- `run()` com decompose → validate → execute → correct

### Fase 11: Checkpoint + Session (1 dia)
- Undo stack (Revoked status), CircuitBreaker
- Session save/load (inclui DecisionStore + AuditLog)

### Fase 12: CLI + Benchmark (1 dia)
- `theo-code agent <repo> <task>` — roda agent
- `theo-code audit <session>` — query decision chain
- `theo-code decisions <repo>` — lista decisoes reutilizaveis
- Benchmark vs Python agent (SWE-bench, target >=50%)

**Total estimado: 15-18 dias**

---

## Estrategia de testes

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

## Verificacao

1. `cargo test --workspace` — 812 existentes + ~143 novos = ~955
2. `theo-code agent <test-repo> <task>` resolve bug simples
3. `theo-code audit <session>` mostra decision chain completa
4. `theo-code decisions <repo>` lista decisoes reutilizaveis
5. SWE-bench (10 tasks Django): manter >=50%
6. ValidationPipeline benchmark: P99 < 50ms
7. Decision reuse: sessao 2 reutiliza decisao da sessao 1, reduz iteracoes

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

---

## Conceitos adicionais (da versao revisada do Deep Research Report)

> Aprendizados da analise linha-a-linha da versao com XACML, FAANG gaps, LangGraph/CrewAI

### 1. Idempotency Records — dedupe de mutacoes

> "APIs de controle devem ser seguras sob retry." (AWS Builders' Library, IETF draft Idempotency-Key)

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

### 2. max_reuse_count — limite de reutilizacao de decisoes

```rust
struct DecisionValidity {
    valid_from: f64,
    valid_to: f64,
    max_reuse_count: Option<usize>,  // ← NOVO: limita quantas vezes pode ser reutilizada
    current_reuse_count: usize,
}
```

Exemplo: "esta decisao de edit em compiler.py pode ser reutilizada no maximo 3 vezes. Na 4a vez, exige revalidacao completa." Previne que uma decisao antiga seja reutilizada infinitamente.

### 3. Arquitetura XACML completa: PEP/PDP/PAP/PIP

```
┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐
│   PEP    │    │   PDP    │    │   PAP    │    │   PIP    │
│  Agent   │───►│Validation│◄───│ Policy   │    │ GRAPHCTX │
│  Loop    │    │ Pipeline │    │ Config   │    │ Pipeline │
│          │◄───│          │    │ (JSON)   │    │          │
│ enforces │    │ decides  │    │ manages  │    │ provides │
│ verdict  │    │ ALLOW/   │    │ policies │    │ facts/   │
│          │    │ DENY     │    │          │    │ context  │
└──────────┘    └──────────┘    └──────────┘    └──────────┘
```

Mapeamento para nosso sistema:
- **PEP** = Agent loop (intercepta tool calls, aplica verdict)
- **PDP** = ValidationPipeline (decide ALLOW/DENY deterministicamente)
- **PAP** = Policy config files (JSON com deny_if/allow_if)
- **PIP** = GRAPHCTX Pipeline (fornece facts: affected_files, communities, co-changes, tests)

### 4. Retry Policy com backoff + jitter

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

Aplicado a: LLM calls (timeout → retry com backoff), tool execution (subprocess timeout), git operations.

### 5. Iteracao Checkpoints — time-travel debugging

> "LangGraph suporta persistencia por checkpoints que habilita recuperacao e time travel."

Salvar checkpoint a CADA iteracao (nao so no final) permite:
- Replay de sessoes problematicas
- Debug de "por que o agent fez X na iteracao 7?"
- Resume de sessoes interrompidas a partir de qualquer ponto

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

    /// Time-travel: volta para iteracao N e replay
    fn restore(&self, iteration: usize) -> Option<&IterationCheckpoint>;

    /// Debug: "o que aconteceu na iteracao 7?"
    fn inspect(&self, iteration: usize) -> Option<&IterationCheckpoint>;

    /// Persistir em disco (bincode)
    fn save_to_disk(&self, path: &Path) -> Result<()>;
}
```

**Integracao com o agent loop**:
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

### 6. CLI de debug com time-travel

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

## Anti-patterns a evitar

> Do Deep Research Report + versao revisada:

- **LLM no hot path de validacao** — Determinismo e auditabilidade exigem regras, nao inferencia
- **"Memoria" do agent como fonte de autorizacao** — Bypass do PDP. Decisoes so no DecisionStore
- **Mutacao in-place de decisao** — Versioning obrigatorio. Nova tentativa = nova versao
- **Cache sem TTL** — DecisionStore tem valid_to. Decisoes expiram
- **done() sem proof** — PromiseGate + ValidationPipeline. Fail-closed SEMPRE
- **Retry sem backoff** — Retry storms causam cascading failures. Backoff + jitter obrigatorio
- **Mutacao sem idempotency** — Retry de edit_file nao pode duplicar efeito
- **Decision reuse sem limite** — max_reuse_count previne reutilizacao infinita de decisoes antigas
- **Alta cardinalidade em metricas** — Nunca usar decision_id como label. Usar decision_type
- **Checkpoints so no final** — Checkpoint por iteracao permite time-travel debugging

---

## Visao

```
ANTES (Python prototipo):
  Agent → subprocess → theo-code CLI → stdout → parse → agent
  (30s/query, sem rastreabilidade, sem governance, sem reuse)

DEPOIS (Rust production):
  Agent → Pipeline.search() → ValidationPipeline.validate() → DecisionStore.record()
  (2s/query, full traceability, deterministic governance, decision reuse)

  Arquitetura XACML:
    PEP: Agent loop (enforce)
    PDP: ValidationPipeline (decide)
    PAP: Policy config (manage)
    PIP: GRAPHCTX Pipeline (provide facts)

  O grafo contem:
    - Codigo (Files, Symbols, Tests)
    - Estrutura (Contains, Calls, Imports, CoChanges)
    - Decisoes (lifecycle completo, versionadas, reutilizaveis, com max_reuse_count)
    - Agentes (identity, trust, capabilities)
    - Policies (regras deterministicas, AST compilado)
    - Auditoria (append-only, queryable: quem fez o que, quando, por que)
    - Checkpoints (por iteracao, time-travel debugging)
    - Idempotency (dedupe de mutacoes, retries seguros)
```
