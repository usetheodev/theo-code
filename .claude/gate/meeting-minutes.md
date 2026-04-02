# Meeting — 2026-04-01 (Fase 01: Core Types & State Machines)

## Proposta
Adicionar 4 novos módulos em theo-domain (identifiers.rs, task.rs, tool_call.rs, agent_run.rs)
com 3 state machines (TaskState, ToolCallState, RunState), newtypes para IDs (TaskId, CallId,
RunId, EventId), e contratos de dados (Task, ToolCallRecord, ToolResultRecord, AgentRun).
~600 linhas, ~45 testes. Zero async, zero IO.

## Participantes
- **governance** — Principal Engineer (veto absoluto)
- **qa** — QA Staff Engineer
- **runtime** — Staff AI Systems Engineer
- **graphctx** — Compiler Engineer

## Análises

### Governance (APPROVE com 7 condições)
- ToolResultRecord deve manter sufixo Record (evita colisão com ToolResult<T>)
- SessionId reutilizado de session.rs, não redefinido
- generate() com std::time + random leve, sem deps pesadas
- Zero async nos novos módulos
- Match arms exaustivos, sem wildcards
- TransitionError com from/to para debuggability

### QA (validated=false)
- Plano de 45 testes é insuficiente; mínimo defensivo é ~100
- Tabelas de transição O(N²) obrigatórias para cada state machine
- Spec ambígua: comportamento de Blocked, TaskId::new(""), trigger de Waiting
- Testes de atomicidade ausentes: transition() não deve mutar estado em caso de Err
- Serde roundtrip deve usar assert_eq!, não apenas verificar que não panics

### Runtime (risk=MEDIUM, APPROVE com condições)
- Borda WaitingTool → Failed ausente no grafo (HIGH)
- Trigger de saída de RunState::Waiting indefinido (HIGH)
- Loop de replanning sem circuit breaker no tipo (MEDIUM)
- Considerar Paused/Suspended para Fase 10 (MEDIUM)
- Phase enum deve ser marcado #[deprecated] desde a Fase 01

### GraphCtx (risk=MEDIUM)
- Novos módulos não criam problemas de dependência
- ToolCallRecord evita colisão com ToolCall de theo-infra-llm
- Fragmentação de identifiers (session.rs vs identifiers.rs) — inconsistência latente
- Confirmar estratégia de ID: newtype String como SessionId, sem uuid

## Conflitos
1. WaitingTool → Failed ausente — Runtime identifica como blocker para Fase 06
2. RunState::Waiting sem trigger de saída — estado zombie potencial
3. Testes 45 vs 100 — QA exige expansão significativa
4. Paused/Suspended — Runtime recomenda para evitar retrofit na Fase 10
5. Fragmentação de identifiers — session.rs vs identifiers.rs

## Veredito
**REJECTED**

Razão: QA.validated = false (testes insuficientes e spec ambígua em pontos críticos)

## Condições para Re-aprovação
1. Adicionar bordas WaitingTool → Failed e WaitingInput → Failed no grafo de TaskState
2. Definir trigger de saída de RunState::Waiting
3. Expandir plano de testes de 45 para ~100 com tabelas O(N²)
4. Definir contrato de IDs vazios (assert ou Result)
5. Adicionar testes de atomicidade para transition()
6. Documentar que loop de replanning não tem circuit breaker no tipo

## Escopo (quando aprovado)
- `crates/theo-domain/src/identifiers.rs` (novo)
- `crates/theo-domain/src/task.rs` (novo)
- `crates/theo-domain/src/tool_call.rs` (novo)
- `crates/theo-domain/src/agent_run.rs` (novo)
- `crates/theo-domain/src/lib.rs` (modificado)
- `crates/theo-domain/src/error.rs` (modificado)
