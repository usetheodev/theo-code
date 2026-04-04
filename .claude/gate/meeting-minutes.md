# Meeting — 2026-04-04 (GRAPHCTX → Agent Wiring)

## Proposta
Conectar os 3 engines de code intelligence (theo-engine-graph, theo-engine-parser, theo-engine-retrieval) ao agent runtime via DIP. Trait `GraphContextProvider` em theo-domain, implementação `GraphContextService` em theo-application, injeção como system message no RunEngine.

## Participantes
- **governance** — Principal Engineer (veto absoluto)
- **qa** — QA Staff Engineer (validação de testabilidade)
- **runtime** — Staff AI Systems Engineer (agent loop, async)
- **graphctx** — Compiler Engineer (parsers, graph, dependências)
- **infra** — SRE (performance, resiliência, custo)

## Análises

### Governance (REJECT proposta original → APPROVE com DIP)
- REJEITOU deps diretas de theo-engine-* no runtime (viola architecture.md)
- APROVOU arquitetura DIP: trait domain, impl application, inject via Arc<dyn>
- Exigiu ADR para dívida existente (runtime já depende de tooling/infra)

### QA (validated=condicionalmente true)
- 991 testes passando, baseline saudável
- theo-application tem ZERO testes — risco alto
- ContextPayload deve ficar em retrieval; domain define GraphContextResult
- 15 testes obrigatórios listados (T1-T15)
- Mock pattern via DIP já existe no codebase (SnapshotStore)

### Runtime (risk_level=MEDIUM)
- Inject entre linha 189 (project context) e 192 (memories) de run_engine.rs
- initialize() em run_agent_session, ANTES de criar RunEngine
- timeout(5s) + spawn_blocking obrigatórios
- catch_unwind no spawn_blocking para prevenir panic propagation
- Sub-agents herdam Arc — seguro (read-only após init)
- Cache: write-to-tmp + atomic rename

### GraphCTX (risk=MEDIUM)
- Bridge gap CRÍTICO: conversor FileExtraction → FileData NÃO existe
- Parser e Graph desacoplados intencionalmente via DTOs intermediários
- Campos perdidos na conversão (confidence, fields) — aceitável
- fastembed ~90MB download primeira vez; TF-IDF fallback implementado
- Feature flag `neural` recomendada para compile time

### Infra (reliability_risk=HIGH)
- Louvain O(n³) com variância 20x — 50K LOC = 30-50s
- Per-turn query rápido (~20-30ms) — não impacta UX
- 8000 tokens/turn = ~$3.60/sessão GPT-4
- P0: timeout no clustering + circuit breaker + validar performance

## Conflitos

1. **Clustering performance**: Infra classifica Louvain O(n³) como CRITICAL. Resolução: timeout 10s + fallback BM25-only para Phase 1.
2. **Token budget**: Infra considera 8000 agressivo. Resolução: começar com 4000 tokens.
3. **FileExtraction → FileData**: Gap não coberto por outros agentes. Resolução: incluir conversor no escopo.

## Veredito
**APPROVED** (com condições)

## Escopo Aprovado

### Arquivos NOVOS
- `crates/theo-domain/src/graph_context.rs` — trait + tipos GraphContextResult
- `crates/theo-application/src/use_cases/graph_context_service.rs` — implementação concreta

### Arquivos EDITÁVEIS
- `crates/theo-domain/src/lib.rs` — pub mod graph_context
- `crates/theo-agent-runtime/src/run_engine.rs` — inject via Option<Arc<dyn GraphContextProvider>>
- `crates/theo-agent-runtime/src/config.rs` — campo graph_context_provider
- `crates/theo-agent-runtime/src/lib.rs` — se necessário
- `crates/theo-application/Cargo.toml` — +3 engine deps
- `crates/theo-application/src/lib.rs` — pub mod/use
- `crates/theo-application/src/use_cases/run_agent_session.rs` — wire GraphContextService
- `crates/theo-application/src/use_cases/mod.rs` — pub mod

### Arquivos PROIBIDOS
- `crates/theo-agent-runtime/Cargo.toml` — NÃO adicionar engine deps (DIP)
- `crates/theo-engine-*/src/**` — engines não devem ser modificados
- `apps/**` — apps não tocados nesta fase

## Condições

### Obrigatórias (bloqueiam merge)
1. DIP estrito: runtime NÃO recebe deps de theo-engine-*
2. Tipo `GraphContextResult` em domain (NÃO mover ContextPayload)
3. Timeout: 10s no clustering, 5s no query_context
4. spawn_blocking para graph build (CPU-bound)
5. catch_unwind dentro do spawn_blocking
6. Graceful degradation: falha → log warning + continue sem contexto
7. Cache atômico: write-to-tmp + rename
8. Conversor FileExtraction → FileData no GraphContextService
9. Testes: T4-T6, T9-T10, T12-T13 (mínimo 7 testes)
10. Budget inicial: 4000 tokens (não 8000)

### Phase 2 (recomendadas, não bloqueiam)
- Feature flag `neural` para fastembed
- Adaptive token windowing
- Progress feedback para UI
- Re-query per-turn
- Incremental graph updates
- Cache version header
- ADR documentando violações em architecture.md
