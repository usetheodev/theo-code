---
type: report
question: "How should theo-code implement smart model routing for Anthropic + OpenAI code agents?"
generated_at: 2026-04-20T00:00:00Z
confidence: 0.78
sources_used: 21
---

# Report: Smart Model Routing for theo-code

## Executive Summary

Smart model routing is a first-class capability in every frontier code agent except theo-code. Anthropic ships an **orchestrator-worker** pattern (Opus plans, Sonnet/Haiku execute) with subagent model overrides; OpenAI's Agents SDK uses typed **handoffs** to swap models mid-run. The reference repos converge on the same design shape: a small set of **named model slots/roles** (normal, compact, vision, subagent) resolved from config with cascading defaults. theo-code has zero routing — one `model` field on `AgentConfig` and one `LlmClient` per agent. A minimal, correct fix is: introduce a `ModelRouter` trait in `theo-domain`, a rule-based default implementation (Hermes-style) in `theo-infra-llm`, and a `role` parameter threaded through `theo-application` so that compaction, subagents, and vision calls can request a cheaper model. Full "learned classifier" routing (RouteLLM/FrugalGPT cascades) is out of scope for MVP and should stay behind a feature flag.

## Analysis

### 1. State of the Art (2026)

#### 1.1 Anthropic: orchestrator-worker + subagent overrides

Anthropic's public "Claude Code" docs specify that every subagent definition carries a `model` field accepting the aliases `sonnet`, `opus`, `haiku`, a full model ID, or `inherit` ([Anthropic — Create custom subagents](https://docs.anthropic.com/en/docs/claude-code/sub-agents)). Internally, Anthropic's own multi-agent research system uses the **orchestrator-worker** pattern: a lead agent on Opus 4 plans and dispatches; worker subagents on Sonnet 4 execute in parallel. They report a **90.2% improvement on BrowseComp-style tasks** over a single-Opus baseline (summarised in the "Claude Code Sub-agents" write-up at [codewithseb.com](https://www.codewithseb.com/blog/claude-code-sub-agents-multi-agent-systems-guide) and echoed in [MindStudio's "Advisor Strategy"](https://www.mindstudio.ai/blog/claude-code-advisor-strategy-opus-sonnet-haiku)).

The economics make this viable because **Anthropic ships prompt-caching discounts** (5-minute/1-hour TTLs on the `cache_control` beta). When the orchestrator re-uses a large system prompt across many worker turns, the worker pays ~10% of the input cost for cached prefix tokens — so dispatching 5 Sonnet workers from a cached Opus plan costs less than one non-cached Opus turn. The Advisor Strategy write-up reports **−11% cost, +2% benchmark** compared to a single-Opus policy.

Limitation (verified): Anthropic's current SDK does **not** support cross-provider routing inside one session — the model field accepts only the Claude aliases, all pointing at `api.anthropic.com` ([claude-code issue #38698](https://github.com/anthropics/claude-code/issues/38698)). Any "Ollama for subagents, Anthropic for orchestrator" routing must be implemented by the harness (theo-code itself), not by the SDK.

#### 1.2 OpenAI: Agents SDK handoffs

OpenAI's Agents SDK treats model selection as a **handoff** — a first-class tool call that transfers control to a differently-configured agent. The canonical example from `openai-agents-python/examples` defines `triage_agent = Agent(..., handoffs=[billing_agent, coder_agent])`, where each target agent can have its own `model=` (e.g. `gpt-5`, `o3-mini`, `gpt-5-codex`). Difficulty-aware routing is done by the triage agent's prompt alone, which picks the handoff target based on the user message. I could not verify a specific GPT-5 model card URL from the environment, so the exact model IDs above are the ones referenced in the OpenAI post, not independently verified.

OpenAI's "Harness Engineering" post (`/home/paulo/theo-code/docs/pesquisas/harness-engineering-openai.md`, Ryan Lopopolo, Feb 2026) describes the agent-to-agent review loop but does **not** describe model routing explicitly — it assumes Codex as the single driver and uses specialised **prompts** rather than specialised **models** for review vs. implementation.

#### 1.3 Academic landmarks

- **FrugalGPT** ([Chen et al., 2023, arXiv:2305.05176](https://arxiv.org/abs/2305.05176), TMLR 12/2024): LLM cascade. Query is sent to cheap model first; an *answer scorer* decides whether to accept or escalate to a stronger model. Reports **up to 98% cost reduction** at GPT-4 quality. Key weakness for coding agents: adds tail latency for any query that escalates (sequential).
- **RouteLLM** ([Ong et al., 2024, arXiv:2406.18665](https://arxiv.org/pdf/2406.18665)): trains a preference-data classifier that picks one LLM per query. Reports **−85% cost on MT-Bench** vs GPT-4-only. Key strength over FrugalGPT: single call, no cascade latency. Key weakness: requires training data and a maintained classifier.
- **Semantic Router** ([Aurelio Labs, github.com/aurelio-labs/semantic-router](https://github.com/aurelio-labs/semantic-router)): embeds prompts and matches against centroids defined per route. No training data required, but quality depends on well-crafted exemplars.
- **Mixture-of-Routers / MoR**: I could not verify a specific canonical MoR paper from the environment. The general idea — multiple routers ensembled — is mentioned in survey work like *Champaign Magazine*'s "Router-R1" roundup ([champaignmagazine.com](https://champaignmagazine.com/2025/10/16/router-r1-and-llm-routing-research/)) but should be treated as folklore until a specific citation is added.

#### 1.4 Cost/latency tradeoff

A back-of-envelope from the numbers above: if 50% of turns in a coding session are "simple" (short answer, no refactor) and simple turns cost ~5× less on Haiku than Sonnet, a rule-based router saves **~40% total spend** at zero added latency (rules are microseconds). A cascade router (FrugalGPT) saves more in the best case (~80%) but adds one full round-trip for every escalation, which is the worst failure mode for an interactive coding agent. **Recommendation for theo-code: start with rules, defer cascades.**

### 2. Reference Repo Audit

#### 2.1 opendev (Rust) — "named slots" pattern

opendev ships a **slot-based** config: one default model plus optional overrides per workflow phase. The slots are declared in `opendev-cli`'s setup flow: `Vision` and `Compact` ([`referencias/opendev/crates/opendev-cli/src/setup/mod.rs:108-126`](file:///home/paulo/theo-code/referencias/opendev/crates/opendev-cli/src/setup/mod.rs)). They are stored on the config struct as `model_vlm`/`model_vlm_provider` plus a generic `agents: HashMap<String, AgentConfigInline>` map that lets users define more slots at will (e.g. `agents.compact`, `agents.thinking`).

Selection at runtime is a direct lookup:
- [`referencias/opendev/crates/opendev-models/src/config/agent.rs:22-66`](file:///home/paulo/theo-code/referencias/opendev/crates/opendev-models/src/config/agent.rs) defines `AgentConfigInline { model, provider, prompt, temperature, ... }` — optional overrides cascade onto the default.
- The "Normal/Thinking/Compact/Self-Critique/VLM" split referenced in the prompt is realised as slot **names** in `agents.<name>`, not as a closed Rust enum.

Pattern for theo-code: slot dictionary with cascading defaults (slot → explicit override → global default → hard-coded fallback).

#### 2.2 Archon (TypeScript) — per-node model overrides

Archon's routing is **workflow-level**, not model-level. `packages/workflows/src/router.ts:73-138` builds a router prompt that asks the LLM itself which workflow to invoke (`/invoke-workflow <name>`). Each workflow node may then set its own model/provider; at execution time the Claude provider does a simple cascade:

```ts
// referencias/Archon/packages/providers/src/claude/provider.ts:562
model: requestOptions?.model ?? assistantDefaults.model,
```

i.e. node-level > workflow-level > `assistants.claude.model` in `.archon/config.yaml` > SDK default. Archon validates the model/provider combo at workflow-load time (`packages/workflows/src/model-validation.ts`) so mismatches fail fast. The "router" in Archon is a workflow picker, not a model picker — model choice rides along with whichever workflow the router picked.

#### 2.3 pi-mono (TypeScript) — registry + scoping, no role routing

`packages/coding-agent/src/core/model-resolver.ts:14-38` defines `defaultModelPerProvider` (a `Record<KnownProvider, string>`) and `parseModelPattern()` for pattern matching (`claude-opus` → alias > dated version). There is a **scope** concept (`resolveModelScope()` at line 246) that lets the user pre-filter which models are available in a session, and a **restore-from-session** fallback (line 559). But the agent picks **one** model per session; there is no role-based routing to a different model for compaction or sub-tasks. The "smart" part is purely user-ergonomics (fuzzy matching, fallback to a different provider when auth is missing).

#### 2.4 hermes-agent (Python) — explicit smart_model_routing

Hermes ships an actual file named `smart_model_routing.py` at [`referencias/hermes-agent/agent/smart_model_routing.py`](file:///home/paulo/theo-code/referencias/hermes-agent/agent/smart_model_routing.py). The algorithm (lines 62-107) is **rule-based and deterministic**:

1. Config opts in via `routing.enabled = true`.
2. User message is routed to the cheap model only if **all** of: ≤ `max_simple_chars` (default 160), ≤ `max_simple_words` (default 28), ≤ 1 newline, no backticks, no URL, and no word intersects `_COMPLEX_KEYWORDS` (lines 11-46: `debug`, `implement`, `refactor`, `analyze`, `pytest`, `docker`, ...).
3. Otherwise fall through to the primary model.

Additionally, Hermes has a **secondary auxiliary router** in [`referencias/hermes-agent/agent/auxiliary_client.py:1-35`](file:///home/paulo/theo-code/referencias/hermes-agent/agent/auxiliary_client.py) with separate resolution chains for text tasks (OpenRouter → Nous → custom → Codex OAuth → Anthropic → direct-key providers) and vision tasks. Per-task overrides live in `config.yaml` under `auxiliary.<task>.{provider,model}` (line 26-27). HTTP 402 triggers an automatic fallback to the next provider in the chain.

Hermes is by far the most theo-code-aligned reference: explicit rules, feature-flagged, conservative by design. **Adopt wholesale.**

#### 2.5 opencode (TypeScript) — typed agent schema

`packages/opencode/src/agent/agent.ts:27-52` defines an `Info` Zod schema where each agent declares an optional `model: { modelID, providerID }` tuple and a `mode: "subagent" | "primary" | "all"`. There is no runtime routing logic in `agent/` — the model selection is whatever the agent config declares, resolved once when the agent is instantiated. The design choice worth borrowing: **model is an optional property on the agent definition**, not a separate router.

### 3. theo-code Gap Analysis

Current state (grep-verified):

- [`crates/theo-infra-llm/src/client.rs:31-42`](file:///home/paulo/theo-code/crates/theo-infra-llm/src/client.rs) — `LlmClient` owns one `model: String`. No helper to spawn a sibling client with a different model.
- [`crates/theo-infra-llm/src/model_limits.rs:26-74`](file:///home/paulo/theo-code/crates/theo-infra-llm/src/model_limits.rs) — knows about `claude-opus-4`, `claude-sonnet-4`, `claude-haiku-4`, but only for token-window math. No pricing, no latency hints, no "tier" concept.
- [`crates/theo-agent-runtime/src/config.rs:252`](file:///home/paulo/theo-code/crates/theo-agent-runtime/src/config.rs) — `AgentConfig.model: String`. One model. Downstream, `RunEngine` passes this unchanged to every `chat()` call.
- [`crates/theo-agent-runtime/src/subagent/mod.rs:20-101`](file:///home/paulo/theo-code/crates/theo-agent-runtime/src/subagent/mod.rs) — `SubAgentRole` already enumerates `Explorer/Implementer/Verifier/Reviewer` with per-role `capability_set()` and `system_prompt()`, but **no** `model()` method. Sub-agents inherit the parent's `AgentConfig.model`.
- [`crates/theo-application/src/`](file:///home/paulo/theo-code/crates/theo-application/src) — `use_cases/` orchestrates agents but has no awareness of "compaction should use a cheap model" or "vision should route to a multimodal model".
- `ripgrep` for `(router|Router|route|Route)` across the workspace returns only retrieval-related matches — zero LLM-routing code in the runtime.

Where would a router live?

- **`theo-domain`**: trait + types only (roles, `RoutingContext`, `ModelChoice`). Depends on nothing.
- **`theo-infra-llm`**: concrete `RuleBasedRouter` (Hermes-style) + a `CascadeRouter` shim (feature-flagged). Owns the pricing table.
- **`theo-application`**: wires a `Box<dyn ModelRouter>` into the agent runtime; translates `RoutingContext` from the current phase (`Compact`, `Subagent(role)`, `Normal`, etc.).
- **`theo-agent-runtime`** must **not** own the router (it already violates the dep rule if it imports infra). The runtime consumes a `&dyn ModelRouter` injected via `AgentConfig`.

Minimum data the router needs:
- User message text (first/latest turn) — for keyword rules
- Conversation turn count and cumulative token count — for budget-aware escalation
- The current phase (`Normal`, `Compact`, `Vision`, `Subagent(role)`, `SelfCritique`) — for slot lookup
- Capability requirements (vision? long context? tool use?) — to rule out models that can't serve the call
- Optional: last-turn failure mode (rate limit, overflow) — for fallback cascade

Minimum code change:
1. New trait `ModelRouter` in `theo-domain` (~30 LOC).
2. `ModelChoice { provider, model, max_tokens, reasoning_effort }` struct.
3. Rule-based impl in `theo-infra-llm` behind feature `routing` (~120 LOC).
4. `AgentConfig.router: Option<Arc<dyn ModelRouter>>` + call site changes in `RunEngine` (~50 LOC).

### 4. Recommended Architecture

#### 4.1 Trait surface (`theo-domain`)

```rust
// theo-domain/src/routing.rs  (new module)
pub enum RoutingPhase {
    Normal,
    Compaction,
    Vision,
    Subagent(SubAgentRoleId), // string id, not the runtime enum — domain has no runtime dep
    SelfCritique,
    Classifier, // cheap model used as a router for another router (meta)
}

pub struct RoutingContext<'a> {
    pub phase: RoutingPhase,
    pub latest_user_message: Option<&'a str>,
    pub conversation_tokens: u64,
    pub iteration: usize,
    pub requires_vision: bool,
    pub requires_tool_use: bool,
    pub previous_failure: Option<RoutingFailureHint>,
}

pub struct ModelChoice {
    pub provider_id: String,
    pub model_id: String,
    pub max_output_tokens: u32,
    pub reasoning_effort: Option<String>,
    pub routing_reason: &'static str, // "simple_turn", "vision_required", "default", ...
}

pub trait ModelRouter: Send + Sync {
    fn route(&self, ctx: &RoutingContext<'_>) -> ModelChoice;
    /// Returns the next choice to try when `previous` failed with `hint`.
    fn fallback(&self, previous: &ModelChoice, hint: RoutingFailureHint) -> Option<ModelChoice>;
}
```

This keeps `theo-domain → (nothing)` intact (verified against `.claude/rules/architecture.md`). `SubAgentRoleId` is a newtype over `&'static str` — the enum mapping lives in `theo-agent-runtime` where `SubAgentRole` already exists ([`subagent/mod.rs:20`](file:///home/paulo/theo-code/crates/theo-agent-runtime/src/subagent/mod.rs)).

#### 4.2 Classifier: deterministic rules (MVP)

Adopt Hermes' rule set verbatim, Rust-ified:

```rust
// theo-infra-llm/src/routing/rules.rs
const COMPLEX_KEYWORDS: &[&str] = &[
    "debug", "refactor", "implement", "traceback", "exception", "analyze",
    "architecture", "design", "optimize", "review", "pytest", "docker", ...
];

impl ModelRouter for RuleBasedRouter {
    fn route(&self, ctx: &RoutingContext<'_>) -> ModelChoice {
        match ctx.phase {
            RoutingPhase::Compaction => self.slot("compact").or_default(&self.default),
            RoutingPhase::Vision     => self.slot("vision").or_default(&self.default),
            RoutingPhase::Subagent(role) => self.slot_for_role(role),
            RoutingPhase::Normal => {
                if let Some(msg) = ctx.latest_user_message {
                    if is_simple_turn(msg) {
                        return self.cheap.clone();
                    }
                }
                self.default.clone()
            }
            _ => self.default.clone(),
        }
    }
}
```

Offline-first: rules run on-device, zero network. Learned classifier (Haiku-backed) is a later phase behind feature flag `routing-learned`.

#### 4.3 Roles → models mapping

| Role         | Anthropic         | OpenAI            | Local (Ollama)       |
|--------------|-------------------|-------------------|----------------------|
| Orchestrator | claude-opus-4     | gpt-5 / o3        | qwen3-coder-32b      |
| Implementer  | claude-sonnet-4   | gpt-5-codex       | qwen3-coder-14b      |
| Navigator    | claude-haiku-4    | gpt-4o-mini       | qwen3-4b             |
| Reviewer     | claude-opus-4     | o3                | qwen3-coder-32b      |
| Compaction   | claude-haiku-4    | gpt-4o-mini       | qwen3-4b             |
| Vision       | claude-sonnet-4   | gpt-4o            | qwen3-vl             |

(Caveat: OpenAI model IDs are as referenced in reference repos; I could not independently verify the current GPT-5 family naming from this environment.)

#### 4.4 Composition with existing features

- **`should_defer` / scheduler** ([`crates/theo-agent-runtime/src/scheduler.rs`](file:///home/paulo/theo-code/crates/theo-agent-runtime/src/scheduler.rs)): no conflict. Router is called **after** deferral decision, before the actual LLM request.
- **`batch_execute` meta-tool** (git log: `edb4619`): routing is per-turn; a batch runs entirely inside one LLM call, so a single `ModelChoice` governs the batch. No change needed.
- **`truncation_rule` / compaction_stages** ([`crates/theo-agent-runtime/src/compaction_stages.rs`](file:///home/paulo/theo-code/crates/theo-agent-runtime/src/compaction_stages.rs)): when a compaction stage runs, it issues its own LLM call; that call **must** go through the router with `phase = Compaction`. Conflict: current code calls `LlmClient::chat` directly with the session model. Fix: take `&dyn ModelRouter` as a constructor arg and ask for `RoutingPhase::Compaction` before building the `ChatRequest`.
- **`doom_loop_threshold`** ([`config.rs:280`](file:///home/paulo/theo-code/crates/theo-agent-runtime/src/config.rs)): potential interaction — if we escalate model tier when a loop is detected, the router should accept a `DoomLoopDetected` signal in `RoutingContext` to bump to the stronger tier.

#### 4.5 Failure handling

Layered cascade, FrugalGPT-style but bounded:

1. **Overflow** (`ContextWindowWillOverflow` from [`model_limits.rs:89`](file:///home/paulo/theo-code/crates/theo-infra-llm/src/model_limits.rs)): router returns fallback with a larger-window model (e.g. Gemini 2.5 Pro, 2M) or forces compaction first.
2. **429 rate limit**: router returns fallback to a sibling provider with same tier (e.g. Anthropic Sonnet → OpenRouter Sonnet) — mirroring Hermes' HTTP 402 pattern ([`auxiliary_client.py:30-34`](file:///home/paulo/theo-code/referencias/hermes-agent/agent/auxiliary_client.py)).
3. **5xx / timeout**: one retry on same model, then fall back to same-tier different provider.
4. **Budget exhausted**: hard stop with typed error; never silently downgrade quality.

Bound: max 2 fallback hops per turn to avoid unbounded cascades.

#### 4.6 Configuration surface

Extend the existing `.theo/config.toml` (the repo already parses `[model]`-style config — see `project_config.rs`):

```toml
[model]
default = "claude-sonnet-4-7"
provider = "anthropic"

[routing]
enabled = true
strategy = "rules"  # "rules" | "learned" | "cascade" (future)
max_simple_chars = 160
max_simple_words = 28

[routing.slots.cheap]
model = "claude-haiku-4-5"
provider = "anthropic"

[routing.slots.compact]
model = "claude-haiku-4-5"
provider = "anthropic"

[routing.slots.vision]
model = "claude-sonnet-4-7"
provider = "anthropic"

[routing.slots.subagent_explorer]
model = "claude-haiku-4-5"
provider = "anthropic"

[routing.slots.subagent_implementer]
# inherits from [model]
```

Per-session override: CLI flag `--router off` disables routing; `--model-for compaction=gpt-4o-mini` overrides a single slot. Env: `THEO_ROUTING_DISABLED=1`.

### 5. Implementation Roadmap

Sized for theo-code's evolution-loop cadence (each phase ≤ 200 LOC).

#### Phase 1 — Domain types + null router (RED → GREEN)
- **Scope**: `theo-domain::routing` module with `ModelRouter`, `RoutingContext`, `RoutingPhase`, `ModelChoice`, `RoutingFailureHint`. A `NullRouter` that always returns the default — behaviour-preserving.
- **LOC**: ~80
- **Risk**: Very low. No runtime wiring yet.
- **Test**: unit tests for `NullRouter` returning expected choice for every phase; trait object-safety compile test.
- **Dependencies**: none.
- **TDD order**: RED — write `test_null_router_returns_default_for_every_phase` → GREEN — impl → REFACTOR.

#### Phase 2 — RuleBasedRouter in theo-infra-llm
- **Scope**: port `hermes-agent/agent/smart_model_routing.py:62-107` to Rust with the full `COMPLEX_KEYWORDS` set. Add a `PricingTable` struct for model tiers. Feature-gate behind `routing-rules` (default on).
- **LOC**: ~150
- **Risk**: Low. Keyword set needs review but is data, not code.
- **Test**: table-driven test with 20 sample prompts (10 "simple", 10 "complex"). Reproduce three cases from `smart_model_routing.py` (lines 83-101) bit-for-bit.
- **Dependencies**: Phase 1.

#### Phase 3 — Wire router into AgentConfig + RunEngine
- **Scope**: `AgentConfig.router: Option<Arc<dyn ModelRouter>>` defaulting to `NullRouter`. At the single call-site in `RunEngine` that builds a `ChatRequest`, call `router.route(&ctx)` and apply the `ModelChoice` to the request. Thread `RoutingPhase::Normal` for now; wire other phases in later passes.
- **LOC**: ~100
- **Risk**: Medium. Touches the hot path; must be backward-compatible.
- **Test**: integration test with `MockRouter` asserting `chat()` receives the router-chosen model. Regression test that `router = None` behaves exactly like current code.
- **Dependencies**: Phase 2.

#### Phase 4 — Route compaction + subagent phases
- **Scope**: compaction_stages calls `router.route(ctx with phase=Compaction)`. Each `SubAgentRole` gets a `role_id() -> SubAgentRoleId` method; the runtime passes this to the router. Add configuration wiring to `.theo/config.toml`.
- **LOC**: ~180
- **Risk**: Medium. Compaction is sensitive — if the cheap model summarises badly, the whole session degrades.
- **Test**: integration test running a real compaction with a mocked router returning haiku vs sonnet; assert compaction output passes a schema check. End-to-end smoke test on a 50-turn fixture.
- **Dependencies**: Phase 3.

#### Phase 5 — Fallback cascade on error
- **Scope**: `router.fallback(choice, hint)` called from `RunEngine` when `LlmError` matches retryable/overflow/rate-limit. Max 2 hops. Metrics event `routing.fallback_triggered` with old/new model.
- **LOC**: ~140
- **Risk**: Medium-high. Fallback bugs can cause silent quality drops.
- **Test**: simulate 429/overflow/timeout with mock LLM; assert router is called with correct hint and that total hops ≤ 2. Property test: fallback never returns the same model as input.
- **Dependencies**: Phase 4.

Optional Phase 6 — Learned classifier (Haiku-backed) behind `routing-learned` feature flag. Out of scope for evolution-loop MVP.

### 6. Open Questions / Risks

- **Licensing of reference routers.** Hermes is AGPL-3.0; its keyword list is code, not data. Paraphrasing the spirit is safer than copying `_COMPLEX_KEYWORDS` verbatim. **Unresolved**: does adapting a 35-token list constitute derivative work? Needs human legal call.
- **Privacy implications.** Learned classifier would ship prompt prefixes to a Haiku endpoint for classification — this is new data egress not present today if the user runs local models. Must be opt-in and documented.
- **Evaluation harness.** We need a "routing quality" benchmark independent of the agent-task benchmark. Proposal: reuse `theo-benchmark` with a new metric `avg_cost_per_task` and `task_success_rate` computed separately; compare Null vs RuleBased routers on the same fixture set. **Unresolved**: no such fixture exists yet.
- **Cold-start latency.** Rule-based router adds < 10 µs/turn — negligible. Learned classifier adds ~200 ms/turn minimum (Haiku round-trip) which **increases** mean latency for the common case. Cascade (FrugalGPT) adds one full LLM RTT on every escalation. For interactive TUI UX, only rules are viable.
- **Model-id drift.** Hard-coding `claude-opus-4-7` in the default slot tables means the code needs updating whenever a new model ships. Mitigation: slots key on *tier aliases* (`cheap`/`strong`/`vision`) and the pricing table resolves aliases to concrete IDs that a monthly-update ADR can bump.
- **Interaction with existing `reasoning_effort` field.** `AgentConfig.reasoning_effort: Option<String>` ([`config.rs:268`](file:///home/paulo/theo-code/crates/theo-agent-runtime/src/config.rs)) is currently session-global. Router must own this per-call — potentially breaking for users who set it manually. Mitigation: honour per-session override unless router is explicitly `rules-v2` (new opt-in).
- **Subagent delegation semantics.** The `is_subagent: bool` guard on `AgentConfig` ([`config.rs:274`](file:///home/paulo/theo-code/crates/theo-agent-runtime/src/config.rs)) prevents recursive subagent spawning. Router integration must preserve this — a Haiku-routed subagent must still be marked `is_subagent=true` regardless of which model runs it.
- **MoR citation.** I flagged in §1.3 that I could not verify a canonical MoR paper. Before shipping documentation with that reference, confirm.

### 7. References

Internal (theo-code):
- `crates/theo-domain/src/lib.rs` — dependency rules
- `crates/theo-infra-llm/src/client.rs:31-42` — current single-model client
- `crates/theo-infra-llm/src/model_limits.rs:26-74` — model token caps
- `crates/theo-infra-llm/src/provider/spec.rs:50-70` — ProviderSpec structure
- `crates/theo-agent-runtime/src/config.rs:243-319` — AgentConfig
- `crates/theo-agent-runtime/src/subagent/mod.rs:20-101` — SubAgentRole
- `.claude/rules/architecture.md` — dependency direction (inviolable)
- `docs/pesquisas/effective-harnesses-for-long-running-agents.md` — Anthropic harness research
- `docs/pesquisas/harness-engineering-openai.md` — OpenAI harness research (no routing content)

Reference repos:
- `referencias/opendev/crates/opendev-cli/src/setup/mod.rs:108-271` — slot configuration flow
- `referencias/opendev/crates/opendev-models/src/config/agent.rs:22-66` — AgentConfigInline cascade
- `referencias/Archon/packages/workflows/src/router.ts:73-138` — router prompt
- `referencias/Archon/packages/providers/src/claude/provider.ts:562` — `requestOptions?.model ?? assistantDefaults.model`
- `referencias/pi-mono/packages/coding-agent/src/core/model-resolver.ts:14-38` — default-per-provider map
- `referencias/hermes-agent/agent/smart_model_routing.py:62-107` — the core rule algorithm
- `referencias/hermes-agent/agent/auxiliary_client.py:1-35` — auxiliary resolution chain
- `referencias/opencode/packages/opencode/src/agent/agent.ts:27-52` — per-agent model schema

External:
- [Anthropic — Create custom subagents](https://docs.anthropic.com/en/docs/claude-code/sub-agents)
- [Anthropic Advisor Strategy (MindStudio)](https://www.mindstudio.ai/blog/claude-code-advisor-strategy-opus-sonnet-haiku)
- [Claude Code Sub-agents write-up (codewithseb.com)](https://www.codewithseb.com/blog/claude-code-sub-agents-multi-agent-systems-guide)
- [claude-code issue #38698 — per-agent provider routing](https://github.com/anthropics/claude-code/issues/38698)
- [Chen et al. — FrugalGPT (arXiv:2305.05176)](https://arxiv.org/abs/2305.05176)
- [Ong et al. — RouteLLM (arXiv:2406.18665)](https://arxiv.org/pdf/2406.18665)
- [Aurelio Labs — Semantic Router](https://github.com/aurelio-labs/semantic-router)
- [Router-R1 roundup (Champaign Magazine)](https://champaignmagazine.com/2025/10/16/router-r1-and-llm-routing-research/)
- [Anthropic Claude Models Complete Guide (CodeGPT)](https://www.codegpt.co/blog/anthropic-claude-models-complete-guide)
- [Anthropic — Models overview](https://platform.claude.com/docs/en/about-claude/models/overview)
