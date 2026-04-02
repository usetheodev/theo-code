# Meeting — 2026-04-02 (Streaming + Thinking Display)

## Proposta
Switch from .chat() to streaming. Display reasoning in real-time.

## Veredito
**APPROVED**

## Escopo Aprovado
- `crates/theo-infra-llm/src/client.rs` (new streaming method)
- `crates/theo-infra-llm/src/codex.rs` (stream Codex incrementally)
- `crates/theo-agent-runtime/src/run_engine.rs` (use streaming + emit events)
- `crates/theo-domain/src/event.rs` (add ReasoningDelta EventType)
- `apps/theo-cli/src/renderer.rs` (display reasoning)

## Condições
1. Streaming para OA-compatible E Codex
2. Reasoning deltas exibidos em tempo real no CLI
3. StreamCollector acumula response completo
4. Fallback para .chat() se streaming falhar
5. cargo check --workspace
