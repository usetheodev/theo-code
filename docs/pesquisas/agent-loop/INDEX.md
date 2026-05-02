# Agent Loop — Pesquisa SOTA

## Escopo
Ciclo ReAct (Plan→Act→Observe→Reflect), doom loop detection, convergence, compaction, self-evolution loop, tool parallelization, session continuity, failure classification.

## Crates alvo
- `theo-agent-runtime` — agent_loop.rs, run_engine.rs, compaction_stages.rs, reflector.rs

## Referências-chave
| Fonte | O que extrair |
|-------|---------------|
| Tsinghua ablation | Self-evolution +4.8, verifiers -0.8, multi-candidate -2.4 |
| Stanford Meta-Harness | 76.4% Terminal Bench 2, cross-model transfer |
| Anthropic harness | Planner→Generator→Evaluator, premature completion fix |
| opendev ReactLoop | Doom-loop detection, 5 workflow slots, nudging on failure |
| hermes-agent AIAgent | 90 iterations max, smart timeouts, parallel-safe tools |
| Archon | DAG executor, node types (prompt/command/bash/loop/approval) |
| GSD (get-shit-done) | Wave-based parallelization, 24+ agents, context engineering |
| superpowers | Skill-based auto-triggering, mandatory TDD enforcement |

## Arquivos nesta pasta
- `harness-engineering-guide.md` — Tsinghua ablation + Stanford meta-harness
- `harness-engineering.md` — Harness engineering overview
- `harness-engineering-openai.md` — OpenAI harness patterns
- `effective-harnesses-for-long-running-agents.md` — Anthropic long-running agents

## Gaps para pesquisar
- Doom loop detection: qual threshold ótimo? (opendev usa contagem de repeats)
- Compaction quality: como medir perda de informação pós-compaction?
- Session continuity: benchmark de taxa de re-trabalho entre sessões
- Self-evolution: implementação concreta do acceptance gate em Rust
