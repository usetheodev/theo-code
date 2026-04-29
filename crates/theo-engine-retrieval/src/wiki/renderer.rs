//! Renders WikiDoc IR into Obsidian-compatible markdown.

use super::model::*;

/// Render a single wiki page to markdown with canonical frontmatter.
pub fn render_page(doc: &WikiDoc) -> String {
    let mut md = String::with_capacity(4096);
    let fm = PageFrontmatter::module(doc.enriched, &doc.summary, &doc.tags);
    md += &render_frontmatter(&fm);
    render_page_header(&mut md, doc);
    render_module_doc(&mut md, doc);
    render_entry_points(&mut md, doc);
    render_public_api(&mut md, doc);
    render_files_section(&mut md, doc);
    render_dependencies_section(&mut md, doc);
    render_call_flow_section(&mut md, doc);
    render_test_coverage_section(&mut md, doc);
    render_footer(&mut md, doc);
    md
}

/// Karpathy header pattern: title + summary + tags + facts.
fn render_page_header(md: &mut String, doc: &WikiDoc) {
    md.push_str(&format!("# {}\n\n", doc.title));
    if !doc.summary.is_empty() {
        md.push_str(&format!("**Summary**: {}\n\n", doc.summary));
    }
    if !doc.tags.is_empty() {
        let tag_str: String = doc
            .tags
            .iter()
            .map(|t| format!("#{}", t))
            .collect::<Vec<_>>()
            .join(" ");
        md.push_str(&format!("**Tags**: {}\n\n", tag_str));
    }
    md.push_str(&format!(
        "> {} files | {} | {} symbols\n\n",
        doc.file_count, doc.primary_language, doc.symbol_count
    ));
}

/// Module-level doc (from `//!` in lib.rs/mod.rs — author's own words).
fn render_module_doc(md: &mut String, doc: &WikiDoc) {
    if let Some(ref module_doc) = doc.module_doc {
        md.push_str("## Overview\n\n");
        md.push_str(module_doc);
        md.push_str("\n\n");
    }
}

fn render_entry_points(md: &mut String, doc: &WikiDoc) {
    if doc.entry_points.is_empty() {
        return;
    }
    md.push_str("## Entry Points\n\n");
    for api in &doc.entry_points {
        md.push_str(&format!(
            "```{}\n{}\n```\n",
            lang_hint(&doc.primary_language),
            api.signature
        ));
        if let Some(doc_str) = &api.doc {
            md.push_str(&format!("> {}\n", doc_str));
        }
        md.push_str(&format!("> Source: `{}`\n\n", api.source_ref.display()));
    }
}

fn render_public_api(md: &mut String, doc: &WikiDoc) {
    if doc.public_api.is_empty() {
        return;
    }
    md.push_str("## Public API\n\n");
    let groups: &[(&str, &str)] = &[
        ("Trait", "Traits"),
        ("Struct", "Types"),
        ("Enum", "Enums"),
        ("Function", "Functions"),
        ("Method", "Methods"),
    ];
    let mut rendered_any_group = false;
    for (kind, label) in groups {
        let items: Vec<&ApiEntry> = doc.public_api.iter().filter(|a| a.kind == *kind).collect();
        if items.is_empty() {
            continue;
        }
        md.push_str(&format!("### {}\n\n", label));
        md.push_str(&format!("```{}\n", lang_hint(&doc.primary_language)));
        for api in &items {
            md.push_str(&format!("{}\n", api.signature));
        }
        md.push_str("```\n");
        for api in &items {
            if let Some(ref doc_str) = api.doc {
                let first_line = doc_str.lines().next().unwrap_or("");
                if !first_line.is_empty() {
                    md.push_str(&format!("> `{}` — {}\n", api.name, first_line));
                }
            }
        }
        md.push('\n');
        rendered_any_group = true;
    }
    if !rendered_any_group {
        md.push_str(&format!("```{}\n", lang_hint(&doc.primary_language)));
        for api in &doc.public_api {
            md.push_str(&format!("{}\n", api.signature));
        }
        md.push_str("```\n\n");
    }
}

fn render_files_section(md: &mut String, doc: &WikiDoc) {
    if doc.files.is_empty() {
        return;
    }
    md.push_str("## Files\n\n");
    md.push_str("| File | Symbols |\n|------|--------|\n");
    for f in &doc.files {
        md.push_str(&format!("| `{}` | {} |\n", f.path, f.symbol_count));
    }
    md.push('\n');
}

fn render_dependencies_section(md: &mut String, doc: &WikiDoc) {
    if doc.dependencies.is_empty() {
        return;
    }
    md.push_str("## Dependencies\n\n");
    for dep in &doc.dependencies {
        md.push_str(&format!("- → [[{}]] ({})\n", dep.target_slug, dep.edge_type));
    }
    md.push('\n');
}

fn render_call_flow_section(md: &mut String, doc: &WikiDoc) {
    if doc.call_flow.is_empty() {
        return;
    }
    md.push_str("## Call Flow\n\n");
    for step in &doc.call_flow {
        md.push_str(&format!("`{}` → `{}`\n", step.from_symbol, step.to_symbol));
    }
    md.push('\n');
}

fn render_test_coverage_section(md: &mut String, doc: &WikiDoc) {
    md.push_str("## Test Coverage\n\n");
    md.push_str(&format!(
        "{}/{} functions tested ({:.0}%)\n\n",
        doc.test_coverage.tested, doc.test_coverage.total, doc.test_coverage.percentage
    ));
    if doc.test_coverage.untested.is_empty() {
        return;
    }
    md.push_str("Untested: ");
    md.push_str(
        &doc.test_coverage
            .untested
            .iter()
            .map(|s| format!("`{}`", s))
            .collect::<Vec<_>>()
            .join(", "),
    );
    md.push_str("\n\n");
}

fn render_footer(md: &mut String, doc: &WikiDoc) {
    md.push_str("---\n");
    md.push_str(&format!(
        "*Generated by GRAPHCTX {} | Sources: {} files, {} symbols*\n",
        WikiManifest::GENERATOR_VERSION,
        doc.file_count,
        doc.symbol_count
    ));
}

/// Render the index page (TOC) — flat list using default groups.
pub fn render_index(docs: &[WikiDoc], project_name: &str) -> String {
    let default_schema = WikiSchema::default_for(project_name);
    render_hierarchical_index(docs, &[], &[], project_name, &default_schema)
}

/// Render hierarchical index grouped by bounded context.
///
/// high_level_pages: slugs of overview/architecture/getting-started
/// concepts: detected concept candidates with related modules
/// schema: user-configurable wiki schema with group definitions
pub fn render_hierarchical_index(
    docs: &[WikiDoc],
    high_level_pages: &[(&str, &str)],    // (slug, title)
    concepts: &[(&str, &str, &[String])], // (slug, name, related_module_slugs)
    project_name: &str,
    schema: &WikiSchema,
) -> String {
    let mut md = String::with_capacity(4096);

    md += &format!("# {}\n\n", project_name);
    md += &format!(
        "> Auto-generated knowledge base | {} modules\n\n",
        docs.len()
    );

    // Overview section (high-level pages)
    if !high_level_pages.is_empty() {
        md += "## Overview\n\n";
        for (slug, title) in high_level_pages {
            md += &format!("- [[{}|{}]]\n", slug, title);
        }
        md += "\n";
    }

    // Concepts section
    if !concepts.is_empty() {
        md += "## Concepts\n\n";
        for (slug, name, _) in concepts {
            md += &format!("- [[{}|{}]]\n", slug, name);
        }
        md += "\n";
    }

    // Modules grouped by bounded context (from schema)
    md += "## Modules\n\n";

    let mut grouped_slugs: std::collections::HashSet<String> = std::collections::HashSet::new();

    for group in &schema.groups {
        let group_docs: Vec<&WikiDoc> = docs
            .iter()
            .filter(|d| {
                group
                    .prefixes
                    .iter()
                    .any(|p| d.slug.starts_with(p) || d.title.starts_with(p))
            })
            .collect();

        if group_docs.is_empty() {
            continue;
        }

        md += &format!("### {}\n\n", group.name);
        md += "| Module | Files | Symbols | Coverage |\n";
        md += "|--------|-------|---------|----------|\n";

        for doc in &group_docs {
            md += &format!(
                "| [[{}|{}]] | {} | {} | {:.0}% |\n",
                doc.slug, doc.title, doc.file_count, doc.symbol_count, doc.test_coverage.percentage
            );
            grouped_slugs.insert(doc.slug.clone());
        }
        md += "\n";
    }

    // Ungrouped modules
    let ungrouped: Vec<&WikiDoc> = docs
        .iter()
        .filter(|d| !grouped_slugs.contains(&d.slug) && d.file_count >= 2)
        .collect();

    if !ungrouped.is_empty() {
        md += "### Other\n\n";
        md += "| Module | Files | Symbols | Coverage |\n";
        md += "|--------|-------|---------|----------|\n";
        for doc in &ungrouped {
            md += &format!(
                "| [[{}|{}]] | {} | {} | {:.0}% |\n",
                doc.slug, doc.title, doc.file_count, doc.symbol_count, doc.test_coverage.percentage
            );
        }
        md += "\n";
    }

    md += "---\n";
    md += &format!(
        "*Generated by GRAPHCTX {}*\n",
        WikiManifest::GENERATOR_VERSION
    );

    md
}

/// Map file extension to code fence language hint.
fn lang_hint(ext: &str) -> &str {
    match ext {
        "rs" => "rust",
        "py" => "python",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "go" => "go",
        "java" | "kt" => "java",
        "rb" => "ruby",
        "php" => "php",
        "cs" => "csharp",
        _ => ext,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_doc() -> WikiDoc {
        WikiDoc {
            slug: "auth".into(),
            title: "Authentication".into(),
            community_id: "c1".into(),
            file_count: 3,
            symbol_count: 10,
            primary_language: "rs".into(),
            files: vec![FileEntry {
                path: "src/auth.rs".into(),
                symbol_count: 5,
                source_ref: SourceRef::file("src/auth.rs"),
            }],
            entry_points: vec![ApiEntry {
                name: "verify_token".into(),
                signature: "pub fn verify_token(t: &str) -> bool".into(),
                doc: Some("Verify JWT token".into()),
                kind: "Function".into(),
                source_ref: SourceRef::symbol("src/auth.rs", "verify_token", Some(10), Some(30)),
            }],
            public_api: vec![ApiEntry {
                name: "verify_token".into(),
                signature: "pub fn verify_token(t: &str) -> bool".into(),
                doc: None,
                kind: "Function".into(),
                source_ref: SourceRef::symbol("src/auth.rs", "verify_token", Some(10), Some(30)),
            }],
            dependencies: vec![DepEntry {
                target_slug: "domain".into(),
                target_name: "domain".into(),
                edge_type: "Imports".into(),
            }],
            call_flow: vec![FlowStep {
                from_symbol: "handle_request".into(),
                to_symbol: "verify_token".into(),
                edge_type: "Calls".into(),
                source_ref: SourceRef::symbol("src/auth.rs", "verify_token", Some(10), Some(30)),
            }],
            test_coverage: TestCoverage {
                tested: 8,
                total: 10,
                percentage: 80.0,
                untested: vec!["untested_fn".into()],
            },
            source_refs: vec![SourceRef::file("src/auth.rs")],
            summary: "rs traits across 3 files (10 symbols). Primary: verify_token.".into(),
            tags: vec!["rs".into(), "auth".into(), "traits".into()],
            crate_description: None,
            module_doc: None,
            generated_at: "0".into(),
            enriched: false,
        }
    }

    #[test]
    fn render_page_contains_title() {
        let md = render_page(&sample_doc());
        assert!(md.contains("# Authentication"));
    }

    #[test]
    fn render_page_contains_entry_points() {
        let md = render_page(&sample_doc());
        assert!(md.contains("## Entry Points"));
        assert!(md.contains("verify_token"));
    }

    #[test]
    fn render_page_contains_provenance() {
        let md = render_page(&sample_doc());
        assert!(md.contains("src/auth.rs:10-30"));
    }

    #[test]
    fn render_page_contains_wiki_links() {
        let md = render_page(&sample_doc());
        assert!(md.contains("[[domain]]"));
    }

    #[test]
    fn render_page_contains_test_coverage() {
        let md = render_page(&sample_doc());
        assert!(md.contains("8/10 functions tested"));
        assert!(md.contains("`untested_fn`"));
    }

    #[test]
    fn render_index_table() {
        let docs = vec![sample_doc()];
        let md = render_index(&docs, "Test Project");
        assert!(md.contains("Test Project"));
        assert!(md.contains("Authentication") || md.contains("auth"));
        assert!(md.contains("80%") || md.contains("80"));
    }

    #[test]
    fn render_page_footer() {
        let md = render_page(&sample_doc());
        assert!(md.contains("Generated by GRAPHCTX"));
    }

    #[test]
    fn renderer_uses_schema_groups() {
        let mut doc = sample_doc();
        doc.slug = "mylib-auth".into();
        doc.title = "MyLib Auth".into();

        let schema = WikiSchema {
            project: ProjectConfig {
                name: "Test".into(),
                description: String::new(),
            },
            groups: vec![
                GroupConfig {
                    name: "Security".into(),
                    prefixes: vec!["mylib-auth".into()],
                },
                GroupConfig {
                    name: "Core".into(),
                    prefixes: vec!["mylib-core".into()],
                },
            ],
            pages: PageConfig::default(),
        };

        let md = render_hierarchical_index(&[doc], &[], &[], "Test", &schema);
        assert!(
            md.contains("### Security"),
            "should use schema group name, got:\n{}",
            md
        );
        assert!(!md.contains("### Other"), "should not fall to Other");
        assert!(
            !md.contains("Code Intelligence"),
            "should not use default groups"
        );
    }
}
