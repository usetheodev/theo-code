---
paths:
  - "theo-code/crates/**/*.rs"
  - "theo-code/apps/theo-cli/**/*.rs"
  - "theo-code/apps/theo-desktop/src/**/*.rs"
---

# Convenções Rust

- Rust edition 2024 — use features modernas (let chains, etc.)
- Erros com `thiserror`: cada crate define seu `Error` enum
- Async runtime: `tokio` com features `full`
- Serialização: `serde` + `serde_json`
- Dependências compartilhadas DEVEM ser declaradas em `[workspace.dependencies]`
- Cada crate publica tipos via `lib.rs` — imports internos são `pub(crate)`
- Testes unitários em `#[cfg(test)] mod tests` no mesmo arquivo
- Testes de integração em `tests/` no nível do crate
- Use `assert_eq!` com mensagens descritivas: `assert_eq!(result, expected, "context: {details}")`
- Prefira `impl Into<T>` e `AsRef<T>` em parâmetros públicos para ergonomia
- NUNCA use `unwrap()` em código de produção — apenas em testes
- Use `?` operator para propagação de erros
