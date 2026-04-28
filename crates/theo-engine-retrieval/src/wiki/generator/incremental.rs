//! Single-purpose slice extracted from `wiki/generator.rs` (T4.1 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::{HashMap, HashSet};

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, EdgeType, NodeType, SymbolKind};

use crate::wiki::model::*;

use super::*;

pub fn generate_wiki_incremental(
    communities: &[Community],
    graph: &CodeGraph,
    project_name: &str,
    existing_manifest: &WikiManifest,
    existing_docs: &[WikiDoc],
) -> (Wiki, IncrementalStats) {
    // Always build the global file→community map (needed for cross-deps)
    let file_to_community = build_file_community_map(communities, graph);

    // Phase 1: Compute per-community hashes, detect changed
    let active_communities: Vec<&Community> = communities
        .iter()
        .filter(|c| !c.node_ids.is_empty())
        .collect();

    let mut new_hashes: HashMap<String, u64> = HashMap::new();
    let mut changed_keys: HashSet<String> = HashSet::new();
    let mut key_to_community: HashMap<String, &Community> = HashMap::new();

    for c in &active_communities {
        let key = community_canonical_key(c, graph);
        let hash = compute_community_hash(c, graph);
        new_hashes.insert(key.clone(), hash);
        key_to_community.insert(key.clone(), c);

        if existing_manifest.page_hashes.get(&key) != Some(&hash) {
            changed_keys.insert(key);
        }
    }

    // Fast path: nothing changed
    if changed_keys.is_empty() {
        let now = chrono_now();
        return (
            Wiki {
                docs: existing_docs.to_vec(),
                manifest: WikiManifest {
                    schema_version: WikiManifest::SCHEMA_VERSION,
                    generator_version: WikiManifest::GENERATOR_VERSION.to_string(),
                    graph_hash: compute_graph_hash(graph),
                    generated_at: now,
                    page_count: existing_docs.len(),
                    page_hashes: new_hashes,
                },
            },
            IncrementalStats {
                changed: 0,
                propagated: 0,
                skipped: active_communities.len(),
            },
        );
    }

    // Threshold: if >50% changed, full regen is simpler
    if changed_keys.len() * 2 > active_communities.len() {
        let wiki = generate_wiki_with_root(communities, graph, project_name, None);
        let mut manifest = wiki.manifest.clone();
        manifest.page_hashes = new_hashes;
        let stats = IncrementalStats {
            changed: active_communities.len(),
            propagated: 0,
            skipped: 0,
        };
        return (
            Wiki {
                docs: wiki.docs,
                manifest,
            },
            stats,
        );
    }

    // Phase 2: Generate changed docs
    let mut changed_docs: HashMap<String, WikiDoc> = HashMap::new();
    for key in &changed_keys {
        if let Some(community) = key_to_community.get(key.as_str()) {
            let doc = generate_doc(community, graph, communities, &file_to_community, None);
            changed_docs.insert(doc.slug.clone(), doc);
        }
    }

    // Phase 3: Dependency propagation (2-hop)
    // Build reverse-dep map: target_slug → set of source_slugs
    let mut reverse_deps: HashMap<String, HashSet<String>> = HashMap::new();
    // Include existing docs' deps
    for doc in existing_docs {
        for dep in &doc.dependencies {
            reverse_deps
                .entry(dep.target_slug.clone())
                .or_default()
                .insert(doc.slug.clone());
        }
    }
    // Include new changed docs' deps
    for doc in changed_docs.values() {
        for dep in &doc.dependencies {
            reverse_deps
                .entry(dep.target_slug.clone())
                .or_default()
                .insert(doc.slug.clone());
        }
    }

    // Find slugs of changed communities
    let changed_slugs: HashSet<String> = changed_docs.keys().cloned().collect();

    // 2-hop propagation
    let mut propagated_keys: HashSet<String> = HashSet::new();
    let mut to_propagate: HashSet<String> = HashSet::new();

    // Hop 1: direct dependents of changed slugs
    for changed_slug in &changed_slugs {
        if let Some(dependents) = reverse_deps.get(changed_slug) {
            for dep_slug in dependents {
                if !changed_slugs.contains(dep_slug) {
                    to_propagate.insert(dep_slug.clone());
                }
            }
        }
    }

    // Hop 2: dependents of hop-1 slugs
    let hop1_slugs = to_propagate.clone();
    for hop1_slug in &hop1_slugs {
        if let Some(dependents) = reverse_deps.get(hop1_slug) {
            for dep_slug in dependents {
                if !changed_slugs.contains(dep_slug) && !hop1_slugs.contains(dep_slug) {
                    to_propagate.insert(dep_slug.clone());
                }
            }
        }
    }

    // Regenerate propagated pages
    for key in new_hashes.keys() {
        if let Some(community) = key_to_community.get(key.as_str()) {
            let slug = slugify(&community.name);
            if to_propagate.contains(&slug) && !changed_slugs.contains(&slug) {
                let doc = generate_doc(community, graph, communities, &file_to_community, None);
                propagated_keys.insert(key.clone());
                changed_docs.insert(doc.slug.clone(), doc);
            }
        }
    }

    // Phase 4: Merge — changed docs override existing
    let mut final_docs: Vec<WikiDoc> = Vec::new();
    let existing_by_slug: HashMap<String, &WikiDoc> =
        existing_docs.iter().map(|d| (d.slug.clone(), d)).collect();

    // Track which existing slugs are still valid
    let _current_slugs: HashSet<String> = active_communities
        .iter()
        .map(|c| slugify(&c.name))
        .collect();

    for c in &active_communities {
        let slug = slugify(&c.name);
        if let Some(new_doc) = changed_docs.remove(&slug) {
            final_docs.push(new_doc);
        } else if let Some(existing) = existing_by_slug.get(&slug) {
            final_docs.push((*existing).clone());
        }
    }

    final_docs.sort_by_key(|doc| std::cmp::Reverse(doc.file_count));

    let now = chrono_now();
    let stats = IncrementalStats {
        changed: changed_keys.len(),
        propagated: propagated_keys.len(),
        skipped: active_communities.len() - changed_keys.len() - propagated_keys.len(),
    };

    (
        Wiki {
            manifest: WikiManifest {
                schema_version: WikiManifest::SCHEMA_VERSION,
                generator_version: WikiManifest::GENERATOR_VERSION.to_string(),
                graph_hash: compute_graph_hash(graph),
                generated_at: now,
                page_count: final_docs.len(),
                page_hashes: new_hashes,
            },
            docs: final_docs,
        },
        stats,
    )
}

/// Stats from incremental generation.
#[derive(Debug)]
pub struct IncrementalStats {
    pub changed: usize,
    pub propagated: usize,
    pub skipped: usize,
}

impl std::fmt::Display for IncrementalStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "changed: {}, propagated: {}, skipped: {}",
            self.changed, self.propagated, self.skipped
        )
    }
}

/// Current timestamp as ISO 8601.
pub(super) fn chrono_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{}", now)
}
