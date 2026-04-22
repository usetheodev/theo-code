//! Wiki lint: health check for wiki quality.
//!
//! Detects: orphan pages, broken links, stale pages, large pages, empty sections.

use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Lint report with detected issues.
#[derive(Debug, Default)]
pub struct LintReport {
    /// Pages with no inbound [[wiki-links]] from other pages.
    pub orphan_pages: Vec<String>,
    /// (source_page, broken_target) — [[links]] pointing to non-existent pages.
    pub broken_links: Vec<(String, String)>,
    /// Pages exceeding token estimate threshold.
    pub large_pages: Vec<(String, usize)>,
    /// (page, section_name) — ## headers followed immediately by another ## header.
    pub empty_sections: Vec<(String, String)>,
    /// Cache pages with stale graph_hash (different from current manifest).
    pub stale_cache_pages: Vec<String>,
    /// Cache pages recommended for eviction (stale + in stale/ dir).
    pub eviction_candidates: Vec<String>,
    /// Pairs of cache pages with high content overlap (potential duplicates).
    pub duplicate_candidates: Vec<(String, String)>,
    /// Total pages scanned.
    pub total_pages: usize,
    /// Total issues found.
    pub total_issues: usize,
}

impl std::fmt::Display for LintReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Wiki Lint: {} pages scanned, {} issues found",
            self.total_pages, self.total_issues
        )?;
        if !self.orphan_pages.is_empty() {
            writeln!(
                f,
                "  Orphan pages (no inbound links): {}",
                self.orphan_pages.len()
            )?;
            for p in &self.orphan_pages {
                writeln!(f, "    - {}", p)?;
            }
        }
        if !self.broken_links.is_empty() {
            writeln!(f, "  Broken links: {}", self.broken_links.len())?;
            for (src, tgt) in &self.broken_links {
                writeln!(f, "    - {} → [[{}]]", src, tgt)?;
            }
        }
        if !self.large_pages.is_empty() {
            writeln!(
                f,
                "  Large pages (>5000 tokens): {}",
                self.large_pages.len()
            )?;
            for (p, t) in &self.large_pages {
                writeln!(f, "    - {} ({} tokens)", p, t)?;
            }
        }
        if !self.empty_sections.is_empty() {
            writeln!(f, "  Empty sections: {}", self.empty_sections.len())?;
            for (p, s) in &self.empty_sections {
                writeln!(f, "    - {}::{}", p, s)?;
            }
        }
        if !self.stale_cache_pages.is_empty() {
            writeln!(f, "  Stale cache pages: {}", self.stale_cache_pages.len())?;
            for p in &self.stale_cache_pages {
                writeln!(f, "    - {}", p)?;
            }
        }
        if !self.eviction_candidates.is_empty() {
            writeln!(
                f,
                "  Eviction candidates: {}",
                self.eviction_candidates.len()
            )?;
            for p in &self.eviction_candidates {
                writeln!(f, "    - {}", p)?;
            }
        }
        if !self.duplicate_candidates.is_empty() {
            writeln!(
                f,
                "  Duplicate candidates: {}",
                self.duplicate_candidates.len()
            )?;
            for (a, b) in &self.duplicate_candidates {
                writeln!(f, "    - {} ≈ {}", a, b)?;
            }
        }
        Ok(())
    }
}

const DEFAULT_LARGE_PAGE_THRESHOLD: usize = 5000; // tokens (estimated as chars/4)

/// Run lint on a wiki directory. Returns report with all detected issues.
pub fn lint(wiki_dir: &Path) -> LintReport {
    lint_with_threshold(wiki_dir, DEFAULT_LARGE_PAGE_THRESHOLD)
}

/// Run lint with configurable large-page token threshold.
pub fn lint_with_threshold(wiki_dir: &Path, large_page_threshold: usize) -> LintReport {
    let mut report = LintReport::default();

    let modules_dir = wiki_dir.join("modules");
    let cache_dir = wiki_dir.join("cache");

    // Collect all pages: slug → content
    let mut pages: HashMap<String, String> = HashMap::new();

    for dir in [&modules_dir, &cache_dir] {
        if !dir.exists() {
            continue;
        }
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            if path.to_string_lossy().contains(".enriched.") {
                continue;
            }
            let slug = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if let Ok(content) = std::fs::read_to_string(&path) {
                pages.insert(slug, content);
            }
        }
    }

    report.total_pages = pages.len();

    // Extract all [[wiki-links]] per page
    let mut outbound_links: HashMap<String, Vec<String>> = HashMap::new();
    let all_slugs: HashSet<String> = pages.keys().cloned().collect();

    for (slug, content) in &pages {
        let mut links = Vec::new();
        let mut search_from = 0;
        while let Some(start) = content[search_from..].find("[[") {
            let abs_start = search_from + start + 2;
            if let Some(end) = content[abs_start..].find("]]") {
                let inner = &content[abs_start..abs_start + end];
                let target = inner.split('|').next().unwrap_or(inner).trim().to_string();
                links.push(target);
            }
            search_from = abs_start;
        }
        outbound_links.insert(slug.clone(), links);
    }

    // Detect orphan pages (no inbound links)
    let mut has_inbound: HashSet<String> = HashSet::new();
    for links in outbound_links.values() {
        for link in links {
            has_inbound.insert(link.clone());
        }
    }
    for slug in &all_slugs {
        if slug == "index" {
            continue;
        } // Index is always root
        if !has_inbound.contains(slug) {
            report.orphan_pages.push(slug.clone());
        }
    }
    report.orphan_pages.sort();

    // Detect broken links
    for (source, links) in &outbound_links {
        for target in links {
            if !all_slugs.contains(target) {
                report.broken_links.push((source.clone(), target.clone()));
            }
        }
    }

    // Detect large pages
    for (slug, content) in &pages {
        let token_estimate = content.len() / 4;
        if token_estimate > large_page_threshold {
            report.large_pages.push((slug.clone(), token_estimate));
        }
    }
    report.large_pages.sort_by_key(|item| std::cmp::Reverse(item.1));

    // Detect empty sections (## followed by ## with no content)
    for (slug, content) in &pages {
        let lines: Vec<&str> = content.lines().collect();
        for i in 0..lines.len().saturating_sub(1) {
            if lines[i].starts_with("## ") {
                // Check if next non-empty line is also a heading
                let mut j = i + 1;
                while j < lines.len() && lines[j].trim().is_empty() {
                    j += 1;
                }
                if j < lines.len()
                    && (lines[j].starts_with("## ")
                        || lines[j].starts_with("# ")
                        || lines[j].starts_with("---"))
                {
                    let section = lines[i].trim_start_matches("## ").trim().to_string();
                    report.empty_sections.push((slug.clone(), section));
                }
            }
        }
    }

    // Detect stale cache pages
    // wiki_dir = .theo/wiki, project_dir = wiki_dir/../../
    let project_dir = wiki_dir.parent().and_then(|p| p.parent());
    let manifest_hash = project_dir
        .and_then(super::persistence::load_manifest)
        .map(|m| m.graph_hash);

    if let Some(current_hash) = manifest_hash {
        let cache_dir = wiki_dir.join("cache");
        if cache_dir.exists()
            && let Ok(entries) = std::fs::read_dir(&cache_dir) {
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
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let fm = super::model::parse_frontmatter(&content);
                        if let Some(page_hash) = fm.graph_hash
                            && page_hash != current_hash {
                                report.stale_cache_pages.push(slug.clone());
                                report.eviction_candidates.push(slug);
                            }
                    }
                }
            }
    }

    report.total_issues = report.orphan_pages.len()
        + report.broken_links.len()
        + report.large_pages.len()
        + report.empty_sections.len()
        + report.stale_cache_pages.len()
        + report.duplicate_candidates.len();

    report
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn create_test_wiki(dir: &Path) {
        let modules = dir.join("modules");
        fs::create_dir_all(&modules).unwrap();

        // Page A links to B
        fs::write(
            modules.join("page-a.md"),
            "# Page A\n\nSee [[page-b]] for details.\n\n## Section\n\nContent here.",
        )
        .unwrap();

        // Page B links to A (mutual)
        fs::write(
            modules.join("page-b.md"),
            "# Page B\n\nRelated: [[page-a]]\n",
        )
        .unwrap();

        // Orphan page C (no inbound links)
        fs::write(
            modules.join("page-c.md"),
            "# Page C\n\nOrphan page, nobody links here.\n",
        )
        .unwrap();

        // Page D with broken link
        fs::write(
            modules.join("page-d.md"),
            "# Page D\n\nSee [[nonexistent-page]]\n",
        )
        .unwrap();

        // Large page
        let big_content = "# Big Page\n\n".to_string() + &"x".repeat(25000);
        fs::write(modules.join("big-page.md"), big_content).unwrap();

        // Page with empty section
        fs::write(modules.join("empty-sec.md"), "# Empty\n\n## Has Content\n\nSome text.\n\n## Empty Section\n\n## Next Section\n\nMore text.").unwrap();
    }

    #[test]
    fn lint_detects_orphan() {
        let dir = tempfile::tempdir().unwrap();
        create_test_wiki(dir.path());
        let report = lint(dir.path());
        assert!(report.orphan_pages.contains(&"page-c".to_string()));
    }

    #[test]
    fn lint_detects_broken_link() {
        let dir = tempfile::tempdir().unwrap();
        create_test_wiki(dir.path());
        let report = lint(dir.path());
        assert!(
            report
                .broken_links
                .iter()
                .any(|(_, t)| t == "nonexistent-page")
        );
    }

    #[test]
    fn lint_detects_large_page() {
        let dir = tempfile::tempdir().unwrap();
        create_test_wiki(dir.path());
        let report = lint(dir.path());
        assert!(report.large_pages.iter().any(|(s, _)| s == "big-page"));
    }

    #[test]
    fn lint_detects_empty_section() {
        let dir = tempfile::tempdir().unwrap();
        create_test_wiki(dir.path());
        let report = lint(dir.path());
        assert!(
            report
                .empty_sections
                .iter()
                .any(|(_, s)| s == "Empty Section")
        );
    }

    #[test]
    fn lint_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let report = lint(dir.path());
        assert_eq!(report.total_pages, 0);
        assert_eq!(report.total_issues, 0);
    }

    #[test]
    fn lint_report_display() {
        let dir = tempfile::tempdir().unwrap();
        create_test_wiki(dir.path());
        let report = lint(dir.path());
        let display = format!("{}", report);
        assert!(display.contains("issues found"));
        assert!(display.contains("Orphan"));
    }

    #[test]
    fn lint_custom_threshold() {
        let dir = tempfile::tempdir().unwrap();
        create_test_wiki(dir.path());

        // With default threshold (5000), big-page (25000 chars / 4 = 6250 tokens) is large
        let report = lint(dir.path());
        assert!(report.large_pages.iter().any(|(s, _)| s == "big-page"));

        // With very high threshold, big-page is NOT large
        let report2 = lint_with_threshold(dir.path(), 100_000);
        assert!(!report2.large_pages.iter().any(|(s, _)| s == "big-page"));
    }
}
