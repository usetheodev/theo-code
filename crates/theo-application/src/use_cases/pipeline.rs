/// GRAPHCTX Pipeline — the E2E orchestrator.
///
/// Connects all subsystems: graph construction → clustering → search → assembly.
/// This is the core of the Theo Code context engine.
use std::path::Path;
use std::time::Instant;

use std::collections::HashMap;

pub use theo_domain::graph_context::ImpactReport;
pub use theo_engine_graph::bridge::{BridgeStats, FileData};
pub use theo_engine_graph::cluster::Community;
use theo_engine_retrieval::assembly::{self, ContextPayload};
use theo_engine_retrieval::budget::BudgetConfig;
use theo_engine_retrieval::escape::ContextMembership;
use theo_engine_retrieval::search::MultiSignalScorer;
use theo_engine_retrieval::summary::{self, CommunitySummary, FileGitInfo};

use super::impact;
use theo_engine_graph::bridge;
use theo_engine_graph::cluster::{self, ClusterAlgorithm};
use theo_engine_graph::git;
use theo_engine_graph::model::{CodeGraph, EdgeType};
use theo_engine_graph::persist;

use crate::use_cases::extraction as extract;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Pipeline configuration.
pub struct PipelineConfig {
    /// Maximum token budget for context assembly.
    pub token_budget: usize,
    /// Maximum commits to process for co-change edges.
    pub max_git_commits: usize,
    /// Skip git commits touching more than this many files.
    pub max_files_per_commit: usize,
    /// BFS depth for impact analysis.
    pub impact_bfs_depth: usize,
    /// Path to persist the graph (None = no persistence).
    pub graph_cache_path: Option<String>,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        PipelineConfig {
            token_budget: 16_384,
            max_git_commits: 500,
            max_files_per_commit: 20,
            impact_bfs_depth: 3,
            graph_cache_path: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Pipeline Result
// ---------------------------------------------------------------------------

/// Result of a full pipeline run.
pub struct PipelineResult {
    /// The assembled context payload.
    pub context: ContextPayload,
    /// Communities detected in the codebase.
    pub communities: Vec<Community>,
    /// Pipeline timing in milliseconds.
    pub timing: PipelineTiming,
    /// Graph statistics.
    pub graph_stats: GraphStats,
}

/// Timing breakdown for each pipeline stage.
pub struct PipelineTiming {
    pub graph_build_ms: u64,
    pub git_cochanges_ms: u64,
    pub clustering_ms: u64,
    pub search_ms: u64,
    pub assembly_ms: u64,
    pub total_ms: u64,
}

/// Graph statistics.
pub struct GraphStats {
    pub nodes: usize,
    pub edges: usize,
    pub files: usize,
    pub symbols: usize,
    pub communities: usize,
}

/// Result of an incremental file update.
pub struct UpdateResult {
    /// Number of old nodes removed from the graph.
    pub nodes_removed: usize,
    /// Number of new nodes added to the graph.
    pub nodes_added: usize,
    /// Total edges that changed (removed + added).
    pub edges_changed: usize,
    /// Whether a full re-cluster was triggered (>10% edge change ratio).
    pub recluster_triggered: bool,
    /// Number of community summaries regenerated.
    pub summaries_regenerated: usize,
    /// Wall-clock time for the update in milliseconds.
    pub elapsed_ms: u64,
}

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

/// The GRAPHCTX pipeline. Stateful — holds the graph and communities.
pub struct Pipeline {
    config: PipelineConfig,
    graph: CodeGraph,
    communities: Vec<Community>,
    summaries: HashMap<String, CommunitySummary>,
    git_info: HashMap<String, FileGitInfo>,
    membership: Option<ContextMembership>,
    /// Edge count at the time of last full clustering (for incremental recluster decision).
    total_edges_at_last_cluster: usize,
    /// Cached multi-signal scorer — rebuilt only when communities change.
    cached_scorer: Option<MultiSignalScorer>,
}

impl Pipeline {
    /// Create a new pipeline with the given configuration.
    pub fn new(config: PipelineConfig) -> Self {
        Pipeline {
            config,
            graph: CodeGraph::new(),
            communities: Vec::new(),
            summaries: HashMap::new(),
            git_info: HashMap::new(),
            membership: None,
            total_edges_at_last_cluster: 0,
            cached_scorer: None,
        }
    }

    /// Create a pipeline with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(PipelineConfig::default())
    }

    /// Build the graph from extracted file data.
    ///
    /// This is the main entry point when Intently (theo-deep-code) provides
    /// the extraction. For lightweight usage without Intently, use `build_from_directory`.
    pub fn build_graph(&mut self, files: &[FileData]) -> BridgeStats {
        let (graph, stats) = bridge::build_graph(files);
        self.graph = graph;
        stats
    }

    /// Build the graph by walking a directory (file nodes only, no symbol extraction).
    pub fn build_from_directory(&mut self, repo_root: &Path) -> BridgeStats {
        let files = bridge::walk_files(repo_root);
        self.build_graph(&files)
    }

    /// Incrementally update the graph when a single file changes.
    ///
    /// 1. Re-parse only the changed file with tree-sitter
    /// 2. Remove old nodes/edges for this file from the graph
    /// 3. Add new nodes/edges from the fresh parse
    /// 4. Track dirty communities (those containing changed nodes)
    /// 5. Only re-cluster if >10% of edges changed; otherwise just regenerate
    ///    summaries for dirty communities
    ///
    /// Returns the number of nodes changed and whether re-clustering was triggered.
    pub fn update_file(&mut self, repo_root: &Path, file_path: &str) -> UpdateResult {
        let start = Instant::now();

        let file_id = bridge::file_node_id(file_path);
        let edges_before = self.graph.edge_count();

        // Step 1: Remove old nodes/edges for this file
        let removed_ids = self.graph.remove_file_and_dependents(&file_id);
        let nodes_removed = removed_ids.len();
        let edges_after_remove = self.graph.edge_count();
        let edges_removed = edges_before - edges_after_remove;

        // Step 2: Identify dirty communities (those that contained removed nodes)
        let dirty_community_ids: Vec<String> = self
            .communities
            .iter()
            .filter(|c| c.node_ids.iter().any(|nid| removed_ids.contains(nid)))
            .map(|c| c.id.clone())
            .collect();

        // Step 3: Re-parse and add new nodes/edges
        let mut nodes_added = 0;
        let mut edges_added = 0;

        if let Some(file_data) = extract::extract_single_file_from_repo(repo_root, file_path) {
            // Build a temporary graph from the single file to get nodes and edges
            let (temp_graph, _stats) = bridge::build_graph(&[file_data]);

            // Merge temp graph nodes into the main graph
            for node_id in temp_graph.node_ids() {
                if let Some(node) = temp_graph.get_node(node_id) {
                    self.graph.add_node(node.clone());
                    nodes_added += 1;
                }
            }

            // Merge temp graph edges into the main graph
            for edge in temp_graph.all_edges() {
                self.graph.add_edge(edge.clone());
                edges_added += 1;
            }
        }
        // If file was deleted (extract returns None), we just removed it — that's correct.

        let edges_changed = edges_removed + edges_added;

        // Step 4: Impact-based invalidation.
        // Central files (high fan-in + fan-out) trigger full recluster.
        // Peripheral files get incremental patch only.
        const CENTRAL_EDGE_THRESHOLD: usize = 20;

        let file_edge_count = self
            .graph
            .all_edges()
            .iter()
            .filter(|e| e.source == file_id || e.target == file_id)
            .count();
        let is_central = file_edge_count >= CENTRAL_EDGE_THRESHOLD;

        let change_ratio = if self.total_edges_at_last_cluster > 0 {
            edges_changed as f64 / self.total_edges_at_last_cluster as f64
        } else {
            1.0
        };

        // Recluster if: central file changed OR >10% edges changed OR no prior clustering
        let recluster_triggered = is_central || change_ratio > 0.10;
        let mut summaries_regenerated = 0;

        if recluster_triggered {
            // Full re-cluster
            self.cluster();
            summaries_regenerated = self.summaries.len();
        } else if !dirty_community_ids.is_empty() && !self.communities.is_empty() {
            // Update community node_ids: remove old nodes, add new nodes
            // from the same file to the same communities
            let new_file_node_ids: Vec<String> = self
                .graph
                .all_edges()
                .iter()
                .filter(|e| e.source == file_id && e.edge_type == EdgeType::Contains)
                .map(|e| e.target.clone())
                .collect();

            for community in &mut self.communities {
                if dirty_community_ids.contains(&community.id) {
                    // Remove old nodes that were deleted
                    community.node_ids.retain(|nid| !removed_ids.contains(nid));

                    // Add the new file node and its dependents to this community
                    if self.graph.get_node(&file_id).is_some() {
                        community.node_ids.push(file_id.clone());
                        community.node_ids.extend(new_file_node_ids.clone());
                    }
                }
            }

            // Regenerate summaries only for dirty communities
            let dirty_communities: Vec<&Community> = self
                .communities
                .iter()
                .filter(|c| dirty_community_ids.contains(&c.id))
                .collect();

            for comm in &dirty_communities {
                let sums =
                    summary::generate_summaries(&[(*comm).clone()], &self.graph, &self.git_info);
                for s in sums {
                    self.summaries.insert(s.community_id.clone(), s);
                    summaries_regenerated += 1;
                }
            }

            // Invalidate cached scorer — communities changed, scorer must be rebuilt on next query.
            self.cached_scorer = None;
        }

        let elapsed_ms = start.elapsed().as_millis() as u64;

        UpdateResult {
            nodes_removed,
            nodes_added,
            edges_changed,
            recluster_triggered,
            summaries_regenerated,
            elapsed_ms,
        }
    }

    /// Populate co-change edges from git history and extract commit messages.
    pub fn add_git_cochanges(
        &mut self,
        repo_root: &Path,
    ) -> Result<git::CoChangeStats, git::GitError> {
        // Extract commit messages for summaries (WHY context).
        if let Ok(file_commits) =
            git::extract_file_commit_messages(repo_root, self.config.max_git_commits)
        {
            self.git_info = file_commits
                .into_iter()
                .map(|(path, info)| {
                    (
                        path,
                        FileGitInfo {
                            last_commit_message: info.last_commit_message,
                            recent_messages: info.recent_messages,
                        },
                    )
                })
                .collect();
        }

        // Populate co-change edges.
        git::populate_cochanges_from_git(
            repo_root,
            &mut self.graph,
            self.config.max_git_commits,
            self.config.max_files_per_commit,
        )
    }

    /// Run hierarchical clustering on the current graph and generate summaries.
    pub fn cluster(&mut self) -> &[Community] {
        let result = cluster::hierarchical_cluster(
            &self.graph,
            ClusterAlgorithm::FileLeiden { resolution: 0.5 },
        );
        self.communities = result.communities;
        self.total_edges_at_last_cluster = self.graph.edge_count();

        // Auto-generate summaries for all communities
        let sums = summary::generate_summaries(&self.communities, &self.graph, &self.git_info);
        self.summaries = sums
            .into_iter()
            .map(|s| (s.community_id.clone(), s))
            .collect();

        // Scorer is built lazily on first assemble_context() call — not here.
        // This avoids ~20s of fastembed/ONNX initialization when only stats/clustering is needed.
        self.cached_scorer = None;

        &self.communities
    }

    /// Ensure the scorer is built (lazy initialization).
    fn ensure_scorer(&mut self) {
        if self.cached_scorer.is_none() && !self.communities.is_empty() {
            self.cached_scorer = Some(MultiSignalScorer::build(&self.communities, &self.graph));
        }
    }

    /// Search and assemble context for a given task query.
    ///
    /// Uses pre-generated human-readable summaries instead of raw symbol dumps.
    pub fn assemble_context(&mut self, query: &str) -> ContextPayload {
        if self.communities.is_empty() {
            return ContextPayload {
                items: vec![],
                total_tokens: 0,
                budget_tokens: self.config.token_budget,
                exploration_hints: String::new(),
            };
        }

        // Build scorer on first context assembly (lazy — avoids 20s fastembed in stats/cluster-only paths)
        self.ensure_scorer();
        // T2.5: prefer `let Some` over `.as_ref().unwrap()`; the early-return
        // above already handled the empty-communities case, but the `let Some`
        // reads honestly instead of relying on a silent invariant.
        let Some(scorer) = self.cached_scorer.as_ref() else {
            return ContextPayload {
                items: vec![],
                total_tokens: 0,
                budget_tokens: self.config.token_budget,
                exploration_hints: String::new(),
            };
        };

        // Score communities
        let scored = scorer.score(query, &self.communities, &self.graph);

        // Assemble context within budget using summaries
        let budget = BudgetConfig::default_16k();
        let allocation = budget.allocate(self.config.token_budget);
        let context_budget = allocation.module_cards + allocation.real_code;

        assembly::assemble_with_summaries(&scored, &self.summaries, context_budget)
    }

    /// Assemble context with real code: use summaries for ranking,
    /// but include actual source code in the output.
    ///
    /// Flow:
    /// 1. Score communities with BM25 (same as `assemble_context`)
    /// 2. For each top-scored community, collect file paths from the graph
    /// 3. Read actual source files from disk (`repo_root`)
    /// 4. Build context items with: summary header + actual code
    /// 5. Pack into token budget using greedy knapsack
    pub fn assemble_context_with_code(&mut self, query: &str, repo_root: &Path) -> ContextPayload {
        if self.communities.is_empty() {
            return ContextPayload {
                items: vec![],
                total_tokens: 0,
                budget_tokens: self.config.token_budget,
                exploration_hints: String::new(),
            };
        }

        self.ensure_scorer();
        let Some(scorer) = self.cached_scorer.as_ref() else {
            return ContextPayload {
                items: vec![],
                total_tokens: 0,
                budget_tokens: self.config.token_budget,
                exploration_hints: String::new(),
            };
        };
        let scored = scorer.score(query, &self.communities, &self.graph);

        let budget = BudgetConfig::default_16k();
        let allocation = budget.allocate(self.config.token_budget);
        let context_budget = allocation.module_cards + allocation.real_code;

        assembly::assemble_with_code(
            &scored,
            &self.summaries,
            &self.graph,
            repo_root,
            context_budget,
            query,
        )
    }

    /// Access the summaries.
    pub fn summaries(&self) -> &HashMap<String, CommunitySummary> {
        &self.summaries
    }

    /// Run the full pipeline: build graph → git → cluster → search → assemble.
    pub fn run(&mut self, repo_root: &Path, files: &[FileData], query: &str) -> PipelineResult {
        let total_start = Instant::now();

        // Stage 1: Build graph
        let t = Instant::now();
        let bridge_stats = self.build_graph(files);
        let graph_build_ms = t.elapsed().as_millis() as u64;

        // Stage 2: Git co-changes
        let t = Instant::now();
        let _ = self.add_git_cochanges(repo_root);
        let git_cochanges_ms = t.elapsed().as_millis() as u64;

        // Stage 3: Clustering
        let t = Instant::now();
        self.cluster();
        let clustering_ms = t.elapsed().as_millis() as u64;

        // Stage 4+5: Search + Assembly
        let t = Instant::now();
        let context = self.assemble_context(query);
        let search_assembly_ms = t.elapsed().as_millis() as u64;

        let total_ms = total_start.elapsed().as_millis() as u64;

        PipelineResult {
            context,
            communities: self.communities.clone(),
            timing: PipelineTiming {
                graph_build_ms,
                git_cochanges_ms,
                clustering_ms,
                search_ms: search_assembly_ms,
                assembly_ms: 0, // combined with search
                total_ms,
            },
            graph_stats: GraphStats {
                nodes: self.graph.node_count(),
                edges: self.graph.edge_count(),
                files: bridge_stats.files,
                symbols: bridge_stats.symbols,
                communities: self.communities.len(),
            },
        }
    }

    /// Analyze the impact of editing a file.
    pub fn impact_analysis(&self, edited_file: &str) -> ImpactReport {
        impact::analyze_impact(
            edited_file,
            &self.graph,
            &self.communities,
            self.config.impact_bfs_depth,
        )
    }

    /// Check if a file is in the current context (for escape hatch).
    pub fn check_context_miss(
        &self,
        file_path: &str,
    ) -> Option<theo_engine_retrieval::escape::ContextMiss> {
        if let Some(ref membership) = self.membership {
            membership.detect_miss(file_path, &self.graph, &self.communities)
        } else {
            None
        }
    }

    /// Update context membership after assembling context.
    pub fn update_membership(&mut self, context_files: &[String]) {
        self.membership = Some(ContextMembership::new(context_files));
    }

    /// Access the internal graph.
    pub fn graph(&self) -> &CodeGraph {
        &self.graph
    }

    /// Access the communities.
    pub fn communities(&self) -> &[Community] {
        &self.communities
    }

    /// Save the graph to disk.
    pub fn save_graph(&self, path: &str) -> Result<(), theo_engine_graph::persist::PersistError> {
        persist::save(&self.graph, Path::new(path))
    }

    /// Load a graph from disk.
    pub fn load_graph(
        &mut self,
        path: &str,
    ) -> Result<(), theo_engine_graph::persist::PersistError> {
        self.graph = persist::load(Path::new(path))?;
        Ok(())
    }

    /// Save communities (clusters) to disk as bincode.
    pub fn save_clusters(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let data = bincode::serialize(&self.communities)?;
        std::fs::write(path, data)?;
        Ok(())
    }

    /// Load communities from disk. Also regenerates summaries and scorer.
    pub fn load_clusters(&mut self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let data = std::fs::read(path)?;
        self.communities = bincode::deserialize(&data)?;

        // Try loading cached summaries first
        let summaries_path = path.replace("clusters.bin", "summaries.bin");
        if let Ok(sum_data) = std::fs::read(&summaries_path) {
            if let Ok(sums) = bincode::deserialize::<HashMap<String, CommunitySummary>>(&sum_data) {
                self.summaries = sums;
            } else {
                self.regenerate_summaries();
            }
        } else {
            self.regenerate_summaries();
        }

        // Scorer is built lazily on first assemble_context() call — not here.
        // This avoids ~28s of fastembed initialization when only stats/clustering is needed.
        self.cached_scorer = None;
        Ok(())
    }

    fn regenerate_summaries(&mut self) {
        let sums = summary::generate_summaries(&self.communities, &self.graph, &self.git_info);
        self.summaries = sums
            .into_iter()
            .map(|s| (s.community_id.clone(), s))
            .collect();
    }

    /// Save summaries to disk.
    pub fn save_summaries(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let data = bincode::serialize(&self.summaries)?;
        std::fs::write(path, data)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;
