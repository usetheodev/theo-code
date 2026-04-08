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

    // Write each module page
    for doc in &wiki.docs {
        let page_md = renderer::render_page(doc);
        let filename = format!("{}.md", doc.slug);
        std::fs::write(modules_dir.join(&filename), page_md)?;
    }

    // Write manifest
    let manifest_json = serde_json::to_string_pretty(&wiki.manifest)
        .unwrap_or_else(|_| "{}".to_string());
    std::fs::write(wiki_dir.join("wiki.manifest.json"), manifest_json)?;

    Ok(())
}

/// Check if cached wiki is still fresh (graph hasn't changed).
pub fn is_fresh(project_dir: &Path, current_hash: u64) -> bool {
    let manifest_path = project_dir.join(".theo").join("wiki").join("wiki.manifest.json");
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

/// Load manifest from disk.
pub fn load_manifest(project_dir: &Path) -> Option<WikiManifest> {
    let path = project_dir.join(".theo").join("wiki").join("wiki.manifest.json");
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
                    tested: 1, total: 2, percentage: 50.0, untested: vec!["foo".into()],
                },
                source_refs: vec![SourceRef::file("test.rs")],
                generated_at: "0".into(),
                enriched: false,
            }],
            manifest: WikiManifest {
                schema_version: WikiManifest::SCHEMA_VERSION,
                generator_version: WikiManifest::GENERATOR_VERSION.to_string(),
                graph_hash: 12345,
                generated_at: "0".into(),
                page_count: 1,
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
        assert!(dir.path().join(".theo/wiki/modules/test-module.md").exists());
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
        assert!(index.contains("test-module") || index.contains("Test Module"),
            "index should reference the module: {}", &index[..index.len().min(500)]);
    }
}
