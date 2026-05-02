# Plan: Wiki-Graph-Memory Blend for Code Retrieval

> **Version 1.0** — Implements a GraphRAG/HippoRAG-aligned retrieval architecture
> for Theo Code by blending three already-existing primitives — the **code graph**,
> the **wiki** (with BM25 lookup + authority tiers), and the **memory subsystem**
> — into a unified retrieval pipeline. Adds dense embedding of wiki pages, joint
> scoring across file/wiki/graph/memory signals, and bidirectional links so the
> agent gets BOTH the right code AND the institutional memory tied to it. Target
> outcome: lift `retrieval.mrr` from 0.760 (cycle-11 best) to ≥0.90 and `recall_at_5`
> from 0.498 to ≥0.85 on the local `theo-code` benchmark, while turning retrieval
> into the authoritative Memory/Context spine the D8 ADR originally promised.

## Context

### What we measured (cycles 1–14)

| Pipeline | MRR | R@5 | R@10 | nDCG@5 |
|---|---|---|---|---|
| BM25 baseline (production) | 0.593 | 0.462 | 0.545 | 0.427 |
| Dense+RRF (in-tree, not wired) | 0.689 | 0.518 | 0.628 | 0.504 |
| BM25 → Codex Rerank | 0.715 | 0.496 | 0.592 | 0.502 |
| **Dense+RRF → Codex Rerank (cycle 11)** | **0.760** | **0.498** | **0.648** | **0.523** |
| Multi-BM25 + Query Rewrite (cycle 14) | 0.734 | 0.465 | 0.612 | 0.484 |
| Dod-gate floor | 0.90 | 0.92 | 0.95 | 0.85 |

Five of thirty queries consistently fail (MRR < 0.30) — all natural-language
conceptual queries with no lexical overlap with the expected files (`community
detection clustering algorithm`, `agent loop state machine transitions`, etc.).
Cycle 14 confirmed that lexical query rewriting *over BM25* dilutes more than it
helps. The bottleneck is the **candidate generator's vocabulary gap** between NL
queries and code identifiers — not rerank quality.

### What already exists (and is underused)

- **Wiki module** at `crates/theo-engine-retrieval/src/wiki/` — generator, renderer,
  BM25 lookup, authority tiers (deterministic > enriched > cached), `SourceRef`
  provenance pointing back to files. `wiki_e2e` benchmark generated 94 pages on
  theo-code in 40 s.
- **Code graph** at `crates/theo-engine-graph/` — file/symbol nodes, Calls/Imports/
  Tests/Contains edges, Leiden hierarchical clustering, attention propagation.
- **Memory subsystem** at `crates/theo-infra-memory/` with separate runtime
  insight loop in `wiki/runtime.rs` (ingest, distill, promote).
- **Dense embeddings** at `crates/theo-engine-retrieval/src/embedding/` —
  `NeuralEmbedder` (Jina v1), `EmbeddingCache` for files.
- **Joint scoring substrate** at `file_retriever.rs::rerank_file` already accepts
  multi-feature configs (`RerankConfig`).

The infra is 70 % built. Missing: **wiki pages are not dense-indexed**, **wiki ↔
file lookup is not used by `retrieve_files`**, **graph multi-hop is shallow
(depth 1, only on file seeds)**, and **memory entries do not flow back as
context cards**.

### What 2024-2026 SOTA papers say

- **GraphRAG** (MSR 2024) — community summaries + KG + dense + LLM rerank. +70%
  on complex queries vs vanilla RAG.
- **HippoRAG** (Stanford 2024) — PageRank over KG seeded by dense. SOTA multi-hop.
- **LightRAG** (2024) — two-level (entity + concept) retrieval.
- **MemoRAG** (2024) — persistent memory bank coupled with retrieval.

This plan implements the same pattern adapted for code, reusing what we have.

## Objective

A `retrieve_files_blended()` entrypoint that, given a query, returns a top-K
file list joint-scored across file dense match + wiki page match + graph
proximity + authority tier + frecency + memory linkage, plus accompanying
memory cards and wiki summaries — measurably exceeding the cycle-11 best on the
30-query `theo-code` benchmark.

Specific goals:

1. Dense-index every wiki page on save / regen (warm cache available at retrieve).
2. Single-call wiki retrieval API: `wiki_search(query) → [(WikiPage, score, files)]`.
3. Joint scorer combining 7 signals with documented weights.
4. Graph multi-hop expansion (depth 2-3) seeded from wiki and dense anchors.
5. Memory ↔ file/wiki linkage layer; retrieval returns memory cards alongside
   files.
6. `retrieve_files` gains opt-in flag `RerankConfig::blend_mode`; default
   behaviour unchanged.
7. New benchmark `benchmark_blended_retrieve_mrr_guard` proves cycle-11 was
   beaten by ≥ 0.10 MRR on the same ground truth.

## ADRs

### D1 — Wiki page dense embedding lives in the wiki crate, not retrieval

**Decision:** Add `wiki/dense_index.rs` that embeds each `WikiDoc` and persists
vectors alongside the wiki manifest. The retrieval crate consumes this index but
does not own its lifecycle.

**Rationale:** Wiki regeneration is event-driven (file saves, manual `wiki
regen`). Embeddings must regenerate together with content to stay consistent.
Co-locating with the wiki module avoids stale-vector bugs and matches the
existing `WikiManifest` ownership model.

**Consequences:** Wiki module gains a hard dependency on the embedder trait.
Retrieval can mock the wiki dense index for tests.

### D2 — Joint score is a documented linear combination, not a learned ranker

**Decision:** The blended score is `α·file_dense + β·wiki_match + γ·graph_prox
+ δ·tier_weight + ε·frecency + ζ·memory_link + η·symbol_overlap` with
constants in `RerankConfig`, calibrated empirically against the benchmark.

**Rationale:** Learned rankers require training data we don't have for this
corpus. Linear blends with documented coefficients are debuggable, deterministic,
and reproducible. SOTA RAG systems (LangChain, LlamaIndex defaults) use the
same pattern.

**Consequences:** Re-tuning requires re-running the benchmark. Adding a new
signal is a single field on `RerankConfig`.

### D3 — Graph proximity uses existing PageRank-on-subgraph, not a new algo

**Decision:** Reuse `theo-engine-graph::cluster::hierarchical_cluster` for
community membership and a small PageRank seeded from anchor files for
proximity scoring. Cap walk depth at 3.

**Rationale:** We already pay the graph-build cost. Adding a new traversal
algorithm doubles maintenance. PageRank-from-seeds is the HippoRAG core idea
applied to our existing graph.

**Consequences:** Latency adds ~50 ms per query for top-K seed walks. Cap at 3
hops bounds blast radius.

### D4 — Memory linkage is bidirectional and lazy

**Decision:** Memory entries (`theo-infra-memory`) gain a `references:
Vec<SourceRef>` field. At retrieval time, files in the result set query memory
for entries pointing at them. Reverse lookup uses an in-memory inverted index
built lazily.

**Rationale:** Pre-joining memory ↔ files at write time tightly couples two
storage systems with different lifecycles. Lazy reverse lookup keeps writes
cheap and only pays cost at retrieval (where we already accept LLM-call latency).

**Consequences:** Retrieval is responsible for memory hydration. Memory
provider trait gains `entries_for_files(&[path])` query.

### D5 — `retrieve_files` blend is opt-in, default behaviour preserved

**Decision:** Add `RerankConfig::blend_mode: BlendMode` (default `BlendMode::Off`).
Production callers stay on the existing path until the blend benchmark beats
the BM25-only path on the in-tree guard.

**Rationale:** No regression risk for existing users. Allows benchmarking blend
vs baseline on the same code without flag flips. Mirrors the `dense-retrieval`
feature gate pattern that already exists.

**Consequences:** Two retrieval code paths to maintain until the blend wins
unconditionally and the old path can be deprecated.

### D6 — Wiki dense index and graph PageRank are computed once per session

**Decision:** Both indexes are built lazily on first call within a process and
cached. Re-build is triggered by graph mutation events (file save → graph
diff → invalidate affected wiki vectors / page entries).

**Rationale:** Cold cost is ~20 s for wiki embedding (94 pages × ~150 ms each)
+ ~5 s for PageRank. Amortised across an agent session it's negligible. Re-using
the existing `EmbeddingCache` write-back-on-disk model keeps warm starts fast.

**Consequences:** First retrieval after server start is slow. Cached `.theo/`
state must include wiki dense vectors.

## Dependency Graph

```
Phase 0 ──▶ Phase 1 ──▶ Phase 2 ──▶ Phase 5 ──▶ Phase 6
              │            │           │
              │            ▼           │
              │         Phase 3 ───────┘
              ▼
           Phase 4 (parallel with 2-3)
```

Phase 0 (foundation) blocks everything. Phase 1 (wiki dense retrieval) blocks
Phase 2 (joint score). Phase 3 (graph multi-hop) and Phase 4 (memory linkage)
parallelise with Phase 2 as long as their interfaces are agreed up front. Phase
5 (wire into `retrieve_files`) waits for 2/3/4. Phase 6 (benchmark + report) is
final.

---

## Phase 0: Foundation — Wiki Dense Index + Trait Surface

**Objective:** Establish the `WikiDenseIndex` primitive plus the `MemoryRetrieval`
trait so later phases have stable interfaces to code against.

### T0.1 — `WikiDenseIndex` data structure and persistence

#### Objective
Add a per-page vector store keyed by wiki page slug, persisted alongside
`wiki_manifest.json`, with the same staleness model as the wiki itself.

#### Evidence
Wiki has 94 pages on theo-code; each page summary + symbol catalog is ~500–2000
chars. Embedding all pages takes ~15 s with cached `NeuralEmbedder`. No current
storage exists for these vectors — `WikiManifest` only tracks page metadata.

#### Files to edit
```
crates/theo-engine-retrieval/src/wiki/dense_index.rs (NEW) — vector store + persistence
crates/theo-engine-retrieval/src/wiki/mod.rs — declare new module, re-export
crates/theo-engine-retrieval/src/wiki/persistence.rs — extend manifest read/write
crates/theo-engine-retrieval/src/wiki/model.rs — add `dense_vector_path` field
```

#### Deep file dependency analysis

- `wiki/mod.rs` — exposes wiki types. Adding `pub mod dense_index;` is the only
  change. Downstream: `lib.rs` re-exports unchanged.
- `wiki/persistence.rs` — currently reads/writes `wiki_manifest.json`. Extension
  reads/writes `dense.bin` adjacent to it. Format: bincode-encoded
  `HashMap<PageId, Vec<f32>>`. Backward compat: missing file → empty index;
  treated as cold start.
- `wiki/model.rs::WikiManifest` — adds `dense_index_path: Option<PathBuf>`
  for explicit pointer when wiki dir layout differs.
- `wiki/dense_index.rs` (new) — owns build-from-pages, save, load, get-vector,
  invalidate-by-page. Depends on `embedding::neural::NeuralEmbedder` trait.

#### Deep Dives

**Vector format**: f32 dense (D=768 for Jina v1; chosen so existing
`EmbeddingCache` patterns transfer).

**Persistence format**: bincode (already a workspace dep) of
`{ schema: u32, embedder_id: String, embedder_dim: u32, vectors: HashMap<String, Vec<f32>> }`.
Schema bump → cold rebuild.

**Embedding text**: per page, concatenate `frontmatter.summary +
"\n" + frontmatter.tags.join(" ") + "\n" + first 2000 chars of page body`.
Captures the human-authored bridge plus the symbol catalog without exceeding
typical embedder context.

**Invariants:**
- `vectors.len() == manifest.pages.len()` after `build_from_pages` succeeds.
- Vector dim equals `embedder.dim()` exactly.
- On persistence read mismatch (dim, embedder_id, schema), index is treated as
  cold and rebuilt.

**Edge cases:**
- Empty wiki dir → empty index, no error.
- Page with empty summary → embed `"path={slug}"` only (still gives a vector).
- Embedder failure on a single page → log, skip page, continue.

#### Tasks

1. Define `WikiDenseIndex { embedder_id: String, dim: u32, vectors: HashMap<String, Vec<f32>> }`.
2. `WikiDenseIndex::build_from_pages(&[WikiDoc], embedder: &impl Embedder) -> Self`.
3. `WikiDenseIndex::save(&self, path: &Path) -> Result<()>` (bincode).
4. `WikiDenseIndex::load(path: &Path) -> Result<Option<Self>>` (returns None on
   missing/incompatible).
5. `WikiDenseIndex::get(&self, page_id: &str) -> Option<&[f32]>`.
6. `WikiDenseIndex::invalidate(&mut self, page_id: &str)` — removes entry.
7. Re-export from `wiki/mod.rs`.

#### TDD

```
RED:     test_dense_index_build_from_empty_returns_empty()
RED:     test_dense_index_build_from_3_pages_has_3_vectors()
RED:     test_dense_index_save_load_round_trip_preserves_vectors()
RED:     test_dense_index_load_returns_none_on_dim_mismatch()
RED:     test_dense_index_load_returns_none_on_embedder_id_mismatch()
RED:     test_dense_index_invalidate_removes_only_target_entry()
RED:     test_dense_index_get_returns_none_for_unknown_id()
GREEN:   Implement WikiDenseIndex + serde derive (bincode).
REFACTOR: Extract per-page text builder into helper `embedding_text_for_page()`.
VERIFY:  cargo test -p theo-engine-retrieval wiki::dense_index
```

#### Acceptance Criteria

- [ ] `WikiDenseIndex::build_from_pages` produces vectors for every page with
      non-empty content.
- [ ] Round-trip save/load preserves all vectors bit-exact for f32.
- [ ] Load gracefully returns `None` (not `Err`) when persisted format mismatches
      current embedder.
- [ ] Pass: `/code-audit complexity` (cyclomatic ≤ 10 per fn).
- [ ] Pass: `/code-audit coverage` (≥ 90 % branch coverage on dense_index.rs).
- [ ] Pass: `/code-audit lint` (zero clippy warnings).
- [ ] Pass: `/code-audit size` (file ≤ 500 lines).

#### DoD
- [ ] All 7 RED tests turned GREEN.
- [ ] `cargo test -p theo-engine-retrieval` green.
- [ ] `cargo clippy -p theo-engine-retrieval -- -D warnings` zero warnings.
- [ ] No public API regression (existing wiki tests still pass).

---

### T0.2 — `MemoryRetrieval` trait — entries-by-files lookup

#### Objective
Add a query interface `entries_for_files(paths: &[String]) -> Vec<MemoryEntry>`
to `theo-infra-memory` so retrieval can hydrate memory cards by file path.

#### Evidence
Today `MemoryEngine` has `register`, `stats`, `providers` (read in cycles 6
exploration). There is no read-side join from files to memory entries. Memory
data lives at `crates/theo-infra-memory/src/wiki/runtime.rs::RuntimeInsight`
which already carries `affected_files: Vec<String>`. The lookup path simply
isn't exposed.

#### Files to edit
```
crates/theo-infra-memory/src/lib.rs — add public trait MemoryRetrieval
crates/theo-infra-memory/src/engine.rs — implement default impl over registered providers
crates/theo-infra-memory/src/wiki/runtime.rs — provider impl using affected_files index
```

#### Deep file dependency analysis

- `lib.rs` — defines `MemoryProvider` trait. Add `MemoryRetrieval` as a
  separate, smaller trait so non-retrieval consumers stay simple. Provider
  trait gains a default `as_retrieval(&self) -> Option<&dyn MemoryRetrieval>`
  returning `None`.
- `engine.rs::MemoryEngine` — gains `entries_for_files(paths)` that iterates
  registered providers, calls `as_retrieval()` on each, unions results, dedups.
- `wiki/runtime.rs` — implements `MemoryRetrieval` for the runtime-insight
  store. Builds a small lazy `HashMap<String, Vec<RuntimeInsight>>` keyed by
  `affected_files` entries on first lookup. Caches until next ingest.

#### Deep Dives

**Trait shape:**
```rust
pub trait MemoryRetrieval: Send + Sync {
    fn entries_for_files(&self, paths: &[String]) -> Vec<MemoryEntry>;
}
pub struct MemoryEntry {
    pub kind: &'static str,         // "insight", "promotion", "learning"
    pub source: String,             // provider id
    pub summary: String,            // <= 500 chars
    pub references: Vec<String>,    // file paths the entry points at
    pub timestamp: u64,
}
```

**Index strategy**: lazy build on first call, invalidated by `ingest_insight`.

**Edge cases:**
- Empty paths → empty result.
- Provider with no implementations → empty contribution.
- Duplicate entries across providers → dedup by `(provider_id, hash(summary))`.

#### Tasks
1. Define `MemoryRetrieval` trait + `MemoryEntry` struct.
2. Add default `as_retrieval()` to `MemoryProvider`.
3. `MemoryEngine::entries_for_files` aggregates over providers.
4. Implement `MemoryRetrieval` for runtime-insight store (`wiki/runtime.rs`).
5. Lazy reverse index built on first call, invalidated by `ingest_insight`.

#### TDD

```
RED:     test_memory_retrieval_empty_paths_returns_empty()
RED:     test_memory_retrieval_finds_insight_by_affected_file()
RED:     test_memory_retrieval_dedups_across_providers()
RED:     test_memory_retrieval_invalidates_after_ingest()
RED:     test_memory_provider_without_retrieval_contributes_nothing()
GREEN:   Implement trait + engine routing + runtime impl.
REFACTOR: Extract dedup helper.
VERIFY:  cargo test -p theo-infra-memory
```

#### Acceptance Criteria

- [ ] Trait is object-safe and `Send + Sync`.
- [ ] Empty path list returns empty vec without provider iteration.
- [ ] Reverse index rebuilds within 100 ms after `ingest_insight` on a 1000-entry
      store.
- [ ] Pass: `/code-audit` complexity / coverage / lint / size.

#### DoD
- [ ] All RED tests GREEN.
- [ ] `cargo test -p theo-infra-memory` green.
- [ ] `cargo clippy -p theo-infra-memory -- -D warnings` zero.
- [ ] `MemoryRetrieval` exposed in `theo-infra-memory::lib::*`.

---

## Phase 1: Wiki Retriever — Search + File Linkage

**Objective:** Single API call: `WikiRetriever::search(query) → Vec<WikiHit>`
where each `WikiHit` carries a wiki page id + match score + linked files.

### T1.1 — `WikiRetriever` dense + BM25 hybrid

#### Objective
Replace direct `lookup.rs` BM25 calls with a hybrid that also consults
`WikiDenseIndex`, fuses via RRF, applies authority tier weights.

#### Evidence
Existing `lookup.rs` is BM25-only. Failing queries `community detection
clustering algorithm` and `agent loop state machine transitions` would benefit
from the per-page summary semantic match (page summary likely has both
"community" and "Leiden" — dense match catches it where BM25 fails).

#### Files to edit
```
crates/theo-engine-retrieval/src/wiki/retriever.rs (NEW) — hybrid retriever
crates/theo-engine-retrieval/src/wiki/lookup.rs — extract BM25 helper for reuse
crates/theo-engine-retrieval/src/wiki/mod.rs — declare module
```

#### Deep file dependency analysis

- `lookup.rs` keeps existing top-level `WikiLookupResult` API; internally exposes
  `bm25_score_pages(query, &wiki) -> Vec<(PageId, f64)>` for the retriever to
  consume. No surface break.
- `retriever.rs` (new) takes `&Wiki`, `&WikiDenseIndex`, `&dyn Embedder`,
  produces `Vec<WikiHit>`. Owns the RRF fusion (k=60), tier weighting, file
  unrolling.
- `wiki/mod.rs` re-exports `WikiRetriever`, `WikiHit`.

#### Deep Dives

**`WikiHit` shape:**
```rust
pub struct WikiHit {
    pub page_id: String,
    pub title: String,
    pub summary: String,
    pub tier: AuthorityTier,
    pub score: f64,             // post-tier, post-RRF
    pub linked_files: Vec<String>,  // from page's SourceRefs
}
```

**Algorithm:**
1. Embed query once.
2. Compute BM25 scores for all pages (existing helper).
3. Compute cosine sim for all pages from `WikiDenseIndex`.
4. Sort each → ranks. RRF fuse: `score(p) = sum_i 1/(k + rank_i(p))` with k=60.
5. Multiply by `tier.weight()` (existing).
6. Top-K by score; for each, dereference `SourceRef`s into linked file paths.

**Invariants:**
- Every `WikiHit.linked_files` entry exists in the wiki page's `SourceRef`s.
- Score is monotone non-decreasing within tier (ties allowed).
- Stale pages get zero contribution (filtered out in `compute_final_score`
  pattern).

**Edge cases:**
- Wiki empty → empty result, no panic.
- Dense index missing → fall back to BM25 only with warning log.
- Query with no token overlap and no semantic neighbours → empty result.

#### Tasks
1. Create `WikiRetriever` struct holding refs to `Wiki`, `WikiDenseIndex`,
   embedder.
2. Add `search(query: &str, top_k: usize) -> Vec<WikiHit>` method.
3. Implement RRF fusion of BM25 + cosine sim.
4. Apply tier weighting after RRF.
5. Unroll `SourceRef`s into `linked_files` per hit.
6. Re-export `WikiHit`, `WikiRetriever` from `wiki::mod`.

#### TDD

```
RED:     test_wiki_retriever_empty_wiki_returns_empty()
RED:     test_wiki_retriever_bm25_match_finds_page()
RED:     test_wiki_retriever_dense_match_finds_page_without_lexical_overlap()
RED:     test_wiki_retriever_rrf_combines_bm25_and_dense_signals()
RED:     test_wiki_retriever_tier_weighting_demotes_cached_pages()
RED:     test_wiki_retriever_falls_back_to_bm25_only_when_dense_index_missing()
RED:     test_wiki_retriever_unrolls_source_refs_into_linked_files()
RED:     test_wiki_retriever_top_k_truncates_to_requested()
GREEN:   Implement hybrid retriever.
REFACTOR: Extract RRF fusion into shared helper if file_retriever can reuse.
VERIFY:  cargo test -p theo-engine-retrieval wiki::retriever
```

#### Acceptance Criteria
- [ ] Pure function: same inputs → same outputs.
- [ ] Latency ≤ 30 ms on 100-page wiki with cached embedder (microbench).
- [ ] Pass: `/code-audit` complexity (≤ 10 per fn) / coverage (≥ 90 %) / lint /
      size.
- [ ] Backward compat: existing `lookup.rs` callers unchanged.

#### DoD
- [ ] All 8 RED tests GREEN.
- [ ] `cargo test -p theo-engine-retrieval` green.
- [ ] Zero clippy warnings.
- [ ] Wiki module documentation updated to reference the new retriever.

---

## Phase 2: Joint Score Blend

**Objective:** Compose seven signals into a single ranking score for files,
with documented weights and a reproducible benchmark.

### T2.1 — `BlendScoreConfig` + `score_file_blended()`

#### Objective
Pure scoring function that combines file dense match, wiki match, graph
proximity, tier, frecency, memory link count, and symbol overlap into a single
f64.

#### Evidence
`file_retriever.rs::rerank_file` already accepts a multi-feature config. Adding
three more signals (wiki, frecency, memory) follows the same pattern, doesn't
break existing tests.

#### Files to edit
```
crates/theo-engine-retrieval/src/file_retriever.rs — add BlendScoreConfig + scorer
crates/theo-engine-retrieval/src/file_retriever_tests.rs — TDD tests
```

#### Deep file dependency analysis

- `file_retriever.rs` — existing `RerankConfig` keeps its weights. New
  `BlendScoreConfig` has 7 weights. New function `score_file_blended(file, ctx,
  &BlendScoreConfig)` is purely additive. `RerankConfig` gains `pub blend:
  Option<BlendScoreConfig>` so callers opt in.
- `file_retriever_tests.rs` — adds blend-specific tests that verify each weight
  isolates correctly (set 6 to 0, vary 1, observe linear response).

#### Deep Dives

**Score function:**
```rust
pub struct BlendScoreContext<'a> {
    pub file_dense_sim: f64,
    pub wiki_match_score: f64,           // max over wiki pages linking this file
    pub graph_proximity: f64,            // 1.0 / (1 + hops_from_anchor)
    pub authority_tier_weight: f64,      // from wiki page tier (0 if no page)
    pub frecency: f64,                   // normalized 0..1
    pub memory_link_count: u32,
    pub symbol_overlap: f64,             // jaccard(query_tokens, file_symbols)
}

pub fn score_file_blended(ctx: &BlendScoreContext, cfg: &BlendScoreConfig) -> f64 {
    cfg.alpha * ctx.file_dense_sim
        + cfg.beta * ctx.wiki_match_score
        + cfg.gamma * ctx.graph_proximity
        + cfg.delta * ctx.authority_tier_weight
        + cfg.epsilon * ctx.frecency
        + cfg.zeta * (ctx.memory_link_count as f64).ln_1p()
        + cfg.eta * ctx.symbol_overlap
}
```

**Default weights** (calibrated against benchmark in T6.2):
- α (file dense) = 0.30
- β (wiki match) = 0.40
- γ (graph prox) = 0.10
- δ (tier) = 0.10
- ε (frecency) = 0.05
- ζ (memory) = 0.03
- η (symbol overlap) = 0.02

Sum = 1.0 by convention; not enforced (caller can use any non-negative reals).

**Invariants:**
- All weights ≥ 0.
- All input signals ≥ 0.
- `ln_1p` on memory count keeps unbounded counts from dominating.

**Edge cases:**
- All-zero context → score = 0, no panic.
- Single-signal weighting (others 0) → linear response in that signal alone.

#### Tasks
1. Define `BlendScoreContext`, `BlendScoreConfig` (with `Default`).
2. Implement `score_file_blended()`.
3. Add `RerankConfig::blend: Option<BlendScoreConfig>` (default None).
4. Update `rerank_file` to call blended scorer when `blend.is_some()`.

#### TDD

```
RED:     test_blend_score_zero_context_is_zero()
RED:     test_blend_score_only_alpha_active_is_linear_in_dense()
RED:     test_blend_score_only_beta_active_is_linear_in_wiki()
RED:     test_blend_score_memory_count_uses_ln1p_not_linear()
RED:     test_blend_score_default_weights_sum_to_one_within_epsilon()
RED:     test_rerank_file_uses_blended_scorer_when_config_set()
RED:     test_rerank_file_uses_legacy_scorer_when_blend_is_none()
GREEN:   Implement scorer + RerankConfig wiring.
REFACTOR: None expected — keep linear blend transparent.
VERIFY:  cargo test -p theo-engine-retrieval file_retriever
```

#### Acceptance Criteria

- [ ] All 7 signals can be isolated by zeroing the others.
- [ ] Zero context → zero score.
- [ ] `RerankConfig::default().blend == None` (backward compat).
- [ ] Pass: `/code-audit complexity` ≤ 10.
- [ ] Pass: `/code-audit coverage` ≥ 95 % on the new code.

#### DoD
- [ ] All RED tests GREEN.
- [ ] Existing 902+ `theo-engine-retrieval` tests still green.
- [ ] Zero clippy warnings.

---

## Phase 3: Graph Multi-Hop Proximity

**Objective:** Compute `graph_proximity(file, anchor_set)` so files reachable
via 2-3 hops from semantic anchors get bonus, addressing cross-cutting queries.

### T3.1 — `graph_proximity_from_seeds`

#### Objective
Add a function that, given a set of anchor file ids and the code graph, returns
a normalized proximity score per file.

#### Evidence
Cross-cutting query `domain types shared across crates traits` fails because
the answer files (`lib.rs` in multiple crates) have no direct lexical match,
but they ARE 2 hops from `theo-domain` symbols via Imports edges. Today's
`expand_from_files` does only depth-1 expansion and doesn't compute scores.

#### Files to edit
```
crates/theo-engine-retrieval/src/graph_attention.rs — add seeded PageRank-style scorer
crates/theo-engine-retrieval/src/file_retriever.rs — call it during blend
```

#### Deep file dependency analysis

- `graph_attention.rs` — already has `propagate_attention` (cycle 7 inspection).
  Add `proximity_from_seeds(graph: &CodeGraph, seeds: &HashSet<String>, depth:
  usize) -> HashMap<String, f64>`. Reuses BFS + decay. No public API break.
- `file_retriever.rs` — when `blend.is_some()`, after dense + wiki retrieval,
  derive the anchor set as the union of (top-5 dense files) + (linked_files of
  top-3 wiki hits), call `proximity_from_seeds`, populate `BlendScoreContext`.

#### Deep Dives

**Algorithm:**
1. BFS from each seed simultaneously (multi-source).
2. Per file, record `min_distance` from any seed.
3. Score = `decay^min_distance` with `decay = 0.5`.
4. Cap at `max_depth = 3`.
5. Files unreachable from any seed → score = 0.

**Invariants:**
- Seeds always score 1.0.
- Score is monotonically non-increasing in distance.
- Bounded depth keeps complexity O(seeds × depth × avg_neighbours).

**Edge cases:**
- Empty seeds → all-zero result.
- Disconnected seed → 0 for everything not in its connected component.
- Self-loop in graph → ignored.

#### Tasks
1. Implement `proximity_from_seeds(graph, seeds, depth) -> HashMap<...>`.
2. Add config `BlendScoreConfig::graph_decay`, `graph_max_depth`.
3. Wire into `file_retriever.rs` blend path.

#### TDD

```
RED:     test_proximity_seed_scores_1()
RED:     test_proximity_neighbour_scores_decay()
RED:     test_proximity_2hop_scores_decay_squared()
RED:     test_proximity_unreachable_scores_zero()
RED:     test_proximity_capped_at_max_depth()
RED:     test_proximity_multi_seed_uses_minimum_distance()
GREEN:   Implement BFS + decay.
REFACTOR: Extract neighbour iterator if reuse with cluster code.
VERIFY:  cargo test -p theo-engine-retrieval graph_attention
```

#### Acceptance Criteria
- [ ] Latency ≤ 50 ms on 16k-node graph with 5 seeds and depth 3.
- [ ] Memory bounded (no full transitive closure).
- [ ] Pass: `/code-audit complexity` / coverage / lint / size.

#### DoD
- [ ] All RED tests GREEN.
- [ ] `cargo test -p theo-engine-retrieval graph_attention` green.
- [ ] Benchmark microtest measures and asserts ≤ 50 ms.

---

## Phase 4: Memory Linkage Layer

**Objective:** When retrieval returns a file list, hydrate it with memory
entries pointing at those files, returned alongside (not as ranking signal —
that's Phase 2).

### T4.1 — `FileRetrievalResult.memory_cards`

#### Objective
Extend `FileRetrievalResult` with `memory_cards: Vec<MemoryEntry>`, populated
when blend mode is on.

#### Evidence
Cycle 6 already added `query_type: QueryType` as additive telemetry on the
result struct without breaking anyone. Same shape applies to memory cards.

#### Files to edit
```
crates/theo-engine-retrieval/src/file_retriever.rs — add field, populate when blend on
crates/theo-engine-retrieval/Cargo.toml — add theo-infra-memory dep (feature-gated)
```

#### Deep file dependency analysis

- `file_retriever.rs::FileRetrievalResult` — gains `pub memory_cards:
  Vec<MemoryEntry>` (default empty). Compatible with existing `Default` derive.
- `Cargo.toml` — add `theo-infra-memory = { workspace = true, optional = true }`
  + `feature = ["memory-blend"]` so the dep is purely opt-in. ADR-011 already
  permits this dependency direction (retrieval → infra-memory is documented
  feature-gated path).
- `retrieve_files_blended` (Phase 5) — when blend on, calls
  `memory_engine.entries_for_files(retrieved_paths)` and stores result.

#### Deep Dives

**Provider lookup:**
```rust
let memory_cards = if cfg.blend.is_some() {
    memory_engine
        .as_ref()
        .map(|me| me.entries_for_files(&primary_paths))
        .unwrap_or_default()
} else {
    Vec::new()
};
```

**Invariants:**
- Memory hydration never affects file ranking.
- Empty memory engine → empty cards, no panic.
- Cards capped at 20 to bound context size.

**Edge cases:**
- No memory engine wired → empty.
- Provider error (disk read fail) → log + empty (graceful).
- Duplicate cards across providers → dedup by `(provider_id, summary_hash)`.

#### Tasks
1. Add `memory_cards: Vec<MemoryEntry>` field.
2. Add feature flag `memory-blend` in `Cargo.toml`.
3. Populate in blend path; cap at 20.
4. Test that non-blend path leaves cards empty.

#### TDD

```
RED:     test_retrieve_blended_returns_memory_cards_for_retrieved_files()
RED:     test_retrieve_legacy_path_leaves_memory_cards_empty()
RED:     test_retrieve_blended_caps_memory_cards_at_20()
RED:     test_retrieve_blended_no_memory_engine_yields_empty_cards()
GREEN:   Implement hydration in retrieve_files_blended.
REFACTOR: None.
VERIFY:  cargo test -p theo-engine-retrieval --features memory-blend
```

#### Acceptance Criteria
- [ ] Default-derived `FileRetrievalResult::default().memory_cards.is_empty()`.
- [ ] Cap of 20 enforced.
- [ ] Feature-gated build still green: `cargo build -p theo-engine-retrieval`
      (no feature) and `cargo build -p theo-engine-retrieval --features
      memory-blend`.
- [ ] Pass: `/code-audit` checks.

#### DoD
- [ ] All RED tests GREEN.
- [ ] Workspace build green with and without `memory-blend` feature.

---

## Phase 5: `retrieve_files_blended` Production Entry

**Objective:** A single new function that runs the full pipeline opt-in via
`RerankConfig::blend_mode`, leaving the legacy path untouched.

### T5.1 — `retrieve_files_blended()`

#### Objective
End-to-end blend retrieval: file dense + wiki dense + RRF + graph hop scoring +
joint scoring + Codex rerank (existing) + memory hydration.

#### Evidence
Cycle 11 proved Dense+RRF + Codex rerank gives 0.760 MRR. Cycle 14 proved
that adding lexical query rewriting doesn't help. Phases 1-4 establish
the missing pieces (wiki dense, joint scoring, graph hop, memory).

#### Files to edit
```
crates/theo-engine-retrieval/src/file_retriever.rs — add entry point
crates/theo-engine-retrieval/src/lib.rs — re-export
```

#### Deep file dependency analysis

- `file_retriever.rs` adds:
  ```rust
  pub fn retrieve_files_blended(
      graph: &CodeGraph,
      communities: &[Community],
      wiki: &Wiki,
      wiki_dense: &WikiDenseIndex,
      tantivy_index: &FileTantivyIndex,
      embedder: &NeuralEmbedder,
      cache: &EmbeddingCache,
      memory_engine: Option<&MemoryEngine>,
      query: &str,
      config: &RerankConfig,
      seen: &HashSet<String>,
  ) -> FileRetrievalResult
  ```

- `lib.rs` re-exports `retrieve_files_blended`.
- Existing `retrieve_files` is unchanged. New function is additive.

#### Deep Dives

**Pipeline:**
1. `WikiRetriever::search(query, 20)` → Vec<WikiHit>.
2. `hybrid_rrf_search` (existing) → file_scores.
3. Build anchor set from top-5 file_scores ∪ top-3 wiki linked_files.
4. `proximity_from_seeds(graph, anchors, 3)` → graph_prox per file.
5. For each unique file in pool: build `BlendScoreContext`, score with
   `score_file_blended`.
6. Sort, take top-50.
7. Existing harm_filter on top-50 (preserves cycle 1-2 fixes).
8. Optional Codex rerank if `config.use_external_reranker == true`.
9. Hydrate memory cards.
10. Return `FileRetrievalResult` with `query_type` populated (cycle 6 telemetry
    preserved).

**Invariants:**
- Top-K sort stable.
- harm_removals counter still incremented.
- query_type still classified.

**Edge cases:**
- Empty wiki → falls back to file-dense + graph hop only.
- No embedder → graceful degradation to existing `retrieve_files`.
- Empty memory engine → empty cards.

#### Tasks
1. Implement `retrieve_files_blended` end-to-end.
2. Wire all phase 1-4 components.
3. Preserve harm_filter + query_type telemetry.
4. Re-export from `lib.rs`.

#### TDD

```
RED:     test_retrieve_blended_uses_wiki_signal_when_enabled()
RED:     test_retrieve_blended_falls_back_when_wiki_empty()
RED:     test_retrieve_blended_preserves_harm_removals_counter()
RED:     test_retrieve_blended_preserves_query_type_telemetry()
RED:     test_retrieve_blended_hydrates_memory_cards_for_top_files()
RED:     test_retrieve_blended_legacy_path_untouched()
GREEN:   Compose phases 1-4 into single function.
REFACTOR: Extract anchor-set builder helper if reused by Phase 6 benchmark.
VERIFY:  cargo test -p theo-engine-retrieval file_retriever
```

#### Acceptance Criteria
- [ ] Existing `retrieve_files` behaviour byte-identical when called with
      legacy config.
- [ ] Latency on theo-code corpus (973 files) ≤ 1.5 s end-to-end (no LLM
      rerank yet — that's external).
- [ ] Pass: `/code-audit complexity` ≤ 10 per fn (extract sub-functions if
      needed).
- [ ] Pass: `/code-audit coverage` ≥ 90 %.

#### DoD
- [ ] All RED tests GREEN.
- [ ] Workspace build with both `dense-retrieval` and `memory-blend` features
      green.
- [ ] Zero clippy warnings.

---

## Phase 6: Benchmark + Calibration

**Objective:** Produce reproducible empirical evidence the blend exceeds
cycle-11 best on the same ground truth, calibrate weights, and produce a
final report.

### T6.1 — `benchmark_blended_retrieve_mrr_guard`

#### Objective
Add an `#[ignore]` Rust benchmark that exercises `retrieve_files_blended` over
the existing `theo-code.json` ground truth, asserts MRR ≥ 0.80 (cycle-11 + 0.04
margin) on the in-tree pipeline.

#### Evidence
The cycle-11 measured MRR 0.760 is our best-known on this corpus with current
hardware. A floor of 0.80 represents a real, evidence-grounded improvement (not
SOTA aspiration). Floor 0.85+ requires LLM rerank stage which lives outside
the in-tree benchmark.

#### Files to edit
```
crates/theo-engine-retrieval/tests/benchmark_suite.rs — add benchmark
```

#### Deep file dependency analysis

- `tests/benchmark_suite.rs` — adds `benchmark_blended_retrieve_mrr_guard` and
  `benchmark_dump_blended_candidates` (parallel of existing dumpers).
  Uses same ground-truth loader pattern as existing tests.

#### Deep Dives

**Test shape:** mirror of `benchmark_retrieve_files_mrr_guard` but calling
`retrieve_files_blended` and asserting `mrr_overall >= 0.80`.

**Calibration step**: a separate `#[ignore]` test
`benchmark_blended_grid_search` that sweeps weight tuples and prints the best.
Used during T6.2.

**Invariants:**
- Test marked `#[ignore]` (not in default `cargo test`).
- Writes aggregates to `/tmp/probe-blended.metrics.txt` (avoids tail-truncation
  issue from cycle 7).

#### Tasks
1. Add `benchmark_blended_retrieve_mrr_guard` (assertion test).
2. Add `benchmark_dump_blended_candidates` (dumper for downstream LLM rerank
   measurement, mirroring cycle 11 pattern).
3. Add `benchmark_blended_grid_search` (calibration helper, no assertion).

#### TDD

```
RED:     benchmark_blended_retrieve_mrr_guard fails until Phases 1-5 land.
GREEN:   Phases 1-5 complete; rerun benchmark; passes 0.80 floor.
REFACTOR: None.
VERIFY:  cargo test -p theo-engine-retrieval --features dense-retrieval --test benchmark_suite -- --ignored benchmark_blended_retrieve_mrr_guard
```

#### Acceptance Criteria
- [ ] Aggregate metrics written to `/tmp/probe-blended.metrics.txt`.
- [ ] MRR ≥ 0.80 on cycle-11 ground truth.
- [ ] Reproducible: 3 runs within 0.02 absolute MRR (no flake).

#### DoD
- [ ] Benchmark green with the chosen weight defaults.
- [ ] Calibration grid produces a documented winner.

---

### T6.2 — Calibrate weights against benchmark

#### Objective
Run the grid search, document the winning weight tuple in a comment block on
`BlendScoreConfig::default()`.

#### Evidence
Default linear weights from D2 are educated guesses. Grid search confirms
optimal weights against the ground truth. Per HippoRAG paper, optimal weights
shift from corpus to corpus — documenting our values is key.

#### Files to edit
```
crates/theo-engine-retrieval/src/file_retriever.rs — update default weights
docs/plans/wiki-graph-memory-blend-retrieval-plan.md — record winning tuple
```

#### Deep file dependency analysis

- `file_retriever.rs::BlendScoreConfig::default` — body changes only. No type
  change.
- This plan file — appended note in Phase 6.

#### Tasks
1. Run `benchmark_blended_grid_search` over 64 weight tuples (small grid).
2. Record winning tuple + MRR.
3. Update `BlendScoreConfig::default` to winning tuple.
4. Add comment in source pointing at this plan.

#### TDD

```
RED:     benchmark_blended_retrieve_mrr_guard pre-calibration MRR < 0.80
GREEN:   After calibration MRR ≥ 0.80
REFACTOR: None.
VERIFY:  cargo test ... benchmark_blended_retrieve_mrr_guard
```

#### Acceptance Criteria
- [ ] Calibration script reproducible from this plan.
- [ ] Winning tuple documented in source AND in this plan.
- [ ] No magic numbers without justification.

#### DoD
- [ ] Default weights match measured optimum within ±0.05.
- [ ] Plan updated with winning tuple, run date, MRR.

---

### T6.3 — End-to-end LLM rerank measurement (extension of cycle 11/14)

#### Objective
Use existing `apps/theo-benchmark/scripts/probe_llm_rerank.py` against the
blended candidate dump to measure the FULL pipeline (blend + Codex rerank).

#### Evidence
Cycle 11 measured Dense+RRF + Codex = 0.760 MRR. Cycle 11 produced
`probe-rrf-candidates.json`. The blended dumper produces an analogous file.
Reusing the Python harness gives us cycle-11-comparable numbers with the
blended candidates.

#### Files to edit
```
apps/theo-benchmark/scripts/probe_llm_rerank.py — already accepts
input/output paths via argv (cycle 11)
sota-output/analysis/cycle-blend-final.md (NEW) — final report
```

#### Deep file dependency analysis

- `probe_llm_rerank.py` is already parametric (cycle 11). No code change.
  Just call:
  `python3 probe_llm_rerank.py /tmp/probe-blended.json /tmp/probe-llm-blended.metrics.txt`.
- `sota-output/analysis/cycle-blend-final.md` — final cycle report following
  the existing `gap-iteration-N.md` template.

#### Tasks
1. Run blended dumper.
2. Run Python LLM rerank probe pointing at the blend dump.
3. Capture aggregates.
4. Write final cycle report to `sota-output/analysis/cycle-blend-final.md`.
5. Update `sota-output/report/sota-validation-report.md`.

#### TDD
N/A (measurement-only step). Acceptance is purely numerical comparison.

#### Acceptance Criteria
- [ ] End-to-end MRR > 0.85 (cycle-11 best 0.760 + 0.10 lift target).
- [ ] R@5 > 0.65.
- [ ] Per-query results show ≥ 3 of the 5 cycle-11 failing queries now
      passing (MRR > 0.5).
- [ ] Final report written and committed.

#### DoD
- [ ] Final report in `sota-output/analysis/`.
- [ ] Report cites measured numbers, run date, command line.

---

## Coverage Matrix

| # | Gap / Requirement | Task(s) | Resolution |
|---|---|---|---|
| 1 | Wiki pages not dense-indexed | T0.1 | New `WikiDenseIndex` data structure + persistence |
| 2 | No file→memory entry lookup | T0.2 | `MemoryRetrieval` trait + impl on runtime store |
| 3 | Wiki retrieval ignores semantics | T1.1 | Hybrid BM25 + dense + RRF + tier weighting |
| 4 | No joint score across signals | T2.1 | `score_file_blended` linear combination |
| 5 | Graph expansion only depth-1 | T3.1 | `proximity_from_seeds` BFS with decay, depth 3 |
| 6 | Memory invisible at retrieval | T4.1 | `FileRetrievalResult.memory_cards` hydration |
| 7 | No production blend entry point | T5.1 | `retrieve_files_blended()` opt-in |
| 8 | No proof blend beats cycle-11 best | T6.1 | `benchmark_blended_retrieve_mrr_guard` |
| 9 | Default weights need empirical justification | T6.2 | Grid search + documented winners |
| 10 | End-to-end LLM rerank measurement | T6.3 | Reuse cycle-11 Python probe with blend dump |
| 11 | Cycle 11/14 evidence not in audit trail | T6.3 | Final report in `sota-output/` |

**Coverage: 11/11 gaps covered (100 %)**

## Global Definition of Done

- [ ] All 6 phases completed and verified.
- [ ] All RED tests in T0.1–T5.1 turned GREEN.
- [ ] T6.1 benchmark passes: blended MRR ≥ 0.80 in-tree.
- [ ] T6.3 end-to-end MRR ≥ 0.85 with LLM rerank, R@5 ≥ 0.65.
- [ ] `cargo test --workspace --exclude theo-code-desktop` green.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` zero warnings.
- [ ] `make check-arch` 0 violations (D1 ADR-011 wiki ↔ embedder dep is
      explicitly allowed).
- [ ] `make check-sizes` 0 NEW / 0 EXPIRED.
- [ ] `make check-secrets` clean.
- [ ] Workspace build with feature combinations: default,
      `dense-retrieval`, `dense-retrieval,memory-blend`.
- [ ] `sota-output/analysis/cycle-blend-final.md` written, citing measured
      numbers and reproducible commands.
- [ ] `sota-thresholds.toml` `current` values updated to the post-blend
      measurements (no floor changes — bypass remains forbidden).
- [ ] Default `RerankConfig::blend == None` (legacy callers untouched).
- [ ] No public API removed; only added.
- [ ] Backward compat: cycle-11 `benchmark_retrieve_files_mrr_guard` unchanged
      and still in suite.
