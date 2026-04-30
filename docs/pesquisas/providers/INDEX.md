# Providers — Pesquisa SOTA

## Escopo
26 LLM provider specs organizados em 5 tiers: Tier 1 (OpenAI-compatible, 12), Tier 2 (non-OpenAI, 1), Tier 3 (cloud special auth, 7), Tier 4 (complex cloud auth, 3), Tier 5 (local models, 3).

## Crates alvo
- `theo-infra-llm` — provider catalog, streaming, retry, converter pipeline
- `theo-infra-auth` — OAuth PKCE, device flow, env keys

## Referências-chave
| Fonte | O que extrair |
|-------|---------------|
| opendev | Provider abstraction, CostTracker per-provider |
| opencode | 15+ LLM providers, provider switching mid-session |
| pi-mono | 25+ providers, defaultModelPerProvider, fuzzy matching |
| hermes-agent | 200+ models, 11 messaging platforms, credential pool |
| Archon | Per-node model/provider, validation at workflow-load |

## Arquivos nesta pasta
- (pesquisas sobre providers vão aqui)

## Gaps para pesquisar
- Streaming: unified interface across OpenAI/Anthropic/local differences
- Retry: exponential backoff vs circuit breaker per provider
- Token counting: pre-request estimation accuracy
- Auth: OAuth refresh flow for long-running sessions
- Local models: vLLM + Ollama integration quality
