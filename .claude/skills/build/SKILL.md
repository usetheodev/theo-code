---
name: build
description: Use when asked to build, compile, or check if the project compiles. Covers cargo build, cargo check, npm build, and cargo tauri build. Also use when a build error needs diagnosis.
allowed-tools: Bash(cargo *), Bash(npm *), Read, Grep
---

## Build do Projeto

Execute o build conforme o argumento:

- Sem argumentos ou "all": `cargo build` no workspace inteiro
- Nome de crate (ex: "theo-engine-graph"): `cargo build -p <crate>`
- "ui" ou "frontend": `cd apps/theo-ui && npm run build`
- "desktop": `cd apps/theo-desktop && cargo tauri build`
- "check": `cargo check` (mais rápido, só verifica compilação)

Diretório do workspace: `/home/paulo/Projetos/usetheo/theo-code/theo-code`

Se o build falhar:
1. Leia o erro completo
2. Identifique o arquivo e linha com problema
3. Explique o erro em português
4. Sugira a correção específica

Argumento: $ARGUMENTS
