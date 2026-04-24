# theo-compat-harness — Revisao

> **Contexto**: Harness de compatibilidade. Extrai manifesto de commands/tools/bootstrap a partir do codigo upstream (`claw-code`/`clawd-code`) para validacao de paridade.
>
> **Observacao**: o `Cargo.toml` atualmente referencia `commands`/`tools`/`runtime` como caminhos (`../commands`, `../tools`, `../runtime`) que nao correspondem a nenhum crate existente no workspace — compila apenas em setups com fixture upstream. Revisar.

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `UpstreamPaths` | Resolucao de caminhos do repo upstream (commands.ts, tools.ts, cli.tsx). | Pendente |
| 2 | `extract_manifest` | Extracao unificada do manifest (commands + tools + bootstrap). | Pendente |
| 3 | `extract_commands` | Parser de `src/commands.ts` (builtin, feature-gated, internal-only). | Pendente |
| 4 | `extract_tools` | Parser de `src/tools.ts` (Base, Conditional). | Pendente |
| 5 | `extract_bootstrap_plan` | Parser de `src/entrypoints/cli.tsx` (fast-paths do boot). | Pendente |
| 6 | Dependencias externas (`commands`, `tools`, `runtime`) | Crates nao presentes no workspace atual — risco de build quebrado em CI. | Pendente |
