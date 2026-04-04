# Meeting — 2026-04-04 (Error Recovery Wiring)

## Proposta
Conectar RetryExecutor + RetryPolicy + LlmError::is_retryable() — tudo existe mas está desconectado.

## Veredito
**APPROVED**

## Escopo Aprovado
- crates/theo-infra-llm/src/client.rs (from_status em vez de Api genérico)
- crates/theo-agent-runtime/src/run_engine.rs (retry wrapper + parse_arguments error reporting)

## Condições
- cargo test 100% verde, 0 warnings
