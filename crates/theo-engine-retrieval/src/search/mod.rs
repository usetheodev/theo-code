//! BM25 search + multi-signal scoring (T4.3 split, D5).

mod bm25_index;
mod document;
mod file_bm25;
mod multi;
mod signals;
mod tokenizing;
mod types;

pub use document::*;
pub use file_bm25::*;
pub use signals::*;
pub use tokenizing::*;
pub use types::*;

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
