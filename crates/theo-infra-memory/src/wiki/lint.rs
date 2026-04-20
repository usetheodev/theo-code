//! Memory wiki linter: schema + broken-link + cross-namespace check.
//!
//! Zero LLM. Pure text processing over the frontmatter + body.
//! Plan: `outputs/agent-memory-plan.md` §RM5a ACs 3..6.

use std::collections::HashSet;

use theo_domain::memory::{MemoryWikiLintError, MemoryWikiPage};

/// Parse a raw markdown file with YAML frontmatter into a `MemoryWikiPage`.
/// Extracts `slug`, `namespace`, `title` from frontmatter and `[[link]]`
/// tokens from the body (both plain and `ns:slug` variants).
///
/// The parser is intentionally minimal — full YAML support is not needed;
/// only three keys are honored.
pub fn parse_page(slug_hint: &str, raw: &str) -> MemoryWikiPage {
    let mut slug = slug_hint.to_string();
    let mut namespace = String::new();
    let mut title = String::new();
    let mut body_start = 0usize;

    // Frontmatter delimited by `---` lines at the top.
    if let Some(rest) = raw.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            let fm = &rest[..end];
            for line in fm.lines() {
                let line = line.trim();
                if let Some(v) = line.strip_prefix("slug:") {
                    slug = v.trim().trim_matches('"').to_string();
                } else if let Some(v) = line.strip_prefix("namespace:") {
                    namespace = v.trim().trim_matches('"').to_string();
                } else if let Some(v) = line.strip_prefix("title:") {
                    title = v.trim().trim_matches('"').to_string();
                }
            }
            // `---\n<fm>\n---` — skip past the closing fence.
            body_start = 4 + end + "\n---".len();
            // Eat leading newline after the closing fence if present.
            if raw.as_bytes().get(body_start) == Some(&b'\n') {
                body_start += 1;
            }
        }
    }

    let body = raw[body_start..].to_string();
    let links = extract_links(&body);
    MemoryWikiPage {
        slug,
        namespace,
        title,
        body,
        links,
    }
}

/// Extract every `[[...]]` shortcode. Preserves `ns:slug` form so lint
/// can route cross-namespace links.
pub fn extract_links(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            let start = i + 2;
            if let Some(rel) = body[start..].find("]]") {
                let inner = &body[start..start + rel];
                if !inner.is_empty() {
                    out.push(inner.to_string());
                }
                i = start + rel + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Lint a full page set. Every page is assumed to live in the memory
/// namespace unless its frontmatter says otherwise. `code_slugs` is the
/// set of slugs known to the sibling code wiki — callers pass an empty
/// set if only the memory wiki is mounted.
pub fn lint_pages(
    pages: &[MemoryWikiPage],
    code_slugs: &HashSet<String>,
) -> Vec<MemoryWikiLintError> {
    let mut errors = Vec::new();
    let mut memory_slugs: HashSet<String> = HashSet::new();

    for page in pages {
        if page.namespace.is_empty() {
            errors.push(MemoryWikiLintError::MissingNamespace {
                slug: page.slug.clone(),
            });
        }
        if !is_valid_slug(&page.slug) {
            errors.push(MemoryWikiLintError::InvalidSlug {
                slug: page.slug.clone(),
            });
        }
        if !memory_slugs.insert(page.slug.clone()) {
            errors.push(MemoryWikiLintError::DuplicateSlug {
                slug: page.slug.clone(),
            });
        }
    }

    // Second pass: link resolution needs the full slug set.
    for page in pages {
        for link in &page.links {
            let resolved = if let Some((ns, slug)) = link.split_once(':') {
                match ns {
                    "memory" => memory_slugs.contains(slug),
                    "code" => code_slugs.contains(slug),
                    _ => false,
                }
            } else {
                memory_slugs.contains(link)
            };
            if !resolved {
                errors.push(MemoryWikiLintError::BrokenLink {
                    from: page.slug.clone(),
                    to: link.clone(),
                });
            }
        }
    }

    errors
}

fn is_valid_slug(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let first = s.as_bytes()[0];
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return false;
    }
    s.as_bytes().iter().all(|&b| {
        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_'
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(slug: &str, ns: &str, body: &str) -> MemoryWikiPage {
        MemoryWikiPage {
            slug: slug.into(),
            namespace: ns.into(),
            title: slug.into(),
            body: body.into(),
            links: extract_links(body),
        }
    }

    // ── RM5a-AC-3 ───────────────────────────────────────────────
    #[test]
    fn test_rm5a_ac_3_lint_rejects_page_without_namespace() {
        let p = page("orphan", "", "hello");
        let errs = lint_pages(&[p], &HashSet::new());
        assert!(errs.iter().any(|e| matches!(
            e,
            MemoryWikiLintError::MissingNamespace { slug } if slug == "orphan"
        )));
    }

    // ── RM5a-AC-4 ───────────────────────────────────────────────
    #[test]
    fn test_rm5a_ac_4_lint_detects_broken_link() {
        let p = page("home", "memory", "see [[nonexistent]] please");
        let errs = lint_pages(&[p], &HashSet::new());
        assert!(errs.iter().any(|e| matches!(
            e,
            MemoryWikiLintError::BrokenLink { to, .. } if to == "nonexistent"
        )));
    }

    // ── RM5a-AC-5 ───────────────────────────────────────────────
    #[test]
    fn test_rm5a_ac_5_lint_cross_namespace_link_resolves() {
        let p = page("index", "memory", "[[code:theo-domain]] is reachable");
        let mut code = HashSet::new();
        code.insert("theo-domain".to_string());
        let errs = lint_pages(&[p], &code);
        assert!(
            errs.is_empty(),
            "cross-namespace link to existing code page must be OK, got {errs:?}"
        );
    }

    // ── RM5a-AC-6 ───────────────────────────────────────────────
    #[test]
    fn test_rm5a_ac_6_memory_wiki_mount_isolated_from_code_wiki() {
        // A memory-namespaced link to a code-namespaced slug that only
        // exists in the code mount must NOT resolve as a local (memory)
        // link — the two mounts are physically separate.
        let p = page("index", "memory", "[[theo-domain]] is local?");
        let mut code = HashSet::new();
        code.insert("theo-domain".to_string());
        let errs = lint_pages(&[p], &code);
        // Plain [[theo-domain]] (no ns prefix) resolves against memory
        // slugs only; the code mount is not consulted.
        assert!(
            errs.iter().any(|e| matches!(
                e,
                MemoryWikiLintError::BrokenLink { to, .. } if to == "theo-domain"
            )),
            "plain link must not leak into code namespace"
        );
    }

    #[test]
    fn lint_detects_duplicate_slug() {
        let a = page("same", "memory", "");
        let b = page("same", "memory", "");
        let errs = lint_pages(&[a, b], &HashSet::new());
        assert!(errs
            .iter()
            .any(|e| matches!(e, MemoryWikiLintError::DuplicateSlug { .. })));
    }

    #[test]
    fn lint_detects_invalid_slug() {
        let p = page("Bad Slug!", "memory", "");
        let errs = lint_pages(&[p], &HashSet::new());
        assert!(errs
            .iter()
            .any(|e| matches!(e, MemoryWikiLintError::InvalidSlug { .. })));
    }

    #[test]
    fn extract_links_parses_both_plain_and_cross_namespace() {
        let links = extract_links("see [[a]] and [[code:b]] and [[memory:c]]");
        assert_eq!(links, vec!["a", "code:b", "memory:c"]);
    }

    #[test]
    fn parse_page_extracts_frontmatter_and_body() {
        let raw = "---\nslug: hello\nnamespace: memory\ntitle: Hello\n---\nbody text [[a]]";
        let p = parse_page("fallback", raw);
        assert_eq!(p.slug, "hello");
        assert_eq!(p.namespace, "memory");
        assert_eq!(p.title, "Hello");
        assert!(p.body.contains("body text"));
        assert_eq!(p.links, vec!["a"]);
    }

    #[test]
    fn parse_page_without_frontmatter_uses_fallback_slug() {
        let p = parse_page("fallback", "no frontmatter here");
        assert_eq!(p.slug, "fallback");
        assert_eq!(p.namespace, "");
        assert_eq!(p.body, "no frontmatter here");
    }
}
