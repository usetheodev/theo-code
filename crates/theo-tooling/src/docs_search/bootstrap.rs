//! T15.1 — `bootstrap_docs_index`: populate a [`DocsIndex`] from
//! well-known Markdown locations on session start.
//!
//! Scans, in order:
//!   1. `<project_dir>/docs/`        — the project's own docs
//!   2. `<project_dir>/.theo/wiki/`  — auto-generated Code Wiki
//!   3. `~/.cache/theo/docs/`        — global doc corpora (curated)
//!
//! Each location that exists is wrapped in a [`MarkdownDirSource`]
//! and its `list_all()` poured into the returned index. Locations
//! that don't exist are silently skipped — bootstrapping always
//! returns *some* index, even if empty.
//!
//! This is the wiring point that makes `docs_search` useful out of
//! the box: any project that ships markdown docs gets searchable
//! results without any manual setup.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::docs_search::index::DocsIndex;
use crate::docs_search::markdown_source::MarkdownDirSource;
use crate::docs_search::source::DocSource;

/// Build a [`DocsIndex`] populated from the project's well-known
/// Markdown locations. Returns an `Arc` ready for
/// [`DocsSearchTool::with_index`](crate::docs_search::DocsSearchTool::with_index).
///
/// Always succeeds; locations that don't exist contribute zero
/// documents. Performance: linear in total `.md` file count under
/// each scanned root.
pub fn bootstrap_docs_index(project_dir: &Path) -> Arc<DocsIndex> {
    Arc::new(populate_index(project_dir))
}

/// Plain (non-Arc) constructor — used inside tests so we can inspect
/// the index without going through `Arc::try_unwrap`.
pub fn populate_index(project_dir: &Path) -> DocsIndex {
    let mut idx = DocsIndex::new();
    for (label, root) in well_known_locations(project_dir) {
        if !root.exists() {
            continue;
        }
        let source = MarkdownDirSource::new(label, root);
        for doc in source.list_all() {
            idx.insert(doc);
        }
    }
    idx
}

/// Locations to scan, in priority order. The label becomes
/// `DocEntry.source` and is used by the tool's `source=` filter.
pub fn well_known_locations(project_dir: &Path) -> Vec<(&'static str, PathBuf)> {
    let mut out: Vec<(&'static str, PathBuf)> = Vec::new();
    out.push(("project-docs", project_dir.join("docs")));
    out.push(("project-wiki", project_dir.join(".theo").join("wiki")));
    if let Some(cache) = global_cache_dir() {
        out.push(("global-docs", cache));
    }
    out
}

/// Resolve `~/.cache/theo/docs/`. Returns `None` when `$HOME` is
/// unset (containerised CI runners sometimes wipe it).
fn global_cache_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let p = PathBuf::from(home).join(".cache").join("theo").join("docs");
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn write_md(dir: &Path, rel: &str, body: &str) {
        let abs = dir.join(rel);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = fs::File::create(&abs).unwrap();
        f.write_all(body.as_bytes()).unwrap();
    }

    #[test]
    fn t151bs_well_known_locations_lists_project_docs_and_wiki() {
        let project = PathBuf::from("/example/project");
        let locs = well_known_locations(&project);
        let labels: Vec<_> = locs.iter().map(|(l, _)| *l).collect();
        // project-docs and project-wiki are unconditional.
        assert!(labels.contains(&"project-docs"));
        assert!(labels.contains(&"project-wiki"));
        let docs_path = locs
            .iter()
            .find(|(l, _)| *l == "project-docs")
            .unwrap()
            .1
            .clone();
        assert_eq!(docs_path, project.join("docs"));
        let wiki_path = locs
            .iter()
            .find(|(l, _)| *l == "project-wiki")
            .unwrap()
            .1
            .clone();
        assert_eq!(wiki_path, project.join(".theo").join("wiki"));
    }

    #[test]
    fn t151bs_populate_with_empty_project_returns_empty_index() {
        let dir = tempfile::tempdir().unwrap();
        let idx = populate_index(dir.path());
        assert_eq!(idx.len(), 0);
    }

    #[test]
    fn t151bs_populate_picks_up_project_docs() {
        let dir = tempfile::tempdir().unwrap();
        write_md(dir.path(), "docs/intro.md", "# Intro\nWelcome");
        write_md(dir.path(), "docs/howto/run.md", "# Run\nrun guide");
        let idx = populate_index(dir.path());
        assert_eq!(idx.len(), 2);
        let hits = idx.search("welcome", Some("project-docs"), 10);
        assert!(!hits.is_empty(), "should find 'welcome' in intro.md");
        assert_eq!(hits[0].doc.source, "project-docs");
    }

    #[test]
    fn t151bs_populate_picks_up_theo_wiki() {
        let dir = tempfile::tempdir().unwrap();
        write_md(
            dir.path(),
            ".theo/wiki/modules/foo.md",
            "# Foo\nthe foo module",
        );
        let idx = populate_index(dir.path());
        assert_eq!(idx.len(), 1);
        let hits = idx.search("foo module", Some("project-wiki"), 10);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].doc.source, "project-wiki");
    }

    #[test]
    fn t151bs_populate_combines_multiple_sources() {
        let dir = tempfile::tempdir().unwrap();
        write_md(dir.path(), "docs/a.md", "# A\nfrom docs");
        write_md(dir.path(), ".theo/wiki/b.md", "# B\nfrom wiki");
        let idx = populate_index(dir.path());
        assert_eq!(idx.len(), 2);
        let docs_hits = idx.search("from docs", Some("project-docs"), 10);
        let wiki_hits = idx.search("from wiki", Some("project-wiki"), 10);
        assert!(!docs_hits.is_empty());
        assert!(!wiki_hits.is_empty());
        assert_eq!(docs_hits[0].doc.source, "project-docs");
        assert_eq!(wiki_hits[0].doc.source, "project-wiki");
    }

    #[test]
    fn t151bs_populate_skips_nonexistent_location() {
        // No docs/ or .theo/wiki/ exist — bootstrap should return
        // empty, not panic.
        let dir = tempfile::tempdir().unwrap();
        let idx = populate_index(dir.path());
        assert_eq!(idx.len(), 0);
    }

    #[test]
    fn t151bs_populate_ignores_non_md_files_in_docs() {
        let dir = tempfile::tempdir().unwrap();
        write_md(dir.path(), "docs/real.md", "# R");
        let txt = dir.path().join("docs").join("note.txt");
        std::fs::write(&txt, b"not markdown").unwrap();
        let idx = populate_index(dir.path());
        assert_eq!(idx.len(), 1);
    }

    #[test]
    fn t151bs_bootstrap_returns_arc_index() {
        let dir = tempfile::tempdir().unwrap();
        write_md(dir.path(), "docs/a.md", "# A");
        let idx_arc = bootstrap_docs_index(dir.path());
        // Arc lets us share the index across multiple tool clones.
        let cloned = idx_arc.clone();
        assert_eq!(idx_arc.len(), 1);
        assert_eq!(cloned.len(), 1);
        assert!(Arc::ptr_eq(&idx_arc, &cloned));
    }

    #[test]
    fn t151bs_global_cache_dir_uses_home() {
        // We can't deterministically test the actual returned path
        // without mutating HOME, but we can verify the function
        // returns Some when HOME is set (which it is in test env).
        if std::env::var_os("HOME").is_some() {
            let p = global_cache_dir().expect("HOME set in test env");
            assert!(p.ends_with(".cache/theo/docs"));
        }
    }

    #[test]
    fn t151bs_populate_does_not_index_hidden_dirs_under_project() {
        // Files under .git/ inside docs/ must not leak in.
        let dir = tempfile::tempdir().unwrap();
        write_md(dir.path(), "docs/visible.md", "# V");
        write_md(dir.path(), "docs/.git/secret.md", "# S");
        let idx = populate_index(dir.path());
        assert_eq!(idx.len(), 1);
    }
}
