//! Wiki backend trait — DIP interface for wiki tools.
//!
//! theo-tooling depends on this trait (in theo-domain).
//! theo-application provides the implementation (using theo-engine-retrieval).
//! This keeps bounded contexts clean: tooling → domain, never tooling → engine.

use serde::{Deserialize, Serialize};

/// Result from a wiki query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiQueryResult {
    pub slug: String,
    pub title: String,
    pub summary: String,
    pub content: String,
    pub confidence: f64,
    pub authority_tier: String,
    pub is_stale: bool,
}

/// Runtime execution data to ingest into the wiki.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiInsightInput {
    pub source: String,
    pub command: String,
    pub exit_code: i32,
    pub success: bool,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
}

/// Ingest result confirmation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiIngestResult {
    pub ingested: bool,
    pub affected_files: Vec<String>,
    pub affected_symbols: Vec<String>,
    pub total_insights: usize,
}

/// Result of wiki generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiGenerateResult {
    pub pages_generated: usize,
    pub pages_updated: usize,
    pub pages_skipped: usize,
    pub duration_ms: u64,
    pub wiki_dir: String,
    pub is_incremental: bool,
}

/// Backend trait for wiki operations.
///
/// Implemented in theo-application using theo-engine-retrieval.
/// Consumed in theo-tooling via Arc<dyn WikiBackend>.
#[async_trait::async_trait]
pub trait WikiBackend: Send + Sync {
    /// Query the wiki for relevant pages.
    async fn query(&self, question: &str, max_results: usize) -> Vec<WikiQueryResult>;

    /// Ingest a runtime execution insight.
    async fn ingest(&self, input: WikiInsightInput) -> Result<WikiIngestResult, String>;

    /// Generate or update the wiki. Creates if not exists, incremental update if exists.
    async fn generate(&self) -> Result<WikiGenerateResult, String>;
}
