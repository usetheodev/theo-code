//! LLM-generated high-level wiki pages: overview, architecture, concepts.
//!
//! These pages provide the human-friendly narrative layer that transforms
//! the wiki from "inventory of signatures" to "documentation you can read".
//!
//! Requires: LLM provider (Copilot, OpenAI, etc.) via theo-infra-llm.

use std::path::Path;

use theo_engine_retrieval::wiki::generator::ConceptCandidate;
use theo_engine_retrieval::wiki::model::WikiDoc;
use theo_infra_llm::client::LlmClient;
use theo_infra_llm::types::{ChatRequest, Message};

/// A generated high-level page.
pub struct HighLevelPage {
    pub slug: String,
    pub title: String,
    pub content: String,
}

/// Generate all high-level pages: overview, architecture, getting-started, concepts.
pub async fn generate_highlevel_pages(
    project_name: &str,
    docs: &[WikiDoc],
    concepts: &[ConceptCandidate],
    client: &LlmClient,
) -> Vec<HighLevelPage> {
    let mut pages = Vec::new();

    // Build module summary for LLM context
    let module_summary = build_module_summary(docs);

    // 1. Overview page
    if let Some(page) = generate_overview(project_name, &module_summary, client).await {
        pages.push(page);
    }

    // 2. Architecture page
    let dep_graph = build_dependency_text(docs);
    if let Some(page) =
        generate_architecture(project_name, &module_summary, &dep_graph, client).await
    {
        pages.push(page);
    }

    // 3. Getting Started page
    if let Some(page) = generate_getting_started(project_name, docs, client).await {
        pages.push(page);
    }

    // 4. Concept pages
    for concept in concepts.iter().take(5) {
        let related_summaries = build_concept_context(concept, docs);
        if let Some(page) = generate_concept_page(concept, &related_summaries, client).await {
            pages.push(page);
        }
    }

    eprintln!(
        "[wiki-highlevel] Generated {} high-level pages",
        pages.len()
    );
    pages
}

/// Write high-level pages to wiki directory.
pub fn write_highlevel_pages(pages: &[HighLevelPage], wiki_dir: &Path) -> std::io::Result<()> {
    let concepts_dir = wiki_dir.join("concepts");
    std::fs::create_dir_all(&concepts_dir)?;

    for page in pages {
        let path = if page.slug.starts_with("concept-") {
            concepts_dir.join(format!("{}.md", page.slug))
        } else {
            wiki_dir.join(format!("{}.md", page.slug))
        };
        std::fs::write(&path, &page.content)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Individual page generators
// ---------------------------------------------------------------------------

async fn generate_overview(
    project_name: &str,
    module_summary: &str,
    client: &LlmClient,
) -> Option<HighLevelPage> {
    let prompt = format!(
        "You are writing the main overview page for a software project's code wiki.\n\
        Project: {}\n\n\
        Modules:\n{}\n\n\
        Write a concise overview page in markdown with:\n\
        1. \"# Project Overview\" header\n\
        2. \"## What is {}\" — 2-3 sentences explaining what the project does\n\
        3. \"## Key Features\" — 5-7 bullet points\n\
        4. \"## Architecture at a Glance\" — a mermaid diagram showing main components\n\
        5. \"## Quick Links\" — links to the most important modules using [[module-slug]] format\n\n\
        Be specific to THIS project. Do not be generic. Use [[wiki-link]] format for cross-references.",
        project_name, module_summary, project_name
    );

    call_llm(
        client,
        &prompt,
        "overview",
        &format!("{} — Project Overview", project_name),
    )
    .await
}

async fn generate_architecture(
    project_name: &str,
    module_summary: &str,
    dep_graph: &str,
    client: &LlmClient,
) -> Option<HighLevelPage> {
    let prompt = format!(
        "You are writing an architecture page for a code wiki.\n\
        Project: {}\n\n\
        Modules:\n{}\n\n\
        Dependencies:\n{}\n\n\
        Write an architecture page in markdown with:\n\
        1. \"# Architecture\" header\n\
        2. \"## System Diagram\" — a mermaid flowchart showing the main bounded contexts and data flow\n\
        3. \"## Bounded Contexts\" — explain each major layer (2-3 sentences each)\n\
        4. \"## Data Flow\" — describe how a typical request flows through the system\n\
        5. \"## Key Design Decisions\" — 3-5 architectural decisions and why\n\n\
        Use [[wiki-link]] format. Be specific to THIS project.",
        project_name, module_summary, dep_graph
    );

    call_llm(
        client,
        &prompt,
        "architecture",
        &format!("{} — Architecture", project_name),
    )
    .await
}

async fn generate_getting_started(
    project_name: &str,
    docs: &[WikiDoc],
    client: &LlmClient,
) -> Option<HighLevelPage> {
    // Collect entry points from largest modules
    let mut entry_points = Vec::new();
    for doc in docs.iter().take(5) {
        for ep in doc.entry_points.iter().take(2) {
            entry_points.push(format!("- `{}` in [[{}]]", ep.signature, doc.slug));
        }
    }

    let prompt = format!(
        "You are writing a getting-started guide for a code wiki.\n\
        Project: {}\n\n\
        Main entry points:\n{}\n\n\
        Write a getting-started page in markdown with:\n\
        1. \"# Getting Started\" header\n\
        2. \"## Prerequisites\" — what you need to build/run\n\
        3. \"## Building\" — build commands\n\
        4. \"## Project Structure\" — brief explanation of the directory layout\n\
        5. \"## Where to Start Reading\" — recommend 3-5 files to read first with [[wiki-links]]\n\
        6. \"## Key Entry Points\" — the main functions that start everything\n\n\
        Be concise and practical.",
        project_name,
        entry_points.join("\n")
    );

    call_llm(
        client,
        &prompt,
        "getting-started",
        &format!("{} — Getting Started", project_name),
    )
    .await
}

async fn generate_concept_page(
    concept: &ConceptCandidate,
    related_summaries: &str,
    client: &LlmClient,
) -> Option<HighLevelPage> {
    let prompt = format!(
        "You are writing a concept page for a code wiki.\n\
        Concept: {}\n\n\
        Related modules:\n{}\n\n\
        Write a concept page in markdown with:\n\
        1. \"# {}\" header\n\
        2. \"## What is this\" — 2-3 sentences\n\
        3. \"## How it works\" — step by step explanation\n\
        4. \"## Key Components\" — which files and symbols matter, with [[wiki-links]]\n\
        5. \"## Related Concepts\" — links to other concept pages\n\n\
        Be specific. Reference actual module names using [[wiki-link]] format.",
        concept.name, related_summaries, concept.name
    );

    let slug = format!("concept-{}", concept.name.to_lowercase().replace(' ', "-"));
    call_llm(client, &prompt, &slug, &concept.name).await
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn call_llm(
    client: &LlmClient,
    prompt: &str,
    slug: &str,
    title: &str,
) -> Option<HighLevelPage> {
    let request = ChatRequest::new(
        client.model().to_string(),
        vec![
            Message::system("You are a technical documentation writer. Write clear, concise wiki pages in markdown. Use [[wiki-link]] format for cross-references to other pages."),
            Message::user(prompt),
        ],
    )
    .with_max_tokens(2000)
    .with_temperature(0.3);

    match client.chat(&request).await {
        Ok(response) => {
            if let Some(content) = response.content() {
                Some(HighLevelPage {
                    slug: slug.to_string(),
                    title: title.to_string(),
                    content: content.to_string(),
                })
            } else {
                eprintln!("[wiki-highlevel] Empty response for {}", slug);
                None
            }
        }
        Err(e) => {
            eprintln!("[wiki-highlevel] LLM error for {}: {}", slug, e);
            None
        }
    }
}

fn build_module_summary(docs: &[WikiDoc]) -> String {
    docs.iter()
        .filter(|d| d.file_count >= 2)
        .take(20)
        .map(|d| {
            let top_ep = d
                .entry_points
                .first()
                .map(|ep| ep.name.as_str())
                .unwrap_or("(no entry point)");
            format!(
                "- {} ({} files, {} symbols) — entry: {}",
                d.title, d.file_count, d.symbol_count, top_ep
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_dependency_text(docs: &[WikiDoc]) -> String {
    let mut deps = Vec::new();
    for doc in docs.iter().filter(|d| !d.dependencies.is_empty()).take(15) {
        for dep in doc.dependencies.iter().take(3) {
            deps.push(format!(
                "{} → {} ({})",
                doc.title, dep.target_name, dep.edge_type
            ));
        }
    }
    deps.join("\n")
}

fn build_concept_context(concept: &ConceptCandidate, docs: &[WikiDoc]) -> String {
    let related: Vec<String> = docs
        .iter()
        .filter(|d| concept.related_modules.contains(&d.slug))
        .take(5)
        .map(|d| {
            let apis = d
                .public_api
                .iter()
                .take(3)
                .map(|a| a.signature.clone())
                .collect::<Vec<_>>()
                .join("; ");
            format!("- {} ({} files): {}", d.title, d.file_count, apis)
        })
        .collect();
    related.join("\n")
}
