# theo-engine-parser — Revisao

> **Contexto**: Parser AST multi-linguagem via Tree-Sitter. Bounded Context: Code Intelligence.
>
> **Dependencias permitidas**: `theo-domain`.
>
> **Linguagens suportadas**: C, C#, C++, Go, Java, JavaScript, Kotlin, PHP, Python, TypeScript (e outras via extractors).
>
> **Status global**: deep-review concluido em 2026-04-25. **469 tests passando**, 0 falhas — maior cobertura entre os crates de Code Intelligence. `cargo clippy --lib --tests` silent.

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `code_compression` | Compressao de AST/codigo para reduzir tokens em LLM. | Revisado |
| 2 | `error` | Hierarquia de erros do parser. | Revisado |
| 3 | `extractors::call_graph` | Extracao de call graph por linguagem. | Revisado |
| 4 | `extractors::common` | Utilidades compartilhadas entre extractors. | Revisado |
| 5 | `extractors::csharp` | Extractor especifico para C#. | Revisado |
| 6 | `extractors::data_models` | Extracao de data models (struct/class/record). | Revisado |
| 7 | `extractors::env_detection` | Deteccao do ambiente/stack do projeto. | Revisado |
| 8 | `extractors::generic` | Extractor generico fallback. | Revisado |
| 9 | `extractors::go` | Extractor especifico para Go. | Revisado |
| 10 | `extractors::java` | Extractor especifico para Java. | Revisado |
| 11 | `extractors::language_behavior` | Regras de comportamento por linguagem. | Revisado |
| 12 | `import_resolver` | Resolucao de imports/modulos por linguagem. | Revisado |
| 13 | `patterns` | Padroes reutilizaveis de matching sobre a AST. | Revisado |
| 14 | `symbol_table` | Tabela de simbolos por arquivo/escopo. | Revisado |
| 15 | `tree_sitter` | Wrapper sobre a API do tree-sitter. | Revisado |
| 16 | `types` | Tipos publicos do parser (`Symbol`, `Range`, `Scope`). | Revisado |
| 17 | `workspace::detect` | Deteccao de tipo/layout de workspace. | Revisado |

---

## Notas de Deep-Review

### 1. code_compression
Strategies para reducir AST/codigo a um schema de tokens-low: skip docstrings, collapse trivial bodies, summarize loops. Drives the "code packed" hint para LLM context.

### 2. error
`ParserError::{TreeSitterFailed, UnsupportedLanguage, ExtractionFailed, IoError, ...}`.

### 3. extractors::call_graph
Cross-linguagem call graph extraction. Tree-sitter queries por linguagem retornam (caller, callee) pairs. Pure analise sintatica (sem resolucao type-aware).

### 4. extractors::common
Helpers compartilhados entre todos os extractors: AST traversal, range-to-text, qualified-name builders.

### 5. extractors::csharp
C# specifics: namespace handling, partial classes, properties get/set as separate symbols.

### 6. extractors::data_models
Struct/class/record extraction com fields. Foundational para schema-aware retrieval.

### 7. extractors::env_detection
Detecta package.json, Cargo.toml, go.mod, requirements.txt, etc. para identificar stack do projeto.

### 8. extractors::generic
Fallback parser para linguagens sem extractor dedicado. Usa heuristicas de regex + tree-sitter generic.

### 9. extractors::go
Go specifics: receivers, interface methods, package-private visibility.

### 10. extractors::java
Java specifics: nested classes, generics, package declarations, annotations.

### 11. extractors::language_behavior
Per-language rules: what's a "function call", what's a "definition", import syntax variations. Drives the call_graph + symbol_table.

### 12. import_resolver
Resolution multi-language: package.json deps, Cargo.toml workspace, go.mod replace directives, Python requirements. Bridge entre symbol references e file boundaries.

### 13. patterns
Tree-sitter query patterns reutilizaveis (`(function_definition name: (identifier) @name)` etc.) compartilhadas entre extractors.

### 14. symbol_table
`SymbolTable { by_file, by_qualified_name }`. `Symbol { id, name, kind, scope, range, file_id }`. Per-file scope resolution.

### 15. tree_sitter
Thin wrapper sobre `tree_sitter::Parser` + language registration (16 langs). Manages parser lifetime + grammar loading.

### 16. types
`Symbol`, `Range`, `Scope`, `SymbolKind::{Function, Method, Class, Struct, Enum, Module, Interface, Variable, Constant, Field, Type}`. Re-exportados do crate.

### 17. workspace::detect
Detecta layout: monorepo (pnpm/Cargo workspace/Bazel/Nx), single-project, mixed. Drives o working-set inicial do agent.

**Validacao:**
- **469 tests** passando, 0 falhas
- `cargo clippy -p theo-engine-parser --lib --tests` silent (zero warnings em codigo proprio)
- ADR dep invariant preservada: theo-domain (workspace) + tree-sitter-* + sintatic helpers (external)
- 16 linguagens cobertas via extractors dedicated + generic fallback

Sem follow-ups bloqueadores. A cobertura ampla de testes (469) confirma a robustez das queries Tree-Sitter por linguagem.
