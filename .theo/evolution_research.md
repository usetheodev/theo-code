# Evolution Research: Memory & State — Theo Ahead of Hermes

> **Source of truth**: `docs/plans/PLAN_MEMORY_SUPERIORITY.md` (APPROVED, revised v2)
> **Meeting ata**: `.claude/meetings/20260420-221947-memory-superiority-plan.md` (16 agentes, consensus APPROVE)
> **Started**: 2026-04-21
> **Note**: `referencias/` directory absent. The research product here synthesizes (a) the meeting-approved plan which cross-validated Hermes-agent SOTA patterns, (b) external paper references absorbed into the plan (MemArchitect, Knowledge Objects, CodeTracer), and (c) the verified current-state map of the codebase.

---

## Current State Map (verified via Grep/Read 2026-04-21)

| Component | Status | Evidence |
|---|---|---|
| `BuiltinMemoryProvider` | Exists, NO OnceLock snapshot | `crates/theo-infra-memory/src/builtin.rs:28-120` — state is `Arc<RwLock<BuiltinState>>` |
| `unicode-normalization` dep | **ABSENT** | `crates/theo-infra-memory/Cargo.toml` — deps: theo-domain, tokio, async-trait, serde, sha2, futures, thiserror |
| `security.rs` | Pattern-only, NO NFKD/zero-width/mixed-script | `crates/theo-infra-memory/src/security.rs:36-87` |
| `MemoryLifecycle` hooks | Implemented but NOT WIRED | `crates/theo-agent-runtime/src/memory_lifecycle.rs:19-69` |
| `run_engine.rs` ad-hoc FileMemoryStore | Present at line 319 | `crates/theo-agent-runtime/src/run_engine.rs:319` |
| Episodes path | `.theo/wiki/episodes/` (WRONG) | `run_engine.rs:227` |
| `EpisodeSummary.schema_version` | Present (u32) | `crates/theo-domain/src/episode.rs:53` |
| `MemoryLesson.schema_version` | **MISSING** | `crates/theo-domain/src/memory/lesson.rs:32-54` |
| `LessonStatus::Retracted` | Active variant (should be `Invalidated`) | `lesson.rs:35` |
| `apply_gates()` | Defined, NOT wired | `lesson.rs:173-222` |
| `build_memory_engine` factory | **ABSENT** | `crates/theo-application/src/` |
| `Hypothesis` / `HypothesisStatus` | Types exist | `crates/theo-domain/src/episode.rs:186-227` |
| `SessionTree` | **ABSENT** | No type found in theo-domain |
| UI `AppSidebar.tsx` Memory group | **ABSENT** | `apps/theo-ui/src/app/AppSidebar.tsx:30-116` |

**Gap from baseline**: ~80 LOC PREP + ~150 LOC WIRE + ~1040 LOC for Phases 1-3.

---

## SOTA Pattern Catalog (from Hermes cross-validation + absorbed references)

### Pattern 1: Atomic WIRE unit (Phase 0) — "dormant code is worse than absent code"

**Source**: Meeting consensus (chief-architect, evolution-agent, code-reviewer). Hermes-agent has this pattern production-wired.

**Why it matters**: 500+ tests pass on logic that never runs in production. The gap between "exists" and "runs" is the critical bottleneck.

**Pattern**:
- `prefetch()` → pre-call injection as fenced system message
- LLM call
- `sync_turn()` → inline write (NOT fire-and-forget — durability > latency)
- `on_pre_compress()` → before compaction
- `on_session_end()` → on every exit path (converged/abort/Drop)

**Anti-pattern**: Dual memory injection — ad-hoc `FileMemoryStore::for_project` at `run_engine.rs:319` conflicts with formal provider. Must be removed atomically with the wiring.

**Translation to theo-code**: `MemoryLifecycle::prefetch/sync_turn/on_pre_compress/on_session_end` (already implemented in `memory_lifecycle.rs:19-69`). Wire them in `run_engine.rs`.

### Pattern 2: Frozen snapshot for prefix-cache stability

**Source**: Hermes-agent. Absorbed reference: prompt-caching literature.

**Why it matters**: LLM prefix cache stability requires deterministic system prompts within a session. Re-reading memory state mid-session breaks prefix cache.

**Tradeoff**: Lose intra-session visibility of mid-session writes (writes persist but are not visible until next session). Same tradeoff Hermes makes — deliberate.

**Primitive**: `std::sync::OnceLock<String>` (NOT `OnceCell` — stdlib, thread-safe).

**Translation**: Add `snapshot: OnceLock<String>` to `BuiltinMemoryProvider`. `prefetch()` calls `get_or_init(|| state.read().entries.join("\n"))`.

### Pattern 3: 7-gate lesson composition (novel, publishable)

**Source**: Meeting (research-agent confirmed: zero prior art in coding agents for this composition).

**Gates** (already implemented in `lesson.rs:173-222`):
1. Confidence bounds [0.0, 1.0]
2. Evidence count >= N
3. Dedup by lesson+trigger
4. Contradiction detection
5. Quarantine for low-confidence
6. Promotion on repeated confirmation
7. (planned) NLI-based contradiction via MemArchitect-style triage

**Translation**: Wire `apply_gates()` after run outcome=Failure|Partial. Persist approved to `.theo/memory/lessons/{id}.json` with `schema_version`.

### Pattern 4: Laplace-smoothed hypothesis tracking (novel)

**Source**: Meeting (research-agent — GENUINAMENTE NOVEL in coding agents). Absorbed from CodeTracer (arXiv:2604.11641).

**Pattern**:
- `evidence_for` / `evidence_against` counts per hypothesis
- Confidence = (evidence_for + 1) / (evidence_for + evidence_against + 2)   // Laplace
- `evidence_against > evidence_for * 2` → Superseded, auto-prune
- Stale marking after N days without update

**Translation**: Persist `unresolved_hypotheses` from episode summaries to `.theo/memory/hypotheses/{id}.json` with `schema_version`. Load Active hypotheses at next run's prefetch.

### Pattern 5: Lifecycle decay (Active → Cooling → Archived)

**Source**: Hermes + absorbed MemArchitect governance.

**Sidecar-based implementation** (Opcao A from plan):
- `.theo/memory/{user_hash}.meta.json` — per-entry metadata (indexed by dedup_key)
- `EntryMetadata { created_at, last_hit_at, hit_count, lifecycle }`
- `prefetch()` → tick → filter by lifecycle
- Entries without metadata treated as Active (graceful legacy migration)

**Translation**: Add sidecar format to `BuiltinMemoryProvider`. ~100 LOC (revised from 60).

### Pattern 6: Oversized-message protection in compaction

**Source**: Validator concern — Hermes uses per-message cap.

**Pattern**: Per-message cap of `context_window / 4`. Single message > cap is truncated even in protected tail. Prevents OOM loop.

**Translation**: Modify `compaction.rs` — add per-message cap after tail protection.

### Pattern 7: Keyword session search with recency decay

**Source**: Meeting resolved Conflict 2 — pragmatic baseline, evolves to RRF later.

**Formula**: `rank = keyword_overlap * 0.6 + recency * 0.4`. Scan episode JSONs in `.theo/memory/episodes/`. Max 3 results, <50ms for 100 episodes.

**Translation**: `SessionSearch` trait in `theo-domain`, impl in `theo-infra-memory`. Expose as agent tool.

### Pattern 8: Token/cost tracking with 6-field usage struct

**Source**: Hermes has this baseline. CLI exposure required.

**Struct** (new in `theo-domain`):
```
TokenUsage {
  input_tokens, output_tokens,
  cache_read_tokens, cache_write_tokens,
  reasoning_tokens, estimated_cost_usd
}
```

**Translation**: Persist in `EpisodeSummary.token_usage: Option<TokenUsage>`. CLI prints at end-of-run.

### Pattern 9: Unicode injection hardening (NFKD + zero-width + mixed-script)

**Source**: Validator finding — cyrillic lookalikes bypass current scanner.

**Pattern**:
1. NFKD normalization before pattern matching
2. Reject content containing U+200B/U+200C/U+200D/U+FEFF
3. Detect mixed-script (latin + cyrillic = suspect)

**Translation**: Add `unicode-normalization` dep to `theo-infra-memory`. Pre-process in `security.rs`.

### Pattern 10: Hash-addressed knowledge objects (Knowledge Objects, arXiv:2603.17781)

**Source**: Absorbed external. Applied to lesson promotion.

**Pattern**: `Confirmed` lesson → hash-keyed immutable knowledge object. Deduplication via content hash.

**Translation**: Reuse SHA256 dedup key already in `BuiltinMemoryProvider`. Lessons keyed by `hash(lesson_text || trigger)`.

---

## Novel Contributions (publishable)

Per research-agent finding: two genuinely novel features versus SOTA:
1. **7-gate lesson composition** (combining Hermes gates + MemArchitect triage + Knowledge Objects hash-addressing)
2. **Laplace-smoothed hypothesis tracking in coding agents** (CodeTracer adapted)

---

## Async Primitive Conventions (code-reviewer)

- **Frozen snapshot**: `std::sync::OnceLock<String>` (stdlib, thread-safe). NOT `once_cell::OnceCell`.
- **Background prefetch**: `tokio::sync::oneshot::{Sender, Receiver}`. NOT `Arc<Mutex<Option<T>>>`.
- **Async I/O**: `tokio::fs` everywhere in async context. Remove `std::fs::write` from `record_session_exit`.
- **No `unwrap()`/`expect()`** added. Convert existing 2 `expect()` in production to `Result`.

---

## Sequencing (from plan)

```
Pre-Phase 0 (PREP)  →  Phase 0 (WIRE — atomic)  →  [Phase 1 ∥ Phase 2]  →  Phase 3
```

**Phase 0 is the critical path**. All other value is unlocked by wiring.

**LOC budget (revised)**: ~1220 total (80 PREP + 150 WIRE + 400 GAPS + 280 ACTIVATE + 310 SURPASS).

**T3.3 BLOCKED** until: eval dataset (20-30 pairs) + `BudgetConfig.memory_pct` + empirical threshold calibration.

---

## Evolution Strategy for This Loop

Given the 200-LOC-per-change guardrail and 15-iteration budget, we execute in this order:

1. **Pre-Phase 0** (small tasks) as commits — P.1+P.3+P.4 batched (~80 LOC). P.2 separate (~40 LOC). P.5 deferred (AppSidebar.tsx is in apps/theo-ui — outside CAN-modify scope since `apps/theo-desktop` is excluded; theo-ui is separate — verify).
2. **Phase 0** atomic WIRE — split into T0.1 (hooks + remove ad-hoc), T0.2 (factory), T0.3 (feed episodes) — 3 commits.
3. **Phase 1** — T1.1 (tokens), T1.2 (OnceLock), T1.3 (compaction), T1.4 (search) — 4 commits.
4. **Phase 2** — T2.1 (lessons), T2.2 (decay sidecar), T2.3 (hypotheses) — 3 commits.
5. **Phase 3** — T3.1 (reasoning — with mini-ADR first), T3.2 (bg prefetch), T3.4 (user/agent split). T3.3 skipped unless eval dataset materializes.

Each commit passes hygiene (`cargo test --workspace` + `cargo check --workspace --tests`).
