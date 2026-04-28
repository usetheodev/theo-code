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
