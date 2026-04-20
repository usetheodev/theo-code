//! Integration-level coverage for the memory-wiki lint pipeline.
//! Plan: `outputs/agent-memory-plan.md` §RM5a ACs 3..6.

use std::collections::HashSet;

use theo_domain::memory::{MemoryWikiLintError, MemoryWikiPage};
use theo_infra_memory::wiki::{lint_pages, parse_page};

fn code_slugs(slugs: &[&str]) -> HashSet<String> {
    slugs.iter().map(|s| s.to_string()).collect()
}

/// Canonical healthy wiki: two pages with bidirectional links + cross-ns.
#[test]
fn healthy_wiki_has_no_lint_errors() {
    let a = parse_page(
        "index",
        "---\nslug: index\nnamespace: memory\ntitle: Index\n---\nsee [[lessons]] and [[code:theo-domain]]",
    );
    let b = parse_page(
        "lessons",
        "---\nslug: lessons\nnamespace: memory\ntitle: Lessons\n---\nback to [[index]]",
    );
    let errs = lint_pages(&[a, b], &code_slugs(&["theo-domain"]));
    assert!(errs.is_empty(), "unexpected errors: {errs:?}");
}

/// Combination: missing-namespace + broken-link in the same run.
#[test]
fn lint_reports_multiple_errors_in_one_pass() {
    let a = parse_page(
        "orphan",
        "no frontmatter here, just [[nowhere]]",
    );
    let errs = lint_pages(&[a], &HashSet::new());

    assert!(errs
        .iter()
        .any(|e| matches!(e, MemoryWikiLintError::MissingNamespace { .. })));
    assert!(errs
        .iter()
        .any(|e| matches!(e, MemoryWikiLintError::BrokenLink { to, .. } if to == "nowhere")));
}

/// Physical separation: a memory page cannot reach a code slug via
/// a plain `[[slug]]` link — it must qualify with `code:`.
#[test]
fn mounts_stay_isolated_without_explicit_namespace() {
    let page = MemoryWikiPage {
        slug: "root".into(),
        namespace: "memory".into(),
        title: "Root".into(),
        body: "unqualified [[theo-domain]]".into(),
        links: vec!["theo-domain".into()],
    };
    let errs = lint_pages(&[page], &code_slugs(&["theo-domain"]));
    assert!(
        errs.iter().any(|e| matches!(
            e,
            MemoryWikiLintError::BrokenLink { to, .. } if to == "theo-domain"
        )),
        "unqualified link must not fall through to code mount"
    );
}
