//! Concrete implementation of `GraphContextProvider` that orchestrates the
//! three code intelligence engines: parser → graph → retrieval.
//!
//! Lives in `theo-application` (not `theo-agent-runtime`) to respect bounded
//! context boundaries — the runtime only sees the trait from `theo-domain`.

use std::path::Path;
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
use theo_engine_retrieval::search::MultiSignalScorer;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Max time for graph build (clustering can be slow for large repos).
/// 60s accommodates debug builds; release builds are ~5-10x faster.
const BUILD_TIMEOUT: Duration = Duration::from_secs(60);

/// Cache validity period.
const CACHE_MAX_AGE: Duration = Duration::from_secs(3600); // 1 hour

/// Leiden resolution parameter (1.0 = standard modularity).
const LEIDEN_RESOLUTION: f64 = 1.0;

// ---------------------------------------------------------------------------
// Internal state machine
// ---------------------------------------------------------------------------

struct GraphState {
    graph: CodeGraph,
    communities: Vec<Community>,
    scorer: MultiSignalScorer,
}

/// Explicit state machine for background graph build lifecycle.
enum GraphBuildState {
    /// No initialization started yet.
    Uninitialized,
    /// Build running in background. Agent operates without context.
    Building,
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
            if matches!(*current, GraphBuildState::Ready(_) | GraphBuildState::Building) {
                return Ok(());
            }
        }

        // Try cache first (synchronous, fast).
        let dir = project_dir.to_path_buf();
        let cache_path = dir.join(".theo").join("graph.bin");

        if let Some(graph) = try_load_cache(&cache_path) {
            let (communities, scorer) = build_index(&graph);
            let mut state = self.state.write().await;
            *state = GraphBuildState::Ready(GraphState {
                graph,
                communities,
                scorer,
            });
            return Ok(());
        }

        // Prevent concurrent builds.
        if self.build_in_progress.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return Ok(()); // Another build already running.
        }

        // Transition to Building.
        {
            let mut state = self.state.write().await;
            *state = GraphBuildState::Building;
        }

        // Spawn background build — fire and forget.
        let state_ref = self.state.clone();
        let build_flag = self.build_in_progress.clone();
        let dir_clone = dir.clone();

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
                Ok(Ok(Ok((graph, communities, scorer)))) => {
                    save_cache_atomic(&cache_path, &graph);
                    *state = GraphBuildState::Ready(GraphState {
                        graph,
                        communities,
                        scorer,
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
            GraphBuildState::Building => return empty, // Agent runs without context.
            GraphBuildState::Failed(e) => return Err(GraphContextError::BuildFailed(e.clone())),
            GraphBuildState::Ready(_) => {} // Fall through to query.
        }

        if budget_tokens == 0 || query.is_empty() {
            return empty;
        }

        // Safe: we checked Ready above.
        let graph_state = match &*state {
            GraphBuildState::Ready(gs) => gs,
            _ => unreachable!(),
        };

        // Score + assemble (fast, ~20-30ms).
        let scored = graph_state
            .scorer
            .score(query, &graph_state.communities, &graph_state.graph);
        let payload = assembly::assemble_greedy(&scored, &graph_state.graph, budget_tokens);

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
) -> (CodeGraph, Vec<Community>, MultiSignalScorer) {
    // Step 1: Walk files and parse with tree-sitter.
    let file_data = parse_project_files(project_dir);

    // Step 2: Build code graph from FileData.
    let (mut graph, _stats) = bridge::build_graph(&file_data);

    // Step 3: Apply git co-change history (best-effort, max 500 commits, 50 files/commit).
    let _ = theo_engine_graph::git::populate_cochanges_from_git(project_dir, &mut graph, 500, 50);

    // Step 4: Build index (cluster + scorer).
    let (communities, scorer) = build_index(&graph);

    (graph, communities, scorer)
}

/// Directories to always exclude from graph indexing.
const EXCLUDED_DIRS: &[&str] = &[
    "target", "node_modules", "vendor", "dist", "build",
    "__pycache__", ".venv", "venv", ".next", ".nuxt",
];

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
        let walker = ignore::WalkBuilder::new(project_dir)
            .hidden(true)
            .git_ignore(true)
            .filter_entry(|entry| {
                if entry.file_type().map_or(false, |ft| ft.is_dir()) {
                    let name = entry.file_name().to_str().unwrap_or("");
                    return !EXCLUDED_DIRS.contains(&name);
                }
                true
            })
            .build();

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

        // Primary language first, then secondary, capped.
        primary.truncate(MAX_FILES_TO_PARSE);
        let remaining = MAX_FILES_TO_PARSE.saturating_sub(primary.len());
        secondary.truncate(remaining);
        primary.extend(secondary);
        primary
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
fn build_index(graph: &CodeGraph) -> (Vec<Community>, MultiSignalScorer) {
    let cluster_result = tokio_safe_cluster(graph);
    let scorer = MultiSignalScorer::build(&cluster_result.communities, graph);
    (cluster_result.communities, scorer)
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

/// Try loading a cached graph if it exists and is fresh enough.
fn try_load_cache(cache_path: &Path) -> Option<CodeGraph> {
    if !cache_path.exists() {
        return None;
    }

    let metadata = std::fs::metadata(cache_path).ok()?;
    let modified = metadata.modified().ok()?;
    let age = SystemTime::now().duration_since(modified).ok()?;

    if age > CACHE_MAX_AGE {
        return None;
    }

    theo_engine_graph::persist::load(cache_path).ok()
}

/// Atomic cache write: write to .tmp, then rename.
fn save_cache_atomic(cache_path: &Path, graph: &CodeGraph) {
    let tmp_path = cache_path.with_extension("bin.tmp");
    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if theo_engine_graph::persist::save(graph, &tmp_path).is_ok() {
        let _ = std::fs::rename(&tmp_path, cache_path);
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
        assert!(try_load_cache(Path::new("/tmp/nonexistent_graph.bin")).is_none());
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
