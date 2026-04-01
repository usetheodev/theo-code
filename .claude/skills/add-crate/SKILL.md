---
name: add-crate
description: Use when creating a new Rust crate in the workspace. Scaffolds Cargo.toml, lib.rs, error.rs, updates workspace members, and validates with cargo check.
allowed-tools: Bash(cargo *), Bash(mkdir *), Read, Write, Edit
---

## Criar Novo Crate

Crie um novo crate no workspace seguindo as convenções do Theo Code.

Argumento esperado: `<nome-do-crate> <descrição>`

Exemplo: `/add-crate theo-engine-symbols "Extração e indexação de símbolos cross-language"`

### Passos:

1. Crie o diretório em `theo-code/crates/<nome>/`
2. Crie `Cargo.toml` com:
   - `package.name = "<nome>"`
   - `package.version.workspace = true`
   - `package.edition.workspace = true`
   - `package.license.workspace = true`
   - `package.description = "<descrição>"`
   - Dependência de `theo-domain` via workspace
3. Crie `src/lib.rs` com módulo base e doc comment
4. Crie `src/error.rs` com enum de erros usando `thiserror`
5. Adicione o crate ao `[workspace.members]` no root `Cargo.toml`
6. Adicione ao `[workspace.dependencies]` no root `Cargo.toml`
7. Rode `cargo check -p <nome>` para validar
8. Atualize o CHANGELOG.md com a adição

### Validações:
- Nome DEVE começar com `theo-`
- DEVE seguir um bounded context existente (engine-*, infra-*, etc.)
- Se não seguir, pergunte ao usuário onde posicionar

Argumento: $ARGUMENTS
