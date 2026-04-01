# 06 — Governance Layer

O GovernanceLayer e o **Mediator** central que media TODAS as interacoes entre agent, tools, graph, e policies. Nenhuma acao do agent bypassa esta camada.

**Depende de**: [03-decision-control-plane.md](03-decision-control-plane.md), [05-validation-pipeline.md](05-validation-pipeline.md)

---

## GovernanceLayer

```
governance/
  mod.rs                   # GovernanceLayer — media TODAS as interacoes
  agent_identity.rs        # AgentIdentity — agent como no no grafo
  scoped_context.rs        # ScopedContext — delegation chain
  audit.rs                 # AuditLog — trail append-only
```

Responsabilidades:
- Registrar agent no grafo (`register_agent()`)
- Propor decisoes (`propose_decision()`)
- Validar via pipeline (`validate()`)
- Registrar outcomes (`record()`)
- Manter audit trail

---

## AgentIdentity — Agent como no no grafo

O agent nao e apenas um caller externo — ele e um **no** no CodeGraph com propriedades:

- **ownership**: quem criou o agent (user)
- **capabilities**: o que o agent pode fazer (edit, search, verify)
- **trust level**: nivel de confianca (user-created, automated, sub-flow)

Representado no grafo:
```
AgentIdentity --OWNED_BY--> User
Decision --MADE_BY--> AgentIdentity
Decision --AFFECTS--> File
Decision --FOLLOWS--> Decision (lineage)
```

---

## ScopedContext — Delegation chain

Cada nivel de delegacao **reduz** o escopo:

```
User("Fix bug X")
  → Agent(task="bug_fix", scope=all_files_in_module)
    → SubFlow(scope=only_compiler.py)
```

Regras:
- `User` define escopo inicial
- `Agent` restringe ao subset relevante
- `SubFlow` restringe ainda mais
- Cada nivel so pode **reduzir**, nunca expandir
- Validacao de escopo e feita pelo ValidationPipeline (step 1: SCOPE_MISMATCH)

```rust
struct ScopedContext {
    readable_files: HashSet<String>,
    writable_files: HashSet<String>,
    communities: HashSet<String>,
    delegation_chain: Vec<Principal>,  // Quem delegou para quem
}

impl ScopedContext {
    /// Cria sub-escopo reduzido
    fn restrict_writable(&self, files: &HashSet<String>) -> ScopedContext;

    /// Verifica se arquivo esta no escopo de escrita
    fn can_write(&self, file: &str) -> bool;

    /// Verifica se arquivo esta no escopo de leitura
    fn can_read(&self, file: &str) -> bool;
}
```

---

## AuditLog — Trail append-only

Toda acao e registrada em log imutavel para auditoria posterior:

```rust
struct AuditEntry {
    timestamp: f64,
    entry_type: AuditEntryType,
    principal: Principal,
    decision_id: Option<String>,
    details: String,
}

enum AuditEntryType {
    AgentRegistered,
    DecisionProposed,
    ValidationResult,     // ALLOW ou DENY + reasons
    DecisionApproved,
    DecisionActivated,
    DecisionCompleted,
    DecisionFailed,
    DecisionBlocked,
    DecisionRevoked,
    ScopeRestricted,
}

struct AuditLog {
    entries: Vec<AuditEntry>,  // Append-only, nunca muta
}

impl AuditLog {
    fn append(&mut self, entry: AuditEntry);

    /// Query: "o que aconteceu nesta sessao?"
    fn query_by_session(&self, session_id: &str) -> Vec<&AuditEntry>;

    /// Query: "quem mexeu neste arquivo?"
    fn query_by_file(&self, file: &str) -> Vec<&AuditEntry>;

    /// Persistir em disco
    fn save(&self, path: &Path) -> Result<()>;
}
```

---

## Testes esperados

| Tipo | Quantidade | O que testa |
|---|---|---|
| Unit (governance) | ~15 | Scope reduction, delegation chain, audit queries, register/propose/record |
| Integration (audit) | ~3 | Session → query chain completa |
