// Core retrieval
pub mod assembly;
pub mod budget;
pub mod code_tokenizer;
#[cfg(feature = "dense-retrieval")]
pub mod dense_search;
pub mod escape;
pub mod file_retriever;
pub mod fs_source_provider;
pub mod graph_attention;
pub mod harm_filter;
pub mod inline_builder;
pub mod metrics;
// T8.1 — `pipeline` (RRF + Rerank) still depends on `dense-retrieval`
// for tantivy_search. The reranker module itself is always compiled.
#[cfg(feature = "dense-retrieval")]
pub mod pipeline;
pub mod reranker;
pub mod search;
pub mod summary;
#[cfg(feature = "tantivy-backend")]
pub mod memory_tantivy;
#[cfg(feature = "tantivy-backend")]
pub mod tantivy_search;
pub mod wiki;

// Organized sub-modules
pub mod embedding;
pub mod experimental;

// Re-exports for backward compatibility
pub use embedding::neural;
pub use embedding::tfidf;
pub use embedding::turboquant;
pub use experimental::compress;
