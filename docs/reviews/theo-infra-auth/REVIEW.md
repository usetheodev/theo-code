# theo-infra-auth — Revisao

> **Contexto**: OAuth PKCE, device flow, token management para provedores de LLM. Bounded Context: Infrastructure.
>
> **Dependencias permitidas**: `theo-domain`.

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `anthropic` | OAuth/API key flow para Anthropic (Claude). | Pendente |
| 2 | `bedrock` | Autenticacao AWS Bedrock (SigV4 + assume role). | Pendente |
| 3 | `callback` | Servidor local de callback OAuth (loopback). | Pendente |
| 4 | `copilot` | Autenticacao GitHub Copilot. | Pendente |
| 5 | `device_flow` | OAuth 2.0 Device Authorization Grant. | Pendente |
| 6 | `error` | `AuthError` tipado (`thiserror`). | Pendente |
| 7 | `gitlab` | Autenticacao GitLab (Code Suggestions / Duo). | Pendente |
| 8 | `google_vertex` | Autenticacao Google Vertex AI (ADC / service account). | Pendente |
| 9 | `mcp` | Autenticacao para servidores MCP externos. | Pendente |
| 10 | `openai` | Autenticacao OpenAI (`AuthMethod`, API key ou OAuth). | Pendente |
| 11 | `pkce` | PKCE helpers (code verifier/challenge S256). | Pendente |
| 12 | `sap` | Autenticacao SAP AI Core. | Pendente |
| 13 | `store` | `AuthStore` — persistencia de tokens em `~/.config/theo/auth.json`. | Pendente |
| 14 | `wellknown` | Descoberta de providers via `.well-known/openid-configuration`. | Pendente |
