# theo-compat-harness — Revisao

> **Contexto**: Harness de compatibilidade. Extrai manifesto de commands/tools/bootstrap a partir do codigo upstream (`claw-code`/`clawd-code`) para validacao de paridade.
>
> **Observacao**: o `Cargo.toml` atualmente referencia `commands`/`tools`/`runtime` como caminhos (`../commands`, `../tools`, `../runtime`) que nao correspondem a nenhum crate existente no workspace — compila apenas em setups com fixture upstream.
>
> **Status global**: deep-review concluido em 2026-04-25. Confirmado: o crate `compat-harness` (note: nome do package nao tem prefixo `theo-`) NAO esta no `workspace.members` do Cargo.toml raiz. As tres deps `commands`/`tools`/`runtime` referenciadas nao existem no workspace. O crate e essencialmente um stub documentado para futuro fixture-driven compat testing.

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `UpstreamPaths` | Resolucao de caminhos do repo upstream (commands.ts, tools.ts, cli.tsx). | Revisado |
| 2 | `extract_manifest` | Extracao unificada do manifest (commands + tools + bootstrap). | Revisado |
| 3 | `extract_commands` | Parser de `src/commands.ts` (builtin, feature-gated, internal-only). | Revisado |
| 4 | `extract_tools` | Parser de `src/tools.ts` (Base, Conditional). | Revisado |
| 5 | `extract_bootstrap_plan` | Parser de `src/entrypoints/cli.tsx` (fast-paths do boot). | Revisado |
| 6 | Dependencias externas (`commands`, `tools`, `runtime`) | Crates nao presentes no workspace atual — risco de build quebrado em CI. | Revisado (limitacao documentada) |

---

## Notas de Deep-Review

### 1. UpstreamPaths
Stub. Estrutura prevista para resolver caminhos do repo upstream (claw-code) quando presente como sibling directory.

### 2. extract_manifest
Stub. Coordenacao prevista: chamar extract_commands + extract_tools + extract_bootstrap_plan, retornar manifest unificado.

### 3. extract_commands
Stub. Parser TypeScript previsto para `src/commands.ts` extraindo builtin/feature-gated/internal-only command lists.

### 4. extract_tools
Stub. Parser TypeScript previsto para `src/tools.ts` distinguishing Base e Conditional tools.

### 5. extract_bootstrap_plan
Stub. Parser TypeScript previsto para `src/entrypoints/cli.tsx` extraindo fast-paths do boot.

### 6. Dependencias externas
**Limitacao documentada**: `Cargo.toml` declara:
```toml
commands = { path = "../commands" }
tools = { path = "../tools" }
runtime = { path = "../runtime" }
```
Esses paths apontam para `crates/commands/`, `crates/tools/`, `crates/runtime/` que NAO existem. Verificacao:
```bash
$ ls crates/ | grep -E '^(commands|tools|runtime)$'
(empty)
```
O crate NAO esta listado em `workspace.members`. Consequencia: NAO afeta `cargo build`/`cargo test` workspace-wide (nao e construido). E uma documentacao em codigo de uma intencao futura — quando o upstream fixture estiver disponivel via sub-modulo ou clone, esses paths deveriam apontar para os crates Rust gerados a partir dos modulos TypeScript upstream.

**Validacao:**
- O crate NAO afeta CI atual (nao e workspace member, nao e buildado)
- A documentacao do REVIEW e o documento principal cobre o assunto — apos esta auditoria, o estado "stub aspiracional" esta explicitamente registrado
- Sem test count: o crate e opt-in via path, nao roda em `cargo test --workspace`

**Follow-up nao-bloqueador:** quando o upstream fixture for incorporado, atualizar `Cargo.toml` para apontar para os caminhos certos (provavel `external/claw-code/commands` etc.) e adicionar como workspace member opcional via feature gate, OU mover o `Cargo.toml` + `lib.rs` para um diretorio `tools/compat-harness/` para clarificar que e infraestrutura externa. Nao bloqueia operacao da workspace atual.
