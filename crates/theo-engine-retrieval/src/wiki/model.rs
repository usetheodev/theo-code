//! Canonical data model for wiki pages (IR layer).
//!
//! Separates structured data from markdown rendering.
//! Every claim has provenance (SourceRef) tracing back to file + symbol + lines.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Provenance
// ---------------------------------------------------------------------------

/// Source provenance: traces a wiki claim back to code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRef {
    pub file_path: String,
    pub symbol_name: Option<String>,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
}

impl SourceRef {
    pub fn file(path: &str) -> Self {
        SourceRef {
            file_path: path.to_string(),
            symbol_name: None,
            line_start: None,
            line_end: None,
        }
    }

    pub fn symbol(path: &str, name: &str, start: Option<usize>, end: Option<usize>) -> Self {
        SourceRef {
            file_path: path.to_string(),
            symbol_name: Some(name.to_string()),
            line_start: start,
            line_end: end,
        }
    }

    /// Format as `file.rs:10-30` for display.
    pub fn display(&self) -> String {
        let mut s = self.file_path.clone();
        if let Some(start) = self.line_start {
            s += &format!(":{}", start);
            if let Some(end) = self.line_end {
                s += &format!("-{}", end);
            }
        }
        s
    }
}

// ---------------------------------------------------------------------------
// Page sections
// ---------------------------------------------------------------------------

/// A file listed in the wiki page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub symbol_count: usize,
    pub source_ref: SourceRef,
}

/// A public API symbol (entry point or exported function/struct/trait).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiEntry {
    pub name: String,
    pub signature: String,
    pub doc: Option<String>,
    pub kind: String, // Function, Method, Struct, Trait, Enum
    pub source_ref: SourceRef,
}

/// A cross-community dependency link.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepEntry {
    pub target_slug: String,
    pub target_name: String,
    pub edge_type: String, // Imports, Calls, TypeDepends
}

/// A step in a call flow (A calls B).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowStep {
    pub from_symbol: String,
    pub to_symbol: String,
    pub edge_type: String,
    pub source_ref: SourceRef,
}

/// Test coverage statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCoverage {
    pub tested: usize,
    pub total: usize,
    pub percentage: f64,
    pub untested: Vec<String>,
}

// ---------------------------------------------------------------------------
// WikiDoc (canonical IR for one page)
// ---------------------------------------------------------------------------

/// A single wiki document representing one community/module.
///
/// Every section carries provenance via SourceRef.
/// This struct is the canonical IR — rendering to markdown is a separate step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiDoc {
    pub slug: String,
    pub title: String,
    pub community_id: String,

    // Summary stats
    pub file_count: usize,
    pub symbol_count: usize,
    pub primary_language: String,

    // Sections (all with provenance)
    pub files: Vec<FileEntry>,
    pub entry_points: Vec<ApiEntry>,
    pub public_api: Vec<ApiEntry>,
    pub dependencies: Vec<DepEntry>,
    pub call_flow: Vec<FlowStep>,
    pub test_coverage: TestCoverage,

    // Aggregate provenance
    pub source_refs: Vec<SourceRef>,

    // Karpathy header — LLM-optimized (Layer 1: author > Layer 2: graph > Layer 3: LLM)
    /// One-line summary. Priority: Cargo.toml description > heuristic.
    pub summary: String,
    /// Auto-detected tags for search and categorization.
    pub tags: Vec<String>,
    /// Crate description from Cargo.toml/pyproject.toml [package].description
    pub crate_description: Option<String>,
    /// Module-level doc comment (//! in Rust, docstring in Python)
    pub module_doc: Option<String>,

    // Metadata
    pub generated_at: String,
    pub enriched: bool,
}

// ---------------------------------------------------------------------------
// Wiki (complete output)
// ---------------------------------------------------------------------------

/// The complete wiki: all pages + manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wiki {
    pub docs: Vec<WikiDoc>,
    pub manifest: WikiManifest,
}

/// Manifest for cache invalidation and versioning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiManifest {
    /// Bump on breaking template changes.
    pub schema_version: u32,
    /// Generator identifier.
    pub generator_version: String,
    /// Hash of graph state (file paths + mtimes).
    pub graph_hash: u64,
    /// ISO 8601 timestamp.
    pub generated_at: String,
    /// Number of pages generated.
    pub page_count: usize,
    /// Per-page hashes for incremental generation (canonical_key → community_hash).
    #[serde(default)]
    pub page_hashes: std::collections::HashMap<String, u64>,
}

impl WikiManifest {
    pub const SCHEMA_VERSION: u32 = 1;
    pub const GENERATOR_VERSION: &'static str = "wiki-bootstrap-v1";
}

// ---------------------------------------------------------------------------
// Authority Tiers — semantic truth hierarchy
// ---------------------------------------------------------------------------

/// Authority tier: classifies the trustworthiness and source of a wiki page.
///
/// NOT Ord — tier is a prior for scoring policy, not universal preference.
/// Different query types may prefer different tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthorityTier {
    /// Deterministic module facts from CodeGraph. Highest authority.
    Deterministic,
    /// LLM-enriched module pages. High authority.
    Enriched,
    /// Promoted cache pages (validated, high-quality). Medium authority.
    PromotedCache,
    /// Raw cache pages from query write-back. Lowest authority.
    RawCache,
    /// Episodic summaries from agent execution. Excluded from main BM25 index.
    /// Queryable only with explicit opt-in. TTL-gated eviction.
    EpisodicCache,
}

impl AuthorityTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthorityTier::Deterministic => "deterministic",
            AuthorityTier::Enriched => "enriched",
            AuthorityTier::PromotedCache => "promoted",
            AuthorityTier::RawCache => "raw_cache",
            AuthorityTier::EpisodicCache => "episodic",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "deterministic" => AuthorityTier::Deterministic,
            "enriched" => AuthorityTier::Enriched,
            "promoted" => AuthorityTier::PromotedCache,
            "raw_cache" => AuthorityTier::RawCache,
            "episodic" => AuthorityTier::EpisodicCache,
            _ => AuthorityTier::RawCache,
        }
    }

    /// Scoring weight — used as input to composite scoring, not as hard ordering.
    pub fn weight(&self) -> f64 {
        match self {
            AuthorityTier::Deterministic => 1.0,
            AuthorityTier::Enriched => 0.95,
            AuthorityTier::PromotedCache => 0.75,
            AuthorityTier::RawCache => 0.5,
            AuthorityTier::EpisodicCache => 0.4,
        }
    }

    /// Whether this tier should be included in the main BM25 index.
    /// EpisodicCache is excluded by default — only queryable with explicit opt-in.
    pub fn included_in_main_index(&self) -> bool {
        !matches!(self, AuthorityTier::EpisodicCache)
    }
}

// ---------------------------------------------------------------------------
// Page Frontmatter — canonical metadata contract
// ---------------------------------------------------------------------------

/// Structured frontmatter for all wiki pages.
///
/// Written as YAML frontmatter (`---\n...\n---\n`) at the top of every .md file.
/// This is the single source of truth for page classification — never infer
/// tier from rendered text content.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PageFrontmatter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authority_tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_hash: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

impl PageFrontmatter {
    /// Create frontmatter for a deterministic module page.
    pub fn module(enriched: bool, summary: &str, tags: &[String]) -> Self {
        PageFrontmatter {
            authority_tier: Some(if enriched { "enriched" } else { "deterministic" }.into()),
            page_kind: Some("module".into()),
            generated_by: Some("generator".into()),
            summary: if summary.is_empty() { None } else { Some(summary.to_string()) },
            tags: if tags.is_empty() { None } else { Some(tags.to_vec()) },
            ..Default::default()
        }
    }

    /// Create frontmatter for a cache write-back page.
    pub fn cache(query: &str, graph_hash: u64) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        PageFrontmatter {
            authority_tier: Some("raw_cache".into()),
            page_kind: Some("cache".into()),
            generated_by: Some("write_back".into()),
            graph_hash: Some(graph_hash),
            generated_at: Some(format!("{}", now)),
            query: Some(query.replace('"', "'")),
            summary: None,
            tags: None,
        }
    }

    /// Parse authority tier, with directory-based fallback for legacy pages.
    pub fn tier(&self, dir_fallback: &str) -> AuthorityTier {
        if let Some(ref tier_str) = self.authority_tier {
            AuthorityTier::from_str(tier_str)
        } else {
            // Legacy page without frontmatter — classify by directory
            match dir_fallback {
                "modules" => AuthorityTier::Deterministic,
                "cache" => AuthorityTier::RawCache,
                _ => AuthorityTier::RawCache,
            }
        }
    }
}

/// Render frontmatter as YAML block.
pub fn render_frontmatter(fm: &PageFrontmatter) -> String {
    let mut lines = Vec::new();
    if let Some(ref v) = fm.authority_tier { lines.push(format!("authority_tier: {}", v)); }
    if let Some(ref v) = fm.page_kind { lines.push(format!("page_kind: {}", v)); }
    if let Some(ref v) = fm.generated_by { lines.push(format!("generated_by: {}", v)); }
    if let Some(v) = fm.graph_hash { lines.push(format!("graph_hash: {}", v)); }
    if let Some(ref v) = fm.generated_at { lines.push(format!("generated_at: \"{}\"", v)); }
    if let Some(ref v) = fm.query { lines.push(format!("query: \"{}\"", v)); }
    if let Some(ref v) = fm.summary { lines.push(format!("summary: \"{}\"", v.replace('"', "'"))); }
    if let Some(ref tags) = fm.tags {
        lines.push(format!("tags: [{}]", tags.join(", ")));
    }
    if lines.is_empty() {
        return String::new();
    }
    format!("---\n{}\n---\n\n", lines.join("\n"))
}

/// Parse YAML frontmatter from markdown content.
pub fn parse_frontmatter(content: &str) -> PageFrontmatter {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return PageFrontmatter::default();
    }
    let after_open = &trimmed[3..];
    let Some(close_pos) = after_open.find("\n---") else {
        return PageFrontmatter::default();
    };
    let yaml_block = &after_open[..close_pos];

    let mut fm = PageFrontmatter::default();
    for line in yaml_block.lines() {
        let line = line.trim();
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim().trim_matches('"').trim_matches('\'');
            match key {
                "authority_tier" => fm.authority_tier = Some(value.to_string()),
                "page_kind" => fm.page_kind = Some(value.to_string()),
                "generated_by" => fm.generated_by = Some(value.to_string()),
                "graph_hash" => fm.graph_hash = value.parse().ok(),
                "generated_at" => fm.generated_at = Some(value.to_string()),
                "query" => fm.query = Some(value.to_string()),
                _ => {}
            }
        }
    }
    fm
}

// ---------------------------------------------------------------------------
// Query Classification
// ---------------------------------------------------------------------------

/// Query category for per-class threshold selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QueryClass {
    ApiLookup,
    Architecture,
    CallFlow,
    Concept,
    Onboarding,
    Unknown,
}

impl QueryClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            QueryClass::ApiLookup => "api_lookup",
            QueryClass::Architecture => "architecture",
            QueryClass::CallFlow => "call_flow",
            QueryClass::Concept => "concept",
            QueryClass::Onboarding => "onboarding",
            QueryClass::Unknown => "unknown",
        }
    }
}

/// Classify a query by keyword heuristic. Simple, interpretable, no ML.
pub fn classify_query(query: &str) -> QueryClass {
    let q = query.to_lowercase();

    // API lookup: concrete symbols, functions, types
    let api_terms = ["fn ", "struct ", "trait ", "impl ", "pub fn", "function", "method",
        "verify", "parse", "token", "handler", "client", "config", "registry",
        "search", "index", "cache", "embed"];
    if api_terms.iter().any(|t| q.contains(t)) {
        return QueryClass::ApiLookup;
    }

    // Call flow: execution paths
    let flow_terms = ["flow", "call chain", "how does", "lifecycle", "pipeline",
        "request path", "execution", "calls", "invokes"];
    if flow_terms.iter().any(|t| q.contains(t)) {
        return QueryClass::CallFlow;
    }

    // Architecture: system structure
    let arch_terms = ["architecture", "bounded context", "layer", "structure",
        "organized", "design", "system diagram", "dependency direction"];
    if arch_terms.iter().any(|t| q.contains(t)) {
        return QueryClass::Architecture;
    }

    // Concept: what is X, what modules handle Y
    let concept_terms = ["what is", "what are", "what modules", "which modules", "which crates",
        "what components", "role of", "purpose of", "concept"];
    if concept_terms.iter().any(|t| q.contains(t)) {
        return QueryClass::Concept;
    }

    // Onboarding: getting started
    let onboard_terms = ["get started", "getting started", "how to build",
        "how to run", "where to start", "entry point", "overview", "tech stack"];
    if onboard_terms.iter().any(|t| q.contains(t)) {
        return QueryClass::Onboarding;
    }

    QueryClass::Unknown
}

// ---------------------------------------------------------------------------
// WikiSchema — user-configurable wiki conventions
// ---------------------------------------------------------------------------

/// User-editable schema controlling wiki structure and conventions.
///
/// Lives at `.theo/wiki/wiki.schema.toml`. If absent, defaults are used.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiSchema {
    pub project: ProjectConfig,
    #[serde(default = "default_groups")]
    pub groups: Vec<GroupConfig>,
    #[serde(default)]
    pub pages: PageConfig,
}

/// Project-level metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// A bounded-context group: maps slug prefixes to a display name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupConfig {
    pub name: String,
    pub prefixes: Vec<String>,
}

/// Page generation thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageConfig {
    /// Minimum file count for a community to generate a wiki page.
    #[serde(default = "default_min_file_count")]
    pub min_file_count: usize,
    /// Token estimate threshold for lint large-page warning.
    #[serde(default = "default_max_token_size")]
    pub max_token_size: usize,
    /// Minimum BM25 confidence for wiki lookup results. Calibrate with wiki_eval.
    #[serde(default = "default_confidence_threshold")]
    pub confidence_threshold: f64,
}

impl Default for PageConfig {
    fn default() -> Self {
        PageConfig {
            min_file_count: default_min_file_count(),
            max_token_size: default_max_token_size(),
            confidence_threshold: default_confidence_threshold(),
        }
    }
}

fn default_confidence_threshold() -> f64 { 0.5 }

fn default_min_file_count() -> usize { 1 }
fn default_max_token_size() -> usize { 5000 }

fn default_groups() -> Vec<GroupConfig> {
    vec![
        GroupConfig { name: "Code Intelligence".into(), prefixes: vec!["theo-engine".into()] },
        GroupConfig { name: "Agent".into(), prefixes: vec!["theo-agent".into()] },
        GroupConfig { name: "Infrastructure".into(), prefixes: vec!["theo-infra".into()] },
        GroupConfig { name: "Tooling".into(), prefixes: vec!["theo-tooling".into()] },
        GroupConfig { name: "Governance".into(), prefixes: vec!["theo-governance".into()] },
        GroupConfig { name: "Domain".into(), prefixes: vec!["theo-domain".into()] },
        GroupConfig { name: "Frontend".into(), prefixes: vec!["theo-ui".into(), "theo-desktop".into()] },
        GroupConfig { name: "Application".into(), prefixes: vec!["theo-application".into(), "theo-cli".into(), "theo-benchmark".into()] },
    ]
}

impl WikiSchema {
    /// Create default schema for a project.
    pub fn default_for(project_name: &str) -> Self {
        WikiSchema {
            project: ProjectConfig {
                name: project_name.to_string(),
                description: String::new(),
            },
            groups: default_groups(),
            pages: PageConfig::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Runtime Insight — Operational Layer (Deep Wiki)
// ---------------------------------------------------------------------------

/// A captured runtime event (test result, build, command execution).
///
/// Standalone — does not depend on agent-runtime or tooling.
/// Any tool/agent/script can produce RuntimeInsight and feed it to the wiki.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeInsight {
    pub timestamp: u64,
    pub source: String,
    pub command: String,
    pub exit_code: i32,
    pub success: bool,
    pub duration_ms: u64,
    pub error_summary: Option<String>,
    pub stdout_excerpt: Option<String>,
    pub stderr_excerpt: Option<String>,
    pub affected_files: Vec<String>,
    pub affected_symbols: Vec<String>,
    pub graph_hash: u64,
}

/// Aggregated operational data for a wiki module page.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OperationalSection {
    pub common_failures: Vec<FailurePattern>,
    pub successful_recipes: Vec<CommandRecipe>,
    pub flaky_tests: Vec<String>,
    pub insight_count: usize,
    pub last_updated: u64,
}

/// A repeated failure pattern distilled from multiple insights.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailurePattern {
    pub pattern: String,
    pub count: usize,
    pub error_hint: Option<String>,
    pub affected_files: Vec<String>,
}

/// A validated command that succeeded (with statistics).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandRecipe {
    pub command: String,
    pub count: usize,
    pub avg_duration_ms: u64,
}

/// A promoted learning distilled from repeated runtime patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Learning {
    pub pattern: String,
    pub occurrences: usize,
    pub affected_modules: Vec<String>,
    pub first_seen: u64,
    pub last_seen: u64,
    pub status: LearningStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LearningStatus {
    Active,
    Resolved,
    Stale,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
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
        for tier in [AuthorityTier::Deterministic, AuthorityTier::Enriched,
                     AuthorityTier::PromotedCache, AuthorityTier::RawCache,
                     AuthorityTier::EpisodicCache] {
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
        assert_eq!(AuthorityTier::from_str("episodic"), AuthorityTier::EpisodicCache);
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
}
