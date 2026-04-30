# Languages — Pesquisa SOTA

## Escopo
Tree-Sitter parsing para 14 linguagens: C, C++, C#, Go, Java, JavaScript, Kotlin, PHP, Python, Ruby, Rust, Scala, Swift, TypeScript. Symbol extraction, AST analysis, import resolution.

## Crates alvo
- `theo-engine-parser` — Tree-Sitter grammars, symbol extraction, import resolution

## Referências-chave
| Fonte | O que extrair |
|-------|---------------|
| qmd | AST-aware chunking via tree-sitter |
| opendev | 14 language parsers (similar set) |
| fff.nvim | TreeSitter highlighting, preview integration |

## Arquivos nesta pasta
- (pesquisas sobre language parsing vão aqui)

## Gaps para pesquisar
- Per-language Recall@5 benchmark (target ≥ 0.85 each)
- New grammar: Zig, Elixir, Dart? (evaluate demand)
- Symbol extraction quality: functions, classes, imports, exports completeness
- Cross-language import resolution (e.g., Python calling C extensions)
