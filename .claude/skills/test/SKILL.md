---
name: test
description: Use when asked to run tests, check test results, or diagnose test failures. Covers cargo test for all crates or specific ones, and identifies which crates have changed files.
allowed-tools: Bash(cargo *), Read, Grep
---

## Testes do Projeto

Execute os testes conforme o argumento:

- Sem argumentos ou "all": `cargo test` no workspace inteiro
- Nome de crate (ex: "theo-engine-graph"): `cargo test -p <crate>`
- Nome de teste específico: `cargo test <nome_do_teste>`
- "changed": identifique crates com arquivos modificados (`git diff --name-only`) e rode testes apenas desses crates

Diretório do workspace: `/home/paulo/Projetos/usetheo/theo-code/theo-code`

Se algum teste falhar:
1. Mostre o nome do teste que falhou
2. Mostre o assert que falhou (expected vs got)
3. Leia o código do teste para entender a intenção
4. Explique o problema em português
5. Sugira a correção

Ao final, reporte: X passed, Y failed, Z ignored.

Argumento: $ARGUMENTS
