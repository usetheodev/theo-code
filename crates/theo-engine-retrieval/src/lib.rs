// Core retrieval
pub mod assembly;
pub mod budget;
pub mod escape;
pub mod graph_attention;
pub mod search;
pub mod summary;

// Organized sub-modules
pub mod embedding;
pub mod experimental;

// Re-exports for backward compatibility
pub use embedding::neural;
pub use embedding::tfidf;
pub use embedding::turboquant;
pub use experimental::{bandit, cascade, compress, contrastive, ensemble, feedback, memory, predictive};
