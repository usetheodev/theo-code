/// Escape hatch — context miss detection.
///
/// Provides O(1) membership checking for files currently in context, and
/// detects which community contains a missing file so the pipeline can
/// suggest an expansion.
use std::collections::{HashMap, HashSet};

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::CodeGraph;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A context miss: a file that was referenced but is not in the current context.
pub struct ContextMiss {
    /// The file path that caused the miss.
    pub file_path: String,
    /// The community that contains this file.
    pub containing_community: String,
    /// Suggested community IDs to add (1-hop neighbors of the containing community).
    pub suggested_expansion: Vec<String>,
}

/// Fast membership checker backed by a `HashSet`.
pub struct ContextMembership {
    file_paths: HashSet<String>,
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

impl ContextMembership {
    /// Build from a slice of file paths currently in context.
    pub fn new(context_files: &[String]) -> Self {
        ContextMembership {
            file_paths: context_files.iter().cloned().collect(),
        }
    }

    /// O(1) amortized membership check.
    pub fn contains(&self, file_path: &str) -> bool {
        self.file_paths.contains(file_path)
    }

    /// Detect a context miss for `file_path`.
    ///
    /// Returns `None` if the file is already in context OR if it cannot be
    /// found in any community node.
    pub fn detect_miss(
        &self,
        file_path: &str,
        graph: &CodeGraph,
        communities: &[Community],
    ) -> Option<ContextMiss> {
        // If the file is already in context, no miss.
        if self.contains(file_path) {
            return None;
        }

        // Find which community contains a node whose file_path matches.
        let containing_community = communities.iter().find(|comm| {
            comm.node_ids.iter().any(|node_id| {
                graph
                    .get_node(node_id)
                    .and_then(|n| n.file_path.as_deref())
                    .map(|fp| fp == file_path)
                    .unwrap_or(false)
            })
        })?;

        // Build a community-level adjacency: community_id -> {neighbor_community_id}
        // Two communities are neighbors if any node in one has an edge to any node in the other.
        let node_to_community: HashMap<&str, &str> = communities
            .iter()
            .flat_map(|comm| {
                comm.node_ids
                    .iter()
                    .map(move |nid| (nid.as_str(), comm.id.as_str()))
            })
            .collect();

        let mut neighbor_communities: HashSet<String> = HashSet::new();

        for node_id in &containing_community.node_ids {
            // Outgoing edges
            for neighbor_node in graph.neighbors(node_id) {
                if let Some(&comm_id) = node_to_community.get(neighbor_node) {
                    if comm_id != containing_community.id {
                        neighbor_communities.insert(comm_id.to_string());
                    }
                }
            }
            // Incoming edges (reverse neighbors)
            for neighbor_node in graph.reverse_neighbors(node_id) {
                if let Some(&comm_id) = node_to_community.get(neighbor_node) {
                    if comm_id != containing_community.id {
                        neighbor_communities.insert(comm_id.to_string());
                    }
                }
            }
        }

        Some(ContextMiss {
            file_path: file_path.to_string(),
            containing_community: containing_community.id.clone(),
            suggested_expansion: neighbor_communities.into_iter().collect(),
        })
    }
}
