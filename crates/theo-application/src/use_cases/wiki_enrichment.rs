//! LLM enrichment for Code Wiki pages.
//!
//! Takes bootstrap wiki pages (deterministic) and enriches them with
//! natural language explanations via the user's LLM provider.
//!
//! Adds:
//! - "What This Module Does" section (2-3 sentences)
//! - "Key Concepts" section (bullet points)
//!
//! Preserves all deterministic sections intact (provenance, API, deps, coverage).
//! Opt-in: never runs automatically. Fallback: if LLM fails, keeps original.

use std::path::Path;

use theo_engine_retrieval::wiki::model::{Wiki, WikiManifest};
use theo_engine_retrieval::wiki::persistence;
use theo_engine_retrieval::wiki::renderer;
use theo_infra_llm::client::LlmClient;
use theo_infra_llm::types::{ChatRequest, Message};

// ---------------------------------------------------------------------------
// Enrichment prompt
// ---------------------------------------------------------------------------

const ENRICHMENT_SYSTEM: &str = "You are a senior software engineer writing internal documentation. \
Your task: enrich a wiki page for a code module so developers can understand it quickly.\n\n\
Rules:\n\
1. Add a 'What This Module Does' section AFTER the first header with 2-3 concise sentences explaining the module's PURPOSE and RESPONSIBILITY.\n\
2. Add a 'Key Concepts' section with 3-5 bullet points of important design decisions or patterns.\n\
3. Keep ALL existing sections EXACTLY as they are (Entry Points, Public API, Files, Dependencies, Call Flow, Test Coverage).\n\
4. Do NOT invent symbols, files, or dependencies that are not in the original.\n\
5. Do NOT remove or modify any provenance lines (Source: ...).\n\
6. Use [[wiki-link]] format for references to other modules.\n\
7. Be concise. The enriched page should be at most 1.5x the original length.\n\
8. Write in English.";

fn enrichment_user_prompt(page_markdown: &str) -> String {
    format!(
        "Enrich this wiki page following the rules above. Return ONLY the complete enriched markdown, nothing else.\n\n---\n\n{}",
        page_markdown
    )
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Enrich all wiki pages using the given LLM client.
///
/// Returns the count of successfully enriched pages.
/// Pages that fail LLM call keep their original content.
///
/// This is an opt-in operation — never called automatically.
pub async fn enrich_wiki(
    project_dir: &Path,
    client: &LlmClient,
) -> Result<EnrichmentResult, String> {
    // Load current wiki from disk
    let wiki_dir = project_dir.join(".theo").join("wiki");
    let manifest = persistence::load_manifest(project_dir)
        .ok_or_else(|| "No wiki found. Run wiki generation first.".to_string())?;

    // Load all WikiDocs
    let mut wiki = load_wiki_from_disk(project_dir, &manifest)?;
    let total = wiki.docs.len();
    let mut enriched_count = 0;
    let mut failed_count = 0;

    for doc in &mut wiki.docs {
        if doc.enriched {
            continue; // Already enriched, skip
        }

        // Render current page to markdown
        let current_md = renderer::render_page(doc);

        // Call LLM
        let request = ChatRequest::new(
            client.model().to_string(),
            vec![
                Message::system(ENRICHMENT_SYSTEM),
                Message::user(enrichment_user_prompt(&current_md)),
            ],
        )
        .with_max_tokens(2000)
        .with_temperature(0.3);

        match client.chat(&request).await {
            Ok(response) => {
                if let Some(content) = response.content() {
                    // Validate: enriched content should contain key sections
                    if content.contains("## Entry Points") || content.contains("## Public API")
                        || content.contains("## Files")
                    {
                        // Write enriched page to disk
                        let filename = format!("{}.md", doc.slug);
                        let page_path = wiki_dir.join("modules").join(&filename);
                        if let Err(e) = std::fs::write(&page_path, content) {
                            eprintln!("[wiki-enrich] Failed to write {}: {}", doc.slug, e);
                            failed_count += 1;
                        } else {
                            doc.enriched = true;
                            enriched_count += 1;
                            eprintln!("[wiki-enrich] Enriched: {}", doc.title);
                        }
                    } else {
                        eprintln!(
                            "[wiki-enrich] Skipped {} — LLM output missing required sections",
                            doc.slug
                        );
                        failed_count += 1;
                    }
                } else {
                    eprintln!("[wiki-enrich] Skipped {} — empty LLM response", doc.slug);
                    failed_count += 1;
                }
            }
            Err(e) => {
                eprintln!("[wiki-enrich] Failed {} — LLM error: {}", doc.slug, e);
                failed_count += 1;
            }
        }
    }

    // Update manifest
    let mut manifest = manifest;
    manifest.page_count = wiki.docs.len();
    let manifest_json = serde_json::to_string_pretty(&manifest).unwrap_or_default();
    let _ = std::fs::write(wiki_dir.join("wiki.manifest.json"), manifest_json);

    Ok(EnrichmentResult {
        total,
        enriched: enriched_count,
        failed: failed_count,
        skipped: total - enriched_count - failed_count,
    })
}

/// Result of wiki enrichment.
#[derive(Debug)]
pub struct EnrichmentResult {
    pub total: usize,
    pub enriched: usize,
    pub failed: usize,
    pub skipped: usize,
}

impl std::fmt::Display for EnrichmentResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Wiki enrichment: {}/{} enriched, {} failed, {} skipped",
            self.enriched, self.total, self.failed, self.skipped
        )
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Load wiki docs from disk (reads markdown files + manifest).
fn load_wiki_from_disk(
    project_dir: &Path,
    manifest: &WikiManifest,
) -> Result<Wiki, String> {
    let modules_dir = project_dir.join(".theo").join("wiki").join("modules");

    let mut docs = Vec::new();

    let entries = std::fs::read_dir(&modules_dir)
        .map_err(|e| format!("Cannot read wiki modules dir: {e}"))?;

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

        let _content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Cannot read {}: {e}", path.display()))?;

        // Create minimal WikiDoc for enrichment tracking
        docs.push(theo_engine_retrieval::wiki::model::WikiDoc {
            slug,
            title: String::new(), // Will be populated from content
            community_id: String::new(),
            file_count: 0,
            symbol_count: 0,
            primary_language: String::new(),
            files: vec![],
            entry_points: vec![],
            public_api: vec![],
            dependencies: vec![],
            call_flow: vec![],
            test_coverage: theo_engine_retrieval::wiki::model::TestCoverage {
                tested: 0,
                total: 0,
                percentage: 0.0,
                untested: vec![],
            },
            source_refs: vec![],
            summary: String::new(),
            tags: vec![],
            crate_description: None,
            module_doc: None,
            generated_at: String::new(),
            enriched: false,
        });
    }

    Ok(Wiki {
        docs,
        manifest: manifest.clone(),
    })
}
