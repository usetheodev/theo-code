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

### 8. Quality Grades

After completing the checklist, generate a quality grade per crate and save to `.theo/quality.json`:

```bash
# For each crate, compute a simple quality score:
# - Has tests? (+1)
# - cargo check clean? (+1)
# - No #[allow(dead_code)]? (+1)
# - No unwrap() in non-test code? (+1)
# - Test count > 10? (+1)
# Score: 0-5 mapped to grade: 0-1=D, 2=C, 3=B, 4=A, 5=A+
```

Write the result to `.theo/quality.json`:
```json
{
  "generated_at": "2026-04-05",
  "crates": {
    "theo-agent-runtime": {"grade": "A", "score": 4, "tests": 308, "warnings": 0, "dead_code": 2},
    "theo-domain": {"grade": "A+", "score": 5, "tests": 172, "warnings": 0, "dead_code": 0}
  }
}
```

This file is tracked by git — temporal evolution is visible via `git log -p .theo/quality.json`.

Argumento: $ARGUMENTS
