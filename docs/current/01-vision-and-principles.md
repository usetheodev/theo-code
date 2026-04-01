# 01 — Visao e Principios

## Contexto

O agent Python (`theo_agent.py`) provou 50% no SWE-bench Lite com Qwen3-30B via 3 mecanismos:
- **State Machine** — fases deterministicas
- **Promise Gate** — done() bloqueado ate prova de resultado
- **Context Loops** — injecao de contexto relevante

Agora precisamos portar para Rust como crate de producao, integrado ao engine existente (812 testes, 7 crates).

## Problema

O agent Python e um prototipo com limitacoes fundamentais:
- Subprocess calls para `theo-code context` a cada query
- String parsing fragil de stdout
- Sem tipagem forte
- Sem session persistence
- Cada chamada rebuilda o scorer (~30s)

Como library Rust, o scorer vive em memoria (~2s/query).

## Resultado Esperado

Crate `theo-code-agent` com:
- Agent loop async
- Decision control plane deterministico
- Context loops integrados
- Integracao direta com Pipeline (zero subprocess)

---

## Principios Fundamentais

### 1. Decouple Agent Intelligence from Agent Governance

O LLM decide **O QUE** fazer. O Theo Code governa **SE** pode fazer, **COMO** registrar, e **QUANDO** parar.

```
  LLM (Intelligence)          Theo Code (Governance / PDP)
  ─────────────────           ──────────────────────────────
  "Quero editar X"    ──►     ValidationPipeline: scope ok? policy ok? → ALLOW/DENY
  "Done!"             ──►     PromiseGate: git diff existe? decisao ACTIVE?
  "search Y"          ──►     ScopedContext: pode acessar Y nesta delegation chain?
  "Tentar de novo"    ──►     DecisionReuse: ja resolvemos algo similar? → reutiliza
```

### 2. Context Graph = Control Plane (PDP)

> "It's not just enough to have data. We need to operationalize it." — Dave Bennett

O CodeGraph NAO e apenas um indice de busca. E o **Policy Decision Point (PDP)**:

- **Provenance**: de onde veio cada informacao (git commit, arquivo, funcao)
- **Temporal Validity**: quando a informacao e valida (recency scores, co-change decay, valid_from/valid_to)
- **Decision Traces**: cada decisao materializada com lifecycle completo (PROPOSED→ACTIVE→COMPLETED)
- **Decision Reuse**: decisoes passadas reutilizaveis se contexto e compativel
- **Agent Identity**: agent como no no grafo com ownership, capabilities e trust level
- **Scoped Access**: delegation chain determina o que o agent pode ver/editar
- **Policy Enforcement**: regras deterministicas avaliadas sem LLM (P99 < 50ms)

### 3. No Standing Privilege + Fail-Closed

> "Na ausencia de autorizacao explicita e consistente, o sistema deve negar."

```
User("Fix bug X") → Agent(task="bug_fix") → SubFlow(scope=reduced)
     subject           actor                   actor_delegated

- Sem autorizacao explicita → DENY (fail-closed)
- Cada nivel REDUZ o escopo
- done() bloqueado ate promise cumprida
- LLM NUNCA no hot path de validacao (deterministico)
```

### 4. Determinismo no Hot Path

> "A decisao ALLOW/BLOCK deve ser computada sem LLM, com regras e politicas auditaveis."

O agent loop chama o LLM para raciocinar. Mas TODA validacao (pode editar? pode completar? escopo ok?) e deterministica:

```
ValidationPipeline (< 50ms, sem LLM):
  Parse → Fetch Decision → Check Status → Time Validity → Scope Match → Conditions → Policy → ALLOW/DENY
```

---

## Visao: Antes vs Depois

```
ANTES (Python prototipo):
  Agent → subprocess → theo-code CLI → stdout → parse → agent
  (30s/query, sem rastreabilidade, sem governance, sem reuse)

DEPOIS (Rust production):
  Agent → Pipeline.search() → ValidationPipeline.validate() → DecisionStore.record()
  (2s/query, full traceability, deterministic governance, decision reuse)
```
