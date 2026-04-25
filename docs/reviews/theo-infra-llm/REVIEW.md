# theo-infra-llm — Revisao

> **Contexto**: 25 provedores de LLM, com protocolo interno OpenAI-compatible. Providers convertem na fronteira.
>
> **Dependencias permitidas**: `theo-domain`.
>
> **Regra**: tudo OA-compat internamente; providers fazem transform in/out.
>
> **Status global**: deep-review concluido em 2026-04-25. 278 tests passando, 0 falhas. `cargo clippy -p theo-infra-llm --lib --tests` zero warnings em codigo proprio. Hygiene fixes aplicados nesta auditoria (vide secao final).

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `client` | `LlmClient` e `ApiKeyResolver` — cliente HTTP de alto nivel. | Revisado |
| 2 | `codex` | Integracao com OpenAI Codex (modo especial para completions de codigo). | Revisado |
| 3 | `error` | `LlmError` tipado. | Revisado |
| 4 | `hermes` | Adaptador Hermes (referencia interna). | Revisado |
| 5 | `mock` | Mock client para testes. | Revisado |
| 6 | `model_limits` | Limites por modelo (`DEFAULT_CONTEXT_WINDOW`, `remaining_budget`, `would_overflow`). | Revisado |
| 7 | `overflow` | `is_context_overflow` — deteccao de overflow de janela. | Revisado |
| 8 | `partial_json` | `parse_partial_json` — parser tolerante para JSON streaming incompleto. | Revisado |
| 9 | `provider::auth` | Auth hooks por provider (integracao com `theo-infra-auth`). | Revisado |
| 10 | `provider::catalog` | Catalogo de providers conhecidos e seus modelos. | Revisado |
| 11 | `provider::client` | Cliente base por provider. | Revisado |
| 12 | `provider::format` | Formatos request/response por provider. | Revisado |
| 13 | `provider::registry` | Registry de providers instanciados. | Revisado |
| 14 | `provider::spec` | Spec declarativa de cada provider. | Revisado |
| 15 | `providers::anthropic` | Implementacao Anthropic (Messages API). | Revisado |
| 16 | `providers::common` | Utilidades comuns entre providers. | Revisado |
| 17 | `providers::converter` | Conversao bidirecional OpenAI-compat ↔ formato do provider. | Revisado |
| 18 | `providers::openai` | Implementacao OpenAI (Chat Completions / Responses). | Revisado |
| 19 | `providers::openai_compatible` | Providers OpenAI-compatible (OpenRouter, Groq, Together etc). | Revisado |
| 20 | `routing::auto` | Roteamento automatico baseado em heuristica. | Revisado |
| 21 | `routing::cascade` | Cascade routing (tenta modelo barato primeiro, escala se falhar). | Revisado |
| 22 | `routing::complexity` | Estimativa de complexidade da query para roteamento. | Revisado |
| 23 | `routing::config` | Configuracao de rotas. | Revisado |
| 24 | `routing::keywords` | Roteamento por keyword matching. | Revisado |
| 25 | `routing::metrics` | Metricas de roteamento (hit rate, cost, latency). | Revisado |
| 26 | `routing::pricing` | Tabela de precos por modelo. | Revisado |
| 27 | `routing::rules` | Rule engine de roteamento. | Revisado |
| 28 | `stream` | `StreamDelta` — protocolo de streaming unificado. | Revisado |
| 29 | `transform` | Transformacoes de payload (normalizacao). | Revisado |
| 30 | `types` | Tipos publicos re-exportados (`types::*`). | Revisado |

---

## Notas de Deep-Review por Dominio

> Auditoria orientada a: (1) protocolo interno OA-compat preservado (regra do crate), (2) providers convertem APENAS na fronteira, (3) deps permitidas (theo-domain only), (4) cobertura de testes, (5) hygiene.

### 1. client (402 LOC)
`LlmClient { base_url, api_key, model, endpoint_override, extra_headers, http: reqwest::Client, api_key_resolver }`. Methods: `chat()` (sync), `chat_stream()` (SseStream), `chat_streaming(F: FnMut(&StreamDelta))`. Codex branch detection via `is_codex()` quando `endpoint_override.contains("codex")`. `ApiKeyResolver` trait para tokens dinamicos (Copilot OAuth refresh). Zero achados residuais.

### 2. codex (361 LOC)
`to_codex_body(request)` + `from_codex_stream(body)`. Parsing de SSE `response.reasoning.delta` / `response.output_text.delta` events. Cobertura via `theo_agent_runtime::tests::llm_mock_smoke`.

### 3. error (155 LOC)
`LlmError` enum: `Network(reqwest::Error)`, `Parse(String)`, `RateLimited{retry_after}`, `QuotaExceeded`, `ContextOverflow{provider, message}`, `AuthFailed`, `ServiceUnavailable`, `Timeout`, `Api{status, message}`, `ProviderNotFound`. `from_status(code, body)` detecta overflow ANTES da classificacao por status. `is_retryable()` filtra os 4 tipos retryables. Cobertura ampla via `from_status_*` tests.

### 4. hermes (55 LOC)
Adaptador Hermes interno. `parse_hermes_tool_calls(content)` extrai `<function=name>{...}</function>` XML embebido em content quando o modelo nao usa o tool_calls field nativo. Pure parser.

### 5. mock (276 LOC)
`MockLlmProvider` implements `LlmProvider` trait com VecDeque<Result<ChatResponse, LlmError>>. Convenience: `with_text_response`, `with_tool_call`, `with_error`, `remaining_responses()`. Default behavior: "Mock response (no more queued responses)" se queue vazio. NAO usado pelo agent-runtime tests (que usa HTTP mock direto via `llm_mock_smoke`), mas disponivel para testes que tocam o `LlmProvider` trait.

### 6. model_limits (91 LOC)
`DEFAULT_CONTEXT_WINDOW = 128_000`. `remaining_budget(used, total)`, `would_overflow(prompt_tokens, max_completion, model_window)`. Pure functions.

### 7. overflow (37 LOC)
`is_context_overflow(error_message)` — heuristic detection de mensagens de overflow. Patterns: "context_length_exceeded", "maximum context length", "exceeds the context window", "context window exceeds limit". Coberto por T0.1 cenario 9 (`agent_recovers_from_context_overflow_then_converges`).

### 8. partial_json (105 LOC)
Parser tolerante para JSON parcial em streaming. Inserts closing braces/quotes faltantes. Used quando tool_calls.arguments chega em pedacos antes do final do stream.

### 9. provider::auth (auth/ subdir)
Auth strategies per provider: `bearer.rs` (26 LOC, Authorization: Bearer token), `header.rs` (30 LOC, X-API-Key header), `mod.rs` (51 LOC, factory + trait). Pequenos e focados.

### 10. provider::catalog (catalog/ subdir)
Catalog de providers conhecidos: `openai.rs` (141 LOC, gpt-4o, gpt-4-turbo, etc.), `anthropic.rs` (16 LOC, claude-3.5/4 series), `cloud.rs` (127 LOC, OpenRouter/Groq/Together), `local.rs` (35 LOC, Ollama). Spec-driven.

### 11. provider::client (67 LOC)
Cliente base por provider que delega ao `LlmClient` mas com `ProviderSpec` injetada (auth + format + base_url).

### 12. provider::format (format/ subdir)
Formatadores per-provider: `passthrough.rs` (16 LOC, OA-compat noop), `codex.rs` (25 LOC), `anthropic.rs` (24 LOC). Conversao no boundary apenas.

### 13. provider::registry (143 LOC)
Registry de providers instanciados. `register(spec)`, `get(provider_id)`, `iter()`. Threadsafe via Arc.

### 14. provider::spec (80 LOC)
`ProviderSpec { id, base_url, auth, format, catalog }`. Declarativa.

### 15. providers::anthropic (896 LOC)
Maior modulo. Messages API: tool_use blocks, content blocks (text/tool_use/tool_result), system message inline (anthropic separa system do messages). Stream events: content_block_start/delta/stop, message_delta, message_stop.

### 16. providers::common (247 LOC)
Utilidades cross-provider: header injection, error mapping, common parsing helpers.

### 17. providers::converter (68 LOC)
Bidirecional OA-compat ↔ provider format. Define os trait methods para cada provider.

### 18. providers::openai (840 LOC)
OpenAI Chat Completions + Responses API. Gestao de `tool_calls`, `tool_call_id`, `function_call`. Incremental streaming via SSE com `delta.tool_calls[N].function.arguments` chunks acumulados via partial_json.

### 19. providers::openai_compatible (535 LOC)
OpenRouter, Groq, Together etc. Variations: model name normalization, custom headers, pricing differences. Reuses openai.rs core via converter.

### 20. routing::auto (124 LOC)
`AutomaticModelRouter`. Iter desta revisao: extracted `type RecorderFn = Arc<dyn Fn(...) + Send + Sync>` para silenciar clippy::type_complexity.

### 21. routing::cascade (83 LOC)
Cascade routing: tenta modelo cheap, escala para grande se falhar/overflow. RoutingFailureHint::ContextOverflow trigger.

### 22. routing::complexity (103 LOC)
Heuristic complexity scoring: token count, keyword presence, prior tool failures.

### 23. routing::config (83 LOC)
`RoutingConfig` carregado de `.theo/config.toml [routing]`. `enabled`, `strategy: rules|auto`, rule list, default model.

### 24. routing::keywords (104 LOC)
Keyword matching para `RuleBasedRouter`. `if query contains "summarize" → cheap_model`.

### 25. routing::metrics (195 LOC)
`RoutingMetricsRecorder` trait + `InMemoryRecorder`. Captures (route_decision, model, reason). Tested by `theo-application::router_loader::load_router_recorder_captures_decisions`.

### 26. routing::pricing (51 LOC)
`PricingTable` com per-model $ per 1M tokens (input/output). Drives auto routing cost-decisions.

### 27. routing::rules (172 LOC)
`RuleBasedRouter` impl + RuleSet evaluator. First-match wins.

### 28. stream (233 LOC)
`StreamDelta::{Content(String), Reasoning(String), ToolCallDelta{index, id, name, arguments}, Done}`. `SseStream` impl Stream<Item=Result<StreamDelta, LlmError>>. `parse_sse_line(line)` shared parser. `StreamCollector` accumula deltas em ChatResponse final.

### 29. transform (137 LOC)
Normalizacoes de payload entre providers. Extra-header injection, request body shaping.

### 30. types (252 LOC)
`Message`, `ChatRequest`, `ChatResponse`, `Choice`, `ToolCall`, `FunctionCall`, `ToolDefinition`, `Role`, `Usage`. Re-exportados em `types::*`. Iter desta revisao: doc list em `with_tool_choice` reformatado para clippy::doc_lazy_continuation (3 itens com `///   - X` ganharam blank line antes + indent regular).

---

## Conclusao

Todos os 30 dominios listados revisitados e marcados **Revisado**. Plus modulos auxiliares: `partial_json`, `transform`, `model_limits`, `overflow`, todos pequenos e focados.

**Hygiene fixes aplicados:**
- `types.rs:206-215` — doc list `with_tool_choice` reformatada (3 items + blank lines) para clippy::doc_lazy_continuation.
- `routing/auto.rs:263` — `Arc<dyn Fn(...) + Send + Sync>` extraido em `type RecorderFn` para clippy::type_complexity.

**Validacao:**
- 278 tests passando, 0 falhas
- `cargo clippy -p theo-infra-llm --lib --tests` zero warnings em codigo proprio
- Cargo.toml deps: apenas theo-domain (workspace) + reqwest/tokio/serde/serde_json/futures/async-trait/thiserror/bytes (external) — invariante preservada
- Maiores modulos: providers/anthropic 896, providers/openai 840, providers/openai_compatible 535 — todos legitimamente grandes pelos respectivos protocolos

Sem follow-ups bloqueadores. O protocolo interno OA-compat e preservado em todos os providers via converters; `partial_json` cobre o caso de tool_calls streaming incompleto que e particular de OpenAI/Anthropic.
