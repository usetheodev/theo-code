# Agent Runtime — Macro Plan

## Visão Geral

Reescrita do `theo-agent-runtime` para atender a especificação técnica formal do runtime agentivo.
De um single-task LLM loop (~1.149 linhas) para um sistema com 3 state machines, 8 invariantes,
persistência, scheduler, failure model, capabilities e observabilidade (~3.850+ linhas).

## Invariantes Globais (CRÍTICOS)

1. Toda Task possui `task_id`, `session_id`, `state`, `created_at`
2. Toda Tool Call possui `call_id` único e rastreável
3. Todo Tool Result referencia um `call_id`
4. Nenhuma Task pode voltar de `completed` para `running`
5. Toda transição de estado gera um Event persistido
6. Toda execução agentiva possui `run_id`
7. Todo `resume` deve partir de snapshot consistente
8. Nenhuma execução pode rodar sem limite de orçamento (tempo/token)

## Grafo de Dependências

```
Fase 01 (Domain Types) ──► Fase 02 (Events) ──┬── Fase 03 (Task)
                                                ├── Fase 04 (ToolCall)
                                                └── Fase 05 (AgentRun)
                                                       │
                                      ┌────────────────┼────────────────┐
                                      ▼                ▼                ▼
                                Fase 06 (Retry)  Fase 07 (Budget)  Fase 08 (Scheduler)
                                      │                │                │
                                      ▼                ▼                ▼
                                Fase 09 (Capabilities)                  │
                                      │                                 │
                                      └────────────────┬────────────────┘
                                                       ▼
                                              Fase 10 (Persistence)
                                                       │
                                                       ▼
                                              Fase 11 (Observability)
                                                       │
                                                       ▼
                                              Fase 12 (Integration)
```

## Resumo de Escopo

| Fase | Foco | Linhas | Testes | Arquivos | Crate |
|------|-------|--------|--------|----------|-------|
| 01 | Core Types & State Machines | ~600 | 45 | 4 | theo-domain |
| 02 | Event System | ~300 | 15 | 2 | domain + runtime |
| 03 | Task Lifecycle | ~200 | 12 | 1 | runtime |
| 04 | Tool Call Lifecycle | ~250 | 15 | 1 | runtime |
| 05 | Agent Run Lifecycle | ~350 | 18 | 1 | runtime |
| 06 | Failure Model (Retry/DLQ) | ~290 | 15 | 3 | domain + runtime |
| 07 | Budget Enforcement | ~300 | 12 | 2 | domain + runtime |
| 08 | Scheduler & Concurrency | ~290 | 12 | 2 | runtime |
| 09 | Capabilities & Security | ~220 | 10 | 2 | domain + runtime |
| 10 | Persistence & Resume | ~350 | 15 | 2 | runtime |
| 11 | Observability | ~300 | 10 | 2 | runtime |
| 12 | Integration & Convergence | ~400 | 20 | 2 | runtime |
| **Total** | | **~3.850** | **199** | **24** | |

## Princípios

- Tipos puros em `theo-domain` (zero async, zero IO)
- Implementação em `theo-agent-runtime`
- Backward compat via facade (`AgentLoop::run`)
- Cada fase tem DoD explícito e meeting obrigatória
- Testes obrigatórios para toda lógica de negócio
