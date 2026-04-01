# 04 — Policy Engine

O Policy Engine avalia regras deterministicas sem LLM. Policies sao configuraveis via JSON e compiladas para AST na inicializacao.

**Depende de**: [03-decision-control-plane.md](03-decision-control-plane.md) (DecisionType, DecisionScope)

---

## Policy Trait

```rust
trait Policy: Send + Sync {
    fn name(&self) -> &str;
    fn evaluate(&self, request: &ValidateRequest) -> Option<DenyReason>;
}
```

Cada policy retorna `None` (permite) ou `Some(DenyReason)` (nega com motivo).

---

## Mini-DSL — deny_if / allow_if

```rust
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
```

### AST — Expressoes deterministicas

```rust
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

---

## Exemplo de policies (JSON)

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

## Mapeamento XACML — PEP/PDP/PAP/PIP

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

| Componente XACML | Mapeamento Theo Code | Responsabilidade |
|---|---|---|
| **PEP** (Policy Enforcement Point) | Agent loop | Intercepta tool calls, aplica verdict |
| **PDP** (Policy Decision Point) | ValidationPipeline | Decide ALLOW/DENY deterministicamente |
| **PAP** (Policy Administration Point) | Policy config files (JSON) | Gerencia e distribui policies |
| **PIP** (Policy Information Point) | GRAPHCTX Pipeline | Fornece facts: affected_files, communities, co-changes, tests |

---

## Testes esperados

| Tipo | Quantidade | O que testa |
|---|---|---|
| Unit (policy DSL) | ~15 | Parse→AST, evaluate, deny_if/allow_if, edge cases |
