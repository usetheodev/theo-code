//! Session quality metrics for GRAPHCTX governance.

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Quality metrics computed for a single coding session.
#[derive(Debug, Clone)]
pub struct SessionMetrics {
    /// Fraction of context files that the LLM actually accessed.
    /// `hits / context_file_paths.len()`, or 0.0 if context is empty.
    pub context_hit_rate: f64,
    /// `1.0 - context_hit_rate`.
    pub context_miss_rate: f64,
    /// `actual_files_accessed.len() / context_file_paths.len()`.
    /// Values > 1.0 mean the LLM read more than was in context.
    /// Returns 0.0 if context is empty.
    pub over_read_ratio: f64,
    /// `touched_community_ids.len() / total_communities`.
    /// Returns 0.0 if `total_communities` is 0.
    pub cluster_coverage: f64,
}

// ---------------------------------------------------------------------------
// Core function
// ---------------------------------------------------------------------------

/// Compute session metrics.
///
/// # Parameters
/// - `context_file_paths`     — files included in the context payload sent to the LLM
/// - `actual_files_accessed`  — files the LLM actually read or edited during the session
/// - `total_communities`      — total number of communities in the graph
/// - `touched_community_ids`  — IDs of communities that contain files accessed during the session
pub fn compute_session_metrics(
    context_file_paths: &[String],
    actual_files_accessed: &[String],
    total_communities: usize,
    touched_community_ids: &[String],
) -> SessionMetrics {
    let context_count = context_file_paths.len();
    let actual_count = actual_files_accessed.len();

    let (context_hit_rate, context_miss_rate, over_read_ratio) = if context_count == 0 {
        (0.0f64, 1.0f64, 0.0f64)
    } else {
        // Count how many context files appear in actual_files_accessed.
        let hits = context_file_paths
            .iter()
            .filter(|f| actual_files_accessed.contains(f))
            .count();

        let hit_rate = hits as f64 / context_count as f64;
        let over_read = actual_count as f64 / context_count as f64;
        (hit_rate, 1.0 - hit_rate, over_read)
    };

    let cluster_coverage = if total_communities == 0 {
        0.0
    } else {
        touched_community_ids.len() as f64 / total_communities as f64
    };

    SessionMetrics {
        context_hit_rate,
        context_miss_rate,
        over_read_ratio,
        cluster_coverage,
    }
}
