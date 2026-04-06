# Meeting — 2026-04-06 (P1 FAANG — 7.5 → 8.5)

## Proposta
5 items P1. 3 aprovados, 2 adiados (YAGNI sem dados).

## Participantes
- **governance** — P1.1/P1.2/P1.4 APPROVE. P1.3/P1.5 NEEDS_REVISION.

## Decisão ajustada

| Item | Decisão | Razão |
|---|---|---|
| P1.1 Stale cache | **APPROVE** | Zero risco, dados stale > vazio |
| P1.2 Impact invalidation | **APPROVE** | Threshold conservador como fallback |
| P1.3 Intent classifier | **ADIADO P2** | Cruza 3 bounded contexts, sem benchmark. YAGNI |
| P1.4 Planning injection | **APPROVE** | Com flag + skip if Building |
| P1.5 Snapshot semantics | **ADIADO P2** | RwLock já suficiente, sem evidência de contention |

## Veredito
**APPROVED** (3 de 5 items)

## Escopo Aprovado

### P1.1 — Stale cache during build
- `crates/theo-application/src/use_cases/graph_context_service.rs`

### P1.2 — Impact-based invalidation
- `crates/theo-application/src/use_cases/pipeline.rs`

### P1.4 — Planning injection
- `crates/theo-agent-runtime/src/run_engine.rs`

## Condições
1. P1.1: Building(Option<GraphState>) — stale_cache field
2. P1.2: usar edge count (fan-in+fan-out) como proxy de centralidade. >20 edges = central
3. P1.4: entre Session Boot Context e Skills. Max 200 tokens. Flag config.inject_graph_planning. Skip if Building.
4. cargo test verde após cada item
