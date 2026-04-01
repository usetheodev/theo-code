# Roadmap: Auth Providers Completos

**Escopo:** Implementar TODOS os flows de autenticação que o opencode suporta no theo-infra-auth.
**Criado:** 2026-04-01
**Referência:** opencode/packages/opencode/src/ (account/, plugin/, provider/, mcp/)

---

## Estado Atual vs Alvo

| # | Provider | Auth Method | opencode | Theo Code | Gap |
|---|---|---|---|---|---|
| 1 | OpenAI (Codex) | PKCE + Device Flow | ✅ | ✅ | - |
| 2 | GitHub Copilot | Device Flow | ✅ | ✅ | - |
| 3 | Anthropic Console | Device Flow OAuth | ✅ | ❌ | **Fase 1** |
| 4 | Google Vertex | ADC (Application Default Credentials) | ✅ | ❌ | **Fase 2** |
| 5 | Google Vertex Anthropic | ADC (mesma infra) | ✅ | ❌ | **Fase 2** |
| 6 | Amazon Bedrock | AWS Credential Chain | ✅ | ❌ | **Fase 3** |
| 7 | GitLab | OAuth + API token | ✅ | ❌ | **Fase 4** |
| 8 | SAP AI Core | Service Key | ✅ | ❌ | **Fase 4** |
| 9 | WellKnown Federation | Discovery + CLI exec | ✅ | ❌ | **Fase 5** |
| 10 | MCP OAuth | PKCE + Dynamic Registration | ✅ | ❌ | **Fase 6** |

---

## Fases

```
Fase 1 ──► Fase 2 ──► Fase 3 ──► Fase 4 ──► Fase 5 ──► Fase 6
Anthropic   Google     AWS        GitLab     WellKnown   MCP
Console     Vertex     Bedrock    + SAP      Federation   OAuth
```

---

## Fase 1: Anthropic Console — Device Flow OAuth

**Objetivo:** Login com conta Anthropic via device code flow, como no opencode.

### Especificacao tecnica

**Endpoints:**
- `POST {server}/auth/device/code` → `{ device_code, user_code, verification_uri_complete, expires_in, interval }`
- `POST {server}/auth/device/token` → poll com `grant_type=urn:ietf:params:oauth:grant-type:device_code`
- `POST {server}/auth/device/token` → refresh com `grant_type=refresh_token`
- `GET {server}/api/user` → `{ id, email }`
- `GET {server}/api/orgs` → orgs do usuario
- `GET {server}/api/config` → config com header `x-org-id`

**Client ID:** `"opencode-cli"` (ou criar `"theo-code"` se necessario)
**Server:** Configuravel (default: console.anthropic.com ou equivalente)

### Entregas

| # | Entrega | Arquivo |
|---|---|---|
| 1.1 | `AnthropicAuth` struct | `crates/theo-infra-auth/src/anthropic.rs` |
| 1.2 | Device flow (start + poll + refresh) | `crates/theo-infra-auth/src/anthropic.rs` |
| 1.3 | User/org resolution | `crates/theo-infra-auth/src/anthropic.rs` |
| 1.4 | Tauri commands | `apps/theo-desktop/src/commands/anthropic_auth.rs` |
| 1.5 | UI: DeviceAuthDialog reutilizado | `apps/theo-ui/src/features/settings/pages/SettingsPage.tsx` |
| 1.6 | Provider models para Anthropic | `apps/theo-desktop/src/commands/copilot.rs` (provider_models) |

### Definition of Done

- [ ] `AnthropicAuth` com `start_device_flow()`, `poll_device_flow()`, `refresh()`, `get_user()`, `get_orgs()`
- [ ] Token storage em auth.json com provider_id "anthropic-console"
- [ ] Refresh automatico quando token expira
- [ ] Tauri commands: anthropic_start_device_flow, anthropic_poll_device_flow, anthropic_status, anthropic_logout, anthropic_apply_to_config
- [ ] UI: botao "Login with Anthropic" no SettingsPage, reutiliza DeviceAuthDialog
- [ ] Preset "Anthropic" no SettingsPage detecta auth console e aplica config
- [ ] Testes: 10+ (device flow mock, token storage, refresh, user/org)
- [ ] Testes existentes passam

---

## Fase 2: Google Vertex — Application Default Credentials

**Objetivo:** Autenticar com Google Cloud via ADC para usar Vertex AI (e Vertex Anthropic).

### Especificacao tecnica

**Metodo:** Application Default Credentials (ADC)
- Detecta automaticamente: gcloud auth, service account, metadata server
- Resolve token via chain: env var → arquivo de credencial → metadata server

**Env vars:**
- `GOOGLE_CLOUD_PROJECT` / `GCP_PROJECT` / `GCLOUD_PROJECT`
- `GOOGLE_VERTEX_LOCATION` / `GOOGLE_CLOUD_LOCATION` / `VERTEX_LOCATION` (default: "us-central1")
- `GOOGLE_APPLICATION_CREDENTIALS` (path para service account JSON)

**Endpoint resolution:**
- Location "global" → `aiplatform.googleapis.com`
- Outros → `{location}-aiplatform.googleapis.com`

**Dependencia Rust:** `gcp-auth` crate (feature-gated)

### Entregas

| # | Entrega | Arquivo |
|---|---|---|
| 2.1 | `GoogleVertexAuth` struct | `crates/theo-infra-auth/src/google_vertex.rs` |
| 2.2 | ADC token resolution | `crates/theo-infra-auth/src/google_vertex.rs` |
| 2.3 | Endpoint resolution por location | `crates/theo-infra-auth/src/google_vertex.rs` |
| 2.4 | Tauri commands | `apps/theo-desktop/src/commands/vertex_auth.rs` |
| 2.5 | UI: config de project/location | `apps/theo-ui/src/features/settings/pages/SettingsPage.tsx` |

### Definition of Done

- [ ] `GoogleVertexAuth` resolve token via ADC chain
- [ ] Suporta service account JSON, gcloud auth, metadata server
- [ ] Endpoint resolvido por location (us-central1-aiplatform.googleapis.com)
- [ ] Suporta Google Vertex Anthropic (mesma auth, endpoint diferente)
- [ ] Tauri commands: vertex_status, vertex_apply_to_config
- [ ] UI: campos Project ID e Location no preset Vertex
- [ ] Feature-gated: `gcp-auth` crate so compila com feature "vertex"
- [ ] Testes: 8+ (env var resolution, endpoint building, token caching)
- [ ] Testes existentes passam

---

## Fase 3: Amazon Bedrock — AWS Credential Chain

**Objetivo:** Autenticar com AWS para usar Bedrock.

### Especificacao tecnica

**Metodo:** AWS Credential Provider Chain
- Precedencia: bearer token direto → env vars → profile → OIDC → container credentials
- Region prefix logic: modelos claude-* precisam de prefixo regional (us., eu., etc)

**Env vars:**
- `AWS_BEARER_TOKEN_BEDROCK` (atalho: token direto)
- `AWS_ACCESS_KEY_ID` + `AWS_SECRET_ACCESS_KEY`
- `AWS_PROFILE`
- `AWS_REGION` (default: "us-east-1")

**Dependencia Rust:** `aws-config` + `aws-sdk-bedrockruntime` (feature-gated)

### Entregas

| # | Entrega | Arquivo |
|---|---|---|
| 3.1 | `BedrockAuth` struct | `crates/theo-infra-auth/src/bedrock.rs` |
| 3.2 | Credential chain resolution | `crates/theo-infra-auth/src/bedrock.rs` |
| 3.3 | Region prefix logic | `crates/theo-infra-auth/src/bedrock.rs` |
| 3.4 | Tauri commands | `apps/theo-desktop/src/commands/bedrock_auth.rs` |
| 3.5 | UI: campos Region/Profile | `apps/theo-ui/src/features/settings/pages/SettingsPage.tsx` |

### Definition of Done

- [ ] `BedrockAuth` resolve credenciais via AWS chain
- [ ] Suporta: bearer token direto, env vars, profile, OIDC
- [ ] Region prefix aplicado corretamente a model IDs
- [ ] Tauri commands: bedrock_status, bedrock_apply_to_config
- [ ] UI: campos Region e Profile no preset Bedrock
- [ ] Feature-gated: `aws-config` so compila com feature "bedrock"
- [ ] Testes: 8+ (credential chain, region prefix, bearer token)
- [ ] Testes existentes passam

---

## Fase 4: GitLab + SAP AI Core

**Objetivo:** Suportar GitLab AI Gateway e SAP AI Core.

### GitLab

**Metodo:** OAuth ou API token
**Env vars:** `GITLAB_TOKEN`, `GITLAB_INSTANCE_URL` (default: gitlab.com)
**Headers especiais:** `anthropic-beta`, `User-Agent` customizado

### SAP AI Core

**Metodo:** Service account key (JSON)
**Env vars:** `AICORE_SERVICE_KEY`, `AICORE_DEPLOYMENT_ID`, `AICORE_RESOURCE_GROUP`

### Entregas

| # | Entrega | Arquivo |
|---|---|---|
| 4.1 | `GitLabAuth` struct | `crates/theo-infra-auth/src/gitlab.rs` |
| 4.2 | `SapAiCoreAuth` struct | `crates/theo-infra-auth/src/sap.rs` |
| 4.3 | Tauri commands | `apps/theo-desktop/src/commands/` |
| 4.4 | UI: presets GitLab e SAP | SettingsPage.tsx |

### Definition of Done

- [ ] `GitLabAuth` suporta OAuth access token e API key
- [ ] Suporta custom instance URL
- [ ] `SapAiCoreAuth` resolve service key de env var ou auth store
- [ ] Tauri commands para ambos
- [ ] Presets na UI
- [ ] Testes: 6+ por provider
- [ ] Testes existentes passam

---

## Fase 5: WellKnown Federation

**Objetivo:** Discovery protocol para servers custom/enterprise.

### Especificacao tecnica

**Discovery:** `GET {url}/.well-known/opencode` → JSON config
**Auth:** Executa comando CLI do response, captura stdout como token
**Storage:** `{ type: "wellknown", key: "ENV_VAR", token: "value" }`

### Entregas

| # | Entrega | Arquivo |
|---|---|---|
| 5.1 | `WellKnownAuth` struct | `crates/theo-infra-auth/src/wellknown.rs` |
| 5.2 | Discovery + exec | `crates/theo-infra-auth/src/wellknown.rs` |
| 5.3 | AuthEntry::WellKnown variant | `crates/theo-infra-auth/src/store.rs` |
| 5.4 | Tauri commands | `apps/theo-desktop/src/commands/` |

### Definition of Done

- [ ] `WellKnownAuth` faz discovery via `.well-known/opencode`
- [ ] Executa comando de auth e captura token
- [ ] `AuthEntry::WellKnown { key, token }` no store
- [ ] Tauri commands: wellknown_discover, wellknown_auth, wellknown_status
- [ ] Testes: 6+ (discovery mock, exec mock, storage)
- [ ] Testes existentes passam

---

## Fase 6: MCP OAuth

**Objetivo:** OAuth para MCP servers com PKCE e dynamic client registration.

### Especificacao tecnica

**Callback:** `http://127.0.0.1:19876/mcp/oauth/callback`
**Flows:** Authorization code + PKCE, refresh token, dynamic client registration
**Storage:** `mcp-auth.json` separado do auth.json principal

### Entregas

| # | Entrega | Arquivo |
|---|---|---|
| 6.1 | `McpAuth` struct | `crates/theo-infra-auth/src/mcp.rs` |
| 6.2 | PKCE flow (reutiliza pkce.rs) | `crates/theo-infra-auth/src/mcp.rs` |
| 6.3 | Dynamic client registration | `crates/theo-infra-auth/src/mcp.rs` |
| 6.4 | mcp-auth.json storage separado | `crates/theo-infra-auth/src/mcp_store.rs` |

### Definition of Done

- [ ] `McpAuth` suporta PKCE + dynamic client registration
- [ ] Callback server na porta 19876
- [ ] Storage separado em mcp-auth.json
- [ ] Refresh automatico
- [ ] Per-server token tracking
- [ ] Testes: 8+ (PKCE, registration, refresh, storage)
- [ ] Testes existentes passam

---

## Dependencias Novas por Fase

| Fase | Crate | Feature flag | Motivo |
|---|---|---|---|
| 2 | `gcp-auth` | "vertex" | Google ADC |
| 3 | `aws-config` | "bedrock" | AWS credential chain |
| 3 | `aws-sdk-bedrockruntime` | "bedrock" | Bedrock API |
| - | Nenhuma nova | - | Fases 1, 4, 5, 6 usam reqwest (ja temos) |

---

## Matriz de Prioridade

| Prioridade | Provider | Motivo |
|---|---|---|
| P0 | Anthropic Console | Provider principal do Theo Code |
| P0 | Google Vertex ADC | Desbloqueia Gemini + Claude via Vertex |
| P1 | Amazon Bedrock | Enterprise AWS |
| P1 | GitLab | Enterprise GitLab |
| P2 | SAP AI Core | Nicho enterprise |
| P2 | WellKnown | Custom servers |
| P3 | MCP OAuth | Futuro MCP ecosystem |

---

## Padrao de Implementacao (DRY)

Todos os providers seguem o mesmo pattern:

```rust
// 1. Auth struct no theo-infra-auth
pub struct XxxAuth {
    http: reqwest::Client,
    store: AuthStore,
    config: XxxConfig,
}

impl XxxAuth {
    pub fn new(store: AuthStore) -> Self;
    pub fn get_tokens(&self) -> Result<Option<XxxTokens>, AuthError>;
    pub fn has_valid_tokens(&self) -> bool;
    pub fn logout(&self) -> Result<(), AuthError>;
    // + metodos especificos do flow
}

// 2. Tauri commands no theo-desktop
#[tauri::command] pub async fn xxx_status() -> Result<Value, String>;
#[tauri::command] pub async fn xxx_logout() -> Result<(), String>;
#[tauri::command] pub async fn xxx_apply_to_config(state: State<AppState>) -> Result<bool, String>;

// 3. UI: preset + auth section no SettingsPage
// Reutiliza DeviceAuthDialog quando flow é device code
// Campos customizados para config (region, project, etc)
```

---

## Criterios de Sucesso

O roadmap esta concluido quando:

1. Todos os 10 providers do opencode tem auth funcional no Theo Code
2. Cada provider tem preset na UI com modelo select dinamico
3. Backend e fonte de verdade para URL, auth, modelos
4. Frontend e thin client — nao hardcoda URLs nem modelos
5. auth.json suporta todos os tipos (oauth, api, wellknown)
6. Testes cobrem cada flow sem dependencia de servidores reais
7. Feature flags para deps pesadas (aws, gcp)
