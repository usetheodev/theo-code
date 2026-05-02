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
    let file_to_community = build_file_community_map(communities, graph);
    let active_communities: Vec<&Community> = communities
        .iter()
        .filter(|c| !c.node_ids.is_empty())
        .collect();

    let HashAndChangeAnalysis {
        new_hashes,
        changed_keys,
        key_to_community,
    } = compute_hashes_and_changes(&active_communities, graph, existing_manifest);

    if changed_keys.is_empty() {
        return fast_path_unchanged(&new_hashes, graph, existing_docs, &active_communities);
    }
    if changed_keys.len() * 2 > active_communities.len() {
        return full_regen_path(communities, graph, project_name, new_hashes, &active_communities);
    }

    let mut changed_docs =
        generate_changed_docs(&changed_keys, &key_to_community, graph, communities, &file_to_community);
    let propagated_keys = propagate_to_dependents(
        &mut changed_docs,
        &new_hashes,
        &key_to_community,
        existing_docs,
        graph,
        communities,
        &file_to_community,
    );
    let final_docs = merge_changed_with_existing(&active_communities, &mut changed_docs, existing_docs);
    let stats = IncrementalStats {
        changed: changed_keys.len(),
        propagated: propagated_keys.len(),
        skipped: active_communities.len() - changed_keys.len() - propagated_keys.len(),
    };
    let now = chrono_now();
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

struct HashAndChangeAnalysis<'a> {
    new_hashes: HashMap<String, u64>,
    changed_keys: HashSet<String>,
    key_to_community: HashMap<String, &'a Community>,
}

fn compute_hashes_and_changes<'a>(
    active_communities: &[&'a Community],
    graph: &CodeGraph,
    existing_manifest: &WikiManifest,
) -> HashAndChangeAnalysis<'a> {
    let mut new_hashes: HashMap<String, u64> = HashMap::new();
    let mut changed_keys: HashSet<String> = HashSet::new();
    let mut key_to_community: HashMap<String, &Community> = HashMap::new();
    for c in active_communities {
        let key = community_canonical_key(c, graph);
        let hash = compute_community_hash(c, graph);
        new_hashes.insert(key.clone(), hash);
        key_to_community.insert(key.clone(), c);
        if existing_manifest.page_hashes.get(&key) != Some(&hash) {
            changed_keys.insert(key);
        }
    }
    HashAndChangeAnalysis {
        new_hashes,
        changed_keys,
        key_to_community,
    }
}

fn fast_path_unchanged(
    new_hashes: &HashMap<String, u64>,
    graph: &CodeGraph,
    existing_docs: &[WikiDoc],
    active_communities: &[&Community],
) -> (Wiki, IncrementalStats) {
    let now = chrono_now();
    (
        Wiki {
            docs: existing_docs.to_vec(),
            manifest: WikiManifest {
                schema_version: WikiManifest::SCHEMA_VERSION,
                generator_version: WikiManifest::GENERATOR_VERSION.to_string(),
                graph_hash: compute_graph_hash(graph),
                generated_at: now,
                page_count: existing_docs.len(),
                page_hashes: new_hashes.clone(),
            },
        },
        IncrementalStats {
            changed: 0,
            propagated: 0,
            skipped: active_communities.len(),
        },
    )
}

fn full_regen_path(
    communities: &[Community],
    graph: &CodeGraph,
    project_name: &str,
    new_hashes: HashMap<String, u64>,
    active_communities: &[&Community],
) -> (Wiki, IncrementalStats) {
    let wiki = generate_wiki_with_root(communities, graph, project_name, None);
    let mut manifest = wiki.manifest.clone();
    manifest.page_hashes = new_hashes;
    let stats = IncrementalStats {
        changed: active_communities.len(),
        propagated: 0,
        skipped: 0,
    };
    (
        Wiki {
            docs: wiki.docs,
            manifest,
        },
        stats,
    )
}

fn generate_changed_docs(
    changed_keys: &HashSet<String>,
    key_to_community: &HashMap<String, &Community>,
    graph: &CodeGraph,
    communities: &[Community],
    file_to_community: &HashMap<String, String>,
) -> HashMap<String, WikiDoc> {
    let mut changed_docs: HashMap<String, WikiDoc> = HashMap::new();
    for key in changed_keys {
        if let Some(community) = key_to_community.get(key.as_str()) {
            let doc = generate_doc(community, graph, communities, file_to_community, None);
            changed_docs.insert(doc.slug.clone(), doc);
        }
    }
    changed_docs
}

#[allow(clippy::too_many_arguments)]
fn propagate_to_dependents(
    changed_docs: &mut HashMap<String, WikiDoc>,
    new_hashes: &HashMap<String, u64>,
    key_to_community: &HashMap<String, &Community>,
    existing_docs: &[WikiDoc],
    graph: &CodeGraph,
    communities: &[Community],
    file_to_community: &HashMap<String, String>,
) -> HashSet<String> {
    let reverse_deps = build_reverse_dependency_map(existing_docs, changed_docs);
    let changed_slugs: HashSet<String> = changed_docs.keys().cloned().collect();
    let to_propagate = collect_two_hop_dependents(&reverse_deps, &changed_slugs);

    let mut propagated_keys: HashSet<String> = HashSet::new();
    for key in new_hashes.keys() {
        let Some(community) = key_to_community.get(key.as_str()) else {
            continue;
        };
        let slug = slugify(&community.name);
        if to_propagate.contains(&slug) && !changed_slugs.contains(&slug) {
            let doc = generate_doc(community, graph, communities, file_to_community, None);
            propagated_keys.insert(key.clone());
            changed_docs.insert(doc.slug.clone(), doc);
        }
    }
    propagated_keys
}

/// target_slug → set of source_slugs (combining existing docs + newly
/// generated `changed_docs`).
fn build_reverse_dependency_map(
    existing_docs: &[WikiDoc],
    changed_docs: &HashMap<String, WikiDoc>,
) -> HashMap<String, HashSet<String>> {
    let mut reverse_deps: HashMap<String, HashSet<String>> = HashMap::new();
    for doc in existing_docs {
        for dep in &doc.dependencies {
            reverse_deps
                .entry(dep.target_slug.clone())
                .or_default()
                .insert(doc.slug.clone());
        }
    }
    for doc in changed_docs.values() {
        for dep in &doc.dependencies {
            reverse_deps
                .entry(dep.target_slug.clone())
                .or_default()
                .insert(doc.slug.clone());
        }
    }
    reverse_deps
}

fn collect_two_hop_dependents(
    reverse_deps: &HashMap<String, HashSet<String>>,
    changed_slugs: &HashSet<String>,
) -> HashSet<String> {
    let mut to_propagate: HashSet<String> = HashSet::new();
    for changed_slug in changed_slugs {
        if let Some(dependents) = reverse_deps.get(changed_slug) {
            for dep_slug in dependents {
                if !changed_slugs.contains(dep_slug) {
                    to_propagate.insert(dep_slug.clone());
                }
            }
        }
    }
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
    to_propagate
}

fn merge_changed_with_existing(
    active_communities: &[&Community],
    changed_docs: &mut HashMap<String, WikiDoc>,
    existing_docs: &[WikiDoc],
) -> Vec<WikiDoc> {
    let existing_by_slug: HashMap<String, &WikiDoc> =
        existing_docs.iter().map(|d| (d.slug.clone(), d)).collect();
    let mut final_docs: Vec<WikiDoc> = Vec::new();
    for c in active_communities {
        let slug = slugify(&c.name);
        if let Some(new_doc) = changed_docs.remove(&slug) {
            final_docs.push(new_doc);
        } else if let Some(existing) = existing_by_slug.get(&slug) {
            final_docs.push((*existing).clone());
        }
    }
    final_docs.sort_by_key(|doc| std::cmp::Reverse(doc.file_count));
    final_docs
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
