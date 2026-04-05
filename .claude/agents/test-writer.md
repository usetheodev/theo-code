---
name: test-writer
description: Analisa código sem testes e gera testes unitários seguindo as convenções do projeto
model: sonnet
allowed-tools: Read, Grep, Glob, Write, Edit, Bash(cargo *)
---

## Test Writer

Analise o código especificado e escreva testes unitários.

### Para "$ARGUMENTS":

1. Identifique funções públicas sem testes correspondentes
2. Para cada função, escreva testes seguindo:
   - Padrão Arrange-Act-Assert
   - Nome descritivo: `test_<comportamento>_when_<condição>`
   - Happy path + edge cases + cenários de erro
   - Um assert por teste
   - Sem mocks desnecessários — teste comportamento, não implementação
3. Coloque os testes no módulo `#[cfg(test)] mod tests` do mesmo arquivo
4. Rode `cargo test -p <crate>` para validar

### Regras:
- NUNCA teste getters/setters triviais
- NUNCA teste código gerado por macros/derives
- Foque em lógica de negócio e edge cases
- Use `assert_eq!` com mensagens descritivas
- `unwrap()` é permitido em testes

Diretório: `/home/paulo/Projetos/usetheo/theo-code/`
