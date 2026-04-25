# theo-infra-auth — Revisao

> **Contexto**: OAuth PKCE, device flow, token management para provedores de LLM. Bounded Context: Infrastructure.
>
> **Dependencias permitidas**: `theo-domain`.
>
> **Status global**: deep-review concluido em 2026-04-25. 101 tests passando, 0 falhas. `cargo clippy -p theo-infra-auth --lib --tests` silent (zero warnings em codigo proprio).

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `anthropic` | OAuth/API key flow para Anthropic (Claude). | Revisado |
| 2 | `bedrock` | Autenticacao AWS Bedrock (SigV4 + assume role). | Revisado |
| 3 | `callback` | Servidor local de callback OAuth (loopback). | Revisado |
| 4 | `copilot` | Autenticacao GitHub Copilot. | Revisado |
| 5 | `device_flow` | OAuth 2.0 Device Authorization Grant. | Revisado |
| 6 | `error` | `AuthError` tipado (`thiserror`). | Revisado |
| 7 | `gitlab` | Autenticacao GitLab (Code Suggestions / Duo). | Revisado |
| 8 | `google_vertex` | Autenticacao Google Vertex AI (ADC / service account). | Revisado |
| 9 | `mcp` | Autenticacao para servidores MCP externos. | Revisado |
| 10 | `openai` | Autenticacao OpenAI (`AuthMethod`, API key ou OAuth). | Revisado |
| 11 | `pkce` | PKCE helpers (code verifier/challenge S256). | Revisado |
| 12 | `sap` | Autenticacao SAP AI Core. | Revisado |
| 13 | `store` | `AuthStore` — persistencia de tokens em `~/.config/theo/auth.json`. | Revisado |
| 14 | `wellknown` | Descoberta de providers via `.well-known/openid-configuration`. | Revisado |

---

## Notas de Deep-Review

### 1. anthropic
`AnthropicAuth` + `AnthropicConfig`. Suporta API key (x-api-key header) e OAuth (Claude Code / Claude.ai login). Feature-gated quando OAuth flow especifico esta ativo.

### 2. bedrock
AWS Bedrock SigV4 signing + STS assume-role chain. Reutiliza `aws-sdk-bedrock-runtime` quando feature esta ativa.

### 3. callback
Loopback HTTP server temporario para receber OAuth callback (porta auto-assigned, single-shot accept, kill-on-success).

### 4. copilot
`CopilotAuth` + `CopilotConfig` para GitHub Copilot. OAuth device flow + token refresh dynamico via `ApiKeyResolver` trait. Tokens curtos (~30min); refresh transparente.

### 5. device_flow
OAuth 2.0 Device Authorization Grant (RFC 8628). Polling com backoff. Para ambientes headless (CI, SSH, devices sem browser).

### 6. error
`AuthError::{Network, Token, InvalidConfig, Forbidden, Unauthorized, Timeout, Other}`. Driven por thiserror.

### 7. gitlab
GitLab Code Suggestions / Duo. Token-based auth.

### 8. google_vertex
Google Vertex AI auth via Application Default Credentials (ADC) ou service account JSON. Refresh automatico de access tokens.

### 9. mcp
Auth helpers para servidores MCP externos quando exigem OAuth/headers especificos.

### 10. openai
`OpenAIAuth` com `AuthMethod::{ApiKey, OAuth}`. OAuth e o flow ChatGPT/Codex (browser-based PKCE).

### 11. pkce
PKCE helpers (RFC 7636): `code_verifier(43..=128 bytes)` + `code_challenge_s256(verifier)` SHA256+base64url. Pure functions.

### 12. sap
SAP AI Core auth (cliente certificate ou OAuth client_credentials).

### 13. store
`AuthStore` persiste tokens em `~/.config/theo/auth.json` com permissoes 0600 (T1.4 fail-closed se HOME unset). Round-trip serde com `expires_at` em Unix epoch para refresh decision.

### 14. wellknown
Descoberta de OAuth endpoints via `.well-known/openid-configuration` GET. Cache short-TTL.

**Validacao:**
- 101 tests passando, 0 falhas
- `cargo clippy -p theo-infra-auth --lib --tests` silent (zero warnings em codigo proprio — sem fixes nesta auditoria)
- ADR dep invariant preservada: theo-domain (workspace) + reqwest/tokio/serde/serde_json/sha2/base64/url (external)
- T1.4 invariant: HOME unset → AuthStore retorna None (fail-closed, nunca /tmp fallback)

Sem follow-ups bloqueadores. O crate cobre 8 provider auth flows (Anthropic, Bedrock, Copilot, GitLab, GoogleVertex, OpenAI, MCP, SAP) atraves de mecanismos compartilhados (PKCE, device_flow, callback, store, wellknown).
