# theo-infra-llm — Revisao

> **Contexto**: 25 provedores de LLM, com protocolo interno OpenAI-compatible. Providers convertem na fronteira.
>
> **Dependencias permitidas**: `theo-domain`.
>
> **Regra**: tudo OA-compat internamente; providers fazem transform in/out.

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `client` | `LlmClient` e `ApiKeyResolver` — cliente HTTP de alto nivel. | Pendente |
| 2 | `codex` | Integracao com OpenAI Codex (modo especial para completions de codigo). | Pendente |
| 3 | `error` | `LlmError` tipado. | Pendente |
| 4 | `hermes` | Adaptador Hermes (referencia interna). | Pendente |
| 5 | `mock` | Mock client para testes. | Pendente |
| 6 | `model_limits` | Limites por modelo (`DEFAULT_CONTEXT_WINDOW`, `remaining_budget`, `would_overflow`). | Pendente |
| 7 | `overflow` | `is_context_overflow` — deteccao de overflow de janela. | Pendente |
| 8 | `partial_json` | `parse_partial_json` — parser tolerante para JSON streaming incompleto. | Pendente |
| 9 | `provider::auth` | Auth hooks por provider (integracao com `theo-infra-auth`). | Pendente |
| 10 | `provider::catalog` | Catalogo de providers conhecidos e seus modelos. | Pendente |
| 11 | `provider::client` | Cliente base por provider. | Pendente |
| 12 | `provider::format` | Formatos request/response por provider. | Pendente |
| 13 | `provider::registry` | Registry de providers instanciados. | Pendente |
| 14 | `provider::spec` | Spec declarativa de cada provider. | Pendente |
| 15 | `providers::anthropic` | Implementacao Anthropic (Messages API). | Pendente |
| 16 | `providers::common` | Utilidades comuns entre providers. | Pendente |
| 17 | `providers::converter` | Conversao bidirecional OpenAI-compat ↔ formato do provider. | Pendente |
| 18 | `providers::openai` | Implementacao OpenAI (Chat Completions / Responses). | Pendente |
| 19 | `providers::openai_compatible` | Providers OpenAI-compatible (OpenRouter, Groq, Together etc). | Pendente |
| 20 | `routing::auto` | Roteamento automatico baseado em heuristica. | Pendente |
| 21 | `routing::cascade` | Cascade routing (tenta modelo barato primeiro, escala se falhar). | Pendente |
| 22 | `routing::complexity` | Estimativa de complexidade da query para roteamento. | Pendente |
| 23 | `routing::config` | Configuracao de rotas. | Pendente |
| 24 | `routing::keywords` | Roteamento por keyword matching. | Pendente |
| 25 | `routing::metrics` | Metricas de roteamento (hit rate, cost, latency). | Pendente |
| 26 | `routing::pricing` | Tabela de precos por modelo. | Pendente |
| 27 | `routing::rules` | Rule engine de roteamento. | Pendente |
| 28 | `stream` | `StreamDelta` — protocolo de streaming unificado. | Pendente |
| 29 | `transform` | Transformacoes de payload (normalizacao). | Pendente |
| 30 | `types` | Tipos publicos re-exportados (`types::*`). | Pendente |
