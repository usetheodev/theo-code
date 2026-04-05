# Meeting — 2026-04-05 (Fase 1 Harness Engineering — CRITICAL Gaps)

## Proposta
4 tasks CRITICAL: GRAPHCTX default ON, done gate com cargo test, compaction semântica, session bootstrap.

## Participantes
- **governance** — APPROVE com condições (confiança 82%)
- **qa** — APPROVE com condições (292 testes baseline)
- **runtime** — APPROVE, risk MEDIUM (6 issues identificados)
- **graphctx** — APPROVE (3 arquivos confirmados, boot impact negligenciável)

## Análises

### Governance
Aprova todas. Exige: file locking em progress.json, CompactionContext desacoplado, fallback cargo check no timeout, done_attempts counter.

### QA
Aprova. Alerta que cargo test é 10-60s+ vs cargo check 3-10s. Exige testes para cada path: timeout, skip non-Rust, graphctx on/off.

### Runtime
Aprova com MEDIUM risk. Alertas: cargo test irrestrito pode levar 3-10min; done→block loop sem counter; std::fs em async é antipadrão. Propõe: cargo test -p <crate>, done_attempts max 3, tokio::fs/spawn_blocking.

### GraphCtx
Aprova. Confirmou 3 arquivos com guard (repl.rs, pilot.rs, run_agent_session.rs). Boot impact negligenciável — graph build é async com cap de 500 files e timeout 60s. CodebaseContextTool retorna vazio enquanto building.

## Conflitos
1. cargo test scope: workspace (proposta) vs per-crate (runtime). Resolução: `cargo test -p <crate>` com timeout 60s
2. ConvergenceEvaluator: remover dead_code (proposta) vs manter até wiring (governance). Resolução: ativar como pré-filtro, manter allow onde necessário
3. done loop: sem counter (proposta) vs max 3 attempts (runtime). Resolução: max 3

## Veredito
**APPROVED**

## Escopo Aprovado

### Task 1.1 — GRAPHCTX default ON
- `apps/theo-cli/src/repl.rs`
- `apps/theo-cli/src/pilot.rs`
- `crates/theo-application/src/use_cases/run_agent_session.rs`

### Task 1.2 — Done gate com cargo test
- `crates/theo-agent-runtime/src/run_engine.rs`

### Task 1.3 — Compaction semântica
- `crates/theo-agent-runtime/src/compaction.rs`

### Task 1.4 — Session bootstrap
- `crates/theo-agent-runtime/src/run_engine.rs`
- `crates/theo-agent-runtime/src/session_bootstrap.rs` (novo)
- `crates/theo-agent-runtime/src/lib.rs` (registrar módulo)

## Condições
1. Task 1.1: alterar nos 3 arquivos (repl.rs, pilot.rs, run_agent_session.rs). Opt-out via THEO_NO_GRAPHCTX=1
2. Task 1.2: cargo test -p <crate-afetado> com timeout 60s, fallback cargo check. done_attempts max 3. ConvergenceEvaluator como pré-filtro (git diff vazio → bloquear done)
3. Task 1.3: CompactionContext como parâmetro, não AgentState direto. Sumário max 150 tokens
4. Task 1.4: I/O async (tokio::fs ou spawn_blocking). Timeout 2s no boot. File locking no progress.json. Testes TDD
5. cargo check + cargo test -p theo-agent-runtime verde após cada task
