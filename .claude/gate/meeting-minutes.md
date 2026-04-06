# Meeting — 2026-04-06 (SCIP Integration — Exact Cross-File References)

## Proposta
Integrar SCIP (Source Code Intelligence Protocol) para referências exatas cross-file via rust-analyzer. Dual-index: Tree-Sitter (always) + SCIP (when available).

## Participantes
- **governance** — APPROVE (88%). Feature gate obrigatório. Merge layer separado.
- **graphctx** — APPROVE. Design: ~170 linhas aditivas. ScipIndex retorna FileData (compatível). SCIP Occurrence/Relationship → EdgeTypes mapeados.
- **qa** — APPROVE. 76 testes intactos via feature gate. MockCodeIntelProvider + fixture .scip obrigatórios.

## Análises

### Governance
- `scip` como optional dependency: `scip = { version = "0.7", optional = true }`
- Feature gate: `#[cfg(feature = "scip")]` em todos os módulos SCIP
- CodeIntelProvider trait em theo-domain (DIP correto)
- Merge layer em módulo separado, NÃO em bridge.rs (SRP)
- protobuf transitivo aceitável com feature gate

### GraphCtx
- ScipIndex: HashMap<symbol_string, node_id> + HashMap<file_path, Vec<Occurrence>>
- resolve_symbol_scip(): O(1) lookup canônico vs heurística O(N)
- Mapeamento: Definition→Contains, Reference+callable→Calls, is_implementation→Inherits
- ~170 linhas novas, zero alteração no path existente
- Compatível: scip 0.7 requer edition 2021, nós temos 2024

### QA
- 76/76 testes intactos (feature gate isola)
- MockCodeIntelProvider obrigatório
- Fixture .scip minimal (3 docs, 10 symbols) commitada
- eval_suite: modo --features scip para comparar P@5

## Conflitos
1. SCIP Occurrence não distingue call vs read-reference → resolver via HashMap<symbol, SymbolKind>
2. rust-analyzer pode não estar instalado → detectar gracefully, fallback Tree-Sitter

## Veredito
**APPROVED**

## Escopo Aprovado

### theo-domain
- `crates/theo-domain/src/code_intel.rs` (novo — trait CodeIntelProvider)
- `crates/theo-domain/src/lib.rs` (registrar módulo)

### theo-engine-graph
- `crates/theo-engine-graph/src/scip/mod.rs` (novo)
- `crates/theo-engine-graph/src/scip/reader.rs` (novo — lê index.scip)
- `crates/theo-engine-graph/src/scip/adapter.rs` (novo — impl CodeIntelProvider)
- `crates/theo-engine-graph/src/scip/indexer.rs` (novo — invoca rust-analyzer)
- `crates/theo-engine-graph/src/scip/merge.rs` (novo — merge com Tree-Sitter edges)
- `crates/theo-engine-graph/src/lib.rs` (registrar módulo)
- `crates/theo-engine-graph/Cargo.toml` (scip optional dependency)
- `crates/theo-engine-graph/tests/fixtures/minimal.scip` (novo — test fixture)

### Workspace
- `Cargo.toml` (scip = "0.7" em workspace.dependencies)

## Condições
1. `scip` como optional dependency com feature gate `scip`
2. `cargo test` (sem feature) DEVE continuar passando 76/76
3. `cargo test --features scip` DEVE passar com testes adicionais
4. MockCodeIntelProvider para testes unitários
5. Fixture .scip commitada (não gerada em runtime)
6. Campos SCIP em structs existentes são `Option<>` (zero breaking change)
7. Merge layer em scip/merge.rs, NÃO em bridge.rs
8. Detectar ausência de rust-analyzer gracefully (log warning, fallback)
9. Build + eval na vast.ai (NÃO na máquina local do desenvolvedor)
