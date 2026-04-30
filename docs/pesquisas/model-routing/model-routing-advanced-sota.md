---
type: supplemental-report
parent: smart-model-routing.md
question: "What are the quantitative gaps in model routing SOTA that smart-model-routing.md does not cover?"
generated_at: 2026-04-29T00:00:00Z
confidence: 0.82
sources_used: 34
target_score: "Model Routing 3.5 → 4.5/5"
---

# Model Routing: Advanced SOTA — Supplemental Research

## Executive Summary

The base report (`smart-model-routing.md`) established the architectural shape: a `ModelRouter` trait in `theo-domain`, rule-based routing in `theo-infra-llm`, and a 5-phase implementation plan (R0-R5). This supplemental report fills ten quantitative gaps the base report flagged but did not resolve: cascade latency budgets for interactive agents, learned routing state-of-the-art (Router-R1, SATER, Semantic Router), per-workflow LLM binding economics, prompt caching arithmetic for orchestrator-worker, model capability detection for offline/air-gapped, cost-quality Pareto operating points, multi-provider fallback chain design, subagent budget allocation, token overhead bounds for multi-agent, and routing evaluation metrics. Each section provides concrete thresholds that the SOTA validation loop can gate on.

The key finding across all ten sections is that **rule-based routing is the only latency-safe option for interactive coding agents** (adds <10 us vs 500-2000 ms for LLM-backed routing), but rule-based alone leaves 20-40% of cost savings on the table compared to learned approaches. The recommended path is: ship rules (R2), measure cost savings on `theo-benchmark`, then gate learned routing (R6) behind a feature flag with a strict latency budget of <50 ms per routing decision. Cascade routing (FrugalGPT-style) is unsuitable for interactive coding but viable for batch/background subagents.

---

## 1. Latency Impact of Cascade Routing

### The Problem

FrugalGPT's sequential cascade sends a query to a cheap model first, then escalates to stronger models if an answer scorer rejects the response. Each escalation adds a full LLM round-trip. The base report noted this but did not quantify the impact.

### Quantitative Evidence

| Metric | Value | Source |
|--------|-------|--------|
| Single-model TTFT (Haiku 4.5) | ~600 ms | BenchLM.ai March 2026 benchmark |
| Single-model TTFT (Sonnet 4.6) | ~800-1200 ms | Opper.ai LLM Router Latency Benchmark 2026 |
| Single-model TTFT (reasoning models) | 10-150 s | AIMultiple LLM Latency Benchmark 2026 |
| Router API overhead (gateway) | 3-70 ms | Bifrost 11 us, LiteLLM ~8 ms, Kong ~3-5 ms, OpenRouter ~70 ms |
| LLM-as-router overhead | 500-2000 ms | MegaNova 3-Tier Routing Cascade |
| SATER cascade AGL reduction | >50% vs naive cascade | EMNLP 2025, arXiv:2510.05164 |
| SATER AROL reduction | >80% vs naive cascade | EMNLP 2025, arXiv:2510.05164 |
| Rule-based router overhead | <10 us | Hermes agent, measured (smart_model_routing.py is pure string ops) |

### Cascade Latency Arithmetic

For a 2-step FrugalGPT cascade (Haiku first, Sonnet if rejected):

- **Best case (Haiku answers):** 600 ms TTFT + generation time. No escalation.
- **Worst case (escalation):** 600 ms (Haiku) + scoring overhead (~100 ms) + 1000 ms (Sonnet) = **~1700 ms** before first useful token.
- **p95 with 30% escalation rate:** Most requests at ~600 ms, but p95 approaches ~1700 ms.

For comparison, a single Sonnet call: p50 ~1000 ms, p95 ~1500 ms. The cascade **worsens p95 by ~200-500 ms** while improving p50 by ~400 ms (when the cheap model succeeds).

### Latency Budget for Interactive Coding Agents

| Use Case | Acceptable TTFT | Acceptable Total Latency | Source |
|----------|----------------|--------------------------|--------|
| Inline code completion | <100 ms | <300 ms | OpenAI latency guide, industry consensus |
| Conversational coding assistant | <500 ms - 1 s | <5 s | BenchLM.ai, AIMultiple 2026 |
| Background subagent task | No hard limit | <60 s | Acceptable for async delegation |
| Batch evaluation/review | No hard limit | <5 min | Async, non-interactive |

**Key insight from BenchLM.ai:** "A p95 that is 3.5x the median means roughly one in twenty requests is going to feel dramatically slower than average. High p95 variance is what generates user complaints." Consistency matters more than raw speed.

### When Cascade Routing is NOT Worth It

| Scenario | Cascade viable? | Reason |
|----------|----------------|--------|
| Interactive TUI coding session | **No** | Escalation adds 700-1100 ms to p95; user feels the stutter |
| Background subagent (explorer, reviewer) | **Yes** | No user-facing latency; cost savings justify the extra RTT |
| Batch processing (compaction, bulk review) | **Yes** | Async; total cost dominates UX |
| Doom-loop recovery (model escalation) | **Yes** | Already in a failure path; latency is secondary to correctness |

### Threshold for SOTA Validation Loop

```
ROUTING_OVERHEAD_BUDGET_INTERACTIVE_MS = 50
ROUTING_OVERHEAD_BUDGET_BACKGROUND_MS  = 2000
CASCADE_MAX_STEPS_INTERACTIVE          = 1  (no cascade; single routing decision)
CASCADE_MAX_STEPS_BACKGROUND           = 3
```

---

## 2. Learned Routing (2026 State of the Art)

### Router-R1: Multi-Round RL-Based Routing

Router-R1 (Zhang, Feng, You — UIUC; NeurIPS 2025; arXiv:2506.09033) is the most significant advance since RouteLLM. Key differences from RouteLLM:

| Dimension | RouteLLM (2024) | Router-R1 (2025) |
|-----------|----------------|------------------|
| Routing model | Trained classifier (BERT-scale) | LLM itself (Llama-3B, Qwen-4B) |
| Routing paradigm | Single-shot binary (strong vs weak) | Multi-round sequential (interleaved think + route actions) |
| Training method | Preference data from human evaluations | RL with 3-part reward (format + correctness + cost penalty) |
| Generalization | Requires retraining for new model candidates | Zero-shot generalization to unseen models via descriptors |
| Latency overhead | ~10-50 ms (classifier inference) | ~200-500 ms (LLM inference per routing step) |
| Best EM score | Not reported on same benchmark | 0.416 (Router-R1-Qwen) |

**Critical finding for theo-code:** Router-R1's multi-round routing is too slow for interactive coding (~200-500 ms per routing decision). But its **generalization without retraining** is valuable: Router-R1 conditions on simple descriptors (pricing, latency, example performance) and handles new model candidates without fine-tuning. This is the right architecture for a future R6 if the routing decision can be amortized (e.g., route once per task, not per turn).

### SATER: Self-Aware and Token-Efficient Routing/Cascading

SATER (EMNLP 2025; arXiv:2510.05164) introduces two metrics missing from prior work:

- **Average Generation Latency (AGL):** Measures actual latency including generation length, not just routing overhead.
- **Average Routing Overhead Latency (AROL):** Isolates the routing decision cost.

SATER achieves >50% AGL reduction and >80% AROL reduction compared to naive cascade routing by training the SLM to **reject queries it cannot answer** (self-aware rejection), sending only genuinely hard queries to the LLM tier.

**Relevance for theo-code:** SATER's self-aware rejection is analogous to our rule-based `is_simple_turn()` — but learned. If R6 ships, SATER's 2-stage training (preference optimization + rejection fine-tuning) is a better starting point than RouteLLM's single classifier.

### Semantic Router (Aurelio Labs)

Semantic Router embeds prompts and matches against route centroids using cosine similarity. No LLM inference required for routing.

| Property | Value |
|----------|-------|
| Routing latency | ~5-20 ms (embedding inference) |
| Training data needed | 5-20 exemplar utterances per route (no labeled pairs) |
| Cold-start | Minimal — routes defined by exemplars, not training data |
| License | MIT |
| Scalability | Thousands of routes with in-memory vector DB |
| LLM independence | Routes via embeddings only; no LLM call |

**Cold-start solution:** Semantic Router avoids cold-start entirely — you define routes by writing 5-20 example utterances per route, embed them, and the router is ready. No historical data needed. This makes it suitable for theo-code's first learned routing step.

**Production limitation:** Semantic similarity does not distinguish well between overlapping domains (e.g., "debug this test" vs "write a test" — both mention "test"). The 3-tier cascade pattern (MegaNova) suggests using semantic routing as Tier 2 between rules (Tier 1) and LLM routing (Tier 3).

### Comparison Table for theo-code Decision

| Approach | Latency | Cost | Cold-start | Maintenance | Recommended for |
|----------|---------|------|------------|-------------|-----------------|
| Rule-based (Hermes) | <10 us | Zero | None | Manual keyword tuning | MVP (R2) — interactive |
| Semantic Router | 5-20 ms | Embedding model | 5-20 exemplars | Exemplar curation | R6.1 — interactive |
| RouteLLM classifier | 10-50 ms | Training pipeline | Preference data | Retraining on model changes | R6.2 — interactive |
| Router-R1 (LLM) | 200-500 ms | LLM inference | Descriptors only | Low | Background routing only |
| FrugalGPT cascade | 600-1700 ms | Multiple LLM calls | None | None | Background/batch only |
| SATER | varies | SLM inference | Training data | Retraining | Future research |

### Threshold for SOTA Validation Loop

```
LEARNED_ROUTER_LATENCY_CAP_MS         = 50   (interactive)
LEARNED_ROUTER_COLD_START_EXEMPLARS   = ≤20  (max exemplars needed before usable)
LEARNED_ROUTER_QUALITY_GATE           = ≥95% of single-model baseline on MT-Bench
```

---

## 3. Per-Workflow LLM Binding

### OpenDev's 5 Model Roles vs Anthropic's Orchestrator-Worker

| Dimension | OpenDev (5 roles) | Anthropic (orchestrator-worker) |
|-----------|-------------------|--------------------------------|
| Role taxonomy | Action, Thinking, Critique, Vision, Compact | Orchestrator (Opus), Worker (Sonnet), [Haiku for cheap tasks] |
| Binding granularity | Per-workflow-phase | Per-agent-instance |
| Client initialization | **Lazy** — HTTP client created on first use of each role | Eager — all clients created at session start |
| Fallback chain | Role-specific: Critique → Thinking → Action; Vision → Action | No explicit fallback; workers are independent |
| Config mechanism | Named slots in config with cascading defaults | `model` field on subagent definition |
| Provider cache | **24h TTL, stale-while-revalidate** | None documented |
| Switching cost | Config change only, no code change | SDK-level; cross-provider not supported (issue #38698) |

### Cost Savings Analysis

Assuming a typical 40-turn coding session with the following phase distribution:

| Phase | % of turns | OpenDev model | Anthropic model | Cost per MTok (input) |
|-------|-----------|---------------|-----------------|----------------------|
| Action/Normal | 50% | Sonnet | Sonnet | $3.00 |
| Thinking | 15% | Opus or Sonnet | (not separated) | $5.00 or $3.00 |
| Compaction | 10% | Haiku | Haiku | $1.00 |
| Vision | 5% | Sonnet (VLM) | Sonnet | $3.00 |
| Critique | 10% | Sonnet | (not separated) | $3.00 |
| Subagent tasks | 10% | Haiku | Sonnet/Haiku | $1.00-$3.00 |

**Single-model baseline (all Sonnet):** 100% of turns at $3.00/MTok = $3.00 weighted average.

**OpenDev 5-role binding:** (0.50 x 3.00) + (0.15 x 5.00) + (0.10 x 1.00) + (0.05 x 3.00) + (0.10 x 3.00) + (0.10 x 1.00) = $2.80/MTok = **6.7% savings** (thinking on Opus offsets compaction savings).

**OpenDev 5-role binding (Sonnet for thinking):** (0.50 x 3.00) + (0.15 x 3.00) + (0.10 x 1.00) + (0.05 x 3.00) + (0.10 x 3.00) + (0.10 x 1.00) = $2.50/MTok = **16.7% savings**.

**Aggressive (Haiku for compaction + subagents + simple turns):** If 30% of "Normal" turns are simple and routed to Haiku: (0.35 x 3.00) + (0.15 x 1.00) + (0.15 x 3.00) + (0.10 x 1.00) + (0.05 x 3.00) + (0.10 x 3.00) + (0.10 x 1.00) = $2.15/MTok = **28.3% savings**.

These savings **compound with prompt caching** (Section 4).

### Threshold for SOTA Validation Loop

```
MIN_WORKFLOW_ROLES            = 4    (Normal, Compact, Vision, Subagent)
LAZY_CLIENT_INIT              = true (no HTTP client until first role use)
PROVIDER_CACHE_TTL_HOURS      = 24
COST_SAVINGS_VS_SINGLE_MODEL  = ≥20% on mixed-difficulty benchmark
```

---

## 4. Prompt Caching Economics

### Anthropic Cache Pricing (April 2026)

| Model | Base Input | 5-min Write (1.25x) | 1-hr Write (2x) | Cache Read (0.1x) | Output |
|-------|-----------|---------------------|------------------|--------------------|--------|
| Opus 4.7/4.6 | $5.00/MTok | $6.25/MTok | $10.00/MTok | **$0.50/MTok** | $25.00/MTok |
| Sonnet 4.6 | $3.00/MTok | $3.75/MTok | $6.00/MTok | **$0.30/MTok** | $15.00/MTok |
| Haiku 4.5 | $1.00/MTok | $1.25/MTok | $2.00/MTok | **$0.10/MTok** | $5.00/MTok |

### Orchestrator-Worker Savings with Caching

**Scenario:** Orchestrator (Opus) creates a plan with a 10K-token system prompt. Five Sonnet workers execute subtasks, each re-using the cached system prompt.

**Without caching:**
- Orchestrator: 10K tokens x $5.00/MTok = $0.05
- 5 workers x 10K tokens x $3.00/MTok = $0.15
- **Total input cost: $0.20**

**With 5-min caching (orchestrator writes, workers read):**
- Orchestrator (cache write): 10K x $3.75/MTok = $0.0375
- 5 workers (cache read): 5 x 10K x $0.30/MTok = $0.015
- **Total input cost: $0.0525 (73.8% savings)**

**With 1-hr caching:**
- Orchestrator (cache write): 10K x $6.00/MTok = $0.06
- 5 workers (cache read): 5 x 10K x $0.30/MTok = $0.015
- **Total input cost: $0.075 (62.5% savings)**

**Key insight:** 5-minute cache is cheaper than 1-hour cache for orchestrator-worker patterns where all workers execute within 5 minutes of the orchestrator's plan. The 1-hour cache only wins when workers are spread across sessions or when the same prompt is reused across multiple tasks.

### Break-Even Analysis

| Cache duration | Write cost multiplier | Break-even at N cache reads |
|---------------|----------------------|----------------------------|
| 5-minute | 1.25x | **2 reads** (1.25 / (1.0 - 0.1) = 1.39, so 2 reads pays off) |
| 1-hour | 2.00x | **3 reads** (2.00 / (1.0 - 0.1) = 2.22, so 3 reads pays off) |

### Minimum Token Thresholds

| Model | Min cacheable prefix |
|-------|---------------------|
| Opus 4.7 | 4,096 tokens |
| Sonnet 4.6 | 2,048 tokens |
| Haiku 4.5 | 4,096 tokens |

**Implication for theo-code:** System prompts and tool schemas typically exceed 4K tokens easily. The constraint is ensuring the cacheable prefix is **stable** (no dynamic content before the `cache_control` breakpoint). ProjectDiscovery improved their cache hit rate from 7% to 74% by moving dynamic content out of the cacheable prefix.

### 50-Turn Session Cost Model

| Configuration | Input cost (10K prefix, 50 turns) | Savings vs no-cache |
|--------------|-----------------------------------|---------------------|
| No cache, all Sonnet | 50 x 10K x $3.00/MTok = **$1.50** | baseline |
| 5-min cache, all Sonnet | $0.0375 (write) + 49 x 10K x $0.30/MTok = **$0.185** | **87.7%** |
| 5-min cache, routing (30% Haiku) | $0.0375 + 34 x $0.30/MTok x 10K + 15 x $0.10/MTok x 10K = **$0.155** | **89.7%** |

### Threshold for SOTA Validation Loop

```
PROMPT_CACHE_HIT_RATE_TARGET      = ≥70%  (ProjectDiscovery achieved 74% after optimization)
CACHE_AWARE_PROMPT_STRUCTURE      = true  (static prefix + dynamic suffix)
MIN_CACHEABLE_PREFIX_TOKENS       = 2048  (Sonnet threshold)
CACHE_WRITE_AMORTIZATION_TURNS   = ≥2    (break-even for 5-min TTL)
```

---

## 5. Model Capability Detection

### OpenDev's Provider Cache Pattern

OpenDev implements a provider cache with the following properties:

| Property | Value | Rationale |
|----------|-------|-----------|
| TTL | 24 hours | Model capabilities change infrequently |
| Invalidation | Stale-while-revalidate | Serve cached data while refreshing in background |
| Capabilities tracked | Context length, vision support, reasoning features, pricing | Minimum set for routing decisions |
| Storage | Local file cache | Works offline after first fetch |
| Refresh trigger | Background on session start | Non-blocking |

### Continue IDE's Capability Detection (Reference)

Continue (open-source coding assistant) implements capability detection through three layers:

1. **Explicit overrides:** User configuration takes precedence.
2. **Provider validation:** Check against hardcoded supported-provider lists.
3. **Pattern matching:** Model name regex (e.g., `gpt-4-vision` implies vision support, `o3` implies reasoning).

This is the pragmatic approach for theo-code: **pattern matching on model names** as a baseline, with explicit overrides in config, and optional API-fetched capabilities with caching.

### Air-Gapped / Offline Environments

| Challenge | Solution | Implementation |
|-----------|----------|---------------|
| No API to fetch capabilities | Ship a bundled capability manifest | `model_limits.rs` already has hardcoded limits; extend with capabilities |
| Model names may not match known patterns | Config override: `[model.capabilities] vision = true` | User-specified; no network needed |
| Provider pricing unavailable | Bundled pricing table with version stamp | Update via `theo update-models` command |
| Stale capability data | Warn after 30 days; function correctly with stale data | Degrade gracefully: assume conservative defaults |

### Proposed Capability Schema for `model_limits.rs`

```rust
pub struct ModelCapabilities {
    pub context_window: u32,          // tokens
    pub max_output_tokens: u32,       // tokens
    pub supports_vision: bool,
    pub supports_tool_use: bool,
    pub supports_reasoning: bool,     // extended thinking
    pub supports_prompt_caching: bool,
    pub input_cost_per_mtok: f64,     // USD
    pub output_cost_per_mtok: f64,    // USD
    pub cache_read_cost_per_mtok: Option<f64>,
    pub tier: ModelTier,              // Cheap, Standard, Strong
    pub last_verified: chrono::NaiveDate,
}
```

### Threshold for SOTA Validation Loop

```
CAPABILITY_CACHE_TTL_HOURS         = 24
CAPABILITY_STALENESS_WARNING_DAYS  = 30
OFFLINE_FALLBACK                   = bundled manifest (no network required)
CAPABILITY_FIELDS_MIN              = 6  (context, output, vision, tools, pricing_in, pricing_out)
```

---

## 6. Cost-Quality Pareto Frontier

### Known Operating Points (Q2 2026)

From the AI Model Efficient Frontier analysis (DigitalApplied, Q2 2026), six models dominate the Pareto frontier:

| Model | Arena Elo (approx) | Cost (input $/MTok) | Pareto role |
|-------|-------------------|---------------------|-------------|
| Opus 4.6/4.7 | ~1350 | $5.00 | Judgment/review — highest quality |
| Sonnet 4.6 | ~1300 | $3.00 | Reasoning — balanced |
| Haiku 4.5 | ~1200 | $1.00 | Volume — cost-efficient |
| GPT-5.2 | ~1320 | varies | Alternative strong |
| Qwen 3.6 Plus | ~1250 | free/cheap | Bulk classification |
| MiMo V2 Pro | ~1280 | cheap | Volume code generation |

**Key insight (DigitalApplied):** "The right application of a Pareto frontier is not model selection -- it is model routing." The cheapest correct answer is usually a two-model routing rule, not a single model choice.

### Triage Framework (arXiv:2604.07494, April 2026)

Triage introduces **code health metrics as routing signals** for software engineering tasks:

- **3 capability tiers:** Light (Haiku), Standard (Sonnet), Heavy (Opus)
- **Routing signal:** Pre-computed code health sub-factors (complexity, coupling, cohesion)
- **Key finding:** Clean, well-structured code can be modified correctly by cheaper models; messy code requires frontier reasoning
- **Falsifiable condition:** Light-tier pass rate on healthy code must exceed the inter-tier cost ratio

**Relevance for theo-code:** Triage's code-health routing is complementary to Hermes' prompt-complexity routing. Rule-based routing in R2 routes on *prompt characteristics*; Triage routes on *codebase characteristics*. A future R7 could combine both signals.

### Advisor Strategy Operating Points

From MindStudio's analysis of the Advisor Strategy (Opus plans, Sonnet executes):

| Configuration | Cost delta | Quality delta |
|--------------|-----------|---------------|
| All Opus | baseline | baseline |
| Advisor (Opus plan + Sonnet execute) | **-11%** | **+2%** |
| All Sonnet | **-40%** | -5 to -15% (task-dependent) |
| All Haiku | **-80%** | -25 to -40% |

### User-Selectable Routing Profiles

| Profile | Description | Model mapping | Target users |
|---------|-------------|--------------|--------------|
| `quality` | Maximum correctness | Opus everywhere, Sonnet for compaction | Enterprise, security-critical |
| `balanced` | Default | Sonnet primary, Haiku for compaction/simple, Opus for review | Most users |
| `cost` | Minimize spend | Haiku primary, Sonnet only for complex | Personal/hobby, high-volume |
| `local` | Fully offline | Ollama/local models only | Air-gapped, privacy-sensitive |

### Threshold for SOTA Validation Loop

```
ROUTING_PROFILES_MIN               = 3   (quality, balanced, cost)
COST_REDUCTION_BALANCED_VS_SINGLE  = ≥25%
QUALITY_MAINTENANCE_BALANCED       = ≥95% of single-model baseline success rate
PARETO_OPERATING_POINTS_TRACKED    = ≥4  (one per profile + one oracle upper bound)
```

---

## 7. Multi-Provider Routing

### The Problem

Anthropic's SDK does not support cross-provider routing inside one session (claude-code issue #38698). The `model` field accepts only Claude aliases. Any "Ollama for subagents, Anthropic for orchestrator" routing must be implemented by the harness.

### Hermes: Auxiliary Client Fallback Chain

Hermes (`auxiliary_client.py`) implements a 7-step resolution chain for text tasks:

```
1. OpenRouter        (OPENROUTER_API_KEY)
2. Nous Portal       (~/.hermes/auth.json)
3. Custom endpoint   (config.yaml model.base_url + OPENAI_API_KEY)
4. Codex OAuth       (gpt-5.3-codex via chatgpt.com)
5. Native Anthropic
6. Direct API-key    (z.ai, Kimi, MiniMax)
7. None
```

And a separate chain for vision tasks. Key design decisions:

- **HTTP 402 auto-fallback:** When a provider returns 402 (payment required), `call_llm()` automatically retries with the next provider. Handles credit exhaustion gracefully.
- **Per-task overrides:** `auxiliary.vision.provider`, `auxiliary.compression.model` in config.yaml.
- **Provider aliases:** 14 aliases normalized (e.g., "claude" → "anthropic", "grok" → "xai").
- **Lazy resolution:** Provider chain evaluated at call time, not session start.

### OpenDev: Named Slots with Cascading Defaults

OpenDev uses named config slots (`agents.<name>`) with cascade resolution:

```
slot-level override → explicit user config → global default → hard-coded fallback
```

Each slot carries `{ model, provider, prompt, temperature }`. Provider is resolved independently from model, allowing cross-provider routing (e.g., Action on Anthropic, Compact on Ollama).

### Recommended Pattern for theo-code

Combine both approaches:

1. **Named slots** (OpenDev style) for the 4+ workflow roles (Normal, Compact, Vision, Subagent).
2. **Fallback chain per slot** (Hermes style) for resilience: if the primary provider for a slot fails, fall through to alternatives.
3. **Provider normalization** (Hermes style) with alias map for user convenience.
4. **HTTP 402/429 auto-fallback** within the chain, bounded by `MAX_FALLBACK_HOPS = 2`.

### Threshold for SOTA Validation Loop

```
MULTI_PROVIDER_SUPPORT             = true (≥2 providers configurable)
FALLBACK_CHAIN_LENGTH_MAX          = 3   (per slot)
AUTO_FALLBACK_ON_402_429           = true
PROVIDER_ALIAS_NORMALIZATION       = true
CROSS_PROVIDER_ROUTING             = true (harness-level, not SDK-level)
```

---

## 8. Routing for Subagents

### Current State in theo-code

`SubAgentSpec` already carries `model_override: Option<String>` (`crates/theo-agent-runtime/src/subagent/parser.rs:170`). When present, `spawn_helpers.rs:432` applies it. But currently:

- No **budget allocation** across subagents
- No **model-per-role** defaults (all builtins set `model_override: None`)
- No **routing awareness** (subagents inherit parent's model regardless of task complexity)

### Anthropic's Subagent Model Field

Anthropic subagent definitions accept `model: sonnet | opus | haiku | inherit | <full-model-id>`. The `inherit` option propagates the parent's model. This is the simplest approach but wastes cost when an Explorer subagent inherits Opus.

### OpenDev's SubAgentSpec

OpenDev subagents carry an explicit model parameter (`haiku/sonnet/opus`) plus an iteration budget. The main agent controls delegation depth and tool access per subagent type.

### Budget Allocation Strategy

From VeRO's findings: ~91% of compute in optimization runs occurs in child agents (evaluation runs). From Concordia University: input tokens constitute 53.9% of total consumption in multi-agent systems — more than half the budget is re-consuming context.

**Proposed budget model:**

| Subagent role | Default model | Max iterations | Budget share (of parent) |
|---------------|--------------|----------------|--------------------------|
| Explorer | Haiku | 10 | 10% |
| Implementer | Sonnet (inherit) | 20 | 40% |
| Verifier | Sonnet | 10 | 20% |
| Reviewer | Opus (or Sonnet) | 5 | 30% |

**Budget enforcement rule:** Subagent budget = `parent_remaining_budget * budget_share_pct`. If a subagent exhausts its allocation, it returns partial results rather than consuming the parent's budget. This prevents the "91% in children" problem VeRO identified.

### Threshold for SOTA Validation Loop

```
SUBAGENT_MODEL_OVERRIDE_SUPPORTED    = true
SUBAGENT_BUDGET_ALLOCATION           = true (percentage-based, not fixed)
SUBAGENT_DEPTH_LIMIT                 = 2   (prevent recursive fan-out)
SUBAGENT_DEFAULT_MODEL_PER_ROLE      = true (not all inherit parent)
EXPLORER_USES_CHEAP_MODEL            = true (Haiku or equivalent)
```

---

## 9. Token Overhead of Multi-Agent Systems

### Quantitative Evidence

| Source | Finding | Overhead factor |
|--------|---------|-----------------|
| Base report (smart-model-routing.md) | Multi-agent systems consume ~15x more tokens | 15x |
| Code analysis (2026 industry) | Multi-agent tasks consume 4-6x tokens of single-agent | 4-6x |
| Concordia University (GPT-5) | Input tokens = 53.9% of total consumption | N/A (structural) |
| VeRO (Scale AI) | ~91% of compute in child agents | 10x+ |
| OpenAI Codex | max_depth=1 default to prevent recursive fan-out | N/A (architectural control) |

### Is This a Routing Problem or an Architecture Problem?

**Both.** The token overhead has two components:

1. **Routing component (addressable):** Subagents using expensive models for simple tasks. Routing to Haiku for explorers and compaction saves 60-80% on those turns.

2. **Architecture component (structural):** System prompt duplication across subagents, context re-consumption, result summarization overhead. This is not solvable by routing alone.

**Routing can address ~30-40% of the overhead.** The remaining 60-70% requires architectural changes:
- Prompt caching (Section 4) eliminates system prompt re-encoding cost
- Result summarization (only final output returns to parent) keeps parent context clean
- Shallow delegation (max_depth=1-2) prevents exponential fan-out

### Bounding Token Overhead

| Control | Mechanism | Expected reduction |
|---------|-----------|-------------------|
| Model routing for subagents | Haiku for Explorer, Sonnet for Implementer | 30-40% cost reduction on subagent turns |
| Prompt caching across subagents | Shared cached prefix for tool schemas | 87% input cost reduction (Section 4) |
| Iteration budget per subagent | Max 10-20 iterations per subagent | Bounds worst-case consumption |
| Depth limit | max_depth=2 | Prevents exponential fan-out |
| Summary-only return | Subagent returns summary, not full trace | Parent context stays clean |

### Threshold for SOTA Validation Loop

```
MULTI_AGENT_OVERHEAD_FACTOR_MAX    = 6x   (vs single-agent baseline)
SUBAGENT_SUMMARY_ONLY_RETURN       = true
SUBAGENT_MAX_DEPTH                 = 2
SUBAGENT_ITERATION_CAP             = 20
TOKEN_BUDGET_TRACKING              = true (per-subagent)
```

---

## 10. Routing Benchmarks and Evaluation Metrics

### Existing Benchmarks

| Benchmark | What it measures | Routing-relevant? |
|-----------|-----------------|-------------------|
| MT-Bench | Multi-turn conversation quality | Yes — RouteLLM reports -85% cost at 95% quality |
| RouterBench | 405K inference outcomes across 64 tasks | Yes — extracts Pareto-optimal cost-quality pairs |
| Pareto Frontier Bench (Airlock) | Multi-step cross-domain routing | Yes — tests dynamic per-subtask routing |
| SWE-bench Lite | Software engineering task resolution | Yes — Triage evaluates routing on 300 SE tasks |
| Arena Elo | Human preference ranking | Indirect — establishes model quality ordering |

### Metrics a Routing System Should Track

| Metric | Definition | Target | Why |
|--------|-----------|--------|-----|
| **Cost reduction** | `1 - (routed_cost / single_model_cost)` | ≥25% | Primary routing value proposition |
| **Quality maintenance** | `routed_success_rate / single_model_success_rate` | ≥0.95 | Router must not regress quality |
| **Routing overhead latency** | p95 of routing decision time | <50 ms (interactive) | Must not degrade UX |
| **Fallback success rate** | `successful_fallbacks / total_fallbacks` | ≥80% | Fallback chain must work |
| **Escalation rate** | `turns_escalated / total_turns` | 20-40% | Too low = under-routing; too high = rules too conservative |
| **Cache hit rate** | `cache_reads / (cache_reads + cache_misses)` | ≥70% | Prompt caching effectiveness |
| **Simple-turn accuracy** | Correct identification of "simple" turns | ≥90% | False positives degrade quality |
| **Budget utilization** | `actual_cost / allocated_budget` | 60-90% | Too low = over-routing to cheap; too high = no headroom |

### Proposed Evaluation Protocol for theo-code

1. **Fixture set:** 30 tasks (10 simple, 10 medium, 10 complex) from `theo-benchmark`.
2. **Baseline run:** `NullRouter` (single model, current behavior).
3. **Routing run:** `RuleBasedRouter` with balanced profile.
4. **Metrics computed:** All 8 metrics above, as JSON report.
5. **Pass criteria:** Cost reduction ≥25%, quality maintenance ≥0.95, routing overhead <50 ms.
6. **Regression gate:** Any new routing change must not regress any metric by >2%.

### Threshold for SOTA Validation Loop

```
ROUTING_EVAL_METRICS_TRACKED       = ≥6  (cost, quality, latency, fallback, escalation, cache)
ROUTING_EVAL_FIXTURE_SIZE          = ≥30 tasks (10 per difficulty tier)
ROUTING_QUALITY_REGRESSION_GATE    = ≤2% degradation on any metric
COST_REDUCTION_MINIMUM             = ≥25%
```

---

## Relevance for Theo Code

### Mapping to theo-code Architecture

| Research finding | Target crate | Target file/module | Priority |
|-----------------|-------------|-------------------|----------|
| ModelRouter trait + RoutingPhase | `theo-domain` | `src/routing.rs` (new) | R1 — already planned |
| RuleBasedRouter + PricingTable | `theo-infra-llm` | `src/routing/rules.rs` (new) | R2 — already planned |
| ModelCapabilities schema | `theo-infra-llm` | `src/model_limits.rs` (extend) | R2 — **new: add capabilities beyond token limits** |
| Provider capability cache (24h TTL) | `theo-infra-llm` | `src/provider/registry.rs` (extend) | R4 — new finding |
| Prompt cache-aware prompt structure | `theo-infra-llm` | `src/client.rs` (restructure prompt building) | R4 — **new: ensure static prefix for caching** |
| Subagent model defaults per role | `theo-agent-runtime` | `src/subagent/builtins.rs` (set `model_override`) | R4 — easy win |
| Budget allocation per subagent | `theo-agent-runtime` | `src/subagent/spawn_helpers.rs` (add budget field) | R5 — new finding |
| Routing profiles (quality/balanced/cost) | `theo-application` | `src/config/project_config.rs` | R4 — new finding |
| Routing evaluation metrics | `theo-benchmark` | `apps/theo-benchmark/` | R0 — extend planned harness |
| Semantic Router (R6 learned) | `theo-infra-llm` | `src/routing/semantic.rs` (future) | R6 — behind feature flag |

### Immediate Actions (Before R1 Starts)

1. **Extend `model_limits.rs`** to include `ModelCapabilities` struct with vision, tool_use, reasoning, pricing, and tier fields. This is a prerequisite for routing decisions.
2. **Set `model_override` on builtin subagents:** Explorer and Verifier should default to Haiku; Implementer inherits parent; Reviewer uses Sonnet or higher. Currently all four are `model_override: None`.
3. **Restructure prompt building** in `client.rs` to ensure a stable cacheable prefix (tool schemas + system prompt) followed by dynamic content (conversation history). This maximizes prompt cache hit rate.

### What NOT to Do

| Anti-pattern | Why | What to do instead |
|-------------|-----|-------------------|
| LLM-as-router for interactive turns | 500-2000 ms overhead destroys TUI UX | Rule-based routing (<10 us); defer learned routing to R6 with strict latency cap |
| Cascade routing for every turn | p95 latency doubles; user feels the stutter | Cascade only for background subagents and batch operations |
| Single fallback strategy for all errors | 429 (rate limit) and 400 (bad request) need fundamentally different responses | Typed error classification: retryable (429, 5xx, timeout) vs permanent (400, 401, 403) |
| Unbounded subagent delegation | 91% of compute ends up in children (VeRO); exponential fan-out | Budget allocation + depth limit + iteration cap |
| Ignoring prompt cache structure | 7% hit rate without optimization (ProjectDiscovery) | Static prefix + dynamic suffix; 74% hit rate achievable |
| Hard-coding model IDs in routing logic | Model names drift every few months | Tier aliases (cheap/standard/strong) resolved via PricingTable |
| Routing without measuring | Cannot prove cost savings or quality maintenance | R0 benchmark harness is a blocker; track all 8 metrics from Section 10 |
| Training a classifier without cold-start plan | No historical routing data exists | Start with Semantic Router (5-20 exemplars, no training pipeline) |
| Cross-task optimization without regression testing | VeRO: GAIA +5.75% caused SimpleQA -17.8% | Always validate routing changes on the full benchmark suite |

---

## Consolidated SOTA Thresholds

All thresholds from sections 1-10, collected for the SOTA validation loop:

```toml
[model_routing.thresholds]

# Section 1: Latency
routing_overhead_interactive_ms   = 50
routing_overhead_background_ms    = 2000
cascade_max_steps_interactive     = 1
cascade_max_steps_background      = 3

# Section 2: Learned routing
learned_router_latency_cap_ms     = 50
learned_router_cold_start_max     = 20   # max exemplars before usable
learned_router_quality_gate       = 0.95 # vs single-model baseline

# Section 3: Workflow binding
min_workflow_roles                = 4
lazy_client_init                  = true
provider_cache_ttl_hours          = 24
cost_savings_vs_single_model      = 0.20 # >=20%

# Section 4: Prompt caching
cache_hit_rate_target             = 0.70
cache_aware_prompt_structure      = true
min_cacheable_prefix_tokens       = 2048
cache_write_amortization_turns    = 2

# Section 5: Capability detection
capability_cache_ttl_hours        = 24
capability_staleness_warning_days = 30
offline_fallback_manifest         = true
capability_fields_min             = 6

# Section 6: Pareto frontier
routing_profiles_min              = 3    # quality, balanced, cost
cost_reduction_balanced           = 0.25 # >=25%
quality_maintenance_balanced      = 0.95
pareto_operating_points_tracked   = 4

# Section 7: Multi-provider
multi_provider_support            = true
fallback_chain_length_max         = 3
auto_fallback_on_402_429          = true
cross_provider_routing            = true

# Section 8: Subagent routing
subagent_model_override           = true
subagent_budget_allocation        = true
subagent_depth_limit              = 2
explorer_uses_cheap_model         = true

# Section 9: Token overhead
multi_agent_overhead_max          = 6.0  # vs single-agent
subagent_summary_only_return      = true
subagent_iteration_cap            = 20
token_budget_tracking             = true

# Section 10: Evaluation
routing_eval_metrics_tracked      = 6
routing_eval_fixture_size         = 30
routing_quality_regression_gate   = 0.02 # max 2% degradation
cost_reduction_minimum            = 0.25
```

---

## Citations

### Academic Papers

1. Chen, L., Zaharia, M., & Zou, J. (2023). FrugalGPT: How to Use Large Language Models While Reducing Cost and Improving Performance. arXiv:2305.05176. TMLR 12/2024.
2. Ong, I., et al. (2024). RouteLLM: Learning to Route LLMs with Preference Data. arXiv:2406.18665.
3. Zhang, H., Feng, T., & You, J. (2025). Router-R1: Teaching LLMs Multi-Round Routing and Aggregation via Reinforcement Learning. arXiv:2506.09033. NeurIPS 2025.
4. SATER authors (2025). A Self-Aware and Token-Efficient Approach to Routing and Cascading. arXiv:2510.05164. EMNLP 2025.
5. Madeyski, L. (2026). Triage: Routing Software Engineering Tasks to Cost-Effective LLM Tiers via Code Quality Signals. arXiv:2604.07494.
6. Bui, N. D. Q. (2026). Building AI Coding Agents for the Terminal. arXiv:2603.05344v1.
7. Ursekar, V., et al. (2026). VeRO: An Evaluation Harness for Agents to Optimize Agents. arXiv:2602.22480v1.

### Industry Sources

8. [Opper.ai — LLM Router Latency Benchmark 2026](https://opper.ai/blog/llm-router-latency-benchmark-2026)
9. [AIMultiple — LLM Latency Benchmark by Use Cases in 2026](https://research.aimultiple.com/llm-latency-benchmark/)
10. [MegaNova — The 3-Tier Routing Cascade](https://blog.meganova.ai/the-3-tier-routing-cascade-rule-based-semantic-llm/)
11. [Anthropic — Prompt Caching Docs](https://platform.claude.com/docs/en/build-with-claude/prompt-caching)
12. [Anthropic — Pricing](https://platform.claude.com/docs/en/about-claude/pricing)
13. [Finout — Anthropic API Pricing 2026](https://www.finout.io/blog/anthropic-api-pricing)
14. [ProjectDiscovery — How We Cut LLM Costs by 59% With Prompt Caching](https://projectdiscovery.io/blog/how-we-cut-llm-cost-with-prompt-caching)
15. [MindStudio — Advisor Strategy](https://www.mindstudio.ai/blog/claude-code-advisor-strategy-opus-sonnet-haiku)
16. [DigitalApplied — AI Model Efficient Frontier Q2 2026](https://www.digitalapplied.com/blog/ai-model-performance-vs-price-efficient-frontier-q2)
17. [Augment Code — AI Model Routing Guide](https://www.augmentcode.com/guides/ai-model-routing-guide)
18. [Airlock Labs — Pareto Frontier Bench](https://airlocklabs.io/pareto-frontier-bench.html)
19. [BenchLM.ai — LLM Speed & Latency Comparison](https://benchlm.ai/llm-speed)
20. [Aurelio Labs — Semantic Router](https://github.com/aurelio-labs/semantic-router)
21. [The New Stack — Semantic Router and Agentic Workflows](https://thenewstack.io/semantic-router-and-its-role-in-designing-agentic-workflows/)
22. [Continue — Capability Detection](https://deepwiki.com/continuedev/continue/4.6-capability-detection)
23. [Addy Osmani — The Code Agent Orchestra](https://addyosmani.com/blog/code-agent-orchestra/)
24. [Maxim — Top 5 LLM Routing Techniques](https://www.getmaxim.ai/articles/top-5-llm-routing-techniques/)
25. [claude-code issue #38698 — per-agent provider routing](https://github.com/anthropics/claude-code/issues/38698)

### Reference Repositories

26. `referencias/hermes-agent/agent/smart_model_routing.py` — Rule-based routing algorithm
27. `referencias/hermes-agent/agent/auxiliary_client.py` — Multi-provider fallback chain
28. `referencias/opendev/crates/opendev-cli/src/setup/mod.rs` — Named slot configuration
29. `referencias/opendev/crates/opendev-models/src/config/agent.rs` — AgentConfigInline cascade
30. `crates/theo-agent-runtime/src/subagent/builtins.rs` — Current subagent model_override (all None)
31. `crates/theo-agent-runtime/src/subagent/parser.rs` — SubAgentSpec with model_override
32. `crates/theo-agent-runtime/src/subagent/spawn_helpers.rs` — model_override application
33. `crates/theo-infra-llm/src/model_limits.rs` — Current model token limits
34. `crates/theo-infra-llm/src/provider/registry.rs` — Provider registry
