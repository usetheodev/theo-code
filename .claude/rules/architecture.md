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

Each line below gives the **upper bound** of workspace crates a target may
depend on (ADR-010 — a crate may declare fewer deps than listed, but never
more). `check-arch-contract.sh` enforces the bound.

```
theo-domain              → (nothing)
theo-engine-graph        → theo-domain
theo-engine-parser       → theo-domain
theo-engine-retrieval    → theo-domain, theo-engine-graph, theo-engine-parser   (ADR-011)
theo-governance          → theo-domain
theo-infra-llm           → theo-domain
theo-infra-auth          → theo-domain
theo-infra-memory        → theo-domain, theo-engine-retrieval (optional, feature-gated)   (ADR-011)
theo-tooling             → theo-domain
theo-agent-runtime       → theo-domain, theo-governance,
                            theo-infra-llm, theo-infra-auth, theo-tooling   (ADR-016)
theo-api-contracts       → theo-domain
theo-application         → all crates above
apps/*                   → theo-application, theo-api-contracts
```

## Prohibitions

- Apps NEVER import engine/infra crates directly
- `theo-domain` NEVER depends on any other crate
- Circular dependencies are forbidden
- Benchmark/research code never enters production runtime
- No `unwrap()` in production code paths — use `?` or typed errors
