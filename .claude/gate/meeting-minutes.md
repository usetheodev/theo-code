# Meeting — 2026-04-05 (Fase 3 Harness Engineering — MEDIUM Gaps)

## Proposta
6 tasks MEDIUM: GC skill, agent review on done, mutation testing, drift detection, self-correction, token estimation.

## Participantes
- **governance** — APPROVE (88%)
- **qa** — APPROVE (2 tasks exigem testes: 3.2 e 3.6)

## Conflitos
1. 3.4 pre-commit pode ser ignorado com --no-verify — camada extra, não único sensor
2. 3.6 deve atualizar callers atomicamente

## Veredito
**APPROVED**

## Escopo Aprovado
- `.claude/skills/gc/SKILL.md` (novo)
- `.claude/skills/mutants/SKILL.md` (novo)
- `crates/theo-agent-runtime/src/run_engine.rs` (3.2)
- `.githooks/pre-commit` (novo)
- `.claude/hooks/post-edit-lint.sh` (3.5)
- `crates/theo-domain/src/lib.rs` (3.6)
- `crates/theo-engine-retrieval/src/assembly.rs` (3.6)
- `crates/theo-agent-runtime/src/compaction.rs` (3.6)
- `mutants.toml` (novo)

## Condições
1. Task 3.2: testes para diff > 100, diff <= 100, diff vazio
2. Task 3.6: atualizar todos callers num commit atômico, testes em theo-domain
3. Task 3.4: detectar crate afetado, não rodar workspace inteiro
4. cargo check + cargo test verde após cada task
