//! Test-only fixtures shared by memory / wiki integration tests.
//!
//! Plan: `outputs/agent-memory-plan.md` §"Test infra — theo-test-memory-fixtures".
//! Must not appear in any production dependency graph.

pub mod mock_llm;
pub mod mock_retrieval;

pub use mock_llm::{CompilerCall, CompilerResponse, MockCompilerLLM};
pub use mock_retrieval::{MockRetrievalEngine, ScoredEntry};
