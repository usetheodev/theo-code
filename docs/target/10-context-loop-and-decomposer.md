# 10 — Context Loop e Decomposer

Dois subsistemas que alimentam o agent com informacao relevante: o Context Loop injeta contexto durante a execucao; o Decomposer quebra a task em subtasks antes da execucao.

---

## Context Loop

### Estrutura

```
crates/agent/src/context_loop/
  mod.rs                   # ContextLoopEngine
  diagnostics.rs           # Deteccao de problemas + prescricao
```

### ContextLoopEngine

Observa o estado do agent a cada iteracao e injeta mensagens de contexto quando necessario:

```rust
struct ContextLoopEngine {
    max_iterations: usize,
}

impl ContextLoopEngine {
    /// Decide se deve injetar contexto nesta iteracao.
    /// Retorna None se nao ha nada a injetar.
    fn maybe_emit(&self, state: &AgentState, task: &str) -> Option<String>;
}
```

### O que injeta

- **SCOPE**: lembrete de quais arquivos o agent pode ler/editar
- **DECISIONS**: historico de decisoes tomadas ate agora (o que ja foi feito)
- **REUSE hints**: decisoes passadas reutilizaveis encontradas pelo DecisionStore
- **Diagnostics**: deteccao de problemas (agent preso em loop, fase errada, etc)

### Diagnostics

Detecta problemas e prescreve acoes corretivas:

| Diagnostico | Sintoma | Prescricao |
|---|---|---|
| **Stuck in LOCATE** | >3 iteracoes sem transicao | "Voce esta buscando ha muito tempo. Tente editar com base no que ja encontrou." |
| **Repeated edits** | Mesmo arquivo editado >2x | "Voce ja editou {file} {n} vezes. Considere uma abordagem diferente." |
| **Empty searches** | Busca sem resultados | "Busca '{query}' retornou 0 resultados. Tente termos mais amplos." |
| **Scope violation** | Tentativa de acessar fora do escopo | "Arquivo {file} fora do escopo. Escopo permitido: {files}" |

---

## Decomposer

### Estrutura

```
crates/agent/src/decomposer/
  mod.rs                   # HybridDecomposer
  intent.rs                # Classificacao por keywords
  templates.rs             # Templates por intent
```

### HybridDecomposer

Quebra a task do usuario em subtasks sem usar LLM. Dois mecanismos:

#### 1. Intent Classification (keywords)

Classifica a task por keywords para determinar o tipo:

| Intent | Keywords | Exemplo |
|---|---|---|
| `bug_fix` | fix, bug, error, crash, fail | "Fix the regex bug in compiler.py" |
| `refactor` | refactor, rename, extract, move | "Refactor the parser module" |
| `feature` | add, implement, create, new | "Add pagination to the API" |
| `test` | test, coverage, spec | "Add tests for the auth module" |

```rust
fn classify_intent(task: &str) -> Intent;
```

#### 2. Template Match

Cada intent tem um template de subtasks:

```rust
fn template_match(intent: &Intent, context: &PipelineContext) -> Vec<SubTask>;
```

Exemplo para `bug_fix`:
1. LOCATE: buscar arquivos relacionados ao bug
2. EDIT: aplicar fix
3. VERIFY: rodar testes
4. (CORRECT: se testes falharam, ajustar)

#### 3. Decision Reuse check

Antes de decompor, verifica se existe decisao reutilizavel no DecisionStore:

```rust
fn decompose(&self, task: &str, store: &DecisionStore, pipeline: &Pipeline) -> Vec<SubTask> {
    let intent = classify_intent(task);

    // Check reuse ANTES de decompor
    if let Some(reusable) = store.find_reusable_for_intent(&intent) {
        // Inject hint: "Decisao similar encontrada"
    }

    // Decompoe normalmente
    template_match(&intent, &pipeline.assemble(task))
}
```

---

## Testes esperados

| Tipo | Quantidade | O que testa |
|---|---|---|
| Unit (context loop) | ~10 | Diagnostics, scope/decision/reuse injection |
| Unit (decomposer) | ~5 | Intent classification, template match |
