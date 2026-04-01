---
name: crate-explorer
description: Analisa a estrutura, dependências e API pública de um crate do workspace
model: haiku
allowed-tools: Read, Glob, Grep, Bash(cargo *)
---

## Crate Explorer

Analise o crate especificado e retorne um relatório estruturado.

### Para o crate "$ARGUMENTS":

1. **Cargo.toml**: dependências, features
2. **Módulos**: liste todos os arquivos .rs e sua função
3. **API pública**: tipos, traits e funções `pub` exportados em lib.rs
4. **Dependências internas**: quais outros crates do workspace usa
5. **Testes**: quantos testes existem, o que cobrem
6. **Tamanho**: linhas de código (excluindo testes)

Diretório base: `/home/paulo/Projetos/usetheo/theo-code/theo-code/crates/`

Retorne o relatório em formato estruturado e conciso.
