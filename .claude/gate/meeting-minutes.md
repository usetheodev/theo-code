# Meeting — 2026-04-05 (Plano HE Gaps)

## Proposta
Plano executável para fechar 8 gaps de harness engineering em 3 fases.

## Participantes
- **governance** — 3 fases, cortou YAGNI (quality grades, skills classification)
- **runtime** — system prompt posição, clean state inline no RunEngine, BLOQUEIA done
- **qa** — validated=false → true com 2 pré-requisitos (bug post-edit-lint, trait CleanStateChecker)

## Plano Executável Aprovado

### Fase 1: System Prompt HE Framing (P0)

**Escopo**: Adicionar seção `## Harness Context` ao default_system_prompt() em config.rs.
**Posição**: Antes do `## CRITICAL` block (primeiros tokens = maior atenção).

**4 cláusulas obrigatórias**:
1. Harness awareness: "You operate inside the Theo harness with state machine, sandbox, and feedback loops."
2. Clean state contract: "Only call done() when the project compiles and tests pass."
3. Generic tools: "Use generic tools (bash, read, write, edit, grep). Don't ask for specialized capabilities."
4. Environment legibility: "Leave the environment in a clean, documented state after each task."

**DoD**:
- [ ] Seção `## Harness Context` presente antes de `## CRITICAL`
- [ ] 4 cláusulas presentes (test: `test_default_prompt_contains_he_clauses`)
- [ ] Cláusulas sobrevivem em todos os modes (test: `test_he_clauses_in_plan_and_ask_modes`)
- [ ] `cargo test -p theo-agent-runtime` 290+ testes passando
- [ ] Adição ≤ 10 linhas no prompt

**Arquivo**: `crates/theo-agent-runtime/src/config.rs`
**Estimativa**: 1 sessão

### Fase 2: Sensors de Feedback (P1)

**2a: Clean State Sensor pós-done**

**Escopo**: No RunEngine, quando `done` meta-tool é chamado, rodar verificação de compilação antes de transitar para Converged. Se falhar, injetar erro como tool_result e continuar loop.

**Pré-requisito**: Trait `CleanStateChecker` para testabilidade (DIP).

```rust
trait CleanStateChecker: Send + Sync {
    async fn check(&self, project_dir: &Path) -> Result<(), String>;
}
```

Implementações: `CargoChecker` (produção) e `MockChecker` (testes).

**DoD**:
- [ ] Trait `CleanStateChecker` definido (theo-domain ou run_engine inline)
- [ ] RunEngine recebe `Option<Box<dyn CleanStateChecker>>` via construtor
- [ ] Bloco `done` verifica clean state antes de Converged
- [ ] Falha = injetar Message::tool_result com erros de compilação, loop continua
- [ ] Timeout 30s — se expirar, done prossegue (best-effort)
- [ ] Skip se sem Cargo.toml (projetos não-Rust)
- [ ] Testes: `test_done_blocked_on_check_failure`, `test_done_passes_on_check_success`, `test_done_skips_non_rust`
- [ ] `cargo test -p theo-agent-runtime` todos passando

**Arquivo**: `crates/theo-agent-runtime/src/run_engine.rs`
**Estimativa**: 1-2 sessões

**2b: Linter Messages LLM-optimized**

**Pré-requisito**: CORRIGIR BUG em post-edit-lint.sh (path inválido `theo-code/theo-code`).

**Escopo**: Melhorar post-edit-lint.sh para:
1. Corrigir path do workspace
2. Remover `--quiet` para capturar mensagens reais do rustc
3. Parsear erros comuns e adicionar remediation hints

**Remediation patterns**:
- `unresolved import` → `FIX: Add 'use crate::module::Type;' at the top`
- `missing field` → `FIX: Add the missing field to the struct literal`
- `cannot borrow as mutable` → `FIX: Change '&self' to '&mut self' or use interior mutability`
- Fallback: mensagem original do rustc (sem remediation)

**DoD**:
- [ ] Bug de path corrigido (cd correto para workspace root)
- [ ] `--quiet` removido, output real do rustc capturado
- [ ] Pelo menos 3 patterns de remediation implementados
- [ ] Hook executa sem erro em projeto limpo (exit 0)
- [ ] Hook emite remediation em projeto com erro proposital
- [ ] Nenhum arquivo Rust alterado nesta sub-fase

**Arquivo**: `.claude/hooks/post-edit-lint.sh`
**Estimativa**: 1 sessão

### Fase 3: Context Quality (P2)

**3a: Skill /doc-garden**

**Escopo**: Nova skill em `.claude/skills/doc-garden/SKILL.md` que instrui o agent a:
1. Ler .theo/theo.md atual
2. Comparar com estado real (crates, endpoints, tools registrados)
3. Identificar seções obsoletas ou faltantes
4. Gerar sugestões de atualização (não aplica automaticamente)

**DoD**:
- [ ] Arquivo `.claude/skills/doc-garden/SKILL.md` existe
- [ ] Skill invocável pelo agent (trigger: "audit project documentation")
- [ ] Instruções claras para comparar doc vs código
- [ ] Nenhum arquivo Rust alterado

**Arquivo**: `.claude/skills/doc-garden/SKILL.md`
**Estimativa**: 1 sessão

**3b: Progressive Disclosure no theo init**

**Escopo**: Template do theo.md gerado por `theo init` segue formato índice (≤100 linhas com ponteiros para docs mais profundos se existirem).

**DoD**:
- [ ] Template do init gera theo.md com ponteiros (`## Architecture → see docs/adr/` se existir)
- [ ] Máximo 100 linhas no template
- [ ] Ponteiros só aparecem se os targets existirem (não linkar doc inexistente)
- [ ] `cargo test -p theo` (CLI tests) passam

**Arquivo**: `apps/theo-cli/src/init.rs`
**Estimativa**: 1 sessão

## Gaps Cortados (YAGNI)

- P3: Skills HE classification — taxonomia sem consumidor
- P2: Quality grades por crate — sem consumidor claro, re-entra quando 3+ regressões detectadas
- Futuro: Frontend agent — quando desktop em produção

## Conflitos Resolvidos

1. Bug post-edit-lint.sh: pré-requisito da Fase 2b (corrigir primeiro)
2. CleanStateChecker trait: DIP obrigatório para testabilidade (Fase 2a)
3. Clean state Rust-only: best-effort, skip se sem Cargo.toml

## Estimativa Total

| Fase | Sessões | Risco |
|---|---|---|
| Fase 1 | 1 | Mínimo (10 linhas de texto) |
| Fase 2 | 2-3 | Médio (trait + RunEngine change + hook fix) |
| Fase 3 | 2 | Baixo (skills + init template) |
| **Total** | **5-6 sessões** | |

## Veredito
**APPROVED**
