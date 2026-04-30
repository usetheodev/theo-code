# Security & Governance — Pesquisa SOTA

## Escopo
Sandbox (bwrap/landlock), tool permissions, memory injection scan, context fence, credential protection, dangerous command detection, policy engine, governance rules.

## Crates alvo
- `theo-governance` — policy engine, sandbox cascade, permission rules
- `theo-isolation` — bwrap, landlock, noop fallback

## Referências-chave
| Fonte | O que extrair |
|-------|---------------|
| opendev | ApprovalRulesManager, pattern-based command rules, permission modes |
| hermes-agent | Dangerous command detection + approval, SSRF protection |
| hermes memory_tool.py:65-103 | Injection scan patterns (15+ regex) |
| Archon | Git worktree isolation, port auto-allocation |
| rippletide | Rule-based governance via hooks, OTP email auth |
| arXiv:2604.14228 (Claude Code) | 7 safety layers, deny-first, defense in depth |
| opencode | Granular permission (ask/allow/deny), .env protection |
| superpowers | Mandatory workflows as guardrails |

## Arquivos nesta pasta
- (pesquisas sobre security/governance vão aqui)

## Gaps para pesquisar
- Injection scan: complete pattern list for code agent context
- Landlock vs bwrap: performance comparison for tool execution
- Governance rules: how to make configurable per-project?
- Shared skills vulnerability: 1 in 4 community skills has a vuln (harness-engineering-guide.md)
- Supply chain: MCP server security scanning
