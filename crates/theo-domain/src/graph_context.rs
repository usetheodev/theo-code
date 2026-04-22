//! Graph context provider trait and types.
//!
//! Defines the contract for code intelligence context injection into the agent
//! runtime. The trait lives in theo-domain (pure types); the concrete
//! implementation lives in theo-application (orchestrates engines).

use std::path::Path;

// ---------------------------------------------------------------------------
// Excluded directories — source of truth for graph indexing
// ---------------------------------------------------------------------------

/// Directories that should ALWAYS be excluded from code graph indexing.
///
/// These are build artifacts, dependency caches, and generated code that
/// pollute the graph with irrelevant nodes. Used by both `extraction.rs`
/// and `graph_context_service.rs` — import from here, don't duplicate.
///
/// Note: the `ignore` crate's WalkBuilder also respects `.gitignore` via
/// `.git_ignore(true)`. This list is a safety net for repos without `.gitignore`
/// or when `.git/` is absent (e.g., tarballs, rsync without .git).
pub const EXCLUDED_DIRS: &[&str] = &[
    // Rust
    "target",
    // Node.js / JavaScript / TypeScript
    "node_modules",
    ".next",
    ".nuxt",
    "bower_components",
    // Python
    "__pycache__",
    ".venv",
    "venv",
    ".tox",
    ".eggs",
    ".mypy_cache",
    // Go
    "vendor",
    // Java / Kotlin / Gradle
    ".gradle",
    ".mvn",
    // Generic build output
    "dist",
    "build",
    "out",
    ".output",
    // Coverage / testing
    "coverage",
    ".nyc_output",
    "htmlcov",
    // Caches
    ".cache",
    ".parcel-cache",
    ".turbo",
    // Rust toolchain (if somehow in tree)
    ".cargo",
    ".rustup",
    // Generated code
    "__generated__",
    "generated",
    // IDE / editor
    ".idea",
    ".vscode",
];

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single block of code context assembled by the retrieval engine.
#[derive(Debug, Clone)]
pub struct ContextBlock {
    /// Unique ID for citation tracking. Generated at assembly time.
    pub block_id: String,
    /// Identifier of the source community/cluster.
    pub source_id: String,
    /// Human-readable content (signatures, summaries, code).
    pub content: String,
    /// Estimated token count for this block.
    pub token_count: usize,
    /// Relevance score (0.0 – 1.0).
    pub score: f64,
}

/// Why a context block was excluded from the assembled result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropReason {
    /// Block was scored but didn't fit within remaining token budget.
    BudgetExhausted,
    /// Block's relevance score was below the inclusion threshold.
    LowScore,
    /// Block's source community was already well-represented.
    CommunityOverlap,
}

/// Budget utilization report for a single context assembly pass.
///
/// Inspired by OpenDev's staged compaction accounting: track not just what
/// was included, but what was excluded and why. This enables the feedback
/// loop to distinguish "irrelevant context" from "relevant but budget-limited".
#[derive(Debug, Clone, Default)]
pub struct BudgetReport {
    /// Token budget that was requested.
    pub budget_tokens: usize,
    /// Tokens actually used by included blocks.
    pub tokens_used: usize,
    /// Number of candidate blocks that were evaluated.
    pub candidates_evaluated: usize,
    /// Number of blocks included in the result.
    pub blocks_included: usize,
    /// Number of blocks excluded with reasons.
    pub blocks_skipped: usize,
    /// Per-reason skip counts for diagnostics.
    pub skip_reasons: Vec<(DropReason, usize)>,
}

impl BudgetReport {
    /// Fraction of budget actually consumed (0.0 – 1.0).
    pub fn utilization(&self) -> f64 {
        if self.budget_tokens == 0 {
            return 0.0;
        }
        self.tokens_used as f64 / self.budget_tokens as f64
    }

    /// Tokens remaining unused in the budget.
    pub fn tokens_remaining(&self) -> usize {
        self.budget_tokens.saturating_sub(self.tokens_used)
    }
}

/// The assembled graph context result, ready for LLM injection.
#[derive(Debug, Clone)]
pub struct GraphContextResult {
    /// Ordered blocks of context (highest relevance first).
    pub blocks: Vec<ContextBlock>,
    /// Total tokens across all blocks.
    pub total_tokens: usize,
    /// Token budget that was requested.
    pub budget_tokens: usize,
    /// Comma-separated names of excluded communities (exploration hints).
    pub exploration_hints: String,
    /// Budget utilization report (populated when available).
    pub budget_report: Option<BudgetReport>,
}

impl GraphContextResult {
    /// Format as a single string suitable for a system message.
    pub fn to_prompt_text(&self) -> String {
        if self.blocks.is_empty() {
            return String::new();
        }
        let mut out = String::with_capacity(self.total_tokens * 4);
        for block in &self.blocks {
            out.push_str(&block.content);
            out.push('\n');
        }
        if !self.exploration_hints.is_empty() {
            out.push_str(&format!(
                "\n<!-- Other modules (not shown): {} -->",
                self.exploration_hints
            ));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors from graph context operations.
#[derive(Debug, thiserror::Error)]
pub enum GraphContextError {
    #[error("Graph context not initialized")]
    NotInitialized,

    #[error("Graph build failed: {0}")]
    BuildFailed(String),

    #[error("Query failed: {0}")]
    QueryFailed(String),

    #[error("Operation timed out after {0}s")]
    Timeout(u64),
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Provider of code intelligence context for the agent runtime.
///
/// The runtime receives `Option<Arc<dyn GraphContextProvider + Send + Sync>>`
/// and calls `query_context()` to inject code structure into the LLM prompt.
///
/// Implementations live in theo-application; the runtime never sees the engines.
/// Sink for `DomainEvent`s emitted by the graph-context pipeline.
///
/// PLAN_CONTEXT_WIRING Phase 4 — the context service lives in
/// `theo-application`, which cannot depend on `theo-agent-runtime::EventBus`
/// (apps → application → infra/domain only). This trait lets the runtime
/// pass a lightweight adapter (e.g. `EventBusSink` in theo-agent-runtime)
/// that forwards events into the broadcast bus.
///
/// Implementations must be cheap and non-blocking — the context service
/// calls this on the read path. The default no-op impl means services
/// can operate without telemetry and be given a sink later.
pub trait EventSink: Send + Sync {
    fn emit(&self, event: crate::event::DomainEvent);
}

/// No-op sink used when telemetry is disabled.
pub struct NoopEventSink;
impl EventSink for NoopEventSink {
    fn emit(&self, _event: crate::event::DomainEvent) {}
}

#[async_trait::async_trait]
pub trait GraphContextProvider: Send + Sync {
    /// Build the code graph for a project directory.
    ///
    /// This is CPU-bound (tree-sitter parsing, clustering) and may take
    /// several seconds. Callers MUST run this via `spawn_blocking` with a
    /// timeout. Implementations MUST NOT panic — return Err instead.
    async fn initialize(&self, project_dir: &Path) -> Result<(), GraphContextError>;

    /// Query the graph for context relevant to the given objective.
    ///
    /// Returns a `GraphContextResult` that fits within `budget_tokens`.
    /// The invariant `result.total_tokens <= budget_tokens` MUST hold.
    ///
    /// If the provider is not initialized, returns `Err(NotInitialized)`.
    async fn query_context(
        &self,
        query: &str,
        budget_tokens: usize,
    ) -> Result<GraphContextResult, GraphContextError>;

    /// Whether the provider has been successfully initialized.
    fn is_ready(&self) -> bool;

    /// Fallback: search by symbol name when query_context returns 0 blocks.
    /// Default returns empty — implementations override with symbol lookup.
    async fn query_by_symbol(
        &self,
        _symbol_query: &str,
        budget_tokens: usize,
    ) -> Result<GraphContextResult, GraphContextError> {
        Ok(GraphContextResult {
            blocks: vec![],
            total_tokens: 0,
            budget_tokens,
            exploration_hints: String::new(),
            budget_report: None,
        })
    }

    /// Navigate the code graph from a specific symbol.
    /// Returns callers, callees, imports, or dependents of the given symbol.
    /// Default returns empty — implementations override with graph traversal.
    async fn navigate_symbol(
        &self,
        _symbol: &str,
        _mode: NavigationMode,
        budget_tokens: usize,
    ) -> Result<GraphContextResult, GraphContextError> {
        Ok(GraphContextResult {
            blocks: vec![],
            total_tokens: 0,
            budget_tokens,
            exploration_hints: String::new(),
            budget_report: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Impact analysis types
// ---------------------------------------------------------------------------

/// Result of impact analysis for a single file edit.
///
/// Pure data type — lives in theo-domain for cross-layer consumption.
/// The analysis algorithm lives in theo-application (needs engine access).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImpactReport {
    /// The file that was edited.
    pub edited_file: String,
    /// Community IDs that contain at least one affected node.
    pub affected_communities: Vec<String>,
    /// IDs of test nodes covering affected symbols.
    pub tests_covering_edit: Vec<String>,
    /// File paths that historically co-change with the edited file.
    pub co_change_candidates: Vec<String>,
    /// Human-readable risk alerts.
    pub risk_alerts: Vec<String>,
    /// The BFS depth used during analysis.
    pub bfs_depth: usize,
}

/// Navigation mode for graph traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationMode {
    /// Who calls this symbol?
    Callers,
    /// What does this symbol call?
    Callees,
    /// What does this file/symbol import?
    Imports,
    /// What depends on this file/symbol?
    Dependents,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_context_result_empty_blocks_returns_empty_string() {
        let result = GraphContextResult {
            blocks: vec![],
            total_tokens: 0,
            budget_tokens: 4000,
            exploration_hints: String::new(),
            budget_report: None,
        };
        assert!(result.to_prompt_text().is_empty());
    }

    #[test]
    fn graph_context_result_formats_blocks_with_hints() {
        let result = GraphContextResult {
            blocks: vec![
                ContextBlock {
                    block_id: String::new(),
                    source_id: "auth".into(),
                    content: "# Auth module\npub fn verify_token()".into(),
                    token_count: 10,
                    score: 0.9,
                },
                ContextBlock {
                    block_id: String::new(),
                    source_id: "db".into(),
                    content: "# DB module\npub fn query()".into(),
                    token_count: 8,
                    score: 0.7,
                },
            ],
            total_tokens: 18,
            budget_tokens: 4000,
            exploration_hints: "logging, metrics".into(),
            budget_report: None,
        };
        let text = result.to_prompt_text();
        assert!(text.contains("Auth module"));
        assert!(text.contains("DB module"));
        assert!(text.contains("logging, metrics"));
    }

    #[test]
    fn graph_context_error_variants_are_distinct() {
        let e1 = GraphContextError::NotInitialized;
        let e2 = GraphContextError::BuildFailed("parse error".into());
        let e3 = GraphContextError::QueryFailed("no results".into());
        let e4 = GraphContextError::Timeout(10);

        assert!(e1.to_string().contains("not initialized"));
        assert!(e2.to_string().contains("parse error"));
        assert!(e3.to_string().contains("no results"));
        assert!(e4.to_string().contains("10"));
    }

    /// Verify the trait is dyn-safe (compiles as Arc<dyn ...>).
    #[test]
    fn trait_is_object_safe() {
        fn _assert_object_safe(_: &dyn GraphContextProvider) {}
        fn _assert_arc(_: std::sync::Arc<dyn GraphContextProvider>) {}
    }

    #[test]
    fn budget_report_utilization_tracks_usage() {
        let report = BudgetReport {
            budget_tokens: 1000,
            tokens_used: 750,
            candidates_evaluated: 10,
            blocks_included: 5,
            blocks_skipped: 5,
            skip_reasons: vec![
                (DropReason::BudgetExhausted, 3),
                (DropReason::LowScore, 2),
            ],
        };
        assert!((report.utilization() - 0.75).abs() < f64::EPSILON);
        assert_eq!(report.tokens_remaining(), 250);
    }

    #[test]
    fn budget_report_zero_budget_returns_zero_utilization() {
        let report = BudgetReport::default();
        assert!((report.utilization() - 0.0).abs() < f64::EPSILON);
        assert_eq!(report.tokens_remaining(), 0);
    }

    #[test]
    fn drop_reason_variants_are_distinct() {
        assert_ne!(DropReason::BudgetExhausted, DropReason::LowScore);
        assert_ne!(DropReason::LowScore, DropReason::CommunityOverlap);
        assert_eq!(DropReason::BudgetExhausted, DropReason::BudgetExhausted);
    }
}
