//! Wiki generator (CodeGraph → WikiDoc) — decomposed per T4.1 of
//! `docs/plans/god-files-2026-07-23-plan.md` (ADR D5).
//!
//! Sub-modules:
//!   - doc.rs         — generate_wiki, generate_doc, find_entry_points, etc.
//!   - metadata.rs    — Cargo.toml + module doc + README extraction
//!   - summary.rs     — generate_summary, generate_tags, slugify
//!   - hashing.rs     — graph/community hash for cache invalidation
//!   - incremental.rs — generate_wiki_incremental, IncrementalStats
//!   - concepts.rs    — ConceptCandidate detection + naming

mod concepts;
mod doc;
mod hashing;
mod incremental;
mod metadata;
mod summary;

pub use concepts::*;
pub use doc::*;
pub use hashing::*;
pub use incremental::*;
pub use metadata::*;
pub use summary::*;

#[cfg(test)]
#[path = "generator_tests.rs"]
mod tests;
