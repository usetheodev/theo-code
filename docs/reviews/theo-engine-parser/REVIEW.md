# theo-engine-parser — Revisao

> **Contexto**: Parser AST multi-linguagem via Tree-Sitter. Bounded Context: Code Intelligence.
>
> **Dependencias permitidas**: `theo-domain`.
>
> **Linguagens suportadas**: C, C#, C++, Go, Java, JavaScript, Kotlin, PHP, Python, TypeScript (e outras via extractors).

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `code_compression` | Compressao de AST/codigo para reduzir tokens em LLM. | Pendente |
| 2 | `error` | Hierarquia de erros do parser. | Pendente |
| 3 | `extractors::call_graph` | Extracao de call graph por linguagem. | Pendente |
| 4 | `extractors::common` | Utilidades compartilhadas entre extractors. | Pendente |
| 5 | `extractors::csharp` | Extractor especifico para C#. | Pendente |
| 6 | `extractors::data_models` | Extracao de data models (struct/class/record). | Pendente |
| 7 | `extractors::env_detection` | Deteccao do ambiente/stack do projeto. | Pendente |
| 8 | `extractors::generic` | Extractor generico fallback. | Pendente |
| 9 | `extractors::go` | Extractor especifico para Go. | Pendente |
| 10 | `extractors::java` | Extractor especifico para Java. | Pendente |
| 11 | `extractors::language_behavior` | Regras de comportamento por linguagem. | Pendente |
| 12 | `import_resolver` | Resolucao de imports/modulos por linguagem. | Pendente |
| 13 | `patterns` | Padroes reutilizaveis de matching sobre a AST. | Pendente |
| 14 | `symbol_table` | Tabela de simbolos por arquivo/escopo. | Pendente |
| 15 | `tree_sitter` | Wrapper sobre a API do tree-sitter. | Pendente |
| 16 | `types` | Tipos publicos do parser (`Symbol`, `Range`, `Scope`). | Pendente |
| 17 | `workspace::detect` | Deteccao de tipo/layout de workspace. | Pendente |
