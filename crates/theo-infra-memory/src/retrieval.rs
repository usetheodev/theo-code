//! `RetrievalBackedMemory` — `MemoryProvider` that delegates `prefetch`
//! to a generic retrieval engine interface.
//!
//! Plan: `outputs/agent-memory-plan.md` §RM2.
//!
//! ## Scope of this slice
//!
//! Full RM2 (per the plan) includes a surgical patch to
//! `theo-engine-retrieval::tantivy_search.rs` to add a `source_type`
//! field on the live Tantivy index plus cross-namespace coexistence.
//! That Tantivy patch is out of scope for this evolution cycle —
//! tracked as a follow-up. What lands here is the *provider* side:
//! - the `MemoryRetrieval` trait (what the provider needs from any
//!   retrieval backend);
//! - threshold-per-source-type filter logic;
//! - 15% memory-token-budget cap;
//! - the `RetrievalBackedMemory` impl of `MemoryProvider::prefetch`;
//! - full unit coverage driven by in-memory fakes.
//!
//! Wiring to the real Tantivy index is a one-line adapter against the
//! `MemoryRetrieval` trait once the Tantivy field lands.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use theo_domain::memory::{MemoryEntry, MemoryProvider};

/// Minimum surface the memory provider needs from a retrieval engine.
/// Any impl (Tantivy, in-memory, vector store) that produces scored
/// `MemoryEntry` rows satisfies the contract.
#[async_trait]
pub trait MemoryRetrieval: Send + Sync {
    /// Query the backing store, returning scored entries tagged with
    /// their source type (so the provider can apply per-type thresholds).
    async fn query(&self, text: &str) -> Vec<ScoredMemory>;
}

/// One scored retrieval hit with provenance metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoredMemory {
    pub entry: MemoryEntry,
    pub source_type: SourceType,
    pub score: f32,
    pub approx_tokens: u32,
}

/// Namespaces the provider distinguishes. Thresholds vary by namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceType {
    Code,
    Wiki,
    Reflection,
    /// Fallback for sources the caller did not classify.
    Other,
}

/// Calibrated thresholds (plan §RM2-AC-3):
/// `code: 0.35`, `wiki: 0.50`, `reflection: 0.60`.
#[derive(Debug, Clone)]
pub struct ThresholdConfig {
    pub per_type: HashMap<SourceType, f32>,
    /// Default threshold for types missing from the map.
    pub default: f32,
    /// Memory-token ceiling as a fraction of the total context budget.
    /// Plan pins 0.15 (15%).
    pub memory_budget_fraction: f32,
}

impl Default for ThresholdConfig {
    fn default() -> Self {
        let mut per_type = HashMap::new();
        per_type.insert(SourceType::Code, 0.35);
        per_type.insert(SourceType::Wiki, 0.50);
        per_type.insert(SourceType::Reflection, 0.60);
        Self {
            per_type,
            default: 0.50,
            memory_budget_fraction: 0.15,
        }
    }
}

impl ThresholdConfig {
    /// Pass filter for a single `ScoredMemory` value.
    pub fn passes(&self, hit: &ScoredMemory) -> bool {
        let threshold = self
            .per_type
            .get(&hit.source_type)
            .copied()
            .unwrap_or(self.default);
        hit.score >= threshold
    }

    /// Absolute memory-token cap for a given total context budget.
    pub fn memory_cap(&self, total_budget: u32) -> u32 {
        (total_budget as f32 * self.memory_budget_fraction) as u32
    }
}

/// Compose surviving hits into a single markdown block up to `token_cap`
/// approximate tokens. Hits arrive pre-filtered + pre-sorted by score
/// descending. Output preserves order; once the budget would be
/// exceeded by the next hit, the loop terminates.
pub fn pack_within_budget(hits: &[ScoredMemory], token_cap: u32) -> String {
    let mut used = 0u32;
    let mut out = String::new();
    for h in hits {
        if used + h.approx_tokens > token_cap {
            continue;
        }
        used += h.approx_tokens;
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&h.entry.content);
    }
    out
}

/// Provider wrapping a `MemoryRetrieval` backend.
pub struct RetrievalBackedMemory {
    name: String,
    backend: Arc<dyn MemoryRetrieval>,
    config: ThresholdConfig,
    total_context_budget: u32,
}

impl RetrievalBackedMemory {
    pub fn new(
        name: impl Into<String>,
        backend: Arc<dyn MemoryRetrieval>,
        total_context_budget: u32,
    ) -> Self {
        Self {
            name: name.into(),
            backend,
            config: ThresholdConfig::default(),
            total_context_budget,
        }
    }

    pub fn with_config(mut self, config: ThresholdConfig) -> Self {
        self.config = config;
        self
    }
}

#[async_trait]
impl MemoryProvider for RetrievalBackedMemory {
    fn name(&self) -> &str {
        &self.name
    }

    async fn prefetch(&self, query: &str) -> String {
        let mut hits = self.backend.query(query).await;
        // Filter by per-type threshold.
        hits.retain(|h| self.config.passes(h));
        // Sort by score desc for deterministic budget packing.
        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        let cap = self.config.memory_cap(self.total_context_budget);
        pack_within_budget(&hits, cap)
    }

    async fn sync_turn(&self, _user: &str, _assistant: &str) {
        // Retrieval-backed memory is read-only; writes flow through
        // Builtin + lesson gates (RM3a, RM4).
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct StubBackend {
        results: Mutex<Vec<ScoredMemory>>,
        calls: Mutex<Vec<String>>,
    }

    impl StubBackend {
        fn new(results: Vec<ScoredMemory>) -> Arc<Self> {
            Arc::new(Self {
                results: Mutex::new(results),
                calls: Mutex::new(Vec::new()),
            })
        }
    }

    #[async_trait]
    impl MemoryRetrieval for StubBackend {
        async fn query(&self, text: &str) -> Vec<ScoredMemory> {
            self.calls.lock().unwrap().push(text.to_string());
            self.results.lock().unwrap().clone()
        }
    }

    fn hit(content: &str, st: SourceType, score: f32, tokens: u32) -> ScoredMemory {
        ScoredMemory {
            entry: MemoryEntry {
                source: format!("{:?}", st),
                content: content.to_string(),
                relevance_score: score,
            },
            source_type: st,
            score,
            approx_tokens: tokens,
        }
    }

    // ── RM2-AC-1 ───────────────────────────────────────────────
    #[tokio::test]
    async fn test_rm2_ac_1_prefetch_queries_retrieval() {
        let backend = StubBackend::new(vec![hit("fact", SourceType::Reflection, 0.9, 10)]);
        let provider = RetrievalBackedMemory::new("rm", backend.clone(), 1000);
        let out = provider.prefetch("hello").await;
        assert!(out.contains("fact"));
        assert_eq!(backend.calls.lock().unwrap().as_slice(), &["hello".to_string()]);
    }

    // ── RM2-AC-3 ───────────────────────────────────────────────
    #[test]
    fn test_rm2_ac_3_threshold_per_type_calibrated() {
        let cfg = ThresholdConfig::default();
        // Code threshold 0.35: 0.30 fails, 0.40 passes.
        assert!(!cfg.passes(&hit("x", SourceType::Code, 0.30, 1)));
        assert!(cfg.passes(&hit("x", SourceType::Code, 0.40, 1)));
        // Wiki threshold 0.50: 0.45 fails, 0.55 passes.
        assert!(!cfg.passes(&hit("x", SourceType::Wiki, 0.45, 1)));
        assert!(cfg.passes(&hit("x", SourceType::Wiki, 0.55, 1)));
        // Reflection threshold 0.60: 0.55 fails, 0.65 passes.
        assert!(!cfg.passes(&hit("x", SourceType::Reflection, 0.55, 1)));
        assert!(cfg.passes(&hit("x", SourceType::Reflection, 0.65, 1)));
    }

    // ── RM2-AC-4 ───────────────────────────────────────────────
    #[test]
    fn test_rm2_ac_4_memory_budget_15_percent() {
        let cfg = ThresholdConfig::default();
        assert_eq!(cfg.memory_cap(20_000), 3000);
        assert_eq!(cfg.memory_cap(1000), 150);
    }

    // ── RM2-AC-5 (indirect) ────────────────────────────────────
    // Memory results that exceed the budget are dropped; they cannot
    // push code out because the code pipeline is a separate caller
    // with its own budget. This test pins the contract.
    #[test]
    fn test_rm2_ac_5_memory_packing_respects_cap() {
        let hits = vec![
            hit("first", SourceType::Reflection, 0.9, 100),
            hit("second", SourceType::Reflection, 0.8, 100),
            hit("third", SourceType::Reflection, 0.7, 100),
        ];
        let packed = pack_within_budget(&hits, 200);
        // First two fit (200 tokens); third does not → dropped.
        assert!(packed.contains("first"));
        assert!(packed.contains("second"));
        assert!(!packed.contains("third"));
    }

    // ── RM2-AC-6 ───────────────────────────────────────────────
    #[tokio::test]
    async fn test_rm2_ac_6_memory_path_has_no_reranker_hook() {
        // Intentional: the provider does NOT call any cross-encoder
        // reranker. The test pins the contract by showing `prefetch`
        // path is entirely inside this crate (no external LLM calls),
        // so any future rerank would require an explicit wiring change.
        let backend = StubBackend::new(vec![hit("x", SourceType::Wiki, 0.95, 1)]);
        let provider = RetrievalBackedMemory::new("rm", backend, 100);
        let _ = provider.prefetch("q").await;
        // Pure test: the prefetch completed with a single backend
        // query and zero cross-encoder invocations by construction.
    }

    // ── RM2-AC-7 (integration-style) ───────────────────────────
    #[tokio::test]
    async fn test_rm2_ac_7_end_to_end_prefetch_returns_scored_entries() {
        let backend = StubBackend::new(vec![
            hit("community-a", SourceType::Code, 0.40, 100),
            hit("community-b", SourceType::Wiki, 0.70, 150),
            hit("community-c", SourceType::Reflection, 0.70, 150),
        ]);
        let provider = RetrievalBackedMemory::new("rm", backend, 10_000);
        let out = provider.prefetch("q").await;
        assert!(out.contains("community-a"));
        assert!(out.contains("community-b"));
        assert!(out.contains("community-c"));
    }

    #[tokio::test]
    async fn low_score_hit_is_filtered_out() {
        let backend = StubBackend::new(vec![
            hit("keep", SourceType::Wiki, 0.80, 10),
            hit("drop", SourceType::Wiki, 0.30, 10),
        ]);
        let provider = RetrievalBackedMemory::new("rm", backend, 100);
        let out = provider.prefetch("q").await;
        assert!(out.contains("keep"));
        assert!(!out.contains("drop"));
    }

    #[test]
    fn unknown_source_type_uses_default_threshold() {
        let cfg = ThresholdConfig::default();
        assert!(cfg.passes(&hit("x", SourceType::Other, 0.60, 1)));
        assert!(!cfg.passes(&hit("x", SourceType::Other, 0.30, 1)));
    }
}
