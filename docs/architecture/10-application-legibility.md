# 10 — Application Legibility (Gap Doc)

**Status:** Roadmap. Nothing in this document describes a shipped capability.

This document exists because the other nine architecture docs risk giving a false impression of parity with the research. Two of the most influential pieces referenced by this reference set — OpenAI's `harness-engineering-openai.md` §3 and Anthropic's `effective-harnesses-for-long-running-agents.md` §Testing — identify **application legibility** as the decisive lever that unlocks high-throughput agent-driven development. Theo does not currently build against this lever at all.

Rather than pretend otherwise, this doc lays out the gap, the proposed shape of the solution, and the decisions that need to happen before any of it can be implemented.

## What "application legibility" means

Böckeler's companion concept (`harness-engineering.md`, "Ambient affordances") is the structural property of an environment that makes it *navigable* and *tractable* for an agent. Application legibility is the runtime-side counterpart: **the running application, its UI, its logs, and its metrics are themselves inspectable by the agent, in the same loop where it is making changes.**

From `harness-engineering-openai.md` §3:

> *We made the app bootable per git worktree, so Codex could launch and drive one instance per change. We also wired the Chrome DevTools Protocol into the agent runtime and created skills for working with DOM snapshots, screenshots, and navigation. This enabled Codex to reproduce bugs, validate fixes, and reason about UI behavior directly. [...] Logs, metrics, and traces are exposed to Codex via a local observability stack that's ephemeral for any given worktree.*

From `effective-harnesses-for-long-running-agents.md` §Testing:

> *Providing Claude with these kinds of testing tools [browser automation] dramatically improved performance, as the agent was able to identify and fix bugs that weren't obvious from the code alone.*

Without this, the Behaviour harness (the third of Böckeler's three categories) degenerates to "the test suite is green" — which both sources agree is insufficient.

## Current state of Theo

| Capability | Status |
|---|---|
| App bootable per git worktree | Not scoped |
| UI driving (Chrome DevTools / Puppeteer / Playwright) | Not scoped |
| Screenshot capture for the agent | Not scoped |
| Local observability stack (logs/metrics/traces) queryable by the agent | Not scoped |
| DOM snapshot tooling | Not scoped |
| Feature-level end-to-end verification (`features.json`, see `04-agent-runtime.md`) | Roadmap |

The closest thing today is the `webfetch` tool, which fetches a URL and returns extracted text — useful for docs, not for exercising a running application.

## Proposed architecture (draft)

This is a **proposal, not a plan**. An ADR should lock in decisions before implementation.

```
┌─────────────────────────────────────────────────────────────┐
│  Agent turn (write → verify)                                │
└──────────────────┬──────────────────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────────────────┐
│  Worktree-scoped app runner                                 │
│    - Detects the application type (web, CLI, service)       │
│    - Starts an ephemeral instance using a project-supplied  │
│      `init.sh` (per Anthropic §Environment management)      │
│    - Assigns a local port; publishes base URL to sensors    │
└──────────────────┬──────────────────────────────────────────┘
                   │
      ┌────────────┴────────────┬─────────────────────┐
      ▼                         ▼                     ▼
┌─────────────┐      ┌────────────────────┐   ┌──────────────┐
│ browser     │      │ observability      │   │ log tail     │
│ tool        │      │ tap                │   │ sensor       │
│ (CDP / PW)  │      │ (LogQL / PromQL    │   │              │
│             │      │  equivalents)      │   │              │
└─────────────┘      └────────────────────┘   └──────────────┘
      │                         │                     │
      └─────────────────────────┴─────────────────────┘
                   │ feedback into agent context
                   ▼
    SensorResult (same contract as sensor.rs today)
```

### Components and their harness category

| Component | Direction | Execution | Notes |
|---|---|---|---|
| Worktree app runner | Guide (affordance) | Computational | Prerequisite — nothing else works without it |
| Browser tool (DOM, click, screenshot) | Sensor (mostly) | Computational | Can also act as guide when used pre-edit to capture baseline |
| Observability tap (logs/metrics/traces) | Sensor | Computational | Ephemeral per worktree — tears down at session end |
| Vision-based regression check | Sensor | Inferential | Optional; compares pre/post screenshots with LLM as judge |
| Feature-level verifier (consumes `features.json`) | Sensor | Computational (runs steps) + Inferential (judges "did it look right") | Flips `passes: false → true` in the features file |

### Why this belongs in its own bounded context

The other nine docs are organized around *the codebase*. Application legibility is organized around *the running system*. Mixing the two in, say, `06-tooling.md` would muddle the responsibility of `theo-tooling` (read/write/search the repository) with a fundamentally different concern (drive and observe a running process). The likely shape:

- A new crate `theo-infra-runtime-probe` (or similar name) for worktree boot, port assignment, process supervision.
- New tool modules under `theo-tooling/src/` for browser driving and observability querying — these stay in `theo-tooling` because they're exposed to the agent as tools.
- Governance additions in `theo-governance` for sandboxing browser processes (they break the current bash sandbox assumptions about network isolation).

## Open decisions

1. **Browser driver.** Chrome DevTools Protocol directly, Playwright MCP, Puppeteer MCP, or a neutral wrapper that accepts any of them? CDP has the highest leverage but is Chromium-only.
2. **Observability stack.** Match OpenAI's Vector + Victoria* stack, or use a minimal OpenTelemetry collector with an in-memory backend? The minimal path is cheaper but may not scale past toy apps.
3. **Per-worktree isolation.** Docker? Nix sandboxes? Plain process groups with port assignment? Each has different sandbox-breakage profiles.
4. **Feature list schema evolution.** `04-agent-runtime.md` proposes `features.json` v1; this document would need it to grow a `verification_steps` array that the browser tool can execute directly. Co-design required.
5. **Vision sensors.** Opt-in, opt-out, or tiered? Inferential sensors have cost — running a screenshot diff on every turn is expensive.
6. **Behaviour harness boundary.** At what point does "driving the app to verify features" become the responsibility of `theo-benchmark` rather than the runtime? Today `theo-benchmark` is isolated; this line could blur.

## Why not skip this document until we build it

Two reasons:

1. **Honesty.** Without this doc, `README.md` can claim the system is "harness-first" while silently omitting the single most material gap. Calling the gap out by name is cheaper than explaining its absence to every new reader.
2. **Anchoring.** When the roadmap work starts, the team has a document to argue against rather than a blank page. ADRs can cite this doc as the starting context.

Until this is built, the Behaviour harness category in `07-governance.md` remains partial — `cargo test` passing is a necessary signal, not a sufficient one.
