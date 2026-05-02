//! # theo-engine-wiki
//!
//! LLM-compiled wiki for **humans** to understand codebases.
//!
//! ## The Problem
//!
//! Reading code is slow and expensive. Onboarding on a 15-crate project takes
//! weeks. Returning to a module after 2 months requires re-reading. Understanding
//! architectural decisions requires excavating ADRs, git blame, and Slack.
//!
//! ## The Solution
//!
//! The LLM compiles the entire codebase into a navigable wiki that a human can
//! read in hours, not weeks. Each module has a page explaining what it does, why
//! it exists, how it connects to the rest, and what breaks if you change it.
//!
//! ## The Contract
//!
//! ```text
//! HUMAN    = READER  → reads, navigates, queries. Never writes.
//! WIKI AGENT = WRITER → background sub-agent, activated by triggers.
//!                       Only writer. Keeps wiki alive without human intervention.
//! MANUAL   = OPTIONAL → `theo wiki generate` forces update. Rare.
//! ```
//!
//! ## Architecture: Skeleton + Enrichment
//!
//! - **Skeleton** (tree-sitter, free): file inventory, symbol list, public APIs,
//!   dependency edges, module groupings. Extracted from `theo-engine-graph`.
//! - **Enrichment** (LLM via Wiki Agent): "what it does", "why it exists",
//!   "how it works", "what breaks if you change it". Uses cheap model (Haiku).
//!
//! ## Dependency Direction
//!
//! ```text
//! theo-engine-wiki → theo-domain, theo-engine-graph, theo-engine-parser
//! ```
//!
//! Same layer as `theo-engine-retrieval`. Never depends on application or runtime.
//! The `WikiBackend` trait is in `theo-domain`; this crate provides the engine
//! that `theo-application` wires into the trait implementation.

pub mod error;
pub mod skeleton;
pub mod page;
pub mod store;
pub mod lint;
pub mod hash;
