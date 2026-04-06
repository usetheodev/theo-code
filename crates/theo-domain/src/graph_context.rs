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
    "node_modules", ".next", ".nuxt", "bower_components",
    // Python
    "__pycache__", ".venv", "venv", ".tox", ".eggs", ".mypy_cache",
    // Go
    "vendor",
    // Java / Kotlin / Gradle
    ".gradle", ".mvn",
    // Generic build output
    "dist", "build", "out", ".output",
    // Coverage / testing
    "coverage", ".nyc_output", "htmlcov",
    // Caches
    ".cache", ".parcel-cache", ".turbo",
    // Rust toolchain (if somehow in tree)
    ".cargo", ".rustup",
    // Generated code
    "__generated__", "generated",
    // IDE / editor
    ".idea", ".vscode",
];

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single block of code context assembled by the retrieval engine.
#[derive(Debug, Clone)]
pub struct ContextBlock {
    /// Identifier of the source community/cluster.
    pub source_id: String,
    /// Human-readable content (signatures, summaries, code).
    pub content: String,
    /// Estimated token count for this block.
    pub token_count: usize,
    /// Relevance score (0.0 – 1.0).
    pub score: f64,
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
        };
        assert!(result.to_prompt_text().is_empty());
    }

    #[test]
    fn graph_context_result_formats_blocks_with_hints() {
        let result = GraphContextResult {
            blocks: vec![
                ContextBlock {
                    source_id: "auth".into(),
                    content: "# Auth module\npub fn verify_token()".into(),
                    token_count: 10,
                    score: 0.9,
                },
                ContextBlock {
                    source_id: "db".into(),
                    content: "# DB module\npub fn query()".into(),
                    token_count: 8,
                    score: 0.7,
                },
            ],
            total_tokens: 18,
            budget_tokens: 4000,
            exploration_hints: "logging, metrics".into(),
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
}
