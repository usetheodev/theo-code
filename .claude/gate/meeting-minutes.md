# Meeting — 2026-04-07 (Benchmark Profissional GRAPHCTX)

## Proposta
Sistema de benchmark profissional: metrics.rs (11 métricas IR), ground truth JSON com dependency annotations, benchmark runner com report agregado.

## Participantes
- Governance (Principal Engineer)
- QA (Staff QA Engineer)

## Análises
- Governance: APPROVE (95%). Zero impacto em produção, bounded context correto.
- QA: validated=false (condicional). Exige 15+ unit tests com hand-computed values para nDCG e MAP.

## Conflitos
1. QA quer ground truth schema validation com file existence check
2. Risco: deps anotadas incorretamente → falsos positivos. Mitigação: validar deps contra graph real.

## Veredito
**APPROVED**

## Escopo Aprovado
- Novo: `crates/theo-engine-retrieval/src/metrics.rs`
- Novo: `crates/theo-engine-retrieval/tests/benchmarks/ground_truth/theo-code.json`
- Novo: `crates/theo-engine-retrieval/tests/benchmark_suite.rs`
- Novo: `crates/theo-engine-retrieval/tests/test_metrics.rs`
- Mod: `crates/theo-engine-retrieval/src/lib.rs`

## Condições
1. Mínimo 15 unit tests para metrics.rs com valores hand-computed
2. nDCG e MAP validados contra cálculo manual (tolerância < 0.001)
3. Ground truth JSON com schema validation
4. Benchmark runner valida que files referenciados existem no repo
5. Sem unwrap() em metrics.rs — guarda para divisão por zero
6. Testes determinísticos
