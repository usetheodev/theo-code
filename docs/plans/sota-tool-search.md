# SOTA Tool Search — Implementation Plan

> Meeting: 20260426-181501 | Verdict: REVISED (approved with modifications)
> Reference: Anthropic measured 85% token reduction, 49%→74% accuracy with tool search

## Problem

Theo sends 35 tool definitions per LLM turn (~6-8K tokens). Infrastructure for deferred tools exists (`should_defer`, `search_hint`, `tool_search` meta-tool) but is idle — zero real tools use deferral. The `tool_search` handler returns only `(id, hint)` pairs without schemas, forcing the agent to guess parameters.

## Goal

Reduce tool token budget from ~6-8K to ~2-3K tokens/turn while maintaining 100% tool discoverability via on-demand search. Target: 12-15 always-visible core tools, 15+ deferred tools discoverable via enriched `tool_search`.

## Architecture Decisions (from meeting)

| Decision | Rationale |
|----------|-----------|
| **NO new `ToolExposure::Deferred` variant** | Deferral is a runtime scheduling decision, not static registration. Ontology-manager ruling. |
| **Scoring in `theo-tooling`** | Weighted token overlap (~60 LOC) operates only on data already in registry. No external deps needed. |
| **`should_defer()` stays `&self -> bool`** | No runtime context in domain trait. Contextual deferral via `DeferralPolicy` in runtime layer. |
| **Schemas returned by default, top-3** | Aligned with Claude Code pattern. Bounded worst-case to ~1500 tokens per search call. |
| **Fallback auto-search** | If agent calls unknown tool that exists as deferred, auto-trigger search. Eliminates silent degradation. |

## Files Affected

### theo-domain (`crates/theo-domain/src/`)
- `tool.rs` — No trait changes. May add `ToolSearchResult` struct if shared across crates.

### theo-tooling (`crates/theo-tooling/src/`)
- `registry/mod.rs` — `search_deferred()` return type changes to `Vec<ToolSearchResult>`. Add scoring logic. Add `visible_definitions_with_policy()`.
- `tool_manifest.rs` — Update notes for deferred tools.
- Individual tool files — Override `should_defer()` + `search_hint()` for deferred tools.

### theo-agent-runtime (`crates/theo-agent-runtime/src/`)
- `tool_bridge/execute_meta.rs` — `handle_tool_search()` serializes full schemas. Add fallback auto-search.
- `tool_bridge/meta_schemas.rs` — `tool_search()` schema unchanged (query param sufficient).
- `tool_bridge/mod.rs` — `registry_to_definitions()` may accept `DeferralPolicy` (Fase 4).

### Documentation
- `.claude/CLAUDE.md` — Fix tool count invariant (currently says 21, actual is 27).
- `CHANGELOG.md` — Entry under `[Unreleased]`.

## Implementation Phases

### Fase 0: Foundation (pre-requisite)

**Scope:** Fix documentation drift, add structural test.

**Tasks:**
1. Update CLAUDE.md tool counts to match reality
2. Add test `visible_registry_has_expected_count` in `theo-tooling/src/registry/mod.rs`
3. Add CHANGELOG entry under `[Unreleased] > Changed`

**TDD:** N/A (documentation + guard test)
**Verify:** `cargo test -p theo-tooling`
**Gate:** All tests pass, CLAUDE.md accurate

---

### Fase 1: Enrich tool_search with schemas

**Scope:** Change `search_deferred` return type. Return full `ToolDefinition` with schema for each hit.

**New type (theo-tooling):**
```rust
pub struct ToolSearchResult {
    pub id: String,
    pub hint: String,
    pub score: f32,
    pub definition: ToolDefinition,
}
```

**TDD — RED:**
```rust
#[test]
fn search_deferred_returns_tool_search_result_with_schema() {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(DeferredStubWithSchema { ... })).unwrap();
    let hits = registry.search_deferred("wiki");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "wiki_search");
    assert!(!hits[0].definition.schema.params.is_empty());
}
```

**TDD — GREEN:**
- Create `ToolSearchResult` struct
- Change `search_deferred()` signature: `fn search_deferred(&self, query: &str) -> Vec<ToolSearchResult>`
- Initial score = 1.0 for all matches (scoring comes in Fase 2)
- Update `handle_tool_search` to serialize schemas in response

**TDD — REFACTOR:**
- Update 4 existing tests to destructure `ToolSearchResult`
- Cap results at 5 (configurable via const)

**Verify:** `cargo test -p theo-tooling && cargo test -p theo-agent-runtime`

---

### Fase 2: Intelligent scoring

**Scope:** Replace substring match with weighted token overlap.

**Algorithm (self-contained, ~60 LOC in theo-tooling):**
```
tokenize(query) → Vec<String>  // split on non-alphanumeric
for each deferred tool:
    id_tokens = tokenize(tool.id)
    hint_tokens = tokenize(tool.search_hint)
    desc_tokens = tokenize(tool.description)  // first 50 chars
    
    id_matches = count(query_tokens ∩ id_tokens) * 5.0
    hint_matches = count(query_tokens ∩ hint_tokens) * 1.0
    desc_matches = count(query_tokens ∩ desc_tokens) * 0.5
    prefix_bonus = if tool.id starts_with query → +3.0 else 0
    
    score = (id_matches + hint_matches + desc_matches + prefix_bonus) / query_tokens.len()
```

**TDD — RED:**
```rust
#[test]
fn search_deferred_ranks_exact_id_match_above_hint_match() { ... }

#[test]
fn search_deferred_ranks_prefix_match_above_substring() { ... }

#[test]
fn search_deferred_wildcard_returns_all_deferred_tools() {
    // query: "*" → escape hatch, returns all
}
```

**TDD — GREEN:** Implement scoring. Add wildcard escape hatch.
**TDD — REFACTOR:** Extract tokenizer to helper function.
**Verify:** `cargo test -p theo-tooling`

---

### Fase 3: Defer tools (incremental batches)

**Scope:** Mark production tools as deferred. Three batches with benchmark gate.

**Batch 1 — Low risk (5 tools):**
`http_get`, `http_post`, `reflect`, `task_create`, `task_update`

**Batch 2 — Planning (6 tools):**
`plan_create`, `plan_summary`, `plan_advance_phase`, `plan_log`, `plan_update_task`, `plan_next_task`

**Batch 3 — Git (4 tools):**
`git_status`, `git_diff`, `git_log`, `git_commit`

**Per-batch TDD — RED:**
```rust
#[test]
fn batch_N_tools_are_deferred_and_discoverable() {
    let registry = create_default_registry();
    for id in BATCH_IDS {
        let tool = registry.get(id).unwrap();
        assert!(tool.should_defer());
        assert!(tool.search_hint().is_some());
        let hits = registry.search_deferred(id);
        assert!(!hits.is_empty(), "{id} must be discoverable");
    }
}
```

**Per-batch gate:** Run benchmark on vast.ai. If pass rate drops >2%, revert batch and investigate.

**Always-visible core (after all batches):**
`bash`, `read`, `write`, `edit`, `grep`, `glob`, `apply_patch`, `webfetch`, `think`, `memory`, `env_info`, `codebase_context`

---

### Fase 4: Contextual deferral

**Scope:** `DeferralPolicy` makes tool visibility mode-aware without changing `Tool` trait.

**New type (theo-domain):**
```rust
pub struct DeferralPolicy {
    /// Tools that override their static should_defer() for this context.
    pub force_visible: Vec<String>,
    pub force_deferred: Vec<String>,
}

impl DeferralPolicy {
    pub fn for_mode(mode: &AgentMode) -> Self { ... }
    pub fn unrestricted() -> Self { ... }  // all tools visible (backward compat)
}
```

**New method (theo-tooling):**
```rust
impl ToolRegistry {
    pub fn visible_definitions_with_policy(&self, policy: &DeferralPolicy) -> Vec<ToolDefinition> { ... }
}
```

**TDD — RED:**
```rust
#[test]
fn plan_tools_visible_in_plan_mode() { ... }

#[test]
fn plan_tools_deferred_in_agent_mode() { ... }
```

**Verify:** `cargo test` + `bash scripts/check-arch-contract.sh`

---

### Fase 5: Fallback auto-search + UI events

**Scope:** Eliminate silent degradation. UI integration.

**Fallback logic (theo-agent-runtime):**
In `execute_tool_call`, if tool not in visible set but exists as deferred → auto-execute `tool_search` → inject schema → execute original call.

**UI (theo-desktop):**
- Emit `ToolDiscovered` event on SSE stream
- Render `tool_search` as meta-step (lighter visual treatment)
- Tool palette in sidebar (read-only, v2)

**TDD — RED:**
```rust
#[test]
fn unknown_deferred_tool_call_auto_searches_and_executes() { ... }
```

---

## Success Metrics

| Metric | Before | Target | How to measure |
|--------|--------|--------|----------------|
| Tools in system prompt | 35 | 12-15 | Count in `registry_to_definitions()` |
| Tokens/turn for tools | ~6-8K | ~2-3K | Instrument token count |
| tool_search recall@3 | N/A | >95% | Fixture: 15 deferred, representative queries |
| Benchmark pass rate | 50% | ≥48% | SWE-bench on vast.ai |
| Latency per turn | baseline | <+200ms | tool_search is microseconds for 45 tools |

## Risks & Mitigations

| Risk | Severity | Mitigation |
|------|----------|------------|
| Agent can't find deferred tool | HIGH | Fallback auto-search (Fase 5), wildcard escape hatch (Fase 2) |
| Schema size explosion | MEDIUM | Cap at top-3 with schemas, top-5 without |
| Breaking search_deferred callers | MEDIUM | Only 1 caller (handle_tool_search), update atomically |
| Benchmark regression | MEDIUM | Per-batch gate, revert if >2% drop |
| CLAUDE.md drift | LOW | Fix in Fase 0 before any implementation |

## References

- Meeting ata: `.claude/meetings/20260426-181501-sota-tool-search-system.md`
- Research: `outputs/reports/sota-tool-search-deferred-loading.md`
- Anthropic: 85% token reduction, 49%→74% accuracy (tool search enabled)
- OpenDev: 40%→5% startup context, 54% peak context reduction
- Claude Code v2.1.69: defers ALL built-in tools behind ToolSearch
