//! Wiki lint — detect quality issues in the wiki.
//!
//! Lint rules catch problems that degrade the wiki's value:
//! - Broken cross-references (wikilinks to nonexistent pages)
//! - Orphan pages (no incoming links)
//! - Stale pages (source changed, enrichment outdated)
//! - Missing enrichment (skeleton-only pages)
//! - Empty sections in enrichment

use crate::page::WikiPage;
use serde::{Deserialize, Serialize};

/// Severity of a lint issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LintSeverity {
    /// Must fix — wiki is misleading or broken
    Error,
    /// Should fix — wiki is degraded
    Warning,
    /// Nice to fix — wiki could be better
    Info,
}

/// A single lint issue found in the wiki.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintIssue {
    pub severity: LintSeverity,
    pub rule: String,
    pub slug: String,
    pub message: String,
}

/// Run all lint rules on a set of pages.
pub fn lint_pages(pages: &[WikiPage]) -> Vec<LintIssue> {
    let mut issues = Vec::new();

    let slugs: std::collections::HashSet<_> = pages.iter().map(|p| p.slug.as_str()).collect();

    for page in pages {
        // Rule 1: Missing enrichment
        if page.enrichment.is_none() {
            issues.push(LintIssue {
                severity: LintSeverity::Warning,
                rule: "missing-enrichment".into(),
                slug: page.slug.clone(),
                message: "Page has skeleton but no LLM enrichment".into(),
            });
        }

        // Rule 2: Stale page (source changed since enrichment)
        if page.is_stale && page.enrichment.is_some() {
            issues.push(LintIssue {
                severity: LintSeverity::Warning,
                rule: "stale-enrichment".into(),
                slug: page.slug.clone(),
                message: "Source code changed since last enrichment".into(),
            });
        }

        // Rule 3: Empty enrichment sections
        if let Some(ref enrichment) = page.enrichment {
            if enrichment.what_it_does.trim().is_empty() {
                issues.push(LintIssue {
                    severity: LintSeverity::Error,
                    rule: "empty-what-it-does".into(),
                    slug: page.slug.clone(),
                    message: "'What it does' section is empty".into(),
                });
            }
            if enrichment.why_it_exists.trim().is_empty() {
                issues.push(LintIssue {
                    severity: LintSeverity::Warning,
                    rule: "empty-why-it-exists".into(),
                    slug: page.slug.clone(),
                    message: "'Why it exists' section is empty".into(),
                });
            }

            // Rule 4: Broken cross-references
            for ref_slug in &enrichment.see_also {
                if !slugs.contains(ref_slug.as_str()) {
                    issues.push(LintIssue {
                        severity: LintSeverity::Error,
                        rule: "broken-link".into(),
                        slug: page.slug.clone(),
                        message: format!("Cross-reference to nonexistent page: {ref_slug}"),
                    });
                }
            }
        }

        // Rule 5: No files (empty skeleton)
        if page.skeleton.files.is_empty() {
            issues.push(LintIssue {
                severity: LintSeverity::Info,
                rule: "empty-skeleton".into(),
                slug: page.slug.clone(),
                message: "Page covers no source files".into(),
            });
        }
    }

    // Rule 6: Orphan pages (no incoming cross-references)
    let referenced: std::collections::HashSet<_> = pages
        .iter()
        .filter_map(|p| p.enrichment.as_ref())
        .flat_map(|e| e.see_also.iter())
        .map(|s| s.as_str())
        .collect();

    for page in pages {
        if page.category() != "index" && !referenced.contains(page.slug.as_str()) {
            issues.push(LintIssue {
                severity: LintSeverity::Info,
                rule: "orphan-page".into(),
                slug: page.slug.clone(),
                message: "No other page links to this page".into(),
            });
        }
    }

    issues
}

impl WikiPage {
    /// Helper for lint: return category as string.
    pub fn category(&self) -> &str {
        match self.category {
            crate::page::PageCategory::Module => "module",
            crate::page::PageCategory::Decision => "decision",
            crate::page::PageCategory::Concept => "concept",
            crate::page::PageCategory::Index => "index",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::*;

    fn make_page(slug: &str, enriched: bool, stale: bool) -> WikiPage {
        let mut page = WikiPage::from_skeleton(
            slug.into(),
            slug.into(),
            PageCategory::Module,
            SkeletonData {
                files: vec!["src/lib.rs".into()],
                symbols: vec![],
                depends_on: vec![],
                depended_by: vec![],
                primary_language: "rust".into(),
                total_lines: 10,
            },
            "hash".into(),
        );
        if enriched {
            page.enrichment = Some(EnrichmentData {
                summary: "test".into(),
                what_it_does: "Does things".into(),
                why_it_exists: "Because".into(),
                how_it_works: "Magic".into(),
                what_breaks: "Everything".into(),
                decisions: vec![],
                see_also: vec![],
                enriched_by: "test".into(),
                enriched_at: "now".into(),
            });
            page.is_stale = stale;
        }
        page
    }

    #[test]
    fn test_lint_missing_enrichment() {
        let pages = vec![make_page("mod-a", false, false)];
        let issues = lint_pages(&pages);
        assert!(issues.iter().any(|i| i.rule == "missing-enrichment"));
    }

    #[test]
    fn test_lint_stale_enrichment() {
        let pages = vec![make_page("mod-a", true, true)];
        let issues = lint_pages(&pages);
        assert!(issues.iter().any(|i| i.rule == "stale-enrichment"));
    }

    #[test]
    fn test_lint_broken_link() {
        let mut page = make_page("mod-a", true, false);
        if let Some(ref mut e) = page.enrichment {
            e.see_also = vec!["nonexistent".into()];
        }
        let pages = vec![page];
        let issues = lint_pages(&pages);
        assert!(issues.iter().any(|i| i.rule == "broken-link"));
    }

    #[test]
    fn test_lint_clean_page() {
        let mut page_a = make_page("mod-a", true, false);
        let mut page_b = make_page("mod-b", true, false);
        if let Some(ref mut e) = page_a.enrichment {
            e.see_also = vec!["mod-b".into()];
        }
        if let Some(ref mut e) = page_b.enrichment {
            e.see_also = vec!["mod-a".into()];
        }
        let pages = vec![page_a, page_b];
        let issues = lint_pages(&pages);
        let errors: Vec<_> = issues.iter().filter(|i| i.severity == LintSeverity::Error).collect();
        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }
}
