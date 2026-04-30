//! Code Wiki: transforms CodeGraph into human-readable, LLM-queryable markdown.
//!
//! Inspired by Karpathy's LLM Wiki + Devin's DeepWiki.
//! Generates persistent knowledge pages from the code graph.
//!
//! Architecture:
//! - `model.rs`: WikiDoc IR (canonical data model, separate from rendering)
//! - `generator.rs`: CodeGraph → WikiDoc (deterministic, zero LLM cost)
//! - `renderer.rs`: WikiDoc → Markdown (Obsidian-compatible)
//! - `persistence.rs`: Disk I/O + cache invalidation

pub mod dense_index;
pub mod generator;
pub mod lint;
pub mod lookup;
pub mod model;
pub mod persistence;
pub mod renderer;
pub mod retriever;
pub mod runtime;

pub use dense_index::{Embedder, WikiDenseIndex};
pub use retriever::{WikiHit, search as wiki_search};
