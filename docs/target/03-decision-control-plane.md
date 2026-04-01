# 03 — Decision Control Plane

> Baseado no Deep Research Report: "Roadmap tecnico para Context Graph como Control Plane de Decisao"

O Decision Control Plane e o nucleo do sistema de governanca. Toda acao do agent e materializada como uma **Decision** com lifecycle completo, versionamento imutavel, e capacidade de reuso.

---

## Decision Lifecycle — Maquina de Estados

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

### DecisionStatus — 8 estados

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
```

### Transicoes validas

```rust
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

Qualquer transicao nao listada e invalida e deve ser rejeitada em compile-time ou runtime.

---

## DecisionVersion — Payload imutavel

> "Mudancas devem ser registradas como eventos append-only para permitir auditoria e reconstrucao."

Cada decisao e imutavel. Uma nova tentativa cria uma **nova versao**, nunca muta a anterior.

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
```

### DecisionScope — ABAC

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DecisionScope {
    files: HashSet<String>,           // Arquivos afetados
    communities: HashSet<String>,     // Communities no escopo
    task_type: String,                // "bug_fix", "refactor", etc
}
```

### DecisionPayload — O que foi feito

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
enum DecisionPayload {
    Edit { path: String, old_text: String, new_text: String },
    Search { query: String, results_count: usize },
    Verify { tests_passed: bool, tests_output: String },
    Done { summary: String, files_changed: Vec<String> },
    Rollback { reverted_decision: String },
    PhaseTransition { from: Phase, to: Phase },
}
```

### Principal — Quem agiu

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Principal {
    id: String,                       // "user:paulo" ou "agent:theo:session_abc"
    principal_type: PrincipalType,    // Human, Agent, SubFlow
}

enum PrincipalType { Human, Agent, SubFlow }
```

---

## DecisionStore — Persistencia append-only

```rust
struct DecisionStore {
    decisions: Vec<DecisionVersion>,  // Append-only
    index_by_type: HashMap<String, Vec<usize>>,   // type → indices
    index_by_file: HashMap<String, Vec<usize>>,   // file → indices
    index_by_scope: HashMap<String, Vec<usize>>,  // scope key → indices
}

impl DecisionStore {
    /// Persiste em disco (bincode, append-only)
    fn save(&self, path: &Path) -> Result<()>;
    fn load(path: &Path) -> Result<Self>;
}
```

---

## Decision Reuse — "Ja resolvemos isso antes?"

> O conceito central do Deep Research Report: reutilizar decisoes passadas sem re-pensar com LLM.

```rust
impl DecisionStore {
    /// Busca decisoes passadas que podem ser reutilizadas no contexto atual.
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
}
```

### Exemplo de reuse

```
Sessao 1: Agent editou django/db/models/sql/compiler.py (regex fix) → Completed
Sessao 2: Nova task similar sobre regex no compiler.py
  → DecisionStore::find_reusable() encontra decisao da sessao 1
  → Inject no context: "Decisao similar encontrada: [edit details]. Reutilizar abordagem."
  → Agent nao precisa re-descobrir — vai direto ao fix
```

### max_reuse_count — Limite de reutilizacao

```rust
struct DecisionValidity {
    valid_from: f64,
    valid_to: f64,
    max_reuse_count: Option<usize>,  // Limita quantas vezes pode ser reutilizada
    current_reuse_count: usize,
}
```

Exemplo: "esta decisao de edit em compiler.py pode ser reutilizada no maximo 3 vezes. Na 4a vez, exige revalidacao completa." Previne que uma decisao antiga seja reutilizada infinitamente.

---

## Testes esperados

| Tipo | Quantidade | O que testa |
|---|---|---|
| Unit (decision lifecycle) | ~15 | 8 estados, transicoes validas/invalidas, versioning |
| Unit (decision reuse) | ~10 | Match por type+scope+time, expirados, revogados, max_reuse_count |
