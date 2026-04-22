//! Wiki disk persistence: write/read/cache invalidation.

use std::path::Path;

use super::model::*;
use super::renderer;

/// Write wiki to disk at `.theo/wiki/`.
pub fn write_to_disk(wiki: &Wiki, project_dir: &Path) -> std::io::Result<()> {
    let wiki_dir = project_dir.join(".theo").join("wiki");
    let modules_dir = wiki_dir.join("modules");

    std::fs::create_dir_all(&modules_dir)?;

    // Write index.md
    let project_name = project_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");
    let index_md = renderer::render_index(&wiki.docs, project_name);
    std::fs::write(wiki_dir.join("index.md"), index_md)?;

    // Clean up orphaned pages from previous runs
    let current_slugs: std::collections::HashSet<String> =
        wiki.docs.iter().map(|d| d.slug.clone()).collect();
    cleanup_orphaned_pages(&wiki_dir, &current_slugs);

    // Write each module page
    for doc in &wiki.docs {
        let page_md = renderer::render_page(doc);
        let filename = format!("{}.md", doc.slug);
        std::fs::write(modules_dir.join(&filename), page_md)?;
    }

    // Write manifest
    let manifest_json =
        serde_json::to_string_pretty(&wiki.manifest).unwrap_or_else(|_| "{}".to_string());
    std::fs::write(wiki_dir.join("wiki.manifest.json"), manifest_json)?;

    Ok(())
}

/// Check if cached wiki is still fresh (graph hasn't changed).
pub fn is_fresh(project_dir: &Path, current_hash: u64) -> bool {
    let manifest_path = project_dir
        .join(".theo")
        .join("wiki")
        .join("wiki.manifest.json");
    let Ok(content) = std::fs::read_to_string(&manifest_path) else {
        return false;
    };
    let Ok(manifest) = serde_json::from_str::<WikiManifest>(&content) else {
        return false;
    };

    // Check schema version compatibility
    if manifest.schema_version != WikiManifest::SCHEMA_VERSION {
        return false;
    }

    manifest.graph_hash == current_hash
}

/// Append an entry to the wiki log (chronological, append-only).
///
/// Format: `## [ISO8601] event_type | details`
/// Grep-friendly: `grep "^## \[" .theo/wiki/log.md | tail -5`
pub fn append_log(project_dir: &Path, event_type: &str, details: &str) {
    let log_path = project_dir.join(".theo").join("wiki").join("log.md");

    // Create header if file doesn't exist
    let needs_header = !log_path.exists();

    let mut file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(f) => f,
        Err(_) => return, // Best-effort
    };

    use std::io::Write;

    if needs_header {
        let _ = writeln!(file, "# Wiki Log\n");
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Simple ISO-like timestamp (no chrono dependency)
    let secs_per_day = 86400u64;
    let days = now / secs_per_day;
    let time_of_day = now % secs_per_day;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Approximate date (good enough for logging)
    let year = 1970 + (days / 365); // Approximate
    let day_of_year = days % 365;
    let month = day_of_year / 30 + 1;
    let day = day_of_year % 30 + 1;

    let _ = writeln!(
        file,
        "## [{:04}-{:02}-{:02}T{:02}:{:02}:{:02}] {} | {}",
        year, month, day, hours, minutes, seconds, event_type, details
    );
    let _ = writeln!(file);
}

/// Write a single wiki page to disk.
pub fn write_page(doc: &WikiDoc, wiki_dir: &std::path::Path) -> std::io::Result<()> {
    let modules_dir = wiki_dir.join("modules");
    std::fs::create_dir_all(&modules_dir)?;
    let page_md = renderer::render_page(doc);
    std::fs::write(modules_dir.join(format!("{}.md", doc.slug)), page_md)
}

/// Remove orphaned pages (modules on disk that are no longer in the wiki).
pub fn cleanup_orphaned_pages(
    wiki_dir: &std::path::Path,
    current_slugs: &std::collections::HashSet<String>,
) -> usize {
    let modules_dir = wiki_dir.join("modules");
    let mut removed = 0;
    if let Ok(entries) = std::fs::read_dir(&modules_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let slug = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if !current_slugs.contains(&slug)
                && std::fs::remove_file(&path).is_ok() {
                    removed += 1;
                }
        }
    }
    removed
}

/// Mark stale cache pages by moving them to `cache/stale/`.
///
/// A page is stale if its frontmatter `graph_hash` differs from `current_graph_hash`.
/// Returns the number of pages moved.
pub fn mark_stale_cache(wiki_dir: &std::path::Path, current_graph_hash: u64) -> usize {
    let cache_dir = wiki_dir.join("cache");
    if !cache_dir.exists() {
        return 0;
    }

    let stale_dir = cache_dir.join("stale");
    let mut moved = 0;

    if let Ok(entries) = std::fs::read_dir(&cache_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                continue;
            } // Skip stale/ and cold/ subdirs
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }

            if let Ok(content) = std::fs::read_to_string(&path) {
                let fm = super::model::parse_frontmatter(&content);
                if let Some(page_hash) = fm.graph_hash
                    && page_hash != current_graph_hash {
                        // Move to stale/
                        if std::fs::create_dir_all(&stale_dir).is_ok() {
                            let dest = stale_dir.join(path.file_name().unwrap_or_default());
                            if std::fs::rename(&path, &dest).is_ok() {
                                moved += 1;
                            }
                        }
                    }
            }
        }
    }
    moved
}

/// Garbage-collect cold cache pages (stale > 7 days).
///
/// Removes files from `cache/stale/` whose `generated_at` timestamp is older
/// than `max_age_secs` (default: 7 days = 604800 seconds).
/// Returns the number of pages removed.
pub fn gc_cold_cache(wiki_dir: &std::path::Path, max_age_secs: u64) -> usize {
    let stale_dir = wiki_dir.join("cache").join("stale");
    if !stale_dir.exists() {
        return 0;
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut removed = 0;
    if let Ok(entries) = std::fs::read_dir(&stale_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }

            if let Ok(content) = std::fs::read_to_string(&path) {
                let fm = super::model::parse_frontmatter(&content);
                if let Some(ref gen_at) = fm.generated_at
                    && let Ok(ts) = gen_at.parse::<u64>()
                        && now.saturating_sub(ts) > max_age_secs
                            && std::fs::remove_file(&path).is_ok() {
                                removed += 1;
                            }
            }
        }
    }
    removed
}

/// Load wiki schema from `.theo/wiki/wiki.schema.toml`.
/// Returns default schema if file doesn't exist or is invalid.
pub fn load_schema(project_dir: &Path, project_name: &str) -> WikiSchema {
    let path = project_dir
        .join(".theo")
        .join("wiki")
        .join("wiki.schema.toml");
    match std::fs::read_to_string(&path) {
        Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
            eprintln!("[wiki] Invalid schema TOML, using defaults: {}", e);
            WikiSchema::default_for(project_name)
        }),
        Err(_) => WikiSchema::default_for(project_name),
    }
}

/// Write default schema to disk if it doesn't exist yet.
pub fn write_schema_default(project_dir: &Path, schema: &WikiSchema) -> std::io::Result<()> {
    let wiki_dir = project_dir.join(".theo").join("wiki");
    std::fs::create_dir_all(&wiki_dir)?;
    let path = wiki_dir.join("wiki.schema.toml");
    if path.exists() {
        return Ok(()); // Don't overwrite user edits
    }
    let content = toml::to_string_pretty(schema)
        .unwrap_or_else(|_| String::from("# Failed to serialize schema"));
    std::fs::write(path, content)
}

/// Load manifest from disk.
pub fn load_manifest(project_dir: &Path) -> Option<WikiManifest> {
    let path = project_dir
        .join(".theo")
        .join("wiki")
        .join("wiki.manifest.json");
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_wiki() -> Wiki {
        Wiki {
            docs: vec![WikiDoc {
                slug: "test-module".into(),
                title: "Test Module".into(),
                community_id: "c1".into(),
                file_count: 3,
                symbol_count: 10,
                primary_language: "rs".into(),
                files: vec![],
                entry_points: vec![],
                public_api: vec![],
                dependencies: vec![],
                call_flow: vec![],
                test_coverage: TestCoverage {
                    tested: 1,
                    total: 2,
                    percentage: 50.0,
                    untested: vec!["foo".into()],
                },
                source_refs: vec![SourceRef::file("test.rs")],
                summary: String::new(),
                tags: vec![],
                crate_description: None,
                module_doc: None,
                generated_at: "0".into(),
                enriched: false,
            }],
            manifest: WikiManifest {
                schema_version: WikiManifest::SCHEMA_VERSION,
                generator_version: WikiManifest::GENERATOR_VERSION.to_string(),
                graph_hash: 12345,
                generated_at: "0".into(),
                page_count: 1,
                page_hashes: std::collections::HashMap::new(),
            },
        }
    }

    #[test]
    fn write_and_check_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let wiki = sample_wiki();

        write_to_disk(&wiki, dir.path()).unwrap();

        // Check files exist
        assert!(dir.path().join(".theo/wiki/index.md").exists());
        assert!(
            dir.path()
                .join(".theo/wiki/modules/test-module.md")
                .exists()
        );
        assert!(dir.path().join(".theo/wiki/wiki.manifest.json").exists());

        // Fresh with same hash
        assert!(is_fresh(dir.path(), 12345));

        // Stale with different hash
        assert!(!is_fresh(dir.path(), 99999));
    }

    #[test]
    fn not_fresh_when_no_cache() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_fresh(dir.path(), 12345));
    }

    #[test]
    fn load_manifest_works() {
        let dir = tempfile::tempdir().unwrap();
        let wiki = sample_wiki();
        write_to_disk(&wiki, dir.path()).unwrap();

        let manifest = load_manifest(dir.path()).unwrap();
        assert_eq!(manifest.graph_hash, 12345);
        assert_eq!(manifest.page_count, 1);
    }

    #[test]
    fn index_contains_module_links() {
        let dir = tempfile::tempdir().unwrap();
        let wiki = sample_wiki();
        write_to_disk(&wiki, dir.path()).unwrap();

        let index = std::fs::read_to_string(dir.path().join(".theo/wiki/index.md")).unwrap();
        // Module should appear somewhere in the index (flat or hierarchical)
        assert!(
            index.contains("test-module") || index.contains("Test Module"),
            "index should reference the module: {}",
            &index[..index.len().min(500)]
        );
    }

    #[test]
    fn load_schema_returns_default_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let schema = load_schema(dir.path(), "test-proj");
        assert_eq!(schema.project.name, "test-proj");
        assert_eq!(schema.groups.len(), 8);
    }

    #[test]
    fn write_and_load_schema() {
        let dir = tempfile::tempdir().unwrap();
        let schema = WikiSchema::default_for("my-project");
        write_schema_default(dir.path(), &schema).unwrap();

        let path = dir.path().join(".theo/wiki/wiki.schema.toml");
        assert!(path.exists());

        let loaded = load_schema(dir.path(), "fallback");
        assert_eq!(loaded.project.name, "my-project"); // reads from disk, not fallback
        assert_eq!(loaded.groups.len(), 8);
    }

    #[test]
    fn write_schema_does_not_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let schema1 = WikiSchema::default_for("first");
        write_schema_default(dir.path(), &schema1).unwrap();

        let schema2 = WikiSchema::default_for("second");
        write_schema_default(dir.path(), &schema2).unwrap();

        let loaded = load_schema(dir.path(), "fallback");
        assert_eq!(loaded.project.name, "first"); // not overwritten
    }

    #[test]
    fn mark_stale_cache_moves_stale_pages() {
        let dir = tempfile::tempdir().unwrap();
        let wiki_dir = dir.path().join(".theo").join("wiki");
        let cache_dir = wiki_dir.join("cache");
        std::fs::create_dir_all(&cache_dir).unwrap();

        // Write 3 cache pages: 2 stale (hash 111), 1 fresh (hash 999)
        std::fs::write(
            cache_dir.join("stale1.md"),
            "---\ngraph_hash: 111\ngenerated_at: \"100\"\n---\n# Stale 1\n",
        )
        .unwrap();
        std::fs::write(
            cache_dir.join("stale2.md"),
            "---\ngraph_hash: 111\ngenerated_at: \"100\"\n---\n# Stale 2\n",
        )
        .unwrap();
        std::fs::write(
            cache_dir.join("fresh.md"),
            "---\ngraph_hash: 999\ngenerated_at: \"100\"\n---\n# Fresh\n",
        )
        .unwrap();

        let moved = mark_stale_cache(&wiki_dir, 999);
        assert_eq!(moved, 2);
        assert!(wiki_dir.join("cache/stale/stale1.md").exists());
        assert!(wiki_dir.join("cache/stale/stale2.md").exists());
        assert!(wiki_dir.join("cache/fresh.md").exists());
    }

    #[test]
    fn gc_cold_cache_removes_old_stale() {
        let dir = tempfile::tempdir().unwrap();
        let wiki_dir = dir.path().join(".theo").join("wiki");
        let stale_dir = wiki_dir.join("cache").join("stale");
        std::fs::create_dir_all(&stale_dir).unwrap();

        // Old page (generated_at = 100, very old)
        std::fs::write(
            stale_dir.join("old.md"),
            "---\ngenerated_at: \"100\"\n---\n# Old\n",
        )
        .unwrap();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Recent page (generated_at = now)
        std::fs::write(
            stale_dir.join("recent.md"),
            format!("---\ngenerated_at: \"{}\"\n---\n# Recent\n", now),
        )
        .unwrap();

        let removed = gc_cold_cache(&wiki_dir, 604800); // 7 days
        assert_eq!(removed, 1);
        assert!(!stale_dir.join("old.md").exists());
        assert!(stale_dir.join("recent.md").exists());
    }
}
