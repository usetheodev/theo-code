//! Community types — Community, ClusterResult, ClusterAlgorithm.
//!
//! Extracted from cluster.rs during T4.2 of god-files-2026-07-23-plan.md.

#![allow(unused_imports, dead_code)]

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Community {
    pub id: String,
    pub name: String,
    /// 0 = domain level (Louvain), 1 = module level (LPA)
    pub level: u32,
    pub node_ids: Vec<String>,
    pub parent_id: Option<String>,
    pub version: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClusterResult {
    pub communities: Vec<Community>,
    /// Newman-Girvan modularity Q ∈ (−0.5, 1.0].
    pub modularity: f64,
}

/// Errors raised by Louvain/LPA heuristics when an algorithm-level
/// invariant is violated. Per ADR-019 these represent programming bugs
/// (the algorithm computes a label-set then immediately reads from it
/// using the same iterated ids, so a missing label means the partition
/// state is internally inconsistent).
///
/// Public API surface: typed errors are propagated up to the highest
/// internal boundary (`subdivide_with_lpa_seeded`, `merge_small_communities`)
/// which then expect-with-context at the public Vec<Community>/ClusterResult
/// boundary, since the user-facing API can't sensibly degrade past
/// "the partition is broken".
#[derive(Debug, thiserror::Error)]
pub enum ClusterError {
    #[error("missing label for node `{0}`; partition state is internally inconsistent")]
    MissingLabel(String),

    #[error("max_by on neighbor weights returned None; expected at least one neighbor in this branch")]
    EmptyNeighbors,
}

/// Algorithm choice for hierarchical clustering at the domain level.
#[derive(Debug, Clone)]
pub enum ClusterAlgorithm {
    Louvain,
    Leiden {
        resolution: f64,
    },
    /// File-level clustering: groups files (not symbols) based on the
    /// aggregated weight of symbol-level edges between them.
    /// Produces 10-30 domains of 3-15 files each — matches how devs think.
    FileLeiden {
        resolution: f64,
    },
}

// ---------------------------------------------------------------------------
// Louvain algorithm
