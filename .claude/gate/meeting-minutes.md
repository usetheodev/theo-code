# Meeting — 2026-04-06 (SCIP Wiring — eval + P@5 measurement)

## Proposta
Conectar SCIP index ao eval para medir impacto no P@5. merge_scip_edges() antes do assembly.

## Participantes
- Facilitador (fast-track — extensão natural do SCIP aprovado)

## Veredito
**APPROVED**

## Escopo Aprovado
- `crates/theo-engine-retrieval/tests/eval_suite.rs` (#[cfg(feature = "scip")] wiring)
- `crates/theo-engine-retrieval/Cargo.toml` (theo-engine-graph com feature scip em dev-deps)

## Condições
1. Condicional: #[cfg(feature = "scip")] — eval sem feature continua idêntico
2. Se index.scip não existe, skip silenciosamente
3. Testar na vast.ai com index.scip gerado
