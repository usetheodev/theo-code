# Observability — Pesquisa SOTA

## Escopo
Cost tracking (tokens + USD), structured logging, trajectory export, dashboard, token usage breakdown, performance metrics (p50/p95 per tool).

## Crates alvo
- `theo-agent-runtime` — cost tracking, trajectory, tracing
- `theo-application` — dashboard HTTP server, metrics aggregation

## Referências-chave
| Fonte | O que extrair |
|-------|---------------|
| opendev CostTracker | Per-session tokens + cost per provider, models.dev API for pricing |
| Archon | Event emitter system, JSONL structured logging (Pino), web dashboard |
| hermes-agent | Trajectory saving for RL training, PostHog analytics |
| opencode | SQLite storage for history, server HTTP for TUI |
| awesome-harness-engineering | Langfuse, Braintrust, Logfire, Helicone, OpenTelemetry |

## Arquivos nesta pasta
- (pesquisas sobre observability vão aqui)

## Gaps para pesquisar
- OpenTelemetry integration for Rust agent loops
- Cost tracking accuracy: how to handle cached tokens?
- Trajectory format: OpenAI format? custom JSONL? RL-compatible?
- Dashboard: real-time vs poll? WebSocket vs SSE?
