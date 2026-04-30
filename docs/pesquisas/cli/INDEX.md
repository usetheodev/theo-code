# CLI — Pesquisa SOTA

## Escopo
17 subcommands (init, agent, pilot, context, impact, stats, memory, login, logout, dashboard, subagent, checkpoints, agents, mcp, skill, trajectory, help). UX, ergonomia, output formatting.

## Crates alvo
- `apps/theo-cli` — CLI binary (pkg name: `theo`)

## Referências-chave
| Fonte | O que extrair |
|-------|---------------|
| opendev | TUI (Ratatui) + Web UI dual, snapshot manager, session export |
| opencode | 5 operation modes (interactive/print/JSON/RPC/SDK) |
| pi-mono | TUI com editor, session tree, 40+ commands, 5 operation modes |
| hermes-agent | TUI com prompt_toolkit + React TUI, session browsing FTS5 |
| cobra (Go) / click (Python) | CLI composition patterns, help generation |
| opensrc | CLI tool para source code access, pipes/redirection |

## Arquivos nesta pasta
- `cli-agent-ux-research.md` — CLI UX research

## Gaps para pesquisar
- Output modes: interactive vs JSON vs RPC — which to prioritize?
- Subcommand discoverability: how to surface 17 subcommands without overwhelm?
- Shell integration: completions, aliases, env vars
- Performance: 4.3ms startup (opendev target)
