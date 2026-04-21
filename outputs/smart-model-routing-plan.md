# Smart Model Routing — Execution Plan

**Source research:** `outputs/smart-model-routing.md` (3245 words)
**Date:** 2026-04-20
**Scope:** 5 sequential phases (R1-R5), each scoped to the evolution-loop (≤ 200 LOC). Optional R6 gated behind feature flag.
**Target branch:** `evolution/apr20` (to be created off current HEAD)

---

## 0. Goal, success metrics, non-goals

**Goal.** Replace theo-code's single-model-per-session policy with a pluggable router that selects the right model per turn based on task phase, prompt complexity, vision requirement, and failure hints.

**Success metrics (measured on `theo-benchmark`):**

| Metric | Baseline | Target (R5 done) |
|---|---|---|
| `avg_cost_per_task` | current Sonnet-only baseline | **≥ 30% lower** on mixed-difficulty fixture |
| `task_success_rate` | current rate on hard tasks | **≥ parity** (router must not regress quality) |
| `p50_turn_latency` | current | **≤ +5%** vs. baseline (routing overhead cap) |
| `workspace tests` | 2724 passing | **≥ 2724**, no regressions |

**Non-goals (explicitly out of scope).**
- Learned classifier (Haiku-backed) — future R6, only after R1-R5 ship.
- MoR (Mixture-of-Routers) — research citation could not be verified; do not ship.
- Provider failover across tiers (e.g. Anthropic → OpenAI) — R5 only covers same-tier sibling providers.
- UI for per-turn router overrides — CLI flags suffice for MVP.
- Budget enforcement (hard spend ceilings) — separate feature; router only reports cost, doesn't enforce.

---

## 1. Global Definition of Done

Every task below **must** satisfy these gates before it is considered complete. These are not repeated in each task spec; they apply universally.

1. **Tests pass.** `cargo test --workspace` exits 0 with the new tests included.
2. **No new warnings.** `cargo check --workspace --tests` emits 0 warnings (current baseline is 0).
3. **Pre-commit hook passes without `--no-verify`.** `.githooks/pre-commit` + `.githooks/commit-msg` both green.
4. **Commit messages honour project conventions.** No `Co-Authored-By:` or `Generated-with` trailers (enforced by hook).
5. **Architecture rules honoured.** `theo-domain → (nothing)`. No new deps in `theo-domain`. No cross-crate duplication.
6. **TDD order documented.** Each commit cites the failing test first, then the implementation.
7. **Change ≤ 200 LOC.** If a phase exceeds the budget, split into sub-commits.
8. **Harness score does not drop.** `bash /home/paulo/autoloop/theocode-loop/scripts/theo-evaluate.sh .` score ≥ 75.15.
9. **No `unwrap()` in production paths.** New code uses `?` or typed errors.
10. **Documentation updated.** Each phase updates `.theo/evolution_research.md` (or a router-specific doc) with the acceptance-test results.

---

## 2. Task breakdown (R1 → R5)

Task IDs are stable — referenced by commit messages, tests, and issue tracking.

---

### R1 — Domain types + NullRouter

**Objective.** Land the trait surface (`ModelRouter`, `RoutingContext`, `ModelChoice`, `RoutingPhase`, `RoutingFailureHint`) in `theo-domain`, plus a behaviour-preserving `NullRouter` implementation. Zero runtime wiring yet.

**Files touched**
- `crates/theo-domain/src/routing.rs` (new, ~60 lines)
- `crates/theo-domain/src/lib.rs` (+1 `pub mod routing;`)
- `crates/theo-domain/tests/routing_trait.rs` (new)

**Acceptance criteria (tests that must exist and pass)**

| ID | Test | Expected behaviour |
|---|---|---|
| R1-AC-1 | `test_null_router_returns_default_for_every_phase` | Given a `NullRouter::new(default_choice)` and every `RoutingPhase` variant, `route(ctx)` returns the injected default. |
| R1-AC-2 | `test_null_router_fallback_returns_none` | `NullRouter::fallback(previous, hint)` returns `None` for every `RoutingFailureHint`. |
| R1-AC-3 | `test_routing_phase_serializes_round_trips` | Every `RoutingPhase` variant round-trips through serde JSON (required for config/log). |
| R1-AC-4 | `test_model_choice_equality_and_clone` | `ModelChoice` is `Clone + Eq + Debug`; equality is structural. |
| R1-AC-5 | `test_model_router_is_object_safe` | Compile-time: `let _: Box<dyn ModelRouter> = Box::new(NullRouter::new(choice));` — proves trait object safety. |
| R1-AC-6 | `test_routing_context_builder_sets_defaults` | `RoutingContext::new(phase)` defaults `iteration=0`, `requires_vision=false`, `requires_tool_use=false`, `previous_failure=None`. |

**Definition of Done (beyond the global list)**
- `RoutingPhase` includes `Normal`, `Compaction`, `Vision`, `Subagent(SubAgentRoleId)`, `SelfCritique`, `Classifier` variants.
- `SubAgentRoleId` is a newtype `pub struct SubAgentRoleId(pub &'static str)` so the enum mapping can live in `theo-agent-runtime` without creating a domain→runtime dep.
- Public API is documented with rustdoc on every item.
- `#[non_exhaustive]` on `RoutingPhase` and `RoutingFailureHint` — they will grow.
- `ModelChoice.routing_reason: &'static str` so log lines are cheap (no allocation per call).

**Risk.** Very low — additive, no runtime touch.
**LOC target.** ~80 implementation + ~120 tests.
**Dependencies.** None.

---

### R2 — `RuleBasedRouter` in `theo-infra-llm`

**Objective.** Implement the MVP classifier as a deterministic rule set ported from `hermes-agent/agent/smart_model_routing.py:62-107`. Expose a `PricingTable` struct so routing decisions are reproducible and auditable.

**Files touched**
- `crates/theo-infra-llm/src/routing/mod.rs` (new)
- `crates/theo-infra-llm/src/routing/rules.rs` (new — the classifier logic)
- `crates/theo-infra-llm/src/routing/keywords.rs` (new — paraphrased keyword list; see R2-AC-5)
- `crates/theo-infra-llm/src/routing/pricing.rs` (new — table)
- `crates/theo-infra-llm/tests/rule_router.rs` (new — integration tests)

**Acceptance criteria**

| ID | Test | Expected behaviour |
|---|---|---|
| R2-AC-1 | `test_simple_prompt_returns_cheap_tier` | 10 representative "simple" prompts ("list files", "show diff", "what is X") all route to cheap tier. |
| R2-AC-2 | `test_complex_prompt_returns_default_tier` | 10 "complex" prompts ("debug this traceback", "refactor ...", "implement ...") route to default tier. |
| R2-AC-3 | `test_vision_phase_forces_vision_slot` | `RoutingPhase::Vision` returns the configured vision slot regardless of prompt length. |
| R2-AC-4 | `test_compaction_phase_forces_compact_slot` | `RoutingPhase::Compaction` returns the compact slot. |
| R2-AC-5 | `test_keyword_list_derivation_documented` | Keyword list header comment cites the paraphrase source (`hermes-agent` spirit, not verbatim copy) — addresses AGPL licensing concern from research §6. A CI grep verifies the header contains `paraphrased-from:`. |
| R2-AC-6 | `test_pricing_table_resolves_tier_alias` | `PricingTable::resolve("cheap")` returns `(provider_id, model_id)` pair from config; unknown tier returns error. |
| R2-AC-7 | `test_router_is_pure_function` | Given identical `RoutingContext`, `route()` returns identical `ModelChoice` across 1000 calls (no hidden state, no RNG). |
| R2-AC-8 | `test_rule_router_fallback_returns_sibling_provider` | On `RoutingFailureHint::RateLimit`, fallback returns a different provider with the same tier (if configured); else `None`. |

**Definition of Done (extra)**
- `PricingTable` is loaded from config; constructor takes `&ProjectConfig` or similar — **no** hard-coded model IDs in `rules.rs`.
- `is_simple_turn(prompt)` is crate-private; exposed only through `route()`.
- The keyword list is **paraphrased**, not copied — see legal note R2-AC-5. A header comment cites the algorithmic spirit, not the source file.
- Module-level rustdoc cites research report lines 273-279.

**Risk.** Low. Keyword list is data; easy to tune. Licensing mitigated by paraphrasing.
**LOC target.** ~150 implementation + ~100 tests.
**Dependencies.** R1 (needs `ModelRouter` trait).

---

### R3 — Wire router into `AgentConfig` + `RunEngine`

**Objective.** Thread the chosen router through the agent lifecycle. Default to `NullRouter` (behaviour preserved). Single call site in `RunEngine` where `ChatRequest` is built must consult `router.route(&ctx)` and apply the result.

**Files touched**
- `crates/theo-agent-runtime/src/config.rs` (add `router: Option<Arc<dyn ModelRouter>>` field)
- `crates/theo-agent-runtime/src/run_engine.rs` (apply router at ChatRequest build site)
- `crates/theo-agent-runtime/tests/run_engine_routing.rs` (new)

**Acceptance criteria**

| ID | Test | Expected behaviour |
|---|---|---|
| R3-AC-1 | `test_run_engine_uses_router_model` | A `MockRouter` returning a specific `ModelChoice` causes `ChatRequest.model` to equal that choice's `model_id`. |
| R3-AC-2 | `test_run_engine_with_none_router_preserves_default_model` | `AgentConfig.router = None` → `ChatRequest.model` equals `AgentConfig.model` (strict regression). |
| R3-AC-3 | `test_routing_context_populated_with_iteration_and_tokens` | `MockRouter` captures the last `RoutingContext`; assert `iteration` and `conversation_tokens` are populated accurately. |
| R3-AC-4 | `test_routing_does_not_mutate_session_model` | After a turn routed to cheap tier, next call with `RoutingPhase::Normal` on a complex prompt receives default tier — router decision is per-turn, session model is stable. |
| R3-AC-5 | `test_router_failure_falls_back_to_session_default` | If `route()` panics (caught via `std::panic::catch_unwind`), RunEngine logs the failure and uses `AgentConfig.model`. Unit test only — production must never panic. |
| R3-AC-6 | `test_routing_reason_appears_in_trace_event` | Emitted `LlmCallStart` event carries `routing_reason` in its payload. |

**Definition of Done (extra)**
- `AgentConfig.router: Option<Arc<dyn ModelRouter>>` with serde skip (trait objects don't serialize); `config.rs` Default impl returns `None`.
- One and only one call site in `run_engine.rs` calls `router.route()`; a compile-enforced grep test in `tests/structural_hygiene.rs` enforces the invariant: `grep -c "router.route(" crates/theo-agent-runtime/src == 1`.
- Routing latency instrumented via `tokio::time::Instant`; logged if > 1ms (rule-based should be < 10µs).

**Risk.** Medium — this is the hot path. The `catch_unwind` safety net + the regression test for `router = None` keep blast radius small.
**LOC target.** ~100.
**Dependencies.** R1 + R2.

---

### R4 — Route compaction + subagent phases + TOML config

**Objective.** Extend routing beyond `Normal`: compaction stages consult the router with `RoutingPhase::Compaction`; `SubAgentRole` carries a `role_id()` that maps to a routing slot; `.theo/config.toml` gains a `[routing]` section.

**Files touched**
- `crates/theo-agent-runtime/src/compaction_stages.rs` (accept `&dyn ModelRouter` in stage constructor)
- `crates/theo-agent-runtime/src/subagent/mod.rs` (add `fn role_id(&self) -> SubAgentRoleId`)
- `crates/theo-application/src/config/project_config.rs` (parse `[routing]` block)
- `crates/theo-application/tests/routing_config.rs` (new)
- `.theo/config.toml.example` (new — documented example)

**Acceptance criteria**

| ID | Test | Expected behaviour |
|---|---|---|
| R4-AC-1 | `test_compaction_uses_routing_phase_compaction` | A mock router records the phase; when compaction runs, the mock sees `RoutingPhase::Compaction`, not `Normal`. |
| R4-AC-2 | `test_subagent_explorer_routes_to_explorer_slot` | Running the `explorer` subagent with `[routing.slots.subagent_explorer]` configured in TOML causes the router to return that slot's model. |
| R4-AC-3 | `test_subagent_missing_slot_falls_back_to_default` | Subagent with no matching slot in config uses `AgentConfig.model` (no panic, no silent quality loss). |
| R4-AC-4 | `test_compaction_quality_preserved_under_cheap_model` | End-to-end fixture: 50-turn conversation compacted with cheap-model mock returns a summary that passes a schema check (contains expected tokens: "## Summary", "Decisions:", "Open questions:"). |
| R4-AC-5 | `test_toml_routing_block_parses` | Valid `[routing]` block with 3 slots parses into `RoutingConfig { enabled, strategy, slots: Vec<Slot> }`. |
| R4-AC-6 | `test_toml_routing_disabled_returns_null_router` | `routing.enabled = false` builds a `NullRouter` regardless of slot config. |
| R4-AC-7 | `test_env_var_overrides_config` | `THEO_ROUTING_DISABLED=1` overrides `routing.enabled = true` in TOML. |
| R4-AC-8 | `test_cli_flag_router_off_disables` | `theo --router off` flag disables routing at runtime. |

**Definition of Done (extra)**
- Config schema documented in `docs/current/routing-config.md` (new file, ~30 lines).
- `.theo/config.toml.example` committed with every slot shown and commented.
- `SubAgentRoleId` values for existing roles (explorer/implementer/verifier/reviewer) declared as `const` in `theo-agent-runtime/src/subagent/mod.rs` — one place to change if the role set grows.
- Deprecation path: existing `AgentConfig.reasoning_effort` is honoured when the router is `NullRouter`; when a real router is active, `ModelChoice.reasoning_effort` takes precedence. Document the override order in the config doc.

**Risk.** Medium. Compaction is sensitive — a bad cheap-model summary degrades the whole session. R4-AC-4 specifically guards this.
**LOC target.** ~180.
**Dependencies.** R3.

---

### R5 — Fallback cascade on error

**Objective.** When an LLM call fails with a retryable/overflow/rate-limit error, ask the router for a fallback choice and retry. Bound to 2 hops per turn to avoid unbounded cascades.

**Files touched**
- `crates/theo-infra-llm/src/routing/rules.rs` (flesh out `fallback()` logic)
- `crates/theo-agent-runtime/src/run_engine.rs` (wire fallback cascade into error path)
- `crates/theo-agent-runtime/tests/routing_fallback.rs` (new)

**Acceptance criteria**

| ID | Test | Expected behaviour |
|---|---|---|
| R5-AC-1 | `test_overflow_error_triggers_fallback_to_larger_window` | Mock LLM returns `ContextWindowWillOverflow`; router returns a choice with a larger `max_output_tokens`; retry succeeds. |
| R5-AC-2 | `test_rate_limit_triggers_sibling_provider` | Mock LLM returns 429; router returns a same-tier sibling provider; retry succeeds. |
| R5-AC-3 | `test_timeout_retries_same_model_once_then_falls_back` | Mock LLM times out once on model A, succeeds on retry; does not hop to model B. |
| R5-AC-4 | `test_timeout_twice_falls_back_to_different_model` | Mock LLM times out twice on model A; cascade hops to model B. |
| R5-AC-5 | `test_max_two_hops_then_typed_error` | Mock LLM fails on every model; cascade stops after 2 hops and returns `LlmError::FallbackExhausted`. |
| R5-AC-6 | `test_fallback_never_returns_same_model_as_input` | Property test (proptest, 1000 cases): for any `previous: ModelChoice` and any `RoutingFailureHint`, `router.fallback(&previous, hint)` either returns `None` or a `ModelChoice` with a different `(provider_id, model_id)` pair. |
| R5-AC-7 | `test_routing_fallback_event_emitted` | On every fallback hop, a `routing.fallback_triggered` event is published with `{ from_model, to_model, reason }`. |
| R5-AC-8 | `test_budget_exhausted_is_hard_stop` | If `RoutingFailureHint::BudgetExhausted`, router returns `None` — no silent downgrade. |

**Definition of Done (extra)**
- Max hop count (`2`) is a named constant `MAX_FALLBACK_HOPS` at module level.
- `LlmError::FallbackExhausted { attempted: Vec<String> }` is a new typed variant that carries every model tried.
- Metrics event names match existing convention in `theo-agent-runtime/src/metrics.rs`.
- No silent retry on 4xx other than 429 — 400/401/403 are permanent and should surface immediately.

**Risk.** Medium-high. Silent quality drops from wrong fallbacks are hard to debug. R5-AC-6 (property) and R5-AC-8 (hard stop) are the main guards.
**LOC target.** ~140.
**Dependencies.** R4.

---

### R6 (optional, post-MVP) — Learned classifier behind `routing-learned` feature flag

**Objective.** Haiku-backed classifier that scores prompt complexity instead of relying on keywords. Gated behind feature flag — disabled by default.

**Not specified in detail in this plan.** To be revisited after R5 ships and we have `avg_cost_per_task` data from the benchmark to motivate the move.

**Open questions** (to answer before spec'ing R6):
- Does the classifier-call latency (~200 ms/turn minimum per research §6) regress the interactive TUI UX beyond the +5% cap?
- Do we need a persistent classifier cache to amortise latency?
- Privacy model: classifier endpoint sees prompt prefix; is this acceptable for offline-first users?

---

## 3. Traceability — phase → research citation

| Phase | Research section | File citation |
|---|---|---|
| R1 | §4.1 Trait surface | `outputs/smart-model-routing.md:118-158` |
| R2 | §4.2 Classifier rules + §6 licensing | `outputs/smart-model-routing.md:160-192` + `305` |
| R3 | §4.4 Composition with existing features + §5 Phase 3 | `outputs/smart-model-routing.md:206-212` + `280-285` |
| R4 | §4.3 Roles + §4.6 Config + §5 Phase 4 | `outputs/smart-model-routing.md:193-204` + `224-259` + `287-292` |
| R5 | §4.5 Failure handling + §5 Phase 5 | `outputs/smart-model-routing.md:213-222` + `294-299` |

---

## 4. Evaluation harness commitment

Before R1 lands, a new benchmark fixture must exist (otherwise the success metrics in §0 are unmeasurable). Spec:

- **Fixture**: 30 tasks drawn from existing `theo-benchmark` suite, labelled `simple: 10 | medium: 10 | complex: 10`.
- **Metrics computed per run**: `avg_cost_per_task`, `task_success_rate`, `p50_turn_latency`.
- **Comparison**: `NullRouter` baseline vs. `RuleBasedRouter` (after R2) vs. `RuleBasedRouter + cascade` (after R5).
- **Non-goal**: absolute cost/latency numbers are environment-dependent — only ratios vs. baseline matter for the acceptance test.

This harness is a **prerequisite task (R0)** and must land before R1 starts; if we can't measure, we can't accept.

### R0 — Routing evaluation fixture

| ID | Acceptance criterion |
|---|---|
| R0-AC-1 | `apps/theo-benchmark/fixtures/routing/` exists with 30 labelled `.json` cases. |
| R0-AC-2 | `cargo run --bin theo-benchmark -- --suite routing --router null` emits a JSON report containing `avg_cost_per_task`, `task_success_rate`, `p50_turn_latency`. |
| R0-AC-3 | Same command with `--router rules` works once R2 lands (this part of R0-AC-3 is deferred; the CLI flag must parse today). |
| R0-AC-4 | Report output is machine-readable JSON (not pretty prose) so CI can diff it. |

**LOC target.** ~120. **Risk.** Low. **Dependencies.** None.

---

## 5. Rollout order and estimated timeline

Assuming one phase ≈ one evolution-loop iteration (~1 day of focused work each):

```
R0  (harness)      [prereq, Day 0]
 └─ R1  (domain)   [Day 1]
     └─ R2  (rules) [Day 2]
         └─ R3 (wire) [Day 3]
             └─ R4 (compact+subagent+cfg) [Day 4]
                 └─ R5 (cascade) [Day 5]
                     ↘ (optional R6 learned — Day 6+, post-MVP)
```

Gate between phases: each phase must leave the workspace green (global DoD §1) before the next starts. No parallel phases — the dependency chain is linear.

---

## 6. What this plan does NOT cover (to manage expectations)

- **Pricing data accuracy.** `PricingTable` model IDs in R2/R4 reference vendor names that drift (see research §6 "model-id drift"). The plan doesn't include a monthly pricing-refresh ADR — that is operational, not engineering work.
- **Subagent topology changes.** Router respects existing roles; it doesn't add new ones. Adding a "reviewer" subagent role (research §4.3) is a separate task.
- **Budget enforcement.** The router reports cost; it does not enforce a spend ceiling. A separate budget-gate feature would intercept `route()` results and block when over budget.
- **Multi-tenant isolation.** Single-user assumption holds. Routing decisions aren't scoped per tenant.
- **Observability dashboards.** Metrics events are emitted (R5-AC-7); visualising them is a separate UI task.

---

## 7. Ready-to-execute checklist

Before starting R1:

- [ ] Research report reviewed and approved (exists: `outputs/smart-model-routing.md`).
- [ ] R0 (benchmark harness) merged — **blocker**.
- [ ] AGPL licensing call made on Hermes keyword list (research §6). Required before R2 starts.
- [ ] `evolution/apr20` branch created from current HEAD.
- [ ] Success metrics §0 snapshotted (baseline numbers from `theo-benchmark` with `NullRouter` equivalent — i.e. today's behaviour).

Once those are checked, R1 starts and the 5-phase chain runs sequentially per §5.
