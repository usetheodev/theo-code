//! Single-purpose slice extracted from `cluster.rs` (T4.2 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::{HashMap, HashSet, VecDeque};

use crate::model::CodeGraph;

use super::*;
use super::helpers::*;
use super::leiden::*;
use super::types::*;

pub fn name_community(community: &Community, graph: &CodeGraph) -> String {
    let paths: Vec<&str> = community
        .node_ids
        .iter()
        .filter_map(|id| graph.get_node(id))
        .filter_map(|n| n.file_path.as_deref())
        .collect();

    if paths.is_empty() {
        return format!("community-{}", community.id);
    }

    // Find common path prefix.
    let segments: Vec<Vec<&str>> = paths
        .iter()
        .map(|p| p.split('/').collect::<Vec<_>>())
        .collect();

    let min_len = segments.iter().map(|s| s.len()).min().unwrap_or(0);
    let mut common_depth = 0;
    for i in 0..min_len {
        let first = segments[0][i];
        if segments.iter().all(|s| s[i] == first) {
            common_depth = i + 1;
        } else {
            break;
        }
    }

    // Use the deepest common directory, falling back to the first 2 segments.
    let prefix = if common_depth > 0 {
        segments[0][..common_depth].join("/")
    } else {
        // No common prefix — use the most common directory.
        let mut dir_counts: HashMap<&str, usize> = HashMap::new();
        for segs in &segments {
            if segs.len() >= 2 {
                *dir_counts.entry(segs[segs.len() - 2]).or_insert(0) += 1;
            }
        }
        dir_counts
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(dir, _)| dir.to_string())
            .unwrap_or_else(|| paths[0].to_string())
    };

    // Extract meaningful name: try crate/package name, then deepest directory.
    
    extract_meaningful_name(&prefix, &paths, community.node_ids.len())
}

/// Extract a meaningful community name from file paths.
///
/// Priority: crate name > package directory > deepest non-trivial directory > fallback.
pub fn extract_meaningful_name(prefix: &str, paths: &[&str], member_count: usize) -> String {
    let parts: Vec<&str> = prefix.split('/').filter(|s| !s.is_empty()).collect();

    // Try to find a crate name pattern: "crates/<name>/..." or "apps/<name>/..."
    for (i, part) in parts.iter().enumerate() {
        if (*part == "crates" || *part == "apps") && i + 1 < parts.len() {
            let crate_name = parts[i + 1];
            // Include subdirectory if available (e.g., "theo-agent-runtime/src/config")
            if i + 3 < parts.len() && parts[i + 2] == "src" {
                return format!("{}::{} ({})", crate_name, parts[i + 3], member_count);
            }
            return format!("{} ({})", crate_name, member_count);
        }
    }

    // Try "src/<meaningful_dir>/..." pattern
    for (i, part) in parts.iter().enumerate() {
        if *part == "src" && i + 1 < parts.len() {
            let module = parts[i + 1];
            if module != "lib.rs" && module != "main.rs" {
                return format!("{} ({})", module, member_count);
            }
        }
    }

    // Fallback: use the deepest non-trivial directory
    let trivial = ["src", "lib", "crates", "apps", ".", ""];
    for part in parts.iter().rev() {
        if !trivial.contains(part) {
            return format!("{} ({})", part, member_count);
        }
    }

    // Last resort: count distinct top-level dirs
    let mut top_dirs: Vec<&str> = paths.iter().filter_map(|p| p.split('/').next()).collect();
    top_dirs.sort();
    top_dirs.dedup();
    if top_dirs.len() <= 3 {
        format!("{} ({})", top_dirs.join("+"), member_count)
    } else {
        format!("mixed ({})", member_count)
    }
}
