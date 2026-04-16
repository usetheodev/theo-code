# Theo Code — Architecture Reference

Technical reference for the Theo Code system architecture. Each document covers one bounded context.

This reference also serves as the architectural map for the **agent harness**: the combination of runtime, repository knowledge, tools, policies, and feedback loops that makes long-running coding agents reliable.

## Research-Grounded Principles

The architecture documents below are aligned to the research synthesized in `docs/pesquisas/`:

- **Harness-first architecture** (`harness-engineering.md`, Böckeler 2026) — Theo is not "model + tools"; it is an explicit harness composed of **guides** (feedforward), **sensors** (feedback), classified as **computational** (fast, deterministic) or **inferential** (LLM-based, probabilistic). Every control in the system has a place in this taxonomy.
- **Three regulation categories** — maintainability, architecture fitness, and behaviour. Complexity and tooling maturity vary across them; the docs identify which category each component serves.
- **Repository as system of record** (`harness-engineering-openai.md`, OpenAI 2026) — critical knowledge lives in versioned files under `docs/` and `.theo/`. `AGENTS.md` / `theo.md` are **tables of contents**, not encyclopedias. Progressive disclosure over monolithic prompts.
- **Incremental progress over one-shot execution** (`effective-harnesses-for-long-running-agents.md`, Anthropic 2025) — the runtime is optimized for bounded progress with verification. Session handoff through durable artifacts (session.jsonl, episode summaries, plans, snapshots) is a first-class concern, not an afterthought.
- **Context engine is read-only infrastructure** (`context-engine.md`) — GRAPHCTX and Code Wiki analyze, rank, and assemble context within a token budget. They never mutate, never call the LLM (except the separate wiki-enrichment pipeline), never make business decisions.
- **Agent legibility over human cleverness** — boundaries, layering, naming, and documentation are architectural assets because agents navigate them every turn. *"Anything the agent cannot access in-context effectively doesn't exist."*
- **Ashby's Law of Requisite Variety** — a regulator must have at least as much variety as the system it governs. Theo's layered architecture and capability-gated sub-agents are deliberate variety-reduction moves that make a comprehensive harness tractable.

## Mental Model — How The Pieces Fit

```
        ┌──────────────────────────────────────────────────────┐
        │  Model (stateless per call)                          │
        └──────────────────────┬───────────────────────────────┘
                               │
        ┌──────────────────────▼───────────────────────────────┐
        │  Behavioral harness  = theo-agent-runtime            │
        │  (rehydrate → plan → act → verify → persist)         │
        └─┬──────────────┬────────────────┬───────────────┬────┘
          │              │                │               │
 feedforward         feedforward       feedback        feedback
 (computational)     (inferential)     (computational) (inferential)
          │              │                │               │
┌─────────▼──────┐ ┌─────▼───────┐ ┌──────▼────────┐ ┌────▼──────────┐
│ GRAPHCTX       │ │ Skills,     │ │ Sensors, done │ │ Reflector,    │
│ Code Wiki      │ │ AGENTS.md,  │ │ gate, linters │ │ LLM review,   │
│ Sandbox policy │ │ plans       │ │ audit trail   │ │ evolution     │
│ Capability gt. │ │             │ │               │ │ loop          │
└────────────────┘ └─────────────┘ └───────────────┘ └───────────────┘
          │              │                │               │
          └──────────────┴────────────────┴───────────────┘
                               │
        ┌──────────────────────▼───────────────────────────────┐
        │  Repository as System of Record                      │
        │  docs/ (versioned) + .theo/ (operational)            │
        └──────────────────────────────────────────────────────┘
```

## Reading Order

For first-time readers, read in this order:
1. **01-overview** — taxonomy, session contract, invariants.
2. **02-domain** — the pure types everything else binds to.
3. **04-agent-runtime** — the behavioral harness (biggest crate, most moving parts).
4. **03-code-intelligence** — the feedforward context engine (Theo's differentiator).
5. **06-tooling** + **07-governance** — the guide/sensor surface.
6. **08-application** + **09-apps** — how it all composes.
7. **05-infrastructure** — LLM and auth plumbing (read last; mostly mechanical).
8. **10-application-legibility** — the largest acknowledged gap; read when evaluating Behaviour harness coverage.

## Documents

| Document | Bounded Context | Crates |
|---|---|---|
| [01-overview.md](01-overview.md) | System Overview | All |
| [02-domain.md](02-domain.md) | Domain Core | `theo-domain` |
| [03-code-intelligence.md](03-code-intelligence.md) | Code Intelligence | `theo-engine-graph`, `theo-engine-parser`, `theo-engine-retrieval` |
| [04-agent-runtime.md](04-agent-runtime.md) | Agent Runtime | `theo-agent-runtime` |
| [05-infrastructure.md](05-infrastructure.md) | Infrastructure | `theo-infra-llm`, `theo-infra-auth` |
| [06-tooling.md](06-tooling.md) | Tooling & Sandbox | `theo-tooling` |
| [07-governance.md](07-governance.md) | Governance | `theo-governance` |
| [08-application.md](08-application.md) | Application Layer | `theo-application`, `theo-api-contracts` |
| [09-apps.md](09-apps.md) | Surface Applications | `theo-cli`, `theo-desktop`, `theo-ui`, `theo-marklive`, `theo-benchmark` |
| [10-application-legibility.md](10-application-legibility.md) | Application Legibility (Gap Doc) | roadmap |

## Dependency Direction (Inviolable)

```
theo-domain         → (nothing)
theo-engine-*       → theo-domain
theo-governance     → theo-domain
theo-infra-*        → theo-domain
theo-tooling        → theo-domain
theo-agent-runtime  → theo-domain, theo-governance, theo-tooling, theo-infra-llm
theo-api-contracts  → (serde only)
theo-application    → all crates above
apps/*              → theo-application, theo-api-contracts
```

## Gaps vs Research (Honesty Surface)

The architecture is research-grounded, but several controls prescribed by the referenced research are **not yet implemented**. Listing them explicitly prevents the docs from overclaiming parity with OpenAI Codex / Anthropic Claude Agent SDK setups:

| Gap | Source | Theo status |
|---|---|---|
| **Typed feature list** (`features.json` with `"passes": false` entries) | Anthropic `effective-harnesses-…` §Environment management | Planned — see `04-agent-runtime.md` §Feature List Artifact |
| **UI/browser driving as sensor** (Chrome DevTools / Puppeteer-style) | OpenAI `§3`, Anthropic `§Testing` | Absent — see `10-application-legibility.md` |
| **Per-worktree app boot + ephemeral observability stack** | OpenAI `§3` | Absent — see `10-application-legibility.md` |
| **Custom architecture linters with LLM-targeted remediation** | OpenAI `§6` | Absent — today enforced at compile-time only via `Cargo.toml` |
| **Recurring doc-gardening / GC agent (PR-opening crawler)** | OpenAI `§10` | Partial — `theo pilot` runs on-demand, not as a continuous crawler |
| **Sensor-coverage metric** (is absence a sign of quality or missing detection?) | Böckeler `§A starting point` | Absent — planned in `07-governance.md` |
| **Merge philosophy / minimal blocking gates** | OpenAI `§7` | Undocumented — decision not captured |
| **Harness templates** (topology-level guide+sensor bundles) | Böckeler `§5` | Not scoped |

These gaps are tracked as roadmap items; `10-application-legibility.md` consolidates the ones that gate Theo's throughput ceiling.

### Open Research Questions (No Fix Yet)

Some concerns raised by the research are genuinely unresolved industry-wide. Flagged here so the docs do not pretend otherwise:

- **Harness coherence as it grows.** Böckeler `§A starting point`: *"How do we keep a harness coherent as it grows, with guides and sensors in sync, not contradicting each other?"* Theo has no automated coherence check across its guides and sensors today; this is the same open problem.
- **Trust in agent trade-offs.** When instructions and feedback signals point in different directions, how far can the agent be trusted? Theo resolves this by conservative escalation (capability gates, Plan mode, done-gate blocking) but the deeper question is unsettled.
- **Single general agent vs multi-agent for long-running work.** Anthropic explicitly keeps this open. Theo picks multi-agent with capability gating; the benchmark harness is the instrument expected to eventually answer this empirically.
