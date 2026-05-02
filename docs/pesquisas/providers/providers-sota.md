# LLM Provider Abstraction — SOTA Research for AI Coding Agents

**Date:** 2026-04-29
**Domain:** Providers
**Target:** Raise score from 1.0 to 4.0
**Status:** Research complete

---

## Executive Summary

LLM provider abstraction is a solved problem in architecture but an unsolved problem in practice. Three reference implementations define the space: OpenDev (lazy client initialization, 5 model roles, provider cache with TTL), Hermes (200+ models, 11 messaging platforms, credential pool with OAuth refresh, HTTP 402 fallback chain), and pi-mono (25+ providers, defaultModelPerProvider, fuzzy model matching). The key challenges are streaming unification (SSE vs WebSocket vs stdio), retry strategies (exponential backoff vs circuit breaker), token counting accuracy (API-reported vs local estimates), and multi-protocol authentication (API key, OAuth PKCE, device flow, AWS STS). Theo Code has 26 provider specs but lacks streaming unification, retry strategies, and auth beyond API keys. This research provides the evidence base to close those gaps.

---

## 1. Reference Implementations

### 1.1 OpenDev: Lazy Client Initialization + 5 Model Roles

**Source:** arXiv:2603.05344 (March 2026)

OpenDev assigns five specialized model roles, each lazily initialized:

| Slot | Purpose | Typical Model | Lazy Init |
|------|---------|---------------|-----------|
| **Normal** (Execution) | Tool calls, code edits | Claude Opus | Yes -- only when first execution task |
| **Thinking** (Reasoning) | Planning, architecture decisions | GPT-o3 | Yes -- only when reasoning needed |
| **Compact** (Summarization) | Context compaction when window fills | Qwen lightweight | Yes -- only when compaction triggers |
| **Self-Critique** | Output verification | Same as Normal or cheaper | Yes -- only when critique enabled |
| **VLM** (Vision) | Screenshot analysis, diagram reading | Gemini Pro Vision | Yes -- only when image provided |

**Provider Cache:** Model capabilities (context length, vision support, reasoning features, tool calling format) are cached locally with TTL refresh (24h default, stale-while-revalidate pattern).

**Key Design Decisions:**
- Only models actually used are initialized, reducing startup latency
- Prompt caching: for providers supporting it (Anthropic), system prompts split into stable (cacheable) and dynamic parts
- Eager loading fails at scale -- load metadata indexes at startup, defer full content to point of use
- Self-healing indexes: if cache is corrupted, re-fetch from provider API

### 1.2 Hermes: 200+ Models, Credential Pool, Fallback Chains

**Source:** github.com/NousResearch/hermes-agent

Hermes is the most complete provider abstraction in open source:

| Feature | Detail |
|---------|--------|
| **Model count** | 200+ models via OpenAI-compatible API normalization |
| **Providers** | Nous Portal, OpenRouter, NVIDIA NIM, OpenAI, Anthropic, DeepSeek, Alibaba, HuggingFace, Google, Kimi, MiniMax, z.ai, self-hosted |
| **Credential Pool** | Multiple API keys per provider, automatic rotation on rate limits/failures |
| **Fallback Chains** | Ordered provider:model pairs. If primary fails, automatic switch mid-session without losing conversation |
| **OAuth Refresh** | Handles token refresh for OAuth-based providers |
| **Per-Provider Timeout** | `providers.<id>.request_timeout_seconds` + model-specific override |
| **Transport** | LiteLLM proxy for unified interface, vLLM for self-hosted serving |

**Fallback Chain Configuration (config.yaml):**
```yaml
model:
  provider: anthropic
  model: claude-sonnet-4-20250514
  fallback_providers:
    - provider: openai
      model: gpt-4o
    - provider: deepseek
      model: deepseek-chat
    - provider: local
      model: qwen3-32b
```

**Credential Pool:**
```yaml
providers:
  openai:
    api_keys:
      - sk-key-1
      - sk-key-2
      - sk-key-3
    # Automatic rotation on 429 or failure
```

### 1.3 pi-mono: Fuzzy Model Matching + Scope Concept

**Source:** github.com/badlogic/pi-mono

pi-mono provides 25+ providers with several unique features:

| Feature | Detail |
|---------|--------|
| **defaultModelPerProvider** | Each provider has a default model in `model-resolver.ts` |
| **Fuzzy model matching** | `/model <ref>` resolves canonical `provider/model` references even when model IDs contain `/` (e.g., LM Studio `unsloth/qwen3.5-35b-a3b`) |
| **Searchable provider login** | `/login` selector supports fuzzy search for quick provider selection |
| **Auth source labels** | Shows whether auth comes from `--api-key`, env var, or custom provider fallback |
| **Custom provider config** | `baseUrl`, `api`, `apiKey`, `supportsEagerToolInputStreaming`, `supportsLongCacheRetention` |
| **Unicode normalization** | Edit tool fuzzy matching normalizes Unicode compatibility variants (CJK, full-width) |

---

## 2. Streaming Unification

### 2.1 Protocol Landscape

| Protocol | Direction | Used By | Agent Context |
|---------|-----------|---------|---------------|
| **SSE (Server-Sent Events)** | Server -> Client (one-way) | OpenAI, Anthropic, Google Gemini, all major LLM APIs | Cloud API streaming |
| **WebSocket** | Bidirectional | Real-time collaborative tools, some voice APIs | Rarely needed for LLM streaming |
| **stdio** | Bidirectional (pipes) | MCP local tools, DAP adapters | Local tool/adapter communication |

### 2.2 SSE as the De Facto Standard

SSE is the dominant protocol for LLM streaming in 2025-2026. OpenAI, Anthropic, and Google all use SSE for their streaming APIs. The conventional wisdom has inverted from "use WebSocket unless you can't" to "start with SSE unless you specifically need bidirectional communication."

HTTP/2 multiplexing killed the 6-connection-per-origin browser limit that made SSE impractical under HTTP/1.1. LLM token streaming has since made SSE the default transport.

### 2.3 Provider-Specific Streaming Differences

| Provider | Stream Event Format | Tool Call Streaming | Thinking Tokens |
|---------|-------------------|-------------------|-----------------|
| **Anthropic** | `content_block_delta`, `content_block_start/stop` | `tool_use` blocks with incremental `input` JSON | `thinking` content blocks |
| **OpenAI** | `chat.completion.chunk` with `delta` | `function` or `tool_calls` with incremental args | Not applicable (reasoning via system prompt) |
| **Google Gemini** | `generateContent` stream with `candidates[]` | `functionCall` in `parts[]` | Not applicable |
| **Local (Ollama/vLLM)** | OpenAI-compatible SSE | Varies by model | Not applicable |

### 2.4 Streaming Abstraction Layer

```rust
// Unified stream event types for Theo Code
enum StreamEvent {
    TextDelta { text: String },
    ToolCallStart { id: String, name: String },
    ToolCallDelta { id: String, input_json: String },
    ToolCallEnd { id: String },
    ThinkingDelta { text: String },  // Anthropic-specific
    UsageUpdate { input_tokens: u64, output_tokens: u64 },
    Error { message: String, retryable: bool },
    Done,
}
```

Each provider adapter translates its native stream format into this unified enum. The agent loop consumes only `StreamEvent`, never raw provider responses.

---

## 3. Retry Strategies

### 3.1 The Problem Space

LLM APIs present unique retry challenges:
- **Rate limits (429)** fluctuate based on provider cluster load. An OpenAI 429 can hit mid-conversation.
- **Latency variance** is extreme: same prompt can take 800ms or 15s depending on queue depth.
- **Silent quality degradation**: during peak load, some providers route to smaller quantized models. API returns 200 OK but quality drops.
- **Partial responses**: stream may cut off mid-token due to timeout or provider error.

### 3.2 Strategy Comparison

| Strategy | Best For | Implementation | Drawback |
|---------|---------|---------------|----------|
| **Exponential backoff** | Transient errors (429, 503) | Wait 1s, 2s, 4s, 8s... with jitter | Slow recovery for sustained outages |
| **Circuit breaker** | Sustained provider outages | Track failure rate, trip at threshold, half-open probe | Requires fallback provider |
| **Hedged requests** | Latency-sensitive operations | Send to 2 providers, use first response | 2x cost |
| **Token bucket** | Proactive rate limiting | Track tokens/requests per minute, pre-throttle | Requires accurate rate limit knowledge |

### 3.3 Recommended Architecture

```
Request Flow:
  Agent -> TokenBucket (pre-throttle) -> Provider Client -> Response
                                              |
                                         [On Error]
                                              |
                              ┌───────────────┼───────────────┐
                              │               │               │
                         [429/503]       [401/403]       [500/502]
                              │               │               │
                    Exponential        Credential         Circuit
                    Backoff +          Rotation           Breaker ->
                    Jitter             (next key)         Fallback
                              │               │           Provider
                              │               │               │
                         [Max retries]   [All keys         [All
                              │          exhausted]     providers
                              │               │          down]
                              v               v               v
                         Return Error    Return Error    Return Error
```

### 3.4 Circuit Breaker Specifics

Netflix Hystrix data shows circuit breakers reduce cascade failures by 94% in microservice architectures. For LLM APIs:

| Parameter | Value | Rationale |
|----------|-------|-----------|
| Failure threshold | 5 failures in 60s | Trip the circuit |
| Half-open probe | 1 request every 30s | Test if provider recovered |
| Success threshold | 3 consecutive successes | Close the circuit |
| Timeout per request | 30s (configurable per model) | Prevent hung connections |

### 3.5 Bifrost Pattern

Bifrost implements automatic failover with zero application downtime: ordered fallback lists, attempt each provider sequentially after retry exhaustion on primary. Fallbacks trigger ONLY after retry exhaustion, not on first failure.

---

## 4. Token Counting

### 4.1 API-Reported vs Local Estimates

| Approach | Accuracy | Latency | Cost |
|---------|---------|---------|------|
| **API-reported `prompt_tokens`** | 100% accurate | Available after response | Free (included in response) |
| **Local tiktoken/tokenizer** | 95-99% for known models | Instant, pre-request | Free (local computation) |
| **Character-based estimate** | 70-80% accuracy | Instant | Free |

### 4.2 OpenDev Lesson

OpenDev's paper emphasizes: **always use API-reported token counts for billing and context management.** Local estimates are acceptable for pre-flight checks ("will this fit in context?") but must never be used for cost tracking or compaction trigger decisions.

### 4.3 Practical Approach

1. **Pre-flight:** Local tokenizer estimate to check "will this fit?" before sending
2. **Post-response:** Use API-reported `usage.prompt_tokens` and `usage.completion_tokens` for:
   - Cost tracking
   - Context window management
   - Compaction trigger decisions
3. **Fallback:** If API does not report usage (some local providers), use local estimate with 1.1x safety margin

---

## 5. Authentication Patterns

### 5.1 Auth Method Matrix

| Auth Method | Providers | Use Case | Complexity |
|------------|----------|----------|------------|
| **API Key** | OpenAI, Anthropic, DeepSeek, most cloud providers | Simplest, most common | Low |
| **OAuth 2.1 + PKCE** | Anthropic (new), GitHub Copilot, enterprise providers | Headless agents, no client secret | High |
| **Device Flow** | GitHub, Azure AD | CLI agents without browser | Medium |
| **AWS STS (AssumeRole)** | Amazon Bedrock | Temporary credentials, fine-grained IAM | High |
| **GCP Service Account** | Google Vertex AI | Service-to-service auth | Medium |
| **Azure Managed Identity** | Azure OpenAI | No credentials in code | Medium |

### 5.2 OAuth 2.1 + PKCE (Emerging Standard)

OAuth 2.1 is the emerging standard for agentic auth, especially via MCP:

- **PKCE is mandatory** (no implicit grant, no client secret for public clients)
- **Dynamic Client Registration (DCR)** allows agents to register with providers at runtime
- **Metadata Discovery** enables auto-detection of auth endpoints
- Critical for headless agents that cannot securely store a client secret

### 5.3 Credential Security

| Risk | Mitigation |
|------|-----------|
| API keys in environment variables | Support keychain/secret manager integration |
| Keys leaked via prompt injection | Brokered credentials pattern: LLM never sees the key |
| Keys in git history | Pre-commit hooks, secret scanning |
| Long-lived tokens | Short-lived tokens via STS/OAuth refresh |
| Single key rate limited | Credential pool with rotation (Hermes pattern) |

**State of Secrets Sprawl 2026:** 28.65 million hardcoded secrets added to public GitHub in 2025 (34% YoY increase). Secret leak rates in AI-assisted code ran roughly double the baseline.

### 5.4 Brokered Credentials Pattern

The LLM never sees the API key or token. A secure service makes the API call on the agent's behalf:

```
Agent -> "Call OpenAI with this prompt" -> Credential Broker -> OpenAI API
                                              ^
                                              |
                                        Vault/Keychain
                                        (keys stored here,
                                         never in agent context)
```

---

## 6. Provider-Specific Capabilities

### 6.1 Capability Matrix

| Capability | Anthropic | OpenAI | Google | DeepSeek | Local |
|-----------|-----------|--------|--------|----------|-------|
| **Tool calling** | `tool_use` content blocks | `function` / `tool_calls` | `functionCall` in parts | OpenAI-compatible | Varies |
| **Streaming** | SSE with content blocks | SSE with delta chunks | SSE with candidates | SSE (OpenAI format) | SSE (OpenAI format) |
| **Prompt caching** | Yes (cache_control) | No | Yes (context caching) | No | No |
| **Extended thinking** | Yes (thinking blocks) | No (use system prompt) | Yes (thinking mode) | Yes (DeepThink) | No |
| **Vision** | Yes (image content blocks) | Yes (image_url) | Yes (inline_data) | No | Varies |
| **JSON mode** | Yes (tool_use or text) | Yes (response_format) | Yes (response_mime_type) | Yes | Varies |
| **Max output tokens** | 64K (Opus), 16K (Sonnet) | 16K-128K | 8K-65K | 64K | Varies |
| **Batch API** | Yes | Yes | No | No | No |

### 6.2 Tool Calling Format Differences

**Anthropic:**
```json
{
  "type": "tool_use",
  "id": "toolu_01A",
  "name": "read_file",
  "input": {"path": "/src/main.rs"}
}
```

**OpenAI:**
```json
{
  "tool_calls": [{
    "id": "call_abc",
    "type": "function",
    "function": {
      "name": "read_file",
      "arguments": "{\"path\":\"/src/main.rs\"}"
    }
  }]
}
```

**Key difference:** Anthropic sends `input` as a parsed JSON object. OpenAI sends `arguments` as a JSON string that must be parsed. The provider abstraction must normalize this.

### 6.3 Extended Thinking Normalization

Only some providers support extended thinking. The abstraction must:
1. Check if the model supports thinking (capability flag)
2. If supported, enable thinking and include thinking tokens in cost tracking
3. If not supported, omit thinking parameters silently
4. Normalize thinking output: Anthropic sends `thinking` content blocks, others may include reasoning in regular text

---

## 7. Provider Count and Coverage

### 7.1 Target Provider List (26+)

| # | Provider | API Format | Auth | Priority |
|---|---------|-----------|------|----------|
| 1 | **Anthropic** | Native | API Key, OAuth PKCE | P0 |
| 2 | **OpenAI** | Native | API Key | P0 |
| 3 | **Google Vertex AI** | Native | GCP Service Account | P0 |
| 4 | **Google AI Studio** | Native | API Key | P0 |
| 5 | **Amazon Bedrock** | AWS SDK | AWS STS | P1 |
| 6 | **Azure OpenAI** | OpenAI-compatible | Azure AD / API Key | P1 |
| 7 | **DeepSeek** | OpenAI-compatible | API Key | P1 |
| 8 | **Mistral** | OpenAI-compatible | API Key | P1 |
| 9 | **Groq** | OpenAI-compatible | API Key | P1 |
| 10 | **Together AI** | OpenAI-compatible | API Key | P1 |
| 11 | **Fireworks AI** | OpenAI-compatible | API Key | P2 |
| 12 | **Cerebras** | OpenAI-compatible | API Key | P2 |
| 13 | **SambaNova** | OpenAI-compatible | API Key | P2 |
| 14 | **OpenRouter** | OpenAI-compatible | API Key | P1 |
| 15 | **Nous Portal** | OpenAI-compatible | API Key | P2 |
| 16 | **Ollama** | OpenAI-compatible | None (local) | P1 |
| 17 | **vLLM** | OpenAI-compatible | None (local) | P2 |
| 18 | **LM Studio** | OpenAI-compatible | None (local) | P2 |
| 19 | **llama.cpp** | OpenAI-compatible | None (local) | P2 |
| 20 | **NVIDIA NIM** | OpenAI-compatible | API Key | P2 |
| 21 | **Cohere** | Native | API Key | P2 |
| 22 | **AI21** | Native | API Key | P3 |
| 23 | **Alibaba (Qwen)** | OpenAI-compatible | API Key | P2 |
| 24 | **GitHub Copilot** | Native | Device Flow / OAuth | P2 |
| 25 | **Hugging Face** | OpenAI-compatible / TGI | API Key | P2 |
| 26 | **xAI (Grok)** | OpenAI-compatible | API Key | P2 |

**Note:** Most providers (16 of 26) are OpenAI-compatible, meaning a single OpenAI-compatible client handles them with different `base_url` and `api_key`. Only Anthropic, Google, AWS Bedrock, Cohere, and AI21 require native API implementations.

---

## 8. Thresholds for SOTA Level

### 8.1 Basic Provider Support (Score 2.0 -> 3.0)

| Threshold | Target | Metric |
|----------|--------|--------|
| Providers with working chat completion | >= 10 (P0 + P1) | Count |
| Streaming unification (unified StreamEvent) | All providers produce StreamEvent | E2E test per provider |
| API-reported token counting | Used for all cost tracking | Binary |
| Exponential backoff with jitter | Implemented for all providers | Binary |
| API key auth | Working for all API-key providers | Count |

### 8.2 Production Provider Support (Score 3.0 -> 4.0)

| Threshold | Target | Metric |
|----------|--------|--------|
| Providers with working chat completion | >= 20 | Count |
| Circuit breaker per provider | Implemented with configurable thresholds | Binary |
| Fallback chains | >= 2 fallback providers configurable | E2E test |
| Credential pool (multiple keys per provider) | Implemented with rotation | Binary |
| OAuth PKCE auth | Working for Anthropic | E2E test |
| AWS STS auth | Working for Bedrock | E2E test |
| Provider capability cache | TTL-based with stale-while-revalidate | Binary |
| Lazy client initialization | Only used providers initialized | Startup latency measurement |
| Auth success rate | >= 99% for configured providers | Metric |
| Streaming reliability | >= 99.5% of streams complete without error | Metric |

### 8.3 Advanced Provider Support (Score 4.0 -> 5.0)

| Threshold | Target | Metric |
|----------|--------|--------|
| Providers with working chat completion | >= 26 | Count |
| Prompt caching | Working for Anthropic + Google | E2E test |
| Extended thinking normalization | Working for all thinking-capable models | E2E test |
| Hedged requests for latency-sensitive operations | Optional config | E2E test |
| Brokered credentials (keychain integration) | Working for macOS Keychain + Linux secret-tool | E2E test |
| Provider health dashboard | Real-time circuit breaker status | Feature |
| Model capability auto-discovery | Query provider API for supported features | E2E test |

---

## 9. Relevance for Theo Code

### 9.1 Immediate Actions

1. **Implement unified StreamEvent enum:** Normalize all provider stream formats into a single Rust enum. This is the most impactful change -- the agent loop should never see raw provider responses.
2. **Add exponential backoff + circuit breaker:** Per-provider retry with jitter. Circuit breaker trips after 5 failures in 60s, half-open probe every 30s.
3. **Implement fallback chains:** Config-driven ordered list of provider:model pairs. Switch on retry exhaustion.
4. **Use API-reported token counts:** Stop using local estimates for cost tracking. Use local estimates only for pre-flight context checks.
5. **Lazy client initialization:** Only initialize provider clients when the model slot is first used.

### 9.2 Architecture Decision

```
theo-providers crate
  |-- ProviderRegistry
  |     |-- register_provider(config) -> ProviderHandle (lazy)
  |     |-- get_client(slot: ModelSlot) -> &dyn LlmClient (initializes on first call)
  |
  |-- LlmClient trait
  |     |-- chat_completion(messages, tools) -> StreamEvent stream
  |     |-- capabilities() -> ProviderCapabilities (cached)
  |
  |-- Adapters
  |     |-- AnthropicAdapter: native API, tool_use blocks, thinking blocks, cache_control
  |     |-- OpenAiCompatibleAdapter: covers ~16 providers with different base_url
  |     |-- GoogleAdapter: native Vertex/AI Studio API
  |     |-- BedrockAdapter: AWS SDK with STS
  |     |-- CohereAdapter: native API
  |
  |-- Resilience
  |     |-- RetryPolicy: exponential backoff + jitter, configurable per provider
  |     |-- CircuitBreaker: per-provider, configurable thresholds
  |     |-- FallbackChain: ordered provider:model list
  |     |-- CredentialPool: multiple keys per provider, rotation on 429/401
  |
  |-- Auth
  |     |-- ApiKeyAuth: env var or config file
  |     |-- OAuthPkceAuth: Anthropic, enterprise providers
  |     |-- AwsStsAuth: Bedrock
  |     |-- DeviceFlowAuth: GitHub Copilot
  |
  |-- TokenCounter
  |     |-- estimate_tokens(text) -> u64 (pre-flight, local)
  |     |-- record_usage(api_response) -> Usage (post-response, authoritative)
  |
  |-- CapabilityCache
  |     |-- get(provider, model) -> Capabilities (TTL 24h, stale-while-revalidate)
```

### 9.3 Key Trade-off: OpenAI-Compatible vs Native

16 of 26 target providers use the OpenAI-compatible API format. Building a robust `OpenAiCompatibleAdapter` with configurable `base_url` covers the majority of providers. Only 5 providers need native adapters (Anthropic, Google, Bedrock, Cohere, AI21). This is the "don't reinvent the wheel" principle applied to provider integration.

### 9.4 Why This Matters

Provider abstraction is foundational infrastructure. Every other feature (agent loop, subagents, model routing, wiki compilation) depends on reliable, cost-effective LLM access. Without fallback chains, a single provider outage kills the entire agent. Without streaming unification, every feature that consumes LLM output must handle provider-specific formats. Without proper token counting, cost tracking is inaccurate and context management unreliable.

---

## Sources

- [OpenDev Paper: Building AI Coding Agents for the Terminal](https://arxiv.org/html/2603.05344v1)
- [OpenDev GitHub](https://github.com/opendev-to/opendev)
- [Hermes Agent Documentation](https://hermes-agent.nousresearch.com/docs/)
- [Hermes Fallback Providers](https://hermes-agent.nousresearch.com/docs/user-guide/features/fallback-providers)
- [Hermes AI Providers](https://hermes-agent.nousresearch.com/docs/integrations/providers)
- [Hermes GitHub](https://github.com/nousresearch/hermes-agent)
- [pi-mono GitHub](https://github.com/badlogic/pi-mono)
- [pi-mono Providers Documentation](https://github.com/badlogic/pi-mono/blob/main/packages/coding-agent/docs/providers.md)
- [pi-mono Models Documentation](https://github.com/badlogic/pi-mono/blob/main/packages/coding-agent/docs/models.md)
- [Streaming LLM Responses: SSE to Real-Time UI](https://dev.to/pockit_tools/the-complete-guide-to-streaming-llm-responses-in-web-applications-from-sse-to-real-time-ui-3534)
- [SSE vs WebSocket for LLM Streaming](https://www.hivenet.com/post/llm-streaming-sse-websockets)
- [Retries, Fallbacks, and Circuit Breakers in LLM Apps](https://www.getmaxim.ai/articles/retries-fallbacks-and-circuit-breakers-in-llm-apps-a-production-guide/)
- [Circuit Breakers for LLM APIs (SRE Patterns)](https://explore.n1n.ai/blog/circuit-breakers-llm-api-sre-reliability-patterns-2026-02-15)
- [OmniRoute: AI Gateway for Multi-Provider LLMs](https://github.com/diegosouzapw/OmniRoute)
- [MCP, OAuth 2.1, PKCE for AI Authorization](https://aembit.io/blog/mcp-oauth-2-1-pkce-and-the-future-of-ai-authorization/)
- [Securing AI Agents Without Static Credentials](https://aembit.io/blog/securing-ai-agents-without-secrets/)
- [AI Agent Authentication (GitGuardian)](https://blog.gitguardian.com/ai-agents-authentication-how-autonomous-systems-prove-identity/)
- [AWS MCP Security Patterns](https://aws.amazon.com/blogs/security/secure-ai-agent-access-patterns-to-aws-resources-using-model-context-protocol/)
- [AI Agent Auth Methods (Stytch)](https://stytch.com/blog/ai-agent-authentication-methods/)
