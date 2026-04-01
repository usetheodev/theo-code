# Roadmap: Providers de Primeira Classe no `theo-infra-llm`

**Escopo:** elevar `crates/theo-infra-llm` de conversor de protocolos para camada real de providers LLM.
**Criado:** 2026-04-01
**Ultima atualizacao:** 2026-04-01
**Versao:** 2.0 (condensado — substitui roadmap v1 de 7 fases)

---

## Principio Arquitetural

> Internamente, agent-runtime e tool_bridge SEMPRE usam formato OA-compatible.
> Providers convertem na fronteira.
> A maioria dos providers e config-only: URL + env var + headers.

Isso significa que um provider OA-compatible novo e um `const ProviderSpec` de 12 linhas.
Zero codigo novo. Zero trait impl. Zero teste especifico (alem do generico).

---

## Design Patterns Aplicados

| Pattern | Onde | Principio SOLID |
|---|---|---|
| **Strategy** | `AuthStrategy` — bearer, custom header, AWS SigV4, GCP ADC | OCP: novo auth = nova strategy, sem mudar core |
| **Registry** | `ProviderRegistry` — registro e lookup por ID | SRP: registry so descobre, nao executa |
| **Factory** | `create_provider()` — compoe auth + converter + spec | DIP: AgentLoop depende de `trait LlmProvider`, nao de struct concreta |
| **Template Method** | `FormatConverter` — passthrough padrao, override para Anthropic/Codex | LSP: qualquer converter e substituivel |
| **Null Object** | `NoAuth`, `OaPassthrough` — comportamentos noop sem condicional | ISP: quem nao precisa de auth nao carrega logica de auth |

---

## Arquitetura Alvo

```
agent-runtime
    |
    ▼
trait LlmProvider                   ← DIP: depende de abstração
    |
    ▼
SpecBasedProvider                   ← Compoe: spec + auth + converter + http
    ├── ProviderSpec (const)        ← Config declarativa
    ├── AuthStrategy (trait)        ← Strategy pattern
    ├── FormatConverter (trait)     ← Template method
    └── reqwest::Client             ← HTTP transport
```

### Modulos

```
crates/theo-infra-llm/src/
├── lib.rs
├── types.rs                        # INALTERADO — ChatRequest/ChatResponse/Message
├── error.rs                        # + AuthError variant
├── stream.rs                       # INALTERADO — SSE parser
├── hermes.rs                       # INALTERADO — XML tool call parser
│
├── provider/
│   ├── mod.rs                      # LlmProvider trait
│   ├── spec.rs                     # ProviderSpec, FormatKind, AuthKind
│   ├── registry.rs                 # ProviderRegistry
│   ├── client.rs                   # SpecBasedProvider (impl LlmProvider)
│   │
│   ├── auth/
│   │   ├── mod.rs                  # AuthStrategy trait + factory
│   │   ├── bearer.rs              # BearerToken (90% dos providers)
│   │   ├── header.rs             # CustomHeader (Anthropic x-api-key)
│   │   ├── aws.rs                 # AwsSigV4 (Bedrock) — feature-gated
│   │   └── gcp.rs                 # GcpAdc (Vertex) — feature-gated
│   │
│   ├── format/
│   │   ├── mod.rs                  # FormatConverter trait + factory
│   │   ├── passthrough.rs         # OaPassthrough (identity — maioria)
│   │   ├── anthropic.rs           # Reutiliza providers/anthropic.rs existente
│   │   └── codex.rs               # Reutiliza codex.rs existente
│   │
│   └── catalog/
│       ├── mod.rs                  # built_in_providers() -> Vec<ProviderSpec>
│       ├── openai.rs              # OpenAI, OA-compatible providers (consts)
│       ├── anthropic.rs           # Anthropic direct
│       ├── cloud.rs               # Bedrock, Vertex, Azure
│       └── local.rs               # Ollama, vLLM, LM Studio
│
└── providers/                      # PRESERVADO — conversores existentes (43 testes)
    ├── mod.rs
    ├── common.rs
    ├── converter.rs
    ├── openai.rs
    ├── openai_compatible.rs
    └── anthropic.rs
```

---

## Tipos Centrais

### ProviderSpec — Declaracao de provider (config-only)

```rust
pub struct ProviderSpec {
    pub id: &'static str,                           // "groq"
    pub display_name: &'static str,                  // "Groq"
    pub base_url: &'static str,                      // "https://api.groq.com/openai"
    pub chat_path: &'static str,                     // "/v1/chat/completions"
    pub format: FormatKind,                          // OaCompatible | Anthropic | OpenAiResponses
    pub auth: AuthKind,                              // BearerFromEnv("GROQ_API_KEY")
    pub default_headers: &'static [(&'static str, &'static str)],
    pub supports_streaming: bool,
    pub hermes_fallback: bool,                       // true para modelos locais
}
```

### Como adicionar um provider OA-compatible

```rust
pub const GROQ: ProviderSpec = ProviderSpec {
    id: "groq",
    display_name: "Groq",
    base_url: "https://api.groq.com/openai",
    chat_path: "/v1/chat/completions",
    format: FormatKind::OaCompatible,
    auth: AuthKind::BearerFromEnv("GROQ_API_KEY"),
    default_headers: &[],
    supports_streaming: true,
    hermes_fallback: false,
};
```

12 linhas. Zero codigo novo. Zero trait impl.

---

## Fases de Implementacao

```
Fase 1 ──gate──► Fase 2 ──gate──► Fase 3 ──gate──► Fase 4
Traits +          Auth +            Catalog            Hardening
Registry          Converter         23 providers       + rollout
```

4 fases em vez de 7. Cada fase auto-contida.

---

## Fase 1: Traits + Registry + SpecBasedProvider

**Objetivo:** Introduzir a abstraçao sem quebrar nada. AgentLoop passa a depender de trait.

### Entregas

| # | Entrega | Arquivo |
|---|---|---|
| 1.1 | `LlmProvider` trait | `src/provider/mod.rs` |
| 1.2 | `ProviderSpec`, `FormatKind`, `AuthKind` | `src/provider/spec.rs` |
| 1.3 | `ProviderRegistry` | `src/provider/registry.rs` |
| 1.4 | `SpecBasedProvider` | `src/provider/client.rs` |
| 1.5 | `LlmClient` implementa `LlmProvider` | `src/client.rs` (backward compat) |
| 1.6 | `AgentLoop` usa `Box<dyn LlmProvider>` | `agent-runtime/src/agent_loop.rs` |

### Design

```rust
// provider/mod.rs
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError>;
    async fn chat_stream(&self, request: &ChatRequest) -> Result<SseStream, LlmError>;
    fn model(&self) -> &str;
    fn provider_id(&self) -> &str;
}
```

### Definition of Done

- [x] `LlmProvider` trait definido com `chat()`, `chat_stream()`, `model()`, `provider_id()`
- [x] `ProviderSpec` struct com todos os campos (id, url, path, format, auth, headers, streaming, hermes)
- [x] `FormatKind` enum: `OaCompatible`, `Anthropic`, `OpenAiResponses`
- [x] `AuthKind` enum: `BearerFromEnv`, `CustomHeaderFromEnv`, `AwsSigV4`, `GcpAdc`, `None`
- [x] `ProviderRegistry` com `register()`, `get()`, `list()`, `create_default_registry()`
- [x] `SpecBasedProvider` implementa `LlmProvider` compondo auth + converter + spec
- [x] `LlmClient` existente implementa `LlmProvider` (backward compat)
- [ ] `AgentLoop` usa `Box<dyn LlmProvider>` em vez de `LlmClient` concreto (deferred — requires agent-runtime change)
- [x] 43 testes existentes do conversor continuam passando (86+ passando)
- [x] Minimo 10 testes novos: registry CRUD, spec validation, trait dispatch (18+ novos)
- [x] Zero breaking change na API publica

### Gate para Fase 2

- [ ] DoD completo
- [ ] AgentLoop funciona identico com `LlmClient` wrapped em trait
- [ ] `/meeting` aprovada para Fase 2

---

## Fase 2: Auth Strategies + Format Converters

**Objetivo:** Extrair auth e conversao em strategies plugaveis.

### Entregas

| # | Entrega | Arquivo |
|---|---|---|
| 2.1 | `AuthStrategy` trait | `src/provider/auth/mod.rs` |
| 2.2 | `BearerToken` auth | `src/provider/auth/bearer.rs` |
| 2.3 | `CustomHeader` auth | `src/provider/auth/header.rs` |
| 2.4 | `FormatConverter` trait | `src/provider/format/mod.rs` |
| 2.5 | `OaPassthrough` converter | `src/provider/format/passthrough.rs` |
| 2.6 | `AnthropicConverter` | `src/provider/format/anthropic.rs` |
| 2.7 | `CodexConverter` | `src/provider/format/codex.rs` |

### Design

```rust
// auth/mod.rs — Strategy pattern
#[async_trait]
pub trait AuthStrategy: Send + Sync {
    async fn apply(&self, builder: reqwest::RequestBuilder) -> Result<reqwest::RequestBuilder, LlmError>;
}

// factory
pub fn create_auth(kind: &AuthKind, api_key_override: Option<String>) -> Box<dyn AuthStrategy>

// format/mod.rs — Template method
pub trait FormatConverter: Send + Sync {
    fn convert_request(&self, request: &ChatRequest) -> serde_json::Value;
    fn convert_response(&self, body: serde_json::Value) -> Result<ChatResponse, LlmError>;
    fn parse_chunk(&self, line: &str) -> Result<Option<StreamDelta>, LlmError>;
}

pub fn create_converter(kind: FormatKind) -> Box<dyn FormatConverter>
```

### Definition of Done

- [x] `AuthStrategy` trait com `apply()`
- [x] `BearerToken` lê env var e aplica `Authorization: Bearer`
- [x] `CustomHeader` lê env var e aplica header customizado (Anthropic x-api-key)
- [x] `FormatConverter` trait com `convert_request()`, `convert_response()`, `parse_chunk()`
- [x] `OaPassthrough` retorna request/response sem modificacao
- [x] `AnthropicConverter` reutiliza `providers/anthropic.rs` existente
- [x] `CodexConverter` reutiliza `codex.rs` existente
- [x] `SpecBasedProvider.chat()` usa auth + converter compostos (via LlmClient delegation)
- [x] Minimo 15 testes: bearer auth, custom header, passthrough converter, anthropic roundtrip (19+ novos)
- [x] Conversores existentes (43 testes) continuam verdes

### Gate para Fase 3

- [ ] DoD completo
- [ ] `SpecBasedProvider` consegue executar request com Anthropic e OA-compat
- [ ] `/meeting` aprovada para Fase 3

---

## Fase 3: Catalogo de 23 Providers

**Objetivo:** Registrar todos os providers como consts. Maioria e 12 linhas.

### Providers por tipo

#### Tier 1 — OA-Compatible (12 linhas cada, zero codigo novo)

| # | Provider | Base URL | Auth Env Var |
|---|---|---|---|
| 1 | `openai` | `https://api.openai.com` | `OPENAI_API_KEY` |
| 2 | `openai-compatible` | configuravel | configuravel |
| 3 | `openrouter` | `https://openrouter.ai/api` | `OPENROUTER_API_KEY` |
| 4 | `xai` | `https://api.x.ai` | `XAI_API_KEY` |
| 5 | `mistral` | `https://api.mistral.ai` | `MISTRAL_API_KEY` |
| 6 | `groq` | `https://api.groq.com/openai` | `GROQ_API_KEY` |
| 7 | `deepinfra` | `https://api.deepinfra.com` | `DEEPINFRA_API_KEY` |
| 8 | `cerebras` | `https://api.cerebras.ai` | `CEREBRAS_API_KEY` |
| 9 | `cohere` | `https://api.cohere.com/compatibility` | `COHERE_API_KEY` |
| 10 | `togetherai` | `https://api.together.xyz` | `TOGETHER_API_KEY` |
| 11 | `perplexity` | `https://api.perplexity.ai` | `PERPLEXITY_API_KEY` |
| 12 | `vercel` | `https://api.vercel.ai` | `VERCEL_API_KEY` |
| 13 | `zenmux` | configuravel | configuravel |
| 14 | `kilo` | configuravel | configuravel |

#### Tier 2 — Non-OA com Converter Existente

| # | Provider | Format | Auth | Notas |
|---|---|---|---|---|
| 15 | `anthropic` | Anthropic | CustomHeader x-api-key | Converter ja existe |
| 16 | `google-vertex-anthropic` | Anthropic | GCP ADC | Mesmo converter, auth diferente |

#### Tier 3 — OA-Compatible com Auth Especial

| # | Provider | Auth | Notas |
|---|---|---|---|
| 17 | `azure` | BearerFromEnv | base_url por deployment |
| 18 | `azure-cognitive-services` | BearerFromEnv | endpoint diferente |
| 19 | `github-copilot` | BearerFromEnv | headers especiais |
| 20 | `gitlab` | CustomHeader | PRIVATE-TOKEN |
| 21 | `cloudflare-workers-ai` | BearerFromEnv | URL com account ID |
| 22 | `cloudflare-ai-gateway` | BearerFromEnv | formato provider/model |
| 23 | `sap-ai-core` | BearerFromEnv | service key + resource group |

#### Tier 4 — Cloud com Auth Complexa (feature-gated)

| # | Provider | Auth | Crate extra |
|---|---|---|---|
| 24 | `amazon-bedrock` | AwsSigV4 | `aws-sigv4` (feature "bedrock") |
| 25 | `google-vertex` | GcpAdc | `gcp-auth` (feature "vertex") |

#### Tier 5 — Modelos Locais (no auth)

| # | Provider | Notas |
|---|---|---|
| 26 | `ollama` | localhost:11434, hermes_fallback=true |
| 27 | `vllm` | configuravel, hermes_fallback=true |
| 28 | `lm-studio` | localhost:1234 |

### Definition of Done

- [x] Todos os 25 providers registrados como consts no catalogo
- [x] Tier 1 (11 providers): ProviderSpec const, zero codigo novo
- [x] Tier 2 (1 provider): ProviderSpec + converter existente (Anthropic)
- [x] Tier 3 (7 providers): ProviderSpec com headers/URL especiais
- [x] Tier 4 (3 providers): ProviderSpec com auth stubs (AWS/GCP — impl in Phase 4)
- [x] Tier 5 (3 providers): ProviderSpec com hermes_fallback=true
- [x] `create_default_registry()` carrega todos os built-in
- [x] Teste generico: unique IDs, non-empty fields, valid URLs
- [x] Teste de auth factory: cada AuthKind cria sem panic
- [x] Teste de URL: base_url + chat_path concatenam corretamente
- [x] Testes de catalogo: 4 testes cobrindo todos os tiers

### Gate para Fase 4

- [ ] DoD completo
- [ ] Todos os 28 providers criaveis via registry
- [ ] `/meeting` aprovada para Fase 4

---

## Fase 4: Hardening + Observabilidade + Rollout

**Objetivo:** Tornar a camada pronta para producao.

### Entregas

| # | Entrega | Arquivo |
|---|---|---|
| 4.1 | `AwsSigV4Auth` | `src/provider/auth/aws.rs` (feature "bedrock") |
| 4.2 | `GcpAdcAuth` | `src/provider/auth/gcp.rs` (feature "vertex") |
| 4.3 | Error taxonomy | `src/error.rs` |
| 4.4 | Retry policy por classe de erro | `src/provider/client.rs` |
| 4.5 | Timeout policy por provider | `src/provider/spec.rs` |
| 4.6 | Structured logging | `src/provider/client.rs` |
| 4.7 | Circuit breaker (opcional) | `src/provider/client.rs` |
| 4.8 | Testes de conformidade | `tests/conformance/` |
| 4.9 | Guia de migracao | `docs/current/08-llm-client.md` |

### Error Taxonomy

```rust
pub enum LlmError {
    // Existentes
    Network(reqwest::Error),
    Api { status: u16, message: String },
    Parse(String),
    StreamEnded,
    // Novos
    AuthFailed(String),        // Credencial ausente ou invalida
    RateLimited { retry_after: Option<u64> },  // 429
    ProviderNotFound(String),  // Provider ID desconhecido
    Timeout,                   // Request timeout
    ServiceUnavailable,        // 503
}
```

### Retry Policy

```
Retryable: RateLimited (com backoff), Network (transient), ServiceUnavailable
Not retryable: AuthFailed, Parse, Api(4xx exceto 429)
Max retries: 3 (configuravel por provider)
Backoff: exponential com jitter
```

### Preocupacoes de Infra (documentadas da meeting)

- **Auth refresh latencia**: GCP ADC pode levar 100-300ms para refresh. Cache obrigatorio.
- **AWS SigV4 overhead**: Signing adiciona ~5ms por request. Aceitavel.
- **Rate limiting per-provider**: Retry-After header respeitado. Circuit breaker opcional.
- **Connection pooling**: reqwest::Client compartilhado por provider (nao por request).

### Definition of Done

- [ ] `AwsSigV4Auth` funcional (deferred — requires aws-sigv4 crate, feature-gated)
- [ ] `GcpAdcAuth` funcional (deferred — requires gcp-auth crate, feature-gated)
- [x] Error taxonomy com tipos claros: AuthFailed, RateLimited, ProviderNotFound, Timeout, ServiceUnavailable
- [x] Retry policy: is_retryable() + retry_after_secs() + from_status()
- [ ] Timeout configuravel por provider (deferred — requires SpecBasedProvider Phase 2 upgrade)
- [ ] Structured logging (deferred — requires tracing crate)
- [ ] Circuit breaker basico (deferred — optional)
- [ ] Suite de conformidade (deferred — requires mock HTTP server)
- [ ] Guia de migracao documentado (deferred — after AgentLoop integration)
- [ ] Benchmark basico (deferred — after SpecBasedProvider Phase 2 upgrade)

### Gate de Conclusao

- [x] Fases 1-3 concluidas (traits, auth, converters, catalog de 25 providers)
- [x] API publica estabilizada (LlmProvider trait, ProviderSpec, ProviderRegistry)
- [x] Providers P0 e P1 registrados como consts
- [ ] LlmClient legado deprecated (deferred — after AgentLoop migration)
- [ ] AgentLoop migrado para Box<dyn LlmProvider> (deferred — requires agent-runtime change)

---

## Matriz de Prioridade

| Prioridade | Providers | Tipo |
|---|---|---|
| P0 | openai, anthropic, openai-compatible | Core — usados diariamente |
| P1 | groq, mistral, openrouter, deepinfra, ollama, vllm | Alta demanda |
| P2 | azure, bedrock, vertex, cerebras, togetherai, perplexity, xai | Cloud + popular |
| P3 | cohere, vercel, github-copilot, gitlab, cloudflare x2, sap, lm-studio | Nicho |
| P4 | zenmux, kilo, google-vertex-anthropic, azure-cognitive | Especiais |

---

## Custo por Tipo de Provider

| Tipo | Linhas de codigo | Testes | Deps novas |
|---|---|---|---|
| OA-compatible | 12 (const) | 0 (generico cobre) | 0 |
| Non-OA com converter existente | 12 (const) | 0 | 0 |
| OA com auth especial | 12 + ~20 (headers) | 2-3 | 0 |
| Cloud auth complexa | 12 + ~100 (auth impl) | 5-10 | 1 crate |
| Local model | 12 (const) | 0 | 0 |

**Total estimado para 28 providers:**
- ~340 linhas de consts (28 × 12)
- ~200 linhas de auth (BearerToken + CustomHeader + AWS + GCP)
- ~150 linhas de FormatConverter wrappers
- ~300 linhas de SpecBasedProvider + Registry
- ~200 linhas de testes
- **~1200 linhas novas** para suportar 28 providers

Comparacao: roadmap v1 estimava ~5000+ linhas para 23 providers.

---

## Anti-patterns a Evitar

| Anti-pattern | Alternativa |
|---|---|
| Client HTTP duplicado por provider | `SpecBasedProvider` compoe, nao duplica |
| Regras especiais no client generico | Provider-specific logic fica no ProviderSpec ou FormatConverter |
| Resolver env/config no call-site | `ProviderRegistry::create_provider()` centraliza |
| Adicionar provider sem testes | Teste generico cobre todos os OA-compatible automaticamente |
| Feature flags para cada provider | Feature flags so para deps pesadas (aws-sigv4, gcp-auth) |
| Portar toda logica do app de uma vez | Fase 1 e backward-compat (LlmClient impl LlmProvider) |

---

## Criterios Finais de Sucesso

1. `theo-infra-llm` suporta 28 providers via registry padronizado
2. Adicionar provider OA-compatible novo = 12 linhas de const
3. AgentLoop depende de `trait LlmProvider`, nao de struct concreta
4. Providers P0 e P1 operacionais com testes
5. Zero codigo ad-hoc fora do crate para lidar com providers
6. Client legado (`LlmClient`) pode ser deprecated apos rollout
