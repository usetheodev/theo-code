# Meeting — 2026-04-02 (Real Streaming for Codex)

## Proposta
Fix chat_streaming() Codex path: response.text().await → response.bytes_stream() incremental.

## Veredito
**APPROVED**

## Escopo Aprovado
- `crates/theo-infra-llm/src/client.rs` (Codex streaming path)
