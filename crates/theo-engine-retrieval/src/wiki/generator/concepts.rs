//! Single-purpose slice extracted from `wiki/generator.rs` (T4.1 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::{HashMap, HashSet};

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, EdgeType, NodeType, SymbolKind};

use crate::wiki::model::*;

use super::*;

#[derive(Debug, Clone)]
pub struct ConceptCandidate {
    /// Concept name (e.g., "retrieval", "authentication", "sandbox").
    pub name: String,
    /// Slugs of related module pages.
    pub related_modules: Vec<String>,
    /// Hint text from top symbols/docs.
    pub description_hint: String,
}

/// Detect high-level concepts using graph topology (cross-dep edge density)
/// with prefix-based fallback.
///
/// Algorithm:
/// 1. Build adjacency matrix from WikiDoc.dependencies
/// 2. Union-find: merge communities with >= 3 mutual cross-deps
/// 3. Fallback to prefix-based grouping for unclustered modules
pub fn detect_concepts(docs: &[crate::wiki::model::WikiDoc]) -> Vec<ConceptCandidate> {
    let filtered: Vec<&crate::wiki::model::WikiDoc> = docs.iter().filter(|d| d.file_count >= 2).collect();

    if filtered.is_empty() {
        return Vec::new();
    }

    // Build slug → index map
    let slug_to_idx: HashMap<String, usize> = filtered
        .iter()
        .enumerate()
        .map(|(i, d)| (d.slug.clone(), i))
        .collect();

    let n = filtered.len();

    // Build adjacency matrix: adj[i][j] = count of deps from i to j
    let mut adj = vec![vec![0u32; n]; n];
    for (i, doc) in filtered.iter().enumerate() {
        for dep in &doc.dependencies {
            if let Some(&j) = slug_to_idx.get(&dep.target_slug)
                && i != j {
                    adj[i][j] += 1;
                }
        }
    }

    // Union-Find
    let mut parent: Vec<usize> = (0..n).collect();
    let find = |parent: &mut Vec<usize>, mut x: usize| -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]]; // path compression
            x = parent[x];
        }
        x
    };

    // Merge communities with >= 3 mutual edges.
    // Index-based loop needed: we read adj[i][j] and adj[j][i] in both directions.
    #[allow(clippy::needless_range_loop)]
    for i in 0..n {
        for j in (i + 1)..n {
            let mutual = adj[i][j] + adj[j][i];
            if mutual >= 3 {
                let ri = find(&mut parent, i);
                let rj = find(&mut parent, j);
                if ri != rj {
                    parent[ri] = rj;
                }
            }
        }
    }

    // Collect topology-based clusters
    let mut clusters: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        clusters.entry(root).or_default().push(i);
    }

    let mut concepts = Vec::new();
    let mut clustered_slugs: HashSet<String> = HashSet::new();

    for members in clusters.values() {
        if members.len() < 2 {
            continue;
        } // Need 2+ for a concept

        let group_docs: Vec<&crate::wiki::model::WikiDoc> =
            members.iter().map(|&i| filtered[i]).collect();

        let related_modules: Vec<String> = group_docs.iter().map(|d| d.slug.clone()).collect();
        for slug in &related_modules {
            clustered_slugs.insert(slug.clone());
        }

        // Name from common prefix or first doc's crate prefix
        let name = derive_concept_name(&group_docs);
        let description_hint = build_description_hint(&group_docs);

        concepts.push(ConceptCandidate {
            name,
            related_modules,
            description_hint,
        });
    }

    // Fallback: prefix-based for unclustered modules
    let mut prefix_groups: HashMap<String, Vec<&crate::wiki::model::WikiDoc>> = HashMap::new();
    for doc in &filtered {
        if clustered_slugs.contains(&doc.slug) {
            continue;
        }
        let key = doc
            .title
            .split(['(', ' '])
            .next()
            .unwrap_or(&doc.title)
            .trim()
            .split('-')
            .take(2)
            .collect::<Vec<_>>()
            .join("-");
        if key.len() >= 4 {
            prefix_groups.entry(key).or_default().push(doc);
        }
    }

    for group_docs in prefix_groups.values() {
        if group_docs.len() < 2 {
            continue;
        }
        let related_modules: Vec<String> = group_docs.iter().map(|d| d.slug.clone()).collect();
        let name = derive_concept_name(group_docs);
        let description_hint = build_description_hint(group_docs);
        concepts.push(ConceptCandidate {
            name,
            related_modules,
            description_hint,
        });
    }

    concepts.sort_by_key(|c| std::cmp::Reverse(c.related_modules.len()));
    concepts.truncate(8);
    concepts
}

/// Derive a human-readable concept name from a group of docs.
pub(super) fn derive_concept_name(docs: &[&crate::wiki::model::WikiDoc]) -> String {
    // Extract common prefix key
    let keys: Vec<String> = docs
        .iter()
        .map(|d| {
            d.title
                .split(['(', ' '])
                .next()
                .unwrap_or(&d.title)
                .trim()
                .split('-')
                .take(2)
                .collect::<Vec<_>>()
                .join("-")
        })
        .collect();

    let most_common = keys
        .iter()
        .fold(HashMap::new(), |mut acc, k| {
            *acc.entry(k.as_str()).or_insert(0) += 1;
            acc
        })
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(k, _)| k.to_string())
        .unwrap_or_default();

    match most_common.as_str() {
        "theo-engine" => "Code Intelligence Engine".to_string(),
        "theo-agent" => "Agent Runtime".to_string(),
        "theo-infra" => "Infrastructure".to_string(),
        "theo-tooling" => "Developer Tools".to_string(),
        "theo-governance" => "Governance & Safety".to_string(),
        "theo-domain" => "Domain Model".to_string(),
        "theo-ui" | "theo-desktop" => "Frontend & Desktop".to_string(),
        "theo-application" => "Application Layer".to_string(),
        other if !other.is_empty() => format!("{} Subsystem", other.replace('-', " ")),
        _ => "Related Modules".to_string(),
    }
}

pub(super) fn build_description_hint(docs: &[&crate::wiki::model::WikiDoc]) -> String {
    let mut hints = Vec::new();
    for doc in docs.iter().take(3) {
        for ep in doc.entry_points.iter().take(2) {
            hints.push(format!("{}: {}", ep.name, ep.signature));
        }
    }
    if hints.is_empty() {
        format!("{} related modules", docs.len())
    } else {
        hints.join("; ")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
