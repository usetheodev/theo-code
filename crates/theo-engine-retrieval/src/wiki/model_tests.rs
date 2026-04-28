//! Sibling test body of `model.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `model.rs` via `#[path = "model_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.


#![cfg(test)]

    use super::*;

    #[test]
    fn source_ref_display_file_only() {
        let sr = SourceRef::file("src/main.rs");
        assert_eq!(sr.display(), "src/main.rs");
    }

    #[test]
    fn source_ref_display_with_lines() {
        let sr = SourceRef::symbol("src/auth.rs", "verify_token", Some(10), Some(30));
        assert_eq!(sr.display(), "src/auth.rs:10-30");
    }

    #[test]
    fn source_ref_display_start_only() {
        let sr = SourceRef {
            file_path: "lib.rs".into(),
            symbol_name: None,
            line_start: Some(5),
            line_end: None,
        };
        assert_eq!(sr.display(), "lib.rs:5");
    }

    #[test]
    fn test_coverage_default() {
        let tc = TestCoverage {
            tested: 0,
            total: 0,
            percentage: 0.0,
            untested: vec![],
        };
        assert_eq!(tc.percentage, 0.0);
    }

    #[test]
    fn schema_default_has_8_groups() {
        let schema = WikiSchema::default_for("test-project");
        assert_eq!(schema.groups.len(), 8);
        assert_eq!(schema.project.name, "test-project");
        assert_eq!(schema.pages.min_file_count, 1);
        assert_eq!(schema.pages.max_token_size, 5000);
    }

    #[test]
    fn schema_round_trip_toml() {
        let schema = WikiSchema::default_for("my-project");
        let toml_str = toml::to_string_pretty(&schema).unwrap();
        let parsed: WikiSchema = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.project.name, "my-project");
        assert_eq!(parsed.groups.len(), 8);
        assert_eq!(parsed.groups[0].name, "Code Intelligence");
        assert_eq!(parsed.pages.min_file_count, 1);
    }

    #[test]
    fn schema_partial_toml_uses_defaults() {
        let toml_str = r#"
[project]
name = "minimal"

[[groups]]
name = "Custom"
prefixes = ["custom-"]
"#;
        let parsed: WikiSchema = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.project.name, "minimal");
        assert_eq!(parsed.groups.len(), 1);
        assert_eq!(parsed.groups[0].name, "Custom");
        // pages uses defaults
        assert_eq!(parsed.pages.min_file_count, 1);
        assert_eq!(parsed.pages.max_token_size, 5000);
    }

    #[test]
    fn authority_tier_from_str_round_trip() {
        for tier in [
            AuthorityTier::Deterministic,
            AuthorityTier::Enriched,
            AuthorityTier::PromotedCache,
            AuthorityTier::RawCache,
            AuthorityTier::EpisodicCache,
        ] {
            assert_eq!(AuthorityTier::from_str(tier.as_str()), tier);
        }
    }

    #[test]
    fn authority_tier_weights() {
        assert!(AuthorityTier::Deterministic.weight() > AuthorityTier::RawCache.weight());
        assert!(AuthorityTier::Enriched.weight() > AuthorityTier::PromotedCache.weight());
        assert!(AuthorityTier::RawCache.weight() > AuthorityTier::EpisodicCache.weight());
    }

    #[test]
    fn episodic_cache_tier_exists_and_has_low_weight() {
        let tier = AuthorityTier::EpisodicCache;
        assert_eq!(tier.weight(), 0.4);
        assert_eq!(tier.as_str(), "episodic");
        assert_eq!(
            AuthorityTier::from_str("episodic"),
            AuthorityTier::EpisodicCache
        );
    }

    #[test]
    fn episodic_cache_excluded_from_main_index() {
        assert!(!AuthorityTier::EpisodicCache.included_in_main_index());
        assert!(AuthorityTier::Deterministic.included_in_main_index());
        assert!(AuthorityTier::Enriched.included_in_main_index());
        assert!(AuthorityTier::PromotedCache.included_in_main_index());
        assert!(AuthorityTier::RawCache.included_in_main_index());
    }

    #[test]
    fn frontmatter_render_and_parse_round_trip() {
        let fm = PageFrontmatter::module(false, "test summary", &["rs".to_string()]);
        let rendered = render_frontmatter(&fm);
        assert!(rendered.starts_with("---\n"));
        assert!(rendered.contains("authority_tier: deterministic"));
        assert!(rendered.contains("page_kind: module"));

        let parsed = parse_frontmatter(&rendered);
        assert_eq!(parsed.authority_tier.as_deref(), Some("deterministic"));
        assert_eq!(parsed.page_kind.as_deref(), Some("module"));
    }

    #[test]
    fn frontmatter_cache_page() {
        let fm = PageFrontmatter::cache("how does auth work", 12345);
        let rendered = render_frontmatter(&fm);
        let parsed = parse_frontmatter(&rendered);
        assert_eq!(parsed.authority_tier.as_deref(), Some("raw_cache"));
        assert_eq!(parsed.graph_hash, Some(12345));
        assert_eq!(parsed.query.as_deref(), Some("how does auth work"));
    }

    #[test]
    fn frontmatter_tier_with_fallback() {
        let fm = PageFrontmatter::default(); // no authority_tier
        assert_eq!(fm.tier("modules"), AuthorityTier::Deterministic);
        assert_eq!(fm.tier("cache"), AuthorityTier::RawCache);

        let fm2 = PageFrontmatter::module(true, "", &[]);
        assert_eq!(fm2.tier("modules"), AuthorityTier::Enriched);
    }

    #[test]
    fn parse_frontmatter_no_frontmatter() {
        let content = "# Just a title\n\nSome content.";
        let fm = parse_frontmatter(content);
        assert!(fm.authority_tier.is_none());
        assert!(fm.graph_hash.is_none());
    }
