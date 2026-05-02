# Self-Evolution / Autodream — Pesquisa SOTA

## Escopo
Self-evolution loop com acceptance gate, narrow-then-expand strategy, ablation measurability, harness pruning, trace analysis, cross-model transfer.

## Crates alvo
- `theo-agent-runtime` — evolution loop, acceptance gate, trace analysis

## Referências-chave
| Fonte | O que extrair |
|-------|---------------|
| Tsinghua ablation | +4.8 SWE-Bench, only consistently beneficial module |
| Stanford Meta-Harness | Auto-optimize harness, 76.4% Terminal Bench 2, Rank 1 Haiku |
| Karpathy autoresearch | Keep/discard pattern for quality iteration |
| Anthropic harness evolution | Components expire as models improve — prune > build |
| Vercel | Removed 80% of tools → better results |
| Manus | Rewrote harness 5x in 6 months |

## Arquivos nesta pasta
- (pesquisas sobre autodream/self-evolution vão aqui)

## Gaps para pesquisar
- Acceptance gate implementation: binary pass/fail or scored threshold?
- Narrow-then-expand: how to measure "scope" concretely?
- Ablation framework: how to disable components and measure impact in Rust?
- Trace analysis: what trace format? (OpenTelemetry? custom JSONL?)
- Co-evolution: can harness improvements feed back into model fine-tuning?
