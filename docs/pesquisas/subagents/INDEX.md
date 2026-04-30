# Sub-agents — Pesquisa SOTA

## Escopo
Orchestrator-worker pattern, role specialization (Explorer/Implementer/Verifier/Reviewer), context isolation, depth policy, delegation efficiency, parallel subagents, file locking, shared task lists.

## Crates alvo
- `theo-agent-runtime` — subagent/mod.rs, SubAgentRole, SubAgentManager

## Referências-chave
| Fonte | O que extrair |
|-------|---------------|
| Claude Code subagents | Markdown-defined, return-only, context isolation |
| Claude Code Agent Teams | Shared task list, peer messaging, file locks |
| arXiv:2604.14228 | 98.4% deterministic infrastructure, 7 safety layers |
| OpenAI Codex CLI | max_depth=1 default, MCP server mode |
| opendev | 5 workflow slots, parallel read-only tools, SubAgent templates |
| hermes-agent | Delegate tool, iteration budgets independentes |
| Archon | DAG executor, concurrent nodes per layer, approval nodes |
| GSD | 24+ specialized agents, wave-based parallel execution |
| superpowers | Subagent dispatch per task, two-stage code review |

## Arquivos nesta pasta
- `sota-subagent-architectures.md` — Full research report (moved copy)

## Gaps para pesquisar
- File locking: how to prevent two subagents writing same file?
- Shared task list: implementation in Rust (actor model? shared state?)
- Peer messaging: message bus vs shared file vs channel?
- Swarm limit: evidence for 4-5 agent sweet spot
- Recursive depth: cost/benefit analysis of depth > 1
