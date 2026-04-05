---
name: agent-check
description: Use when asked for a health check, system status, or to verify project integrity. Checks build, tests, architecture boundaries, warnings, dead code, missing asserts, and unwrap() in production.
allowed-tools: Bash(cargo *), Read, Grep, Glob
---

## Health Check do Projeto

Executa uma verificação completa do estado do projeto.

### Checklist:

1. **Build**: `cargo check` passa sem erros?
2. **Testes**: `cargo test` — todos passam?
3. **Dependências circulares**: Verificar se algum crate importa quem não deveria
   - `theo-domain` não pode depender de nenhum outro crate
   - Apps não podem importar engines diretamente (só via `theo-application`)
4. **Warnings**: `cargo check 2>&1 | grep warning` — listar warnings ativos
5. **Dead code**: Verificar `#[allow(dead_code)]` — listar ocorrências
6. **Testes sem assert**: Buscar funções `#[test]` que não contêm `assert`
7. **unwrap() em produção**: Buscar `unwrap()` fora de `#[cfg(test)]`

### Output:

Reporte como checklist com status:
- OK — item passa
- WARN — item tem observações
- FAIL — item precisa de ação

Diretório: `/home/paulo/Projetos/usetheo/theo-code`

Argumento: $ARGUMENTS
