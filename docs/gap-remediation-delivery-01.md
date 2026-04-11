# Gap Remediation — Delivery 01

Esta entrega cobre o primeiro bloco executado do plano em `docs/theo-gap-remediation-plan.md`.

## Escopo entregue

### 1. Single Source of Truth da superficie de tools

Entregue:

- `crates/theo-tooling/src/tool_manifest.rs`
- export do modulo em `crates/theo-tooling/src/lib.rs`

O manifest agora classifica a superficie por:

- `DefaultRegistry`
- `MetaTool`
- `ExperimentalModule`
- `InternalModule`

E por status:

- `Implemented`
- `Partial`
- `Stub`

### 2. README principal alinhado ao runtime real

Entregue:

- secao de tools do `README.md` reescrita para separar:
  - default registry tools
  - meta-tools
  - experimental modules

### 3. `crates/theo-tooling/README.md` corrigido

Entregue:

- status explicito por tool
- distincao entre `default-registry`, `meta-tool`, `experimental`, `stub`
- comandos de teste corrigidos para `theo-tooling`

### 4. Contrato incorreto de persistencia corrigido

Entregue:

- comentario de `session_bootstrap::save_progress()` ajustado para refletir atomic replace sem advisory locking

### 5. Warnings triviais removidos

Entregue:

- import inutil removido em `agent_loop.rs`
- variavel inutil removida em `compaction.rs`
- import inutil removido em `sandbox/env_sanitizer.rs`

## Validacao

Comandos executados:

```bash
cargo fmt --all
cargo test -q -p theo-tooling
cargo test -q -p theo-agent-runtime --lib
```

Resultados:

- `theo-tooling`: 240 testes passando
- `theo-agent-runtime --lib`: 333 testes passando

## Validacao de DoD

### Epic 1 — status parcial validado

Criticos cumpridos nesta entrega:

- existe inventario unificado de superficie
- README principal alinhado ao runtime real
- tooling README alinhado ao registry/meta-tools
- stubs principais marcados explicitamente

Ainda pendente para fechar o epic completamente:

- decidir se o inventario sera consumido por um comando CLI ou outro ponto automatico

### Epic 4 — status parcial validado

Critico cumprido nesta entrega:

- comentario de contrato incorreto em persistencia foi corrigido

Ainda pendente:

- consolidacao conceitual completa de sessao/run/progress/snapshot
- recovery e endurecimento do subsistema

### Epic 5 — status parcial validado

Critico cumprido nesta entrega:

- warnings triviais observados nos crates afetados foram removidos

Ainda pendente:

- baseline formal de warnings
- reducao sistematica nos demais crates
- guardrail de CI

## Pendencias principais apos esta entrega

1. Resolver stubs operacionais:
   - `websearch`
   - `codesearch`
   - `lsp`
   - `multiedit`
   - `task` placeholder
2. Concluir migracao do runtime legado/deprecated
3. Consolidar sessao e persistencia
4. Reduzir warnings no restante do caminho critico

## Veredito da entrega

Entrega 01 aprovada.

Os DoDs parciais deste bloco foram validados por:

- source of truth em codigo
- docs alinhadas
- testes verdes nos crates impactados
