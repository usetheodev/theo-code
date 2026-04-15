---
paths:
  - "crates/**/*.rs"
  - "apps/**/*.rs"
---

# Architectural Boundaries

## Bounded Contexts

1. **Code Intelligence**: `theo-engine-graph`, `theo-engine-parser`, `theo-engine-retrieval`
   - Parser and graph are read-only over source code
   - Retrieval consumes graph, never the reverse

2. **Agent Runtime**: `theo-agent-runtime`
   - Orchestrates LLM + tools + governance
   - State machine governs phase transitions

3. **Governance**: `theo-governance`
   - Policy engine, simplified
   - Sits in the critical path but lightweight

4. **Infrastructure**: `theo-infra-llm`, `theo-infra-auth`, `theo-tooling`
   - Concrete implementations behind domain traits

## Dependency Direction (INVIOLABLE)

```
theo-domain         → (nothing)
theo-engine-*       → theo-domain only
theo-governance     → theo-domain only
theo-infra-*        → theo-domain only
theo-tooling        → theo-domain only
theo-agent-runtime  → theo-domain, theo-governance
theo-api-contracts  → theo-domain only
theo-application    → all crates above
apps/*              → theo-application, theo-api-contracts
```

## Prohibitions

- Apps NEVER import engine/infra crates directly
- `theo-domain` NEVER depends on any other crate
- Circular dependencies are forbidden
- Benchmark/research code never enters production runtime
- No `unwrap()` in production code paths — use `?` or typed errors
