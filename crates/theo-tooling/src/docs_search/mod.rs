//! T15.1 — External documentation search.
//!
//! Tool that searches a local index of external API docs (crates.io,
//! MDN, npm, etc.). The index abstraction is pluggable; this commit
//! ships an in-memory implementation backed by a tokenised inverted
//! list with TF-IDF-ish scoring. A Tantivy-backed implementation in
//! `theo-engine-retrieval` is the natural follow-up — gated by a
//! feature flag so the tooling crate stays light.
//!
//! Sources are TRAIT-based: `DocSource` implementors fetch documents
//! on demand (crates.io, MDN, npm, ReadTheDocs, ...). This commit
//! ships a `StaticDocSource` (hard-coded test corpus) so the tool
//! surface and integration are testable WITHOUT network. Real network
//! fetchers are deferred — they require both crawling configuration
//! and a cache strategy that's out of scope for autonomous iteration.
//!
//! See `docs/plans/sota-tier1-tier2-plan.md` §T15.1.

pub mod bootstrap;
pub mod index;
pub mod markdown_source;
pub mod source;
pub mod tool;

pub use bootstrap::{bootstrap_docs_index, well_known_locations};
pub use index::{DocEntry, DocsIndex, ScoredDoc};
pub use markdown_source::MarkdownDirSource;
pub use source::{DocSource, StaticDocSource};
pub use tool::DocsSearchTool;
