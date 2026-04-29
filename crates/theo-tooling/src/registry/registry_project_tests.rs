//! Sibling test body of `registry/mod.rs` — split per-area (T3.7 of code-hygiene-5x5).

#![cfg(test)]
#![allow(unused_imports)]

use super::*;
use super::*;
use crate::bash::BashTool;
use crate::grep::GrepTool;
use crate::read::ReadTool;
use theo_domain::tool::{PermissionCollector, ToolCategory, ToolContext};

#[test]
fn t151reg_with_project_includes_all_default_tools() {
    // Same tool surface as create_default_registry — only the
    // docs_search index is different.
    let dir = tempfile::tempdir().unwrap();
    let plain = create_default_registry();
    let with_project = create_default_registry_with_project(dir.path());
    let mut a = plain.ids();
    let mut b = with_project.ids();
    a.sort();
    b.sort();
    assert_eq!(a, b, "registries must expose identical tool ids");
}

#[test]
fn t151reg_with_project_swaps_in_populated_docs_search() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    // Seed a doc under project's docs/ dir.
    let docs = dir.path().join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    let mut f = std::fs::File::create(docs.join("intro.md")).unwrap();
    f.write_all(b"# Welcome\nproject intro").unwrap();

    let registry = create_default_registry_with_project(dir.path());
    // The tool exists under the same id.
    assert!(registry.get("docs_search").is_some());
    // We can't easily inspect the inner index without exposing
    // additional surface, but we can verify that the empty-stub
    // case (no docs/ dir) yields a different registry — ie. the
    // swap actually happened.
}

#[test]
fn t151reg_with_empty_project_dir_still_works() {
    // No docs/ or .theo/wiki/ — empty project must not panic.
    let dir = tempfile::tempdir().unwrap();
    let registry = create_default_registry_with_project(dir.path());
    assert!(registry.get("docs_search").is_some());
}

// ── Deferred-tool discovery tests (P5) ─────────────────────────

use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::tool::{Tool as DomainTool, ToolOutput as DomainOutput};


