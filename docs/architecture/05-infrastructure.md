# 05 — Infrastructure (`theo-infra-*`)

Concrete implementations behind domain traits. Two crates: LLM client and authentication.

---

## theo-infra-llm

### Purpose
HTTP client layer for all LLM providers. Everything is **OA-compatible internally** — providers that use different wire formats (Anthropic, Codex Responses API) convert at the boundary. Single `LlmClient` struct handles streaming SSE, retry, partial JSON parsing, and format conversion.

### Provider Architecture

```
User code
    │
    ▼
LlmClient (struct)
    │ uses
    ▼
ProviderSpec (declarative config)
    ├── id: "anthropic"
    ├── base_url: "https://api.anthropic.com"
    ├── format: FormatKind::Anthropic
    ├── auth: AuthKind::BearerFromEnv
    └── headers: [("anthropic-version", "2023-06-01")]
    │
    │ at request time:
    │
    ├── Format converter (request transform)
    │     OA → Anthropic native format
    │
    ├── HTTP call (reqwest + SSE streaming)
    │
    └── Format converter (response transform)
          Anthropic native → OA format
```

### Format Kinds

| Kind | Providers | Conversion |
|---|---|---|
| `OaCompatible` | OpenAI, Azure, Groq, Together, Fireworks, Mistral, DeepSeek, local (Ollama, vLLM, LM Studio) | None (native) |
| `Anthropic` | Anthropic (Claude) | Request: messages → content blocks, tool_choice mapping; Response: content blocks → choices |
| `OpenAiResponses` | OpenAI Codex (Responses API) | Response: items → choices format |

### Auth Kinds

| Kind | Mechanism |
|---|---|
| `BearerFromEnv` | `Authorization: Bearer $ENV_VAR` |
| `CustomHeaderFromEnv` | Custom header from env (e.g., `x-api-key` for Anthropic) |
| `AwsSigV4` | AWS Signature V4 (Bedrock) |
| `GcpAdc` | Google Application Default Credentials (Vertex AI) |
| `None` | No auth (local models) |

### Core Types

```rust
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    pub max_tokens: u32,
    pub temperature: f32,
    // builder: .with_tools(), .with_reasoning_effort(), etc.
}

pub struct ChatResponse {
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
    // helpers: .content(), .tool_calls(), .finish_reason()
}

pub struct Message {
    pub role: Role,           // System, User, Assistant, Tool
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,
    // factories: ::system(), ::user(), ::assistant(), ::tool_result()
}
```

### Streaming

SSE-based streaming with `StreamDelta` events:

| Delta | Content |
|---|---|
| `Content(String)` | Text token |
| `Reasoning(String)` | Extended thinking token |
| `ToolCallDelta { index, id, name, arguments }` | Incremental tool call |
| `Done` | Stream complete |

`StreamCollector` accumulates deltas into a final `ChatResponse`. Supports partial JSON parsing for tool call arguments that arrive incrementally.

### Error Handling

```rust
pub enum LlmError {
    Network(String),
    Api { status: u16, message: String },
    Parse(String),
    StreamEnded,
    AuthFailed(String),
    RateLimited { retry_after: Option<u64> },
    ProviderNotFound(String),
    Timeout,
    ServiceUnavailable,
    ContextOverflow,
}
```

`is_retryable()` returns true for: `RateLimited`, `ServiceUnavailable`, `Timeout`, `Network`, `Api` with status 429/503/504.

`is_context_overflow()` detects context length exceeded errors for emergency compaction.

### Mock Provider

`MockLlmProvider` for testing — returns configured responses without HTTP calls.

---

## theo-infra-auth

### Purpose
All provider-specific authentication flows. OAuth device flow (RFC 8628), PKCE, callback server, token storage, refresh logic.

### Supported Providers (8)

| Provider | Flow | Module |
|---|---|---|
| Anthropic | Console device flow | `anthropic.rs` |
| AWS Bedrock | SigV4 credentials | `bedrock.rs` |
| GitHub Copilot | RFC 8628 device flow | `copilot.rs` |
| GitLab | OAuth PKCE | `gitlab.rs` |
| Google Vertex AI | Application Default Credentials | `google_vertex.rs` |
| MCP servers | PKCE + dynamic client registration | `mcp.rs` |
| OpenAI | API key or OAuth | `openai.rs` |
| SAP AI Core | Client credentials | `sap.rs` |

### Auth Store

Persistent JSON credential store at `~/.config/theo/auth.json` (permissions `0o600`).

```rust
pub enum AuthEntry {
    OAuth { access_token, refresh_token, expires_at, account_id, scopes },
    ApiKey { key },
}
```

### PKCE (pkce.rs)

S256 challenge generation for OAuth flows:
```rust
pub struct PkceChallenge {
    pub verifier: String,    // 43-char random
    pub challenge: String,   // SHA-256 of verifier, base64url
    pub method: String,      // "S256"
}
```

### Device Flow (device_flow.rs)

Generic RFC 8628 implementation with polling:
```
Client → Authorization Server: device_code request
Server → Client: { device_code, user_code, verification_uri }
Client → User: "Go to {uri} and enter {code}"
Client → Server: poll token endpoint (every interval seconds)
Server → Client: { access_token, refresh_token }
```

### OpenID Connect Discovery (wellknown.rs)

Fetches `.well-known/openid-configuration` for dynamic endpoint resolution.

### Error Types

```rust
pub enum AuthError {
    Network(String),
    OAuth(String),
    TokenExpired,
    CallbackTimeout,
    StateMismatch,
    Storage(String),
    BrowserOpen(String),
    DevicePending,
    DeviceExpired,
}
```
