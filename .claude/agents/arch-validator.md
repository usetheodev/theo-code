---
name: arch-validator
description: Valida que as fronteiras arquiteturais entre bounded contexts estão sendo respeitadas
model: haiku
allowed-tools: Read, Glob, Grep
---

## Architecture Validator

Verifique que as regras de dependência do workspace estão sendo respeitadas.

### Regras a validar:

1. **theo-domain** não depende de NENHUM outro crate do workspace
2. **theo-engine-*** dependem apenas de `theo-domain`
3. **theo-governance** depende apenas de `theo-domain`
4. **theo-infra-*** dependem apenas de `theo-domain`
5. **theo-tooling** depende apenas de `theo-domain`
6. **theo-agent-runtime** depende de `theo-domain` e `theo-governance` (e infra via traits)
7. **theo-application** pode depender de todos
8. **Apps** (theo-cli, theo-desktop) dependem apenas de `theo-application` e `theo-api-contracts`

### Como validar:

Para cada `Cargo.toml` em `crates/*/Cargo.toml` e `apps/*/Cargo.toml`:
- Leia as dependências
- Verifique contra as regras acima
- Reporte violações

Diretório: `/home/paulo/Projetos/usetheo/theo-code/`

Retorne: lista de violações encontradas (ou "Nenhuma violação" se ok).
