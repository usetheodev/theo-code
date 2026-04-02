# Fase 12 — Integration & Convergence

## Objetivo

Conectar tudo. Implementar critérios de convergência, estratégias de correção,
e o fluxo end-to-end completo. Atualizar o CLI binary.

## Dependências

- Todas as fases anteriores (01-11)

## Arquivos

### Novos

| Arquivo | Crate | Conteúdo | Linhas Est. |
|---------|-------|----------|-------------|
| `src/convergence.rs` | theo-agent-runtime | `ConvergenceEvaluator`, critérios | ~120 |
| `src/correction.rs` | theo-agent-runtime | `CorrectionEngine` | ~150 |

### Modificados

| Arquivo | Mudança |
|---------|---------|
| `src/run_engine.rs` | Integrar convergência + correção |
| `src/lib.rs` | Re-exports finais |
| `src/bin/theo-agent.rs` | Flags: `--resume`, `--budget-*`, `--priority` |

## Tipos Definidos

### convergence.rs

```rust
#[async_trait]
pub trait ConvergenceCriterion: Send + Sync {
    fn name(&self) -> &str;
    async fn is_converged(
        &self,
        run: &AgentRun,
        task: &Task,
        project_dir: &Path,
    ) -> bool;
}

pub struct GitDiffConvergence;       // converged se git diff != vazio + sem erros
pub struct EditSuccessConvergence;   // pelo menos 1 edit bem-sucedido

pub enum ConvergenceMode { AllOf, AnyOf }

pub struct ConvergenceEvaluator {
    criteria: Vec<Box<dyn ConvergenceCriterion>>,
    mode: ConvergenceMode,
}

impl ConvergenceEvaluator {
    pub fn new(criteria: Vec<Box<dyn ConvergenceCriterion>>, mode: ConvergenceMode) -> Self;
    pub async fn evaluate(&self, run: &AgentRun, task: &Task, project_dir: &Path) -> bool;
}
```

### correction.rs

```rust
pub struct CorrectionEngine {
    event_bus: Arc<EventBus>,
}

impl CorrectionEngine {
    pub fn select_strategy(
        &self,
        failure: &ToolResultRecord,
        attempt: u32,
        run_state: &RunState,
    ) -> CorrectionStrategy;

    pub async fn apply(
        &self,
        strategy: CorrectionStrategy,
        engine: &mut AgentRunEngine,
    ) -> Result<(), CorrectionError>;
}
```

### Regra Crítica de Correção

Toda correção deve reduzir pelo menos uma incerteza:
- Escopo
- Erro
- Falta de dados

Se não reduzir → abortar ou escalar.

## CLI Updates

```
theo-agent run "task" [OPTIONS]
  --resume <run-id>        Resume de run anterior
  --budget-time <secs>     Override time budget
  --budget-tokens <n>      Override token budget
  --priority <level>       low|normal|high|critical
  --snapshot-dir <path>    Override diretório de snapshots
```

## Estados de Resultado (da Spec)

| Estado | Definição |
|--------|-----------|
| success | objetivo atingido |
| partial | incompleto |
| failed | erro terminal |
| aborted | interrompido |
| stalled | sem progresso |

## Critérios de Convergência

Uma execução converge quando:
1. Objetivo validado (ConvergenceCriterion)
2. Nenhuma ação pendente
3. Qualidade mínima atingida

## Critérios de Abort

- Limite de iteração atingido
- Custo excedido
- Falha crítica
- Loop detectado

## Testes Requeridos (~20)

### Convergence (~6)
- GitDiffConvergence com/sem mudanças
- EditSuccessConvergence com/sem edits
- AllOf mode: todos critérios devem passar
- AnyOf mode: qualquer critério basta
- Avaliação com criteria vazio
- ConvergenceMode serde roundtrip

### Correction (~6)
- Strategy selection baseado em tipo de falha
- RetryLocal para falha transitória
- Replan para falha semântica
- Subtask para escopo grande
- AgentSwap quando agente não converge
- Apply correction com cada strategy

### Integration E2E (~5)
- Full run com mock LLM: task created → run executes → tools called → converges → snapshot
- Resume: run parcial → save snapshot → resume → completa
- Budget exceeded → abort
- All 8 invariants verified em teste dedicado
- CLI argument parsing para novos flags

### Backward Compat (~3)
- `AgentLoop::run` funciona sem mudanças de API
- `AgentResult` mantém campos existentes
- Old binary commands continuam funcionando

## Verificação Final — Todos os 8 Invariantes

| # | Invariante | Teste |
|---|-----------|-------|
| 1 | Task tem task_id, session_id, state, created_at | `test_invariant_1_task_fields` |
| 2 | Tool Call tem call_id único | `test_invariant_2_unique_call_id` |
| 3 | Tool Result referencia call_id | `test_invariant_3_result_refs_call` |
| 4 | Completed não volta para Running | `test_invariant_4_terminal_state` |
| 5 | Transição gera Event | `test_invariant_5_event_on_transition` |
| 6 | Execução tem run_id | `test_invariant_6_unique_run_id` |
| 7 | Resume de snapshot consistente | `test_invariant_7_resume_consistency` |
| 8 | Sem execução sem budget | `test_invariant_8_budget_required` |

## Definition of Done

| # | Critério | Verificação |
|---|----------|-------------|
| 1 | Teste E2E end-to-end: create task → run → tools → converge → result | Teste de integração |
| 2 | Todos os 8 invariantes verificados em testes dedicados | 8 testes nomeados |
| 3 | `--resume` funciona: carrega snapshot, retoma execução | Teste de integração |
| 4 | `cargo test -p theo-agent-runtime` passa com 199+ testes totais | `cargo test` |
| 5 | `cargo check --workspace` compila limpo | `cargo check` |

## Limites do Sistema (Reconhecidos)

O sistema NÃO garante:
- Planejamento perfeito
- Execução sem falha
- Convergência garantida

O sistema garante:
- Rastreabilidade
- Recuperação
- Auditabilidade
- Controle
