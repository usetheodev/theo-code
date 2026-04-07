// Core retrieval
pub mod assembly;
pub mod budget;
pub mod code_tokenizer;
pub mod escape;
pub mod graph_attention;
pub mod search;
pub mod summary;
#[cfg(feature = "tantivy-backend")]
pub mod tantivy_search;
#[cfg(feature = "dense-retrieval")]
pub mod dense_search;
#[cfg(feature = "reranker")]
pub mod reranker;
#[cfg(feature = "reranker")]
pub mod pipeline;

// Organized sub-modules
pub mod embedding;
pub mod experimental;

// Re-exports for backward compatibility
pub use embedding::neural;
pub use embedding::tfidf;
pub use embedding::turboquant;
pub use experimental::compress;
