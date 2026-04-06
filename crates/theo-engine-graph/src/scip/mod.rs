//! SCIP (Source Code Intelligence Protocol) integration.
//!
//! Provides exact cross-file symbol resolution via compiler analysis.
//! When available, SCIP edges replace Tree-Sitter heuristics in the graph.
//!
//! Architecture:
//! - `reader` — parses index.scip protobuf into lookup tables
//! - `adapter` — implements CodeIntelProvider trait (Strategy Pattern)
//! - `indexer` — invokes rust-analyzer scip in background
//! - `merge` — merges SCIP edges into existing CodeGraph

#[cfg(feature = "scip")]
pub mod reader;

#[cfg(feature = "scip")]
pub mod adapter;

pub mod indexer;

#[cfg(feature = "scip")]
pub mod merge;
