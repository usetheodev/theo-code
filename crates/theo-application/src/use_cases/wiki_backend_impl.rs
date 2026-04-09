//! WikiBackend implementation using theo-engine-retrieval.
//!
//! This is the concrete implementation of the WikiBackend trait from theo-domain.
//! It bridges the abstract tool interface to the actual wiki retrieval + runtime modules.

use std::path::{Path, PathBuf};
use async_trait::async_trait;
use theo_domain::wiki_backend::*;

/// Concrete wiki backend backed by theo-engine-retrieval.
pub struct WikiRetrievalBackend {
    project_dir: PathBuf,
    wiki_dir: PathBuf,
}

impl WikiRetrievalBackend {
    pub fn new(project_dir: &Path) -> Self {
        Self {
            project_dir: project_dir.to_path_buf(),
            wiki_dir: project_dir.join(".theo").join("wiki"),
        }
    }
}

#[async_trait]
impl WikiBackend for WikiRetrievalBackend {
    async fn query(&self, question: &str, max_results: usize) -> Vec<WikiQueryResult> {
        let results = theo_engine_retrieval::wiki::lookup::lookup(&self.wiki_dir, question, max_results);

        results.into_iter().map(|r| {
            // Extract summary from frontmatter if available
            let fm = theo_engine_retrieval::wiki::model::parse_frontmatter(&r.content);
            let summary = fm.summary.unwrap_or_default();

            WikiQueryResult {
                slug: r.slug,
                title: r.title,
                summary,
                content: r.content,
                confidence: r.confidence,
                authority_tier: r.authority_tier.as_str().to_string(),
                is_stale: r.is_stale,
            }
        }).collect()
    }

    async fn ingest(&self, input: WikiInsightInput) -> Result<WikiIngestResult, String> {
        use theo_engine_retrieval::wiki::runtime;

        // Extract affected entities from output
        let (affected_files, affected_symbols) = runtime::extract_affected_entities(
            &input.stdout, &input.stderr,
        );

        // Extract error summary
        let error_summary = runtime::extract_error_summary(&input.stderr);

        // Build insight
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Load graph hash from manifest
        let project_dir = self.wiki_dir.parent().and_then(|p| p.parent())
            .unwrap_or(Path::new("."));
        let graph_hash = theo_engine_retrieval::wiki::persistence::load_manifest(project_dir)
            .map(|m| m.graph_hash)
            .unwrap_or(0);

        let insight = theo_engine_retrieval::wiki::model::RuntimeInsight {
            timestamp: now,
            source: input.source,
            command: input.command,
            exit_code: input.exit_code,
            success: input.success,
            duration_ms: input.duration_ms,
            error_summary,
            stdout_excerpt: Some(runtime::excerpt(&input.stdout, 500)),
            stderr_excerpt: Some(runtime::excerpt(&input.stderr, 500)),
            affected_files: affected_files.clone(),
            affected_symbols: affected_symbols.clone(),
            graph_hash,
        };

        runtime::ingest_insight(&self.wiki_dir, insight)
            .map_err(|e| format!("Ingest failed: {}", e))?;

        let total = runtime::load_all_insights(&self.wiki_dir).len();

        Ok(WikiIngestResult {
            ingested: true,
            affected_files,
            affected_symbols,
            total_insights: total,
        })
    }

    async fn generate(&self) -> Result<WikiGenerateResult, String> {
        use theo_engine_graph::bridge;
        use theo_engine_graph::cluster::{hierarchical_cluster, ClusterAlgorithm};
        use theo_engine_retrieval::wiki;

        let start = std::time::Instant::now();

        // Check if wiki is fresh (skip if no changes)
        let existing_manifest = wiki::persistence::load_manifest(&self.project_dir);
        let wiki_exists = self.wiki_dir.join("modules").exists();

        // Step 1: Parse project
        let (files, _stats) = super::extraction::extract_repo(&self.project_dir);
        if files.is_empty() {
            return Err("No source files found to parse".into());
        }

        // Step 2: Build graph
        let (graph, _) = bridge::build_graph(&files);

        // Step 3: Check freshness
        let current_hash = wiki::generator::compute_graph_hash(&graph);
        if wiki::persistence::is_fresh(&self.project_dir, current_hash) {
            return Ok(WikiGenerateResult {
                pages_generated: 0,
                pages_updated: 0,
                pages_skipped: existing_manifest.map(|m| m.page_count).unwrap_or(0),
                duration_ms: start.elapsed().as_millis() as u64,
                wiki_dir: self.wiki_dir.display().to_string(),
                is_incremental: true,
            });
        }

        // Step 4: Cluster
        let cluster = hierarchical_cluster(
            &graph,
            ClusterAlgorithm::FileLeiden { resolution: 1.0 },
        );

        // Step 5: Generate wiki
        let project_name = self.project_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project");

        let wiki_data = wiki::generator::generate_wiki_with_root(
            &cluster.communities,
            &graph,
            project_name,
            Some(&self.project_dir),
        );

        let total_pages = wiki_data.docs.len();

        // Step 6: Write to disk
        wiki::persistence::write_to_disk(&wiki_data, &self.project_dir)
            .map_err(|e| format!("Failed to write wiki: {}", e))?;

        // Step 7: Write schema default
        let schema = wiki::persistence::load_schema(&self.project_dir, project_name);
        let _ = wiki::persistence::write_schema_default(&self.project_dir, &schema);

        // Step 8: Log
        let duration = start.elapsed();
        wiki::persistence::append_log(
            &self.project_dir,
            "generate",
            &format!("{} pages in {}ms ({})",
                total_pages, duration.as_millis(),
                if wiki_exists { "update" } else { "initial" }),
        );

        let is_incremental = wiki_exists;

        Ok(WikiGenerateResult {
            pages_generated: if is_incremental { 0 } else { total_pages },
            pages_updated: if is_incremental { total_pages } else { 0 },
            pages_skipped: 0,
            duration_ms: duration.as_millis() as u64,
            wiki_dir: self.wiki_dir.display().to_string(),
            is_incremental,
        })
    }
}
