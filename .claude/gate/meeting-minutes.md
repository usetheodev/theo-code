# Meeting — 2026-04-05 (Fase 0 Harness Engineering — Fundação)

## Proposta
Fase 0 do plano de Harness Engineering — Fundação. 4 tasks para resolver bugs ativos e debt que bloqueiam fases seguintes, identificados por auditoria de 4 agentes contra pesquisas de Harness Engineering (Anthropic, OpenAI, Martin Fowler, Dex Horthy).

## Participantes
- **governance** (Principal Engineer) — APPROVE com condições
- **qa** (QA Staff Engineer) — APPROVE com condições (404 testes baseline)
- **graphctx** (Compiler Engineer) — APPROVE (revelou 4 deps escondidas na Pipeline)
- **runtime** (Staff AI Engineer) — APPROVE (confirmou 4 testes tautológicos)

## Análises

### Governance
Confirmou violação de boundary. Alertou que Pipeline tem 560 linhas — refactor é cirurgia delicada. Sugeriu ordem 0.3→0.5→0.2→0.4. Condição: cargo test verde após cada task.

### QA
404 testes passando como baseline. Confirmou 4 testes tautológicos. theo-application tem 10 testes (corrigido de 0 relatado). Risco de regressão MEDIO na task 0.1 original.

### GraphCtx
Revelou que Pipeline tem 21 funções públicas e 4 dependências escondidas (bincode, rayon, Community tipo concreto, duplicação extract.rs/graph_context_service.rs). Argumentou que boundary test deve ser workspace-level, não em theo-governance (SRP).

### Runtime
Confirmou 3 testes tautológicos + 1 compilation-only mascarado. Sugeriu asserts específicos para cada caso. Risco LOW.

## Conflitos
1. **Localização boundary test**: proposta (theo-governance) vs GraphCtx (workspace-level). Resolução: workspace-level — SRP.
2. **Escopo task 0.1**: refactor completo de Pipeline é grande demais para Fase 0. Resolução: reduzir para boundary test que DETECTA violação. Refactor vai para Fase 1.
3. **Ponto cego (advocacia do diabo)**: task 0.1 original pode bloquear todo o plano se falhar. Reduzir escopo mitiga risco.

## Veredito
**APPROVED**

## Escopo Aprovado (ordem de execução)

### Task 0.3 — Fix paths errados em agents/skills
- `.claude/agents/governance.md`
- `.claude/agents/qa.md`
- `.claude/agents/runtime.md`
- `.claude/agents/graphctx.md`
- `.claude/agents/arch-validator.md`
- `.claude/agents/crate-explorer.md`
- `.claude/agents/test-writer.md`
- `.claude/skills/agent-check/SKILL.md`
- Qualquer outro arquivo em `.claude/` com path `theo-code/theo-code`

### Task 0.5 — Meeting gate refinement
- `.claude/hooks/meeting-gate.sh`

### Task 0.2 — Teste estrutural de boundary (workspace-level)
- `tests/boundary_test.rs` (novo, workspace root)

### Task 0.4 — Fix testes tautológicos
- `crates/theo-agent-runtime/src/tool_call_manager.rs`
- `crates/theo-agent-runtime/src/agent_loop.rs`
- `crates/theo-agent-runtime/src/run_engine.rs`

## Condições
1. `cargo check` sem warnings novos após cada task
2. `cargo test` verde após cada task individual
3. Task 0.5: bypass DEVE continuar para `.claude/gate/*` — testar que /meeting funciona
4. Task 0.4: cada assert substituído DEVE poder falhar — proibido tautologia
5. Task 0.2: boundary test determinístico via leitura de Cargo.toml
6. Não acumular tasks sem validação
