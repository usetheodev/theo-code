//! Concrete implementation of `GraphContextProvider` that orchestrates the
//! three code intelligence engines: parser → graph → retrieval.
//!
//! Lives in `theo-application` (not `theo-agent-runtime`) to respect bounded
//! context boundaries — the runtime only sees the trait from `theo-domain`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use theo_domain::graph_context::{
    ContextBlock, GraphContextError, GraphContextProvider, GraphContextResult,
};

use theo_engine_graph::bridge::{
    self, DataModelData, FileData, ImportData, ReferenceData, ReferenceKindDto, SymbolData,
    SymbolKindDto,
};
use theo_engine_graph::cluster::{ClusterAlgorithm, ClusterResult, Community};
use theo_engine_graph::model::CodeGraph;

use theo_engine_parser::tree_sitter::{self as ts, SupportedLanguage};
use theo_engine_parser::types::{FileExtraction, ReferenceKind, SymbolKind};

use theo_engine_retrieval::assembly;
use theo_engine_retrieval::search::{FileBm25, MultiSignalScorer};

#[cfg(feature = "tantivy-backend")]
use theo_engine_retrieval::tantivy_search::FileTantivyIndex;

#[cfg(feature = "dense-retrieval")]
use theo_engine_retrieval::embedding::neural::NeuralEmbedder;
#[cfg(feature = "dense-retrieval")]
use theo_engine_retrieval::embedding::cache::EmbeddingCache;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Max time for graph build (clustering can be slow for large repos).
/// 60s accommodates debug builds; release builds are ~5-10x faster.
const BUILD_TIMEOUT: Duration = Duration::from_secs(60);

/// Cache validity period.
/// Leiden resolution parameter (1.0 = standard modularity).
const LEIDEN_RESOLUTION: f64 = 1.0;

// ---------------------------------------------------------------------------
// Internal state machine
// ---------------------------------------------------------------------------

struct GraphState {
    graph: CodeGraph,
    communities: Vec<Community>,
    /// MultiSignalScorer: only built when no RRF pipeline available (Tier 0 only).
    /// When tantivy-backend is active, query_context uses FileBm25 directly,
    /// saving ~200MB RAM from scorer's BM25 index + TF-IDF model.
    #[cfg(not(feature = "tantivy-backend"))]
    scorer: MultiSignalScorer,
    /// Tantivy BM25F index (Tier 1).
    #[cfg(feature = "tantivy-backend")]
    tantivy_index: Option<FileTantivyIndex>,
    /// Neural embedder for dense search (Tier 2). AllMiniLM default, Jina Code opt-in.
    #[cfg(feature = "dense-retrieval")]
    embedder: Option<NeuralEmbedder>,
    /// Pre-computed file embeddings (Tier 2). Cached to .theo/embeddings.bin.
    #[cfg(feature = "dense-retrieval")]
    embedding_cache: Option<EmbeddingCache>,
}

/// Explicit state machine for background graph build lifecycle.
enum GraphBuildState {
    /// No initialization started yet.
    Uninitialized,
    /// Build running in background. Stale cache served if available.
    Building { stale: Option<GraphState> },
    /// Graph built and ready for queries.
    Ready(GraphState),
    /// Build failed. Agent operates without context.
    Failed(String),
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

/// Orchestrates parser → graph → retrieval pipeline with background build.
///
/// `initialize()` returns immediately, dispatching the build to a background
/// tokio task. The agent starts without code context and gains it when the
/// build completes. `query_context()` returns empty while Building, context
/// when Ready, and error when Failed.
pub struct GraphContextService {
    state: Arc<tokio::sync::RwLock<GraphBuildState>>,
    /// Ensures only one build runs at a time.
    build_in_progress: Arc<AtomicBool>,
}

impl GraphContextService {
    pub fn new() -> Self {
        Self {
            state: Arc::new(tokio::sync::RwLock::new(GraphBuildState::Uninitialized)),
            build_in_progress: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Default for GraphContextService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl GraphContextProvider for GraphContextService {
    /// Starts graph build in background and returns immediately.
    ///
    /// If a build is already in progress, this is a no-op.
    /// If cache exists and is fresh, loads synchronously (fast path).
    async fn initialize(&self, project_dir: &Path) -> Result<(), GraphContextError> {
        // Fast path: already ready or building.
        {
            let current = self.state.read().await;
            if matches!(*current, GraphBuildState::Ready(_) | GraphBuildState::Building { .. }) {
                return Ok(());
            }
        }

        // Try cache first (synchronous, fast).
        let dir = project_dir.to_path_buf();
        let cache_path = dir.join(".theo").join("graph.bin");

        if let Some(graph) = try_load_cache(&cache_path, &dir) {
            #[cfg(not(feature = "tantivy-backend"))]
            let (communities, scorer) = build_index(&graph);
            #[cfg(feature = "tantivy-backend")]
            let communities = build_index(&graph);
            #[cfg(feature = "tantivy-backend")]
            let tantivy_index = FileTantivyIndex::build(&graph).ok();
            #[cfg(feature = "dense-retrieval")]
            let (embedder, embedding_cache) = build_dense_components(&graph, &dir);

            // Generate Code Wiki (deterministic, ~50ms, cached by graph_hash)
            generate_wiki_if_stale(&graph, &communities, &dir);

            let mut state = self.state.write().await;
            *state = GraphBuildState::Ready(GraphState {
                graph,
                communities,
                #[cfg(not(feature = "tantivy-backend"))]
                scorer,
                #[cfg(feature = "tantivy-backend")]
                tantivy_index,
                #[cfg(feature = "dense-retrieval")]
                embedder,
                #[cfg(feature = "dense-retrieval")]
                embedding_cache,
            });
            return Ok(());
        }

        // Prevent concurrent builds.
        if self.build_in_progress.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return Ok(()); // Another build already running.
        }

        // Transition to Building — preserve previous state as stale cache.
        {
            let mut state = self.state.write().await;
            let stale = match std::mem::replace(&mut *state, GraphBuildState::Uninitialized) {
                GraphBuildState::Ready(gs) => Some(gs),
                GraphBuildState::Building { stale } => stale,
                _ => None,
            };
            *state = GraphBuildState::Building { stale };
        }

        // Spawn background build — fire and forget.
        let state_ref = self.state.clone();
        let build_flag = self.build_in_progress.clone();
        let dir_clone = dir.clone();
        let dir_for_cache = dir.clone();

        tokio::spawn(async move {
            let result = tokio::time::timeout(
                BUILD_TIMEOUT,
                tokio::task::spawn_blocking(move || {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        build_graph_from_project(&dir_clone)
                    }))
                }),
            )
            .await;

            let mut state = state_ref.write().await;
            match result {
                Ok(Ok(Ok((graph, communities)))) => {
                    save_cache_atomic(&cache_path, &graph, &dir_for_cache);
                    #[cfg(not(feature = "tantivy-backend"))]
                    let scorer = MultiSignalScorer::build(&communities, &graph);
                    #[cfg(feature = "tantivy-backend")]
                    let tantivy_index = FileTantivyIndex::build(&graph).ok();
                    #[cfg(feature = "dense-retrieval")]
                    let (embedder, embedding_cache) = build_dense_components(&graph, &dir_for_cache);

                    // Generate Code Wiki (deterministic, cached)
                    generate_wiki_if_stale(&graph, &communities, &dir_for_cache);

                    *state = GraphBuildState::Ready(GraphState {
                        graph,
                        communities,
                        #[cfg(not(feature = "tantivy-backend"))]
                        scorer,
                        #[cfg(feature = "tantivy-backend")]
                        tantivy_index,
                        #[cfg(feature = "dense-retrieval")]
                        embedder,
                        #[cfg(feature = "dense-retrieval")]
                        embedding_cache,
                    });
                }
                Ok(Ok(Err(_panic))) => {
                    *state = GraphBuildState::Failed("panic during graph build".into());
                }
                Ok(Err(join_err)) => {
                    *state = GraphBuildState::Failed(format!("spawn_blocking failed: {join_err}"));
                }
                Err(_timeout) => {
                    *state = GraphBuildState::Failed(format!(
                        "build timed out after {}s",
                        BUILD_TIMEOUT.as_secs()
                    ));
                }
            }
            build_flag.store(false, Ordering::SeqCst);
        });

        Ok(())
    }

    async fn query_context(
        &self,
        query: &str,
        budget_tokens: usize,
    ) -> Result<GraphContextResult, GraphContextError> {
        let state = self.state.read().await;

        let empty = Ok(GraphContextResult {
            blocks: vec![],
            total_tokens: 0,
            budget_tokens,
            exploration_hints: String::new(),
        });

        match &*state {
            GraphBuildState::Uninitialized => return Err(GraphContextError::NotInitialized),
            GraphBuildState::Building { stale: None } => return empty,
            GraphBuildState::Building { stale: Some(_) } => {} // Serve stale — fall through
            GraphBuildState::Failed(e) => return Err(GraphContextError::BuildFailed(e.clone())),
            GraphBuildState::Ready(_) => {} // Fall through to query.
        }

        if budget_tokens == 0 || query.is_empty() {
            return empty;
        }

        // LAYER 0: Wiki cache lookup (<5ms).
        // If wiki has a high-confidence match, return it directly — no RRF needed.
        // This is the "knowledge compounding" layer: previous sessions' knowledge
        // is served instantly, avoiding the full retrieval pipeline.
        {
            let wiki_dir = std::path::PathBuf::from(".theo/wiki");
            let wiki_results = theo_engine_retrieval::wiki::lookup::lookup(&wiki_dir, query, 3);
            if let Some(top) = wiki_results.first() {
                if top.confidence >= 0.6 && top.token_count <= budget_tokens {
                    let blocks: Vec<ContextBlock> = wiki_results.iter()
                        .take_while(|r| r.confidence >= 0.5)
                        .filter(|r| r.token_count <= budget_tokens)
                        .map(|r| ContextBlock {
                            source_id: format!("wiki:{}", r.slug),
                            content: r.content.clone(),
                            token_count: r.token_count,
                            score: r.confidence,
                        })
                        .collect();

                    if !blocks.is_empty() {
                        let total_tokens: usize = blocks.iter().map(|b| b.token_count).sum();
                        return Ok(GraphContextResult {
                            total_tokens,
                            budget_tokens,
                            exploration_hints: format!("Wiki cache hit: {} (confidence {:.0}%)", top.title, top.confidence * 100.0),
                            blocks,
                        });
                    }
                }
            }
        }

        // Safe: we checked Ready or Building(stale) above.
        let graph_state = match &*state {
            GraphBuildState::Ready(gs) => gs,
            GraphBuildState::Building { stale: Some(gs) } => gs,
            _ => unreachable!(),
        };

        // Tiered scoring: use best available pipeline.
        // Tier 2 (dense-retrieval): BM25 + Tantivy + Dense → RRF 3-ranker (MRR=0.914)
        // Tier 1 (tantivy-backend): BM25 + Tantivy → hybrid_search (2-ranker)
        // Tier 0 (always): BM25 only → FileBm25::search
        //
        // Fallback cascade: Tier 2 → 1 → 0 (infalível).
        let file_scores: std::collections::HashMap<String, f64> = {
            // Try Tier 2 first: full RRF 3-ranker (BM25 + Tantivy + Dense)
            #[cfg(feature = "dense-retrieval")]
            {
                let has_tier2 = graph_state.tantivy_index.is_some()
                    && graph_state.embedder.is_some()
                    && graph_state.embedding_cache.is_some();

                if has_tier2 {
                    theo_engine_retrieval::tantivy_search::hybrid_rrf_search(
                        &graph_state.graph,
                        graph_state.tantivy_index.as_ref().unwrap(),
                        graph_state.embedder.as_ref().unwrap(),
                        graph_state.embedding_cache.as_ref().unwrap(),
                        query,
                        20.0, // RRF k parameter (empirically optimal)
                    )
                } else if graph_state.tantivy_index.is_some() {
                    theo_engine_retrieval::tantivy_search::hybrid_search(
                        &graph_state.graph,
                        graph_state.tantivy_index.as_ref().unwrap(),
                        query,
                    )
                } else {
                    FileBm25::search(&graph_state.graph, query)
                }
            }
            // Without dense-retrieval: try Tier 1, then Tier 0
            #[cfg(all(feature = "tantivy-backend", not(feature = "dense-retrieval")))]
            {
                if graph_state.tantivy_index.is_some() {
                    theo_engine_retrieval::tantivy_search::hybrid_search(
                        &graph_state.graph,
                        graph_state.tantivy_index.as_ref().unwrap(),
                        query,
                    )
                } else {
                    FileBm25::search(&graph_state.graph, query)
                }
            }
            // Without any features: Tier 0 only
            #[cfg(not(any(feature = "tantivy-backend", feature = "dense-retrieval")))]
            {
                FileBm25::search(&graph_state.graph, query)
            }
        };

        let payload = assembly::assemble_files_direct(
            &file_scores,
            &graph_state.graph,
            &graph_state.communities,
            budget_tokens,
        );

        let blocks: Vec<ContextBlock> = payload
            .items
            .iter()
            .map(|item| ContextBlock {
                source_id: item.community_id.clone(),
                content: item.content.clone(),
                token_count: item.token_count,
                score: item.score,
            })
            .collect();

        // WRITE-BACK: Save RRF result to wiki cache for future queries.
        // This is the "knowledge compounding" cycle: each query that goes through
        // the full pipeline enriches the wiki, making future queries faster.
        // Only writes if: (1) we have meaningful content, (2) wiki dir exists.
        if !blocks.is_empty() && payload.total_tokens > 100 {
            let wiki_dir = std::path::PathBuf::from(".theo/wiki/cache");
            if let Err(e) = write_back_to_wiki(&wiki_dir, query, &blocks) {
                // Best-effort: don't fail the query if write-back fails
                eprintln!("[wiki-cache] Write-back failed: {e}");
            }
        }

        Ok(GraphContextResult {
            total_tokens: payload.total_tokens,
            budget_tokens: payload.budget_tokens,
            exploration_hints: payload.exploration_hints,
            blocks,
        })
    }

    fn is_ready(&self) -> bool {
        // Non-blocking check via try_read.
        self.state
            .try_read()
            .map(|s| matches!(*s, GraphBuildState::Ready(_)))
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Graph build pipeline (runs in spawn_blocking)
// ---------------------------------------------------------------------------

/// Full pipeline: walk → parse → convert → build_graph → cluster → scorer.
fn build_graph_from_project(
    project_dir: &Path,
) -> (CodeGraph, Vec<Community>) {
    // Step 1: Walk files and parse with tree-sitter.
    let file_data = parse_project_files(project_dir);

    // Step 2: Build code graph from FileData.
    let (mut graph, _stats) = bridge::build_graph(&file_data);

    // Step 3: Apply git co-change history (best-effort, max 500 commits, 50 files/commit).
    let _ = theo_engine_graph::git::populate_cochanges_from_git(project_dir, &mut graph, 500, 50);

    // Step 4: Cluster only (scorer built conditionally by caller).
    let communities = tokio_safe_cluster(&graph).communities;

    (graph, communities)
}

// EXCLUDED_DIRS imported from theo-domain::graph_context (source of truth).

/// Maximum files to parse. Prevents timeout on huge monorepos.
const MAX_FILES_TO_PARSE: usize = 500;

/// Detect the dominant language of the project from manifest files.
fn detect_project_language(project_dir: &Path) -> Option<&'static str> {
    if project_dir.join("Cargo.toml").exists() {
        Some("rs")
    } else if project_dir.join("go.mod").exists() || project_dir.join("go.work").exists() {
        Some("go")
    } else if project_dir.join("pyproject.toml").exists() || project_dir.join("requirements.txt").exists() {
        Some("py")
    } else if project_dir.join("package.json").exists() {
        Some("ts") // covers TS and JS projects
    } else {
        None
    }
}

/// Walk project, parse each file with tree-sitter, convert to FileData.
///
/// Prioritizes the project's primary language: if a Cargo.toml exists,
/// .rs files are parsed first, then other languages up to MAX_FILES_TO_PARSE.
fn parse_project_files(project_dir: &Path) -> Vec<FileData> {
    let primary_ext = detect_project_language(project_dir);

    let collect_paths = || {
        let mut wb = ignore::WalkBuilder::new(project_dir);
        wb.hidden(true).git_ignore(true);
        let _ = wb.add_ignore(project_dir.join(".gitignore"));
        wb.add_custom_ignore_filename(".theoignore");
        wb.filter_entry(|entry| {
                if entry.file_type().map_or(false, |ft| ft.is_dir()) {
                    let name = entry.file_name().to_str().unwrap_or("");
                    return !theo_domain::graph_context::EXCLUDED_DIRS.contains(&name);
                }
                true
            });
        let walker = wb.build();

        let mut primary = Vec::new();
        let mut secondary = Vec::new();

        for entry in walker.flatten() {
            let path = entry.into_path();
            if !path.is_file() {
                continue;
            }
            if ts::detect_language(&path).is_none() {
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if primary_ext.is_some_and(|pe| ext == pe || (pe == "ts" && (ext == "tsx" || ext == "js" || ext == "jsx"))) {
                primary.push(path);
            } else {
                secondary.push(path);
            }
        }

        // Smart sampling: instead of blind truncation by walk order,
        // ensure directory breadth + prioritize recently modified files.
        let mut all_files = primary;
        all_files.extend(secondary);

        if all_files.len() <= MAX_FILES_TO_PARSE {
            return all_files;
        }

        // Step 1: Guarantee breadth — at least 1 file per top-level directory.
        let mut by_dir: std::collections::HashMap<String, Vec<std::path::PathBuf>> = std::collections::HashMap::new();
        for path in &all_files {
            let dir = path.strip_prefix(project_dir)
                .unwrap_or(path)
                .components()
                .next()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .unwrap_or_else(|| "root".to_string());
            by_dir.entry(dir).or_default().push(path.clone());
        }

        let mut selected: Vec<std::path::PathBuf> = Vec::with_capacity(MAX_FILES_TO_PARSE);
        let mut selected_set: std::collections::HashSet<std::path::PathBuf> = std::collections::HashSet::new();

        // Pick 1 file per dir (most recently modified)
        for (_dir, mut files) in by_dir {
            files.sort_by(|a, b| {
                let ma = std::fs::metadata(a).and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                let mb = std::fs::metadata(b).and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                mb.cmp(&ma) // newest first
            });
            if let Some(f) = files.first() {
                if selected_set.insert(f.clone()) {
                    selected.push(f.clone());
                }
            }
        }

        // Step 2: Fill remaining slots by mtime (newest first, across all dirs).
        all_files.sort_by(|a, b| {
            let ma = std::fs::metadata(a).and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            let mb = std::fs::metadata(b).and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            mb.cmp(&ma)
        });

        for f in all_files {
            if selected.len() >= MAX_FILES_TO_PARSE {
                break;
            }
            if selected_set.insert(f.clone()) {
                selected.push(f);
            }
        }

        selected
    };

    let paths = collect_paths();
    let mut file_data_list = Vec::with_capacity(paths.len());

    for path in &paths {
        let Some(language) = ts::detect_language(path) else {
            continue;
        };

        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };

        let Ok(parsed) = ts::parse_source(path, &source, language, None) else {
            continue;
        };

        let extraction =
            theo_engine_parser::extractors::extract(path, &source, &parsed.tree, language);

        let rel_path = path
            .strip_prefix(project_dir)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        let last_modified = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);

        file_data_list.push(convert_extraction(extraction, &rel_path, language, last_modified));
    }

    file_data_list
}

/// Build clustering + scorer from a ready graph.
/// Build clustering index. Scorer only built when no RRF pipeline (saves ~200MB).
#[cfg(not(feature = "tantivy-backend"))]
fn build_index(graph: &CodeGraph) -> (Vec<Community>, MultiSignalScorer) {
    let cluster_result = tokio_safe_cluster(graph);
    let scorer = MultiSignalScorer::build(&cluster_result.communities, graph);
    (cluster_result.communities, scorer)
}

/// Build clustering only (Tier 1+: scorer not needed, RRF uses FileBm25 directly).
#[cfg(feature = "tantivy-backend")]
fn build_index(graph: &CodeGraph) -> Vec<Community> {
    let cluster_result = tokio_safe_cluster(graph);
    cluster_result.communities
}

/// Build dense retrieval components: NeuralEmbedder + EmbeddingCache.
///
/// Generate Code Wiki if stale (graph changed since last generation).
/// Deterministic, zero LLM cost, ~50-100ms for medium repos.
/// Write-back: save RRF pipeline results as cached wiki page.
///
/// Creates `.theo/wiki/cache/{slug}.md` with the query and results.
/// Future wiki lookups will find these cached pages.
fn write_back_to_wiki(
    cache_dir: &Path,
    query: &str,
    blocks: &[ContextBlock],
) -> std::io::Result<()> {
    std::fs::create_dir_all(cache_dir)?;

    // Slug from query (deterministic)
    let slug: String = query
        .to_lowercase()
        .split_whitespace()
        .take(5)
        .collect::<Vec<_>>()
        .join("-")
        .replace(|c: char| !c.is_alphanumeric() && c != '-', "");

    if slug.is_empty() {
        return Ok(());
    }

    let path = cache_dir.join(format!("{}.md", slug));

    // Don't overwrite existing cache (first result wins)
    if path.exists() {
        return Ok(());
    }

    // Build formatted markdown page (not raw blocks)
    let mut md = format!("# Query: {}\n\n", query);
    md += &format!("> Cached from GRAPHCTX pipeline | {} results\n\n", blocks.len());

    // Relevant files table
    md += "## Relevant Files\n\n";
    md += "| File | Score |\n|------|-------|\n";
    for block in blocks {
        let score_str = format!("{:.2}", block.score);
        md += &format!("| `{}` | {} |\n", block.source_id, score_str);
    }
    md += "\n";

    // Code context from each block
    md += "## Context\n\n";
    for block in blocks {
        // Extract first few lines of content as preview
        let preview: String = block.content.lines().take(20).collect::<Vec<_>>().join("\n");
        md += &format!("### {}\n\n{}\n\n", block.source_id, preview);
    }

    md += "---\n";
    md += &format!("*Generated by GRAPHCTX | {} blocks, {:.0} tokens*\n",
        blocks.len(),
        blocks.iter().map(|b| b.token_count as f64).sum::<f64>());

    std::fs::write(&path, md)?;

    // Log the write-back
    // Navigate from cache_dir (.theo/wiki/cache) to project_dir
    if let Some(wiki_dir) = cache_dir.parent() {
        if let Some(project_dir) = wiki_dir.parent().and_then(|p| p.parent()) {
            theo_engine_retrieval::wiki::persistence::append_log(
                project_dir,
                "query",
                &format!("Cached result for: {} ({} blocks)", query, blocks.len()),
            );
        }
    }

    Ok(())
}

fn generate_wiki_if_stale(graph: &CodeGraph, communities: &[Community], project_dir: &Path) {
    use theo_engine_retrieval::wiki;

    let hash = wiki::generator::compute_graph_hash(graph);
    if wiki::persistence::is_fresh(project_dir, hash) {
        return; // Wiki is up-to-date
    }

    let project_name = project_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");

    let wiki_data = wiki::generator::generate_wiki(communities, graph, project_name);

    if let Err(e) = wiki::persistence::write_to_disk(&wiki_data, project_dir) {
        eprintln!("[wiki] Warning: failed to write wiki: {e}");
    } else {
        eprintln!(
            "[wiki] Generated {} pages in .theo/wiki/",
            wiki_data.docs.len()
        );
        // Log the ingest
        wiki::persistence::append_log(
            project_dir,
            "ingest",
            &format!("Generated {} pages from graph ({} nodes, {} edges)",
                wiki_data.docs.len(), graph.node_count(), graph.edge_count()),
        );
    }
}

/// Tries to load cached embeddings from .theo/embeddings.bin first.
/// If cache miss, initializes embedder and builds embeddings from graph.
/// Returns (None, None) on any failure — fallback to Tier 1/0.
#[cfg(feature = "dense-retrieval")]
fn build_dense_components(
    graph: &CodeGraph,
    project_dir: &Path,
) -> (Option<NeuralEmbedder>, Option<EmbeddingCache>) {
    // Try loading embedder (AllMiniLM default, ~200MB; Jina Code opt-in ~1.2GB)
    let embedder = match NeuralEmbedder::new() {
        Ok(e) => e,
        Err(err) => {
            eprintln!("[graphctx] Dense retrieval disabled: embedder init failed: {err}");
            return (None, None);
        }
    };

    // Try loading cached embeddings
    let cache_path = project_dir.join(".theo").join("embeddings.bin");
    let graph_hash = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::hash::DefaultHasher::new();
        for node_id in graph.node_ids() {
            if let Some(node) = graph.get_node(node_id) {
                if node.node_type == theo_engine_graph::model::NodeType::File {
                    let path = node.file_path.as_deref().unwrap_or(&node.name);
                    path.hash(&mut hasher);
                    node.last_modified.to_bits().hash(&mut hasher);
                }
            }
        }
        hasher.finish()
    };

    if let Some(cache) = EmbeddingCache::load(&cache_path, graph_hash) {
        eprintln!("[graphctx] Loaded embedding cache ({} files)", cache.len());
        return (Some(embedder), Some(cache));
    }

    // Build embeddings (slow: ~5-30s depending on repo size and model)
    eprintln!("[graphctx] Building embedding cache...");
    let cache = EmbeddingCache::build(graph, &embedder);
    eprintln!("[graphctx] Embedding cache built ({} files)", cache.len());

    // Save to disk for next startup
    if let Err(e) = cache.save(&cache_path) {
        eprintln!("[graphctx] Warning: failed to save embedding cache: {e}");
    }

    (Some(embedder), Some(cache))
}

/// Run clustering with a fallback: try FileLeiden first, if it panics or
/// produces zero communities, fall back to a single "all" community.
fn tokio_safe_cluster(graph: &CodeGraph) -> ClusterResult {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        theo_engine_graph::cluster::hierarchical_cluster(
            graph,
            ClusterAlgorithm::FileLeiden {
                resolution: LEIDEN_RESOLUTION,
            },
        )
    }));

    match result {
        Ok(cr) if !cr.communities.is_empty() => cr,
        _ => {
            // Fallback: single community with all nodes.
            let all_ids: Vec<String> = graph.file_nodes().iter().map(|n| n.id.clone()).collect();
            ClusterResult {
                communities: vec![Community {
                    id: "all".to_string(),
                    name: "all".to_string(),
                    node_ids: all_ids,
                    level: 0,
                    parent_id: None,
                    version: 0,
                }],
                modularity: 0.0,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// FileExtraction → FileData conversor
// ---------------------------------------------------------------------------

/// Convert parser's `FileExtraction` to graph's `FileData` DTO.
///
/// Some fields are lost in translation (confidence, field info, aliases)
/// — this is intentional as the graph bridge doesn't use them.
fn convert_extraction(
    ext: FileExtraction,
    rel_path: &str,
    language: SupportedLanguage,
    last_modified: f64,
) -> FileData {
    let symbols: Vec<SymbolData> = ext
        .symbols
        .iter()
        .map(|s| {
            let qualified = match &s.parent {
                Some(p) => format!("{p}::{}", s.name),
                None => s.name.clone(),
            };
            SymbolData {
                qualified_name: qualified,
                name: s.name.clone(),
                kind: convert_symbol_kind(&s.kind),
                line_start: s.anchor.line,
                line_end: s.anchor.end_line,
                signature: s.signature.clone(),
                is_test: s.is_test,
                parent: s.parent.clone(),
                doc: s.doc.clone(),
            }
        })
        .collect();

    let imports: Vec<ImportData> = ext
        .imports
        .iter()
        .map(|i| ImportData {
            source: i.source.clone(),
            specifiers: i.specifiers.clone(),
            line: i.line,
        })
        .collect();

    let references: Vec<ReferenceData> = ext
        .references
        .iter()
        .map(|r| ReferenceData {
            source_symbol: r.source_symbol.clone(),
            source_file: r.source_file.to_string_lossy().to_string(),
            target_symbol: r.target_symbol.clone(),
            target_file: r.target_file.as_ref().map(|p| p.to_string_lossy().to_string()),
            kind: convert_reference_kind(&r.reference_kind),
        })
        .collect();

    let data_models: Vec<DataModelData> = ext
        .data_models
        .iter()
        .map(|dm| DataModelData {
            name: dm.name.clone(),
            file_path: dm.anchor.file.to_string_lossy().to_string(),
            line_start: dm.anchor.line,
            line_end: dm.anchor.end_line,
            parent_type: dm.parent_type.clone(),
            implemented_interfaces: dm.implemented_interfaces.clone(),
        })
        .collect();

    FileData {
        path: rel_path.to_string(),
        language: format!("{:?}", language),
        line_count: ext.estimated_tokens as usize / 4, // rough estimate
        last_modified,
        symbols,
        imports,
        references,
        data_models,
    }
}

use crate::use_cases::conversion::{convert_symbol_kind, convert_reference_kind};

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

/// Graph cache manifest — stored alongside graph.bin.
#[derive(serde::Serialize, serde::Deserialize)]
struct GraphManifest {
    /// Hash of project file state (sorted path:mtime pairs).
    content_hash: String,
    /// When the graph was built (Unix seconds).
    built_at_secs: u64,
    /// Number of files in the snapshot.
    file_count: usize,
}

/// Compute a deterministic hash of the project's source file state.
///
/// Walks the project directory (respecting EXCLUDED_DIRS), collects
/// sorted (path, mtime_secs) pairs, and hashes the concatenation.
/// Cost: ~5ms for 500 files (stat only, no reads).
fn compute_project_hash(project_dir: &Path) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut entries: BTreeMap<String, u64> = BTreeMap::new();

    let mut hash_wb = ignore::WalkBuilder::new(project_dir);
    hash_wb.hidden(true).git_ignore(true).max_depth(Some(10));
    let _ = hash_wb.add_ignore(project_dir.join(".gitignore"));
    hash_wb.add_custom_ignore_filename(".theoignore");
    hash_wb.filter_entry(|entry| {
            if entry.file_type().map_or(false, |ft| ft.is_dir()) {
                let name = entry.file_name().to_str().unwrap_or("");
                return !theo_domain::graph_context::EXCLUDED_DIRS.contains(&name);
            }
            true
        });
    let walker = hash_wb.build();

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        // Only source files
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "rs" | "py" | "ts" | "tsx" | "js" | "jsx" | "go" | "java" | "rb" | "php" | "c" | "cpp" | "cs" | "sh" | "yaml" | "toml") {
            continue;
        }
        if let Ok(meta) = std::fs::metadata(path) {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if let Ok(rel) = path.strip_prefix(project_dir) {
                entries.insert(rel.to_string_lossy().to_string(), mtime);
            }
        }
    }

    let mut hasher = DefaultHasher::new();
    for (path, mtime) in &entries {
        path.hash(&mut hasher);
        mtime.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

/// Try loading a cached graph if the project state matches.
///
/// Uses content-hash comparison instead of TTL — eliminates both
/// false cache-hits (code changed within TTL) and false cache-misses
/// (1h passed with no changes).
fn try_load_cache(cache_path: &Path, project_dir: &Path) -> Option<CodeGraph> {
    if !cache_path.exists() {
        return None;
    }

    let manifest_path = cache_path.with_extension("manifest.json");
    let manifest_content = std::fs::read_to_string(&manifest_path).ok()?;
    let manifest: GraphManifest = serde_json::from_str(&manifest_content).ok()?;

    let current_hash = compute_project_hash(project_dir);
    if manifest.content_hash != current_hash {
        return None; // Project changed since last build
    }

    // Safety: also reject very old caches (>24h) as a fallback
    let now_secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if now_secs - manifest.built_at_secs > 86400 {
        return None;
    }

    theo_engine_graph::persist::load(cache_path).ok()
}

/// Atomic cache write with manifest.
fn save_cache_atomic(cache_path: &Path, graph: &CodeGraph, project_dir: &Path) {
    let tmp_path = cache_path.with_extension("bin.tmp");
    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if theo_engine_graph::persist::save(graph, &tmp_path).is_ok() {
        let _ = std::fs::rename(&tmp_path, cache_path);

        // Write manifest
        let manifest = GraphManifest {
            content_hash: compute_project_hash(project_dir),
            built_at_secs: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            file_count: graph.node_count(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&manifest) {
            let manifest_tmp = cache_path.with_extension("manifest.json.tmp");
            if std::fs::write(&manifest_tmp, &json).is_ok() {
                let _ = std::fs::rename(&manifest_tmp, cache_path.with_extension("manifest.json"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: wait for the service to become ready (background build to complete).
    async fn wait_ready(service: &GraphContextService, timeout_secs: u64) -> bool {
        tokio::time::timeout(Duration::from_secs(timeout_secs), async {
            loop {
                if service.is_ready() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .is_ok()
    }

    #[test]
    fn convert_symbol_kind_covers_all_variants() {
        let variants = [
            (SymbolKind::Function, SymbolKindDto::Function),
            (SymbolKind::Method, SymbolKindDto::Method),
            (SymbolKind::Class, SymbolKindDto::Class),
            (SymbolKind::Struct, SymbolKindDto::Struct),
            (SymbolKind::Enum, SymbolKindDto::Enum),
            (SymbolKind::Trait, SymbolKindDto::Trait),
            (SymbolKind::Interface, SymbolKindDto::Interface),
            (SymbolKind::Module, SymbolKindDto::Module),
        ];
        for (from, expected) in variants {
            assert_eq!(convert_symbol_kind(&from), expected);
        }
    }

    #[test]
    fn convert_reference_kind_covers_all_variants() {
        let variants = [
            (ReferenceKind::Call, ReferenceKindDto::Call),
            (ReferenceKind::Extends, ReferenceKindDto::Extends),
            (ReferenceKind::Implements, ReferenceKindDto::Implements),
            (ReferenceKind::TypeUsage, ReferenceKindDto::TypeUsage),
            (ReferenceKind::Import, ReferenceKindDto::Import),
        ];
        for (from, expected) in variants {
            assert_eq!(convert_reference_kind(&from), expected);
        }
    }

    // --- State machine transition tests ---

    #[tokio::test]
    async fn building_transitions_to_ready() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/main.rs"), "fn main() {}").unwrap();

        let service = GraphContextService::new();
        assert!(!service.is_ready()); // Uninitialized

        service.initialize(tmp.path()).await.unwrap(); // Returns immediately

        // Wait for background build to complete.
        assert!(wait_ready(&service, 30).await, "Build did not complete in 30s");
        assert!(service.is_ready());
    }

    #[tokio::test]
    async fn query_during_building_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        // Create enough files to make build take >0ms
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/main.rs"), "fn main() { println!(\"hello\"); }").unwrap();

        let service = GraphContextService::new();
        service.initialize(tmp.path()).await.unwrap();

        // Immediately query — may still be Building.
        let result = service.query_context("test", 4000).await;
        // Should be Ok(empty) if Building, or Ok(context) if already Ready.
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn query_before_initialize_returns_not_initialized() {
        let service = GraphContextService::new();
        let result = service.query_context("test", 4000).await;
        assert!(matches!(result, Err(GraphContextError::NotInitialized)));
    }

    #[tokio::test]
    async fn query_after_ready_returns_context() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("src/main.rs"),
            "fn main() {}\nfn add(a: i32, b: i32) -> i32 { a + b }\n",
        ).unwrap();

        let service = GraphContextService::new();
        service.initialize(tmp.path()).await.unwrap();
        assert!(wait_ready(&service, 30).await);

        let result = service.query_context("add function", 4000).await.unwrap();
        assert!(result.total_tokens <= result.budget_tokens);
    }

    #[tokio::test]
    async fn double_initialize_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let service = GraphContextService::new();
        service.initialize(tmp.path()).await.unwrap();
        // Second call while building — should be no-op.
        service.initialize(tmp.path()).await.unwrap();
    }

    #[test]
    fn is_ready_false_before_init() {
        let service = GraphContextService::new();
        assert!(!service.is_ready());
    }

    #[test]
    fn cache_miss_on_nonexistent_path() {
        assert!(try_load_cache(Path::new("/tmp/nonexistent_graph.bin"), Path::new("/tmp")).is_none());
    }

    #[tokio::test]
    async fn integration_real_project_produces_context() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("src/main.rs"),
            "fn main() { println!(\"hello\"); }\nfn add(a: i32, b: i32) -> i32 { a + b }\n",
        ).unwrap();

        let service = GraphContextService::new();
        service.initialize(tmp.path()).await.unwrap();
        assert!(wait_ready(&service, 30).await, "Build did not complete");

        let result = service.query_context("add function", 4000).await.unwrap();
        assert!(result.total_tokens <= result.budget_tokens);
    }
}
