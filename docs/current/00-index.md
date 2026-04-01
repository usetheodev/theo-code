# Theo Code Agent — Documentacao Tecnica

## Visao Geral

Crate `theo-code-agent`: agent autonomo em Rust que porta o prototipo Python (50% SWE-bench Lite com Qwen3-30B) para producao. O LLM decide O QUE fazer; o Theo Code governa SE pode, COMO registrar, e QUANDO parar.

## Dominios

Os documentos seguem ordem logica de dependencia — cada dominio depende apenas dos anteriores.

| # | Dominio | Documento | Descricao |
|---|---------|-----------|-----------|
| 01 | Visao e Principios | [01-vision-and-principles.md](01-vision-and-principles.md) | Contexto, problema, resultado esperado, principios fundamentais |
| 02 | Arquitetura | [02-architecture.md](02-architecture.md) | Estrutura de crates, data flow, integracao com crates existentes, design patterns |
| 03 | Decision Control Plane | [03-decision-control-plane.md](03-decision-control-plane.md) | Decision lifecycle, versionamento imutavel, store, reuse |
| 04 | Policy Engine | [04-policy-engine.md](04-policy-engine.md) | Policy trait, mini-DSL, policies built-in, mapeamento XACML |
| 05 | Validation Pipeline | [05-validation-pipeline.md](05-validation-pipeline.md) | Pipeline deterministico < 50ms, reason codes, fail-fast |
| 06 | Governance Layer | [06-governance-layer.md](06-governance-layer.md) | GovernanceLayer, AgentIdentity, ScopedContext, AuditLog |
| 07 | Agent Loop | [07-agent-loop.md](07-agent-loop.md) | Main async loop, fases, transicoes, processamento de tool calls |
| 08 | LLM Client | [08-llm-client.md](08-llm-client.md) | LlmClient trait, OpenAI-compatible, Hermes parser, MessageHistory |
| 09 | Promise System | [09-promise-system.md](09-promise-system.md) | Promise trait, PromiseGate, GitDiffPromise, combinators |
| 10 | Context Loop e Decomposer | [10-context-loop-and-decomposer.md](10-context-loop-and-decomposer.md) | ContextLoopEngine, diagnostics, intent classification, templates |
| 11 | Checkpoint e Resiliencia | [11-checkpoint-and-resilience.md](11-checkpoint-and-resilience.md) | Checkpoint, session, idempotency, retry, circuit breaker, time-travel |
| 12 | Roadmap de Implementacao | [12-implementation-roadmap.md](12-implementation-roadmap.md) | Fases, estrategia de testes, verificacao, anti-patterns |

## Grafo de Dependencia

```
01 Visao/Principios
 └──► 02 Arquitetura
       ├──► 03 Decision Control Plane
       │     ├──► 04 Policy Engine
       │     └──► 05 Validation Pipeline ◄── 04
       │           └──► 06 Governance Layer ◄── 03
       │                 └──► 07 Agent Loop ◄── 08, 09, 10
       ├──► 08 LLM Client
       ├──► 09 Promise System ◄── 03
       ├──► 10 Context Loop / Decomposer
       └──► 11 Checkpoint / Resiliencia ◄── 03
             └──► 12 Roadmap ◄── todos
```

## Origem

Baseado no documento original [plan-theo-code-agent.md](plan-theo-code-agent.md), inspirado em:
- "Context Graphs as the Control Plane for the Agentic Enterprise" (IndyKite, Dave Bennett)
- "Roadmap tecnico para Context Graph como Control Plane de Decisao" (Deep Research Report)
