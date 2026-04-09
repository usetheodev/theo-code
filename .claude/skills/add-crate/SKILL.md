---
name: add-crate
description: Scaffold a new Rust crate in the workspace. Creates Cargo.toml, lib.rs, error.rs, updates workspace members.
user-invocable: true
allowed-tools: Bash(cargo *) Read Write Edit Glob
argument-hint: "theo-xxx \"description\""
---

Create a new crate in the Theo Code workspace.

## Arguments

- `$0`: crate name (must start with `theo-`)
- `$1`: description (quoted string)

## Steps

1. Validate: name starts with `theo-`, doesn't already exist in `crates/`
2. Create directory: `crates/$0/src/`
3. Create `crates/$0/Cargo.toml`:
   ```toml
   [package]
   name = "$0"
   version = "0.1.0"
   edition = "2024"
   description = "$1"

   [dependencies]
   theo-domain = { path = "../theo-domain" }
   thiserror.workspace = true
   ```
4. Create `crates/$0/src/lib.rs` with module doc comment
5. Create `crates/$0/src/error.rs` with thiserror enum
6. Add to workspace members in root `Cargo.toml`
7. Run `cargo check -p $0` to validate
8. Report success or failure
