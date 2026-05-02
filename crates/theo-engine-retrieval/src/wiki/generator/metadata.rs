//! Single-purpose slice extracted from `wiki/generator.rs` (T4.1 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::{HashMap, HashSet};

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, EdgeType, NodeType, SymbolKind};

use crate::wiki::model::*;

use super::*;

pub struct CrateMetadata {
    pub name: Option<String>,
    pub description: Option<String>,
    pub crate_dir: String,
}

/// Extract crate metadata from all Cargo.toml files in the project.
pub fn extract_crate_metadata(project_dir: &std::path::Path) -> HashMap<String, CrateMetadata> {
    let mut metadata = HashMap::new();

    // Walk for Cargo.toml files (max depth 3 to avoid target/)
    if let Ok(entries) = std::fs::read_dir(project_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                if name == "target" || name == ".git" || name == "node_modules" {
                    continue;
                }
                // Check for Cargo.toml in this directory
                let cargo_path = path.join("Cargo.toml");
                if cargo_path.exists()
                    && let Some(meta) = parse_cargo_toml(&cargo_path) {
                        let key = meta.name.clone().unwrap_or_else(|| name.clone());
                        metadata.insert(
                            key,
                            CrateMetadata {
                                crate_dir: name,
                                ..meta
                            },
                        );
                    }
            }
        }
    }

    // Also check root Cargo.toml (single-crate projects)
    let root_cargo = project_dir.join("Cargo.toml");
    if root_cargo.exists()
        && let Some(meta) = parse_cargo_toml(&root_cargo) {
            let key = meta.name.clone().unwrap_or_else(|| {
                project_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("project")
                    .to_string()
            });
            metadata.entry(key).or_insert(meta);
        }

    // Check pyproject.toml for Python projects
    let pyproject = project_dir.join("pyproject.toml");
    if pyproject.exists()
        && let Ok(content) = std::fs::read_to_string(&pyproject) {
            let name = extract_toml_value(&content, "name");
            let desc = extract_toml_value(&content, "description");
            if name.is_some() || desc.is_some() {
                let key = name.clone().unwrap_or_else(|| "project".into());
                metadata.entry(key).or_insert(CrateMetadata {
                    name,
                    description: desc,
                    crate_dir: ".".into(),
                });
            }
        }

    metadata
}

pub(super) fn parse_cargo_toml(path: &std::path::Path) -> Option<CrateMetadata> {
    let content = std::fs::read_to_string(path).ok()?;
    let name = extract_toml_value(&content, "name");
    let description = extract_toml_value(&content, "description");
    Some(CrateMetadata {
        name,
        description,
        crate_dir: String::new(),
    })
}

/// Simple TOML value extraction (no full parser needed — just key = "value" lines).
pub(super) fn extract_toml_value(content: &str, key: &str) -> Option<String> {
    // Look for: key = "value" in [package] section
    let mut in_package = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]" || trimmed == "[project]";
        }
        if in_package
            && let Some(rest) = trimmed.strip_prefix(key) {
                let rest = rest.trim();
                if let Some(rest) = rest.strip_prefix('=') {
                    let val = rest.trim().trim_matches('"').trim_matches('\'');
                    if !val.is_empty() {
                        return Some(val.to_string());
                    }
                }
            }
    }
    None
}

/// Extract //! module-level doc comments from lib.rs or main.rs.
pub fn extract_module_doc(project_dir: &std::path::Path, crate_dir: &str) -> Option<String> {
    // Try crate-level lib.rs first, then main.rs, then src/lib.rs
    let candidates = [
        project_dir.join(crate_dir).join("src").join("lib.rs"),
        project_dir.join(crate_dir).join("src").join("main.rs"),
        project_dir.join(crate_dir).join("lib.rs"),
        project_dir.join("src").join("lib.rs"), // root-level project
        project_dir.join("src").join("main.rs"),
    ];

    for path in &candidates {
        if !path.exists() {
            continue;
        }
        let content = std::fs::read_to_string(path).ok()?;

        // Collect consecutive //! lines at the start
        let mut doc_lines: Vec<String> = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("//!") {
                let text = trimmed.strip_prefix("//!").unwrap_or("").to_string();
                // Remove leading space if present
                let text = text.strip_prefix(' ').unwrap_or(&text).to_string();
                doc_lines.push(text);
            } else if trimmed.is_empty() && !doc_lines.is_empty() {
                doc_lines.push(String::new()); // preserve paragraph breaks
            } else if !trimmed.is_empty() && !doc_lines.is_empty() {
                break; // end of module doc
            } else if trimmed.starts_with("//") || trimmed.starts_with("#![") {
                continue; // skip regular comments and attributes
            } else if !trimmed.is_empty() {
                break; // code started
            }
        }

        if !doc_lines.is_empty() {
            // Trim trailing empty lines
            while doc_lines.last().is_some_and(|l| l.is_empty()) {
                doc_lines.pop();
            }
            return Some(doc_lines.join("\n"));
        }
    }
    None
}

/// Extract first meaningful paragraph from README.md.
pub fn extract_readme_summary(project_dir: &std::path::Path, crate_dir: &str) -> Option<String> {
    let candidates = [
        project_dir.join(crate_dir).join("README.md"),
        project_dir.join("README.md"),
    ];

    for path in &candidates {
        if !path.exists() {
            continue;
        }
        let content = std::fs::read_to_string(path).ok()?;

        // Skip: headings, badges, empty lines, links-only lines
        let paragraph: Vec<&str> = content
            .lines()
            .skip_while(|l| {
                let t = l.trim();
                t.is_empty()
                    || t.starts_with('#')
                    || t.starts_with('[')
                    || t.starts_with('!')
                    || t.starts_with("[![")
                    || t.starts_with("More information")
            })
            .take_while(|l| !l.trim().is_empty())
            .collect();

        if !paragraph.is_empty() {
            let text = paragraph.join(" ");
            if text.len() > 10 {
                // skip very short fragments
                return Some(text);
            }
        }
    }
    None
}

/// Match a WikiDoc to its crate metadata by finding which crate directory contains its files.
pub(super) fn find_crate_for_doc(
    doc: &crate::wiki::model::WikiDoc,
    metadata: &HashMap<String, CrateMetadata>,
) -> Option<String> {
    // Build sorted list of crate dirs (longest first for precise matching)
    let mut dirs: Vec<(&String, &str)> = metadata
        .iter()
        .map(|(name, meta)| {
            let dir = if meta.crate_dir.is_empty() {
                name.as_str()
            } else {
                meta.crate_dir.as_str()
            };
            (name, dir)
        })
        .collect();
    dirs.sort_by_key(|item| std::cmp::Reverse(item.1.len())); // longest prefix first

    // Check majority of files — which crate dir has most file matches?
    let mut best_match: Option<(String, usize)> = None;
    for (name, dir) in &dirs {
        let prefix = format!("{}/", dir);
        let count = doc
            .files
            .iter()
            .filter(|f| f.path.starts_with(&prefix))
            .count();
        if count > 0
            && best_match
                .as_ref()
                .is_none_or(|(_, best_count)| count > *best_count)
            {
                best_match = Some(((*name).clone(), count));
            }
    }
    if let Some((name, _)) = best_match {
        return Some(name);
    }

    // Fallback: check title/slug (longest match first)
    for (name, _) in &dirs {
        if doc.title.contains(name.as_str()) || doc.slug.contains(name.as_str()) {
            return Some((*name).clone());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Karpathy header generators (deterministic, zero LLM)
// ---------------------------------------------------------------------------

/// Derive a semantic title from path structure + primary entry point.
/// "axum-core (10)" → "axum-core — IntoResponseParts"
pub(super) fn derive_semantic_title(
    community_name: &str,
    files: &[crate::wiki::model::FileEntry],
    entry_points: &[crate::wiki::model::ApiEntry],
) -> String {
    // Base: common path prefix or crate directory
    let base = if let Some(first) = files.first() {
        let segments: Vec<&str> = first.path.split('/').collect();
        if segments.len() >= 2 {
            // "axum-core/src/..." → "axum-core"
            // "src/auth.rs" → keep community name
            let first_seg = segments[0];
            if first_seg == "src" || first_seg == "lib" || first_seg == "." {
                community_name
                    .split('(')
                    .next()
                    .unwrap_or(community_name)
                    .trim()
                    .to_string()
            } else {
                first_seg.to_string()
            }
        } else {
            community_name
                .split('(')
                .next()
                .unwrap_or(community_name)
                .trim()
                .to_string()
        }
    } else {
        community_name
            .split('(')
            .next()
            .unwrap_or(community_name)
            .trim()
            .to_string()
    };

    // Append primary concept (first trait/struct entry point)
    let primary = entry_points
        .first()
        .filter(|e| e.kind == "Trait" || e.kind == "Struct" || e.kind == "Function")
        .map(|e| format!(" — {}", e.name));

    let full = format!("{}{}", base, primary.unwrap_or_default());
    if full.len() > 60 {
        full[..57].to_string() + "..."
    } else {
        full
    }
}
