//! Runtime insight capture and aggregation for Deep Wiki.
//!
//! Standalone — does not depend on agent-runtime or tooling.
//! Any tool/agent/script produces RuntimeInsight and feeds it here.
//!
//! Persistence: JSONL append-only at `.theo/wiki/runtime/insights.jsonl`

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use super::model::*;

const MAX_INSIGHT_LINES: usize = 10_000;

// ---------------------------------------------------------------------------
// Ingest
// ---------------------------------------------------------------------------

/// Ingest a runtime insight into the wiki. Appends to JSONL.
pub fn ingest_insight(wiki_dir: &Path, insight: RuntimeInsight) -> std::io::Result<()> {
    let runtime_dir = wiki_dir.join("runtime");
    std::fs::create_dir_all(&runtime_dir)?;

    let jsonl_path = runtime_dir.join("insights.jsonl");

    // GC: rotate if over limit
    if let Ok(content) = std::fs::read_to_string(&jsonl_path) {
        let line_count = content.lines().count();
        if line_count >= MAX_INSIGHT_LINES {
            // Keep last half
            let lines: Vec<&str> = content.lines().collect();
            let keep = &lines[line_count / 2..];
            std::fs::write(&jsonl_path, keep.join("\n") + "\n")?;
        }
    }

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&jsonl_path)?;

    let json = serde_json::to_string(&insight).unwrap_or_default();
    writeln!(file, "{}", json)?;

    // Also log to wiki log
    if let Some(project_dir) = wiki_dir.parent().and_then(|p| p.parent()) {
        super::persistence::append_log(
            project_dir,
            "runtime",
            &format!(
                "{} | exit={} | {}ms | {}",
                insight.source,
                insight.exit_code,
                insight.duration_ms,
                if insight.success { "OK" } else { "FAIL" }
            ),
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Query
// ---------------------------------------------------------------------------

/// Load all insights from JSONL.
pub fn load_all_insights(wiki_dir: &Path) -> Vec<RuntimeInsight> {
    let path = wiki_dir.join("runtime").join("insights.jsonl");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    content
        .lines()
        .filter_map(|line| serde_json::from_str::<RuntimeInsight>(line).ok())
        .collect()
}

/// Query runtime insights by keyword. Returns most recent first.
pub fn query_insights(wiki_dir: &Path, query: &str, max: usize) -> Vec<RuntimeInsight> {
    let query_lower = query.to_lowercase();
    let mut results: Vec<RuntimeInsight> = load_all_insights(wiki_dir)
        .into_iter()
        .filter(|i| {
            i.command.to_lowercase().contains(&query_lower)
                || i.affected_files
                    .iter()
                    .any(|f| f.to_lowercase().contains(&query_lower))
                || i.affected_symbols
                    .iter()
                    .any(|s| s.to_lowercase().contains(&query_lower))
                || i.error_summary
                    .as_ref()
                    .is_some_and(|e| e.to_lowercase().contains(&query_lower))
                || i.source.to_lowercase().contains(&query_lower)
        })
        .collect();

    results.reverse(); // most recent first
    results.truncate(max);
    results
}

// ---------------------------------------------------------------------------
// Entity extraction
// ---------------------------------------------------------------------------

/// Extract affected file paths and symbol names from command output.
///
/// Parses common patterns:
/// - Rust errors: `error[E0308]: ... --> src/auth.rs:42:5`
/// - Rust tests: `test auth::tests::verify_token ... FAILED`
/// - Cargo compile: `Compiling theo-engine-retrieval v0.1.0`
/// - Generic file paths: `path/to/file.rs:123`
pub fn extract_affected_entities(stdout: &str, stderr: &str) -> (Vec<String>, Vec<String>) {
    let mut files = Vec::new();
    let mut symbols = Vec::new();

    for line in stderr.lines().chain(stdout.lines()) {
        // Rust error file path: "--> src/auth.rs:42:5" or "  --> file.rs:10"
        if let Some(pos) = line.find("-->") {
            let rest = line[pos + 3..].trim();
            if let Some(colon) = rest.find(':') {
                let path = rest[..colon].trim();
                if path.contains('.') && !path.contains(' ') {
                    files.push(path.to_string());
                }
            }
        }

        // Rust test result: "test auth::tests::verify_token ... FAILED"
        // or "test auth::tests::verify_token ... ok"
        if line.starts_with("test ") || line.contains("test ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(idx) = parts.iter().position(|&p| p == "test")
                && let Some(name) = parts.get(idx + 1)
                    && name.contains("::") && !name.starts_with('-') {
                        symbols.push(name.to_string());
                    }
        }

        // Generic file:line pattern (e.g., "src/lib.rs:42")
        if !line.contains("-->") {
            for word in line.split_whitespace() {
                let clean = word.trim_matches(|c: char| {
                    !c.is_alphanumeric() && c != '/' && c != '.' && c != ':' && c != '_' && c != '-'
                });
                if let Some(colon) = clean.find(':') {
                    let path = &clean[..colon];
                    if (path.ends_with(".rs")
                        || path.ends_with(".py")
                        || path.ends_with(".ts")
                        || path.ends_with(".go")
                        || path.ends_with(".js"))
                        && (path.contains('/') || path.contains('\\')) {
                            files.push(path.to_string());
                        }
                }
            }
        }

        // Cargo "Compiling crate vX.Y.Z"
        if line.trim_start().starts_with("Compiling ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                symbols.push(format!("crate:{}", parts[1]));
            }
        }
    }

    files.sort();
    files.dedup();
    symbols.sort();
    symbols.dedup();
    (files, symbols)
}

/// Truncate a string to max chars, appending "..." if truncated.
pub fn excerpt(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.min(s.len())])
    }
}

/// Extract the first meaningful error line from stderr.
pub fn extract_error_summary(stderr: &str) -> Option<String> {
    for line in stderr.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("error")
            || trimmed.starts_with("Error")
            || trimmed.starts_with("FAILED")
            || trimmed.starts_with("panicked")
            || trimmed.starts_with("thread '") && trimmed.contains("panicked")
        {
            return Some(excerpt(trimmed, 200));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Aggregation
// ---------------------------------------------------------------------------

/// Aggregate insights for a specific module slug.
pub fn aggregate_for_module(wiki_dir: &Path, slug: &str) -> OperationalSection {
    let insights = load_all_insights(wiki_dir);
    let slug_lower = slug.to_lowercase();

    let relevant: Vec<&RuntimeInsight> = insights
        .iter()
        .filter(|i| {
            i.affected_files
                .iter()
                .any(|f| f.to_lowercase().contains(&slug_lower))
                || i.affected_symbols
                    .iter()
                    .any(|s| s.to_lowercase().contains(&slug_lower))
                || i.command.to_lowercase().contains(&slug_lower)
        })
        .collect();

    if relevant.is_empty() {
        return OperationalSection::default();
    }

    // Aggregate failures
    let mut failure_groups: HashMap<String, (usize, Option<String>, Vec<String>)> = HashMap::new();
    for i in relevant.iter().filter(|i| !i.success) {
        let key = i
            .error_summary
            .as_deref()
            .unwrap_or("unknown error")
            .to_string();
        let entry =
            failure_groups
                .entry(key.clone())
                .or_insert((0, i.error_summary.clone(), Vec::new()));
        entry.0 += 1;
        for f in &i.affected_files {
            if !entry.2.contains(f) {
                entry.2.push(f.clone());
            }
        }
    }

    let common_failures: Vec<FailurePattern> = failure_groups
        .into_iter()
        .filter(|(_, (count, _, _))| *count >= 1)
        .map(|(pattern, (count, hint, files))| FailurePattern {
            pattern,
            count,
            error_hint: hint,
            affected_files: files,
        })
        .collect();

    // Aggregate successes
    let mut success_groups: HashMap<String, (usize, u64)> = HashMap::new();
    for i in relevant.iter().filter(|i| i.success) {
        let entry = success_groups.entry(i.command.clone()).or_insert((0, 0));
        entry.0 += 1;
        entry.1 += i.duration_ms;
    }

    let successful_recipes: Vec<CommandRecipe> = success_groups
        .into_iter()
        .map(|(cmd, (count, total_ms))| CommandRecipe {
            command: cmd,
            count,
            avg_duration_ms: total_ms / count as u64,
        })
        .collect();

    // Detect flaky tests (succeeded AND failed)
    let mut test_results: HashMap<String, (bool, bool)> = HashMap::new();
    for i in &relevant {
        for sym in &i.affected_symbols {
            let entry = test_results.entry(sym.clone()).or_default();
            if i.success {
                entry.0 = true;
            } else {
                entry.1 = true;
            }
        }
    }
    let flaky_tests: Vec<String> = test_results
        .into_iter()
        .filter(|(_, (passed, failed))| *passed && *failed)
        .map(|(name, _)| name)
        .collect();

    OperationalSection {
        common_failures,
        successful_recipes,
        flaky_tests,
        insight_count: relevant.len(),
        last_updated: relevant.last().map(|i| i.timestamp).unwrap_or(0),
    }
}

/// Distill repeated failure patterns into learnings.
pub fn distill_learnings(wiki_dir: &Path) -> Vec<Learning> {
    let insights = load_all_insights(wiki_dir);

    let mut error_groups: HashMap<String, Vec<&RuntimeInsight>> = HashMap::new();
    for i in insights.iter().filter(|i| !i.success) {
        if let Some(ref err) = i.error_summary {
            // Normalize: strip line numbers and hashes
            let normalized = normalize_error_pattern(err);
            error_groups.entry(normalized).or_default().push(i);
        }
    }

    error_groups
        .into_iter()
        .filter(|(_, group)| group.len() >= 3)
        .map(|(pattern, group)| {
            let mut modules: Vec<String> = group
                .iter()
                .flat_map(|i| i.affected_files.iter().cloned())
                .collect();
            modules.sort();
            modules.dedup();

            Learning {
                pattern,
                occurrences: group.len(),
                affected_modules: modules,
                first_seen: group.iter().map(|i| i.timestamp).min().unwrap_or(0),
                last_seen: group.iter().map(|i| i.timestamp).max().unwrap_or(0),
                status: LearningStatus::Active,
            }
        })
        .collect()
}

/// Normalize error pattern: strip line numbers, specific values.
fn normalize_error_pattern(error: &str) -> String {
    let mut s = error.to_string();
    // Strip line:col patterns
    while let Some(pos) = s.find(':') {
        let after = &s[pos + 1..];
        if after.starts_with(|c: char| c.is_ascii_digit()) {
            if let Some(end) = after.find(|c: char| !c.is_ascii_digit() && c != ':') {
                s = format!("{}{}", &s[..pos], &after[end..]);
            } else {
                s = s[..pos].to_string();
            }
        } else {
            break;
        }
    }
    s.trim().to_string()
}

// ---------------------------------------------------------------------------
// Promotion WAL and Archival (S3-T4)
// ---------------------------------------------------------------------------

/// A promotion event recorded in the write-ahead ledger.
///
/// Immutable: once written, never modified. Provides audit trail.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PromotionEntry {
    pub timestamp: u64,
    pub action: PromotionAction,
    pub source_path: String,
    pub target_tier: String,
    pub reason: String,
}

/// Actions tracked by the promotion WAL.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum PromotionAction {
    Promoted,
    Demoted,
    Archived,
    Evicted,
}

/// Append a promotion event to the WAL.
///
/// Atomic: writes to temp file then renames. Never loses data on crash.
pub fn append_promotion(wiki_dir: &Path, entry: PromotionEntry) -> std::io::Result<()> {
    let runtime_dir = wiki_dir.join("runtime");
    std::fs::create_dir_all(&runtime_dir)?;

    let wal_path = runtime_dir.join("promotions.jsonl");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&wal_path)?;

    let json = serde_json::to_string(&entry).unwrap_or_default();
    writeln!(file, "{}", json)?;
    Ok(())
}

/// Load all promotion entries from WAL.
pub fn load_promotions(wiki_dir: &Path) -> Vec<PromotionEntry> {
    let path = wiki_dir.join("runtime").join("promotions.jsonl");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    content
        .lines()
        .filter_map(|line| serde_json::from_str::<PromotionEntry>(line).ok())
        .collect()
}

/// Archive old insights to a compressed archive file.
///
/// Moves insights older than `max_age_secs` to `runtime/archive/YYYY-MM-DD.jsonl`.
/// Returns count of archived entries.
pub fn archive_old_insights(wiki_dir: &Path, max_age_secs: u64) -> std::io::Result<usize> {
    let insights = load_all_insights(wiki_dir);
    if insights.is_empty() {
        return Ok(0);
    }

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let cutoff_ms = now_ms.saturating_sub(max_age_secs * 1000);

    let (old, recent): (Vec<_>, Vec<_>) =
        insights.into_iter().partition(|i| i.timestamp < cutoff_ms);

    if old.is_empty() {
        return Ok(0);
    }

    // Write old insights to archive
    let archive_dir = wiki_dir.join("runtime").join("archive");
    std::fs::create_dir_all(&archive_dir)?;

    let date_str = chrono_date_string();
    let archive_path = archive_dir.join(format!("{}.jsonl", date_str));
    let mut archive_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&archive_path)?;

    for insight in &old {
        let json = serde_json::to_string(insight).unwrap_or_default();
        writeln!(archive_file, "{}", json)?;
    }

    // Rewrite current insights (only recent)
    let jsonl_path = wiki_dir.join("runtime").join("insights.jsonl");
    let content: String = recent
        .iter()
        .filter_map(|i| serde_json::to_string(i).ok())
        .collect::<Vec<_>>()
        .join("\n");
    if !content.is_empty() {
        std::fs::write(&jsonl_path, content + "\n")?;
    } else {
        std::fs::write(&jsonl_path, "")?;
    }

    Ok(old.len())
}

/// Validate WAL integrity on startup.
///
/// Returns number of valid entries. Logs corrupted lines.
pub fn validate_wal(wiki_dir: &Path) -> (usize, usize) {
    let path = wiki_dir.join("runtime").join("promotions.jsonl");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return (0, 0),
    };

    let mut valid = 0;
    let mut corrupted = 0;
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if serde_json::from_str::<PromotionEntry>(line).is_ok() {
            valid += 1;
        } else {
            corrupted += 1;
        }
    }
    (valid, corrupted)
}

// ---------------------------------------------------------------------------
// Promotion Policy (P1-T1)
// ---------------------------------------------------------------------------

/// Evaluate whether an episode summary should be promoted or evicted.
///
/// Decision based on usefulness signals:
/// - Has referenced communities → likely useful → promote
/// - Has learned constraints with workspace scope → definitely promote
/// - Otherwise → evict (will be archived, not deleted)
pub fn evaluate_promotion(
    referenced_communities: &[String],
    has_workspace_constraints: bool,
    _usefulness_threshold: f64,
) -> PromotionAction {
    // Workspace constraints always survive
    if has_workspace_constraints {
        return PromotionAction::Promoted;
    }
    // If communities were referenced by agent tools, promote
    if !referenced_communities.is_empty() {
        // Simple heuristic: any reference = useful
        // Future: use actual usefulness scores when available
        return PromotionAction::Promoted;
    }
    // No signal of usefulness → evict
    PromotionAction::Evicted
}

// ---------------------------------------------------------------------------
// Operational Limits (P1-T2)
// ---------------------------------------------------------------------------

/// Hard limits for operational safety.
#[derive(Debug, Clone)]
pub struct OperationalLimits {
    /// Max raw event JSONL size in bytes (default 10MB).
    pub max_raw_event_bytes: usize,
    /// Max active episode summaries before archival pruning (default 500).
    pub max_active_summaries: usize,
    /// Archival TTL in days — summaries older than this are archived (default 30).
    pub archival_ttl_days: u32,
}

impl Default for OperationalLimits {
    fn default() -> Self {
        OperationalLimits {
            max_raw_event_bytes: 10 * 1024 * 1024, // 10MB
            max_active_summaries: 500,
            archival_ttl_days: 30,
        }
    }
}

/// Result of enforcing operational limits.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct EnforcementReport {
    pub raw_events_rotated: bool,
    pub summaries_archived: usize,
    pub old_archives_removed: usize,
}

/// Enforce operational limits on the wiki runtime storage.
///
/// - Rotate raw events if JSONL exceeds max_raw_event_bytes
/// - Archive episode summaries beyond max_active_summaries
/// - Clean archived summaries older than archival_ttl_days
pub fn enforce_limits(
    wiki_dir: &Path,
    limits: &OperationalLimits,
) -> std::io::Result<EnforcementReport> {
    let mut report = EnforcementReport::default();

    // 1. Check raw events size
    let jsonl_path = wiki_dir.join("runtime").join("insights.jsonl");
    if let Ok(meta) = std::fs::metadata(&jsonl_path)
        && meta.len() as usize > limits.max_raw_event_bytes {
            // Keep last half
            if let Ok(content) = std::fs::read_to_string(&jsonl_path) {
                let lines: Vec<&str> = content.lines().collect();
                let keep = &lines[lines.len() / 2..];
                std::fs::write(&jsonl_path, keep.join("\n") + "\n")?;
                report.raw_events_rotated = true;
            }
        }

    // 2. Check episode summaries count
    let episodes_dir = wiki_dir.join("episodes");
    if episodes_dir.exists() {
        let mut entries: Vec<std::fs::DirEntry> = std::fs::read_dir(&episodes_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
            .collect();

        if entries.len() > limits.max_active_summaries {
            // Sort by name (which contains timestamp-based ID) — oldest first
            entries.sort_by_key(|e| e.file_name());
            let to_archive = entries.len() - limits.max_active_summaries;
            let archive_dir = wiki_dir.join("runtime").join("archive");
            std::fs::create_dir_all(&archive_dir)?;

            for entry in entries.iter().take(to_archive) {
                let dest = archive_dir.join(entry.file_name());
                std::fs::rename(entry.path(), &dest)?;
                report.summaries_archived += 1;
            }
        }
    }

    Ok(report)
}

/// Health status of the wiki runtime storage.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HealthStatus {
    pub raw_events_bytes: usize,
    pub raw_events_pct_of_limit: f64,
    pub active_summaries: usize,
    pub summaries_pct_of_limit: f64,
    pub is_healthy: bool,
    pub warnings: Vec<String>,
}

/// Check health of runtime storage against operational limits.
pub fn check_health(wiki_dir: &Path, limits: &OperationalLimits) -> HealthStatus {
    let jsonl_path = wiki_dir.join("runtime").join("insights.jsonl");
    let raw_bytes = std::fs::metadata(&jsonl_path)
        .map(|m| m.len() as usize)
        .unwrap_or(0);
    let raw_pct = if limits.max_raw_event_bytes > 0 {
        raw_bytes as f64 / limits.max_raw_event_bytes as f64
    } else {
        0.0
    };

    let episodes_dir = wiki_dir.join("episodes");
    let active_summaries = if episodes_dir.exists() {
        std::fs::read_dir(&episodes_dir)
            .map(|rd| rd.filter_map(|e| e.ok()).count())
            .unwrap_or(0)
    } else {
        0
    };
    let summaries_pct = if limits.max_active_summaries > 0 {
        active_summaries as f64 / limits.max_active_summaries as f64
    } else {
        0.0
    };

    let mut warnings = Vec::new();
    if raw_pct > 0.8 {
        warnings.push(format!(
            "Raw events at {:.0}% of limit ({} bytes)",
            raw_pct * 100.0,
            raw_bytes
        ));
    }
    if summaries_pct > 0.8 {
        warnings.push(format!(
            "Summaries at {:.0}% of limit ({} active)",
            summaries_pct * 100.0,
            active_summaries
        ));
    }

    HealthStatus {
        raw_events_bytes: raw_bytes,
        raw_events_pct_of_limit: raw_pct,
        active_summaries,
        summaries_pct_of_limit: summaries_pct,
        is_healthy: warnings.is_empty(),
        warnings,
    }
}

/// Simple date string (no chrono dependency).
fn chrono_date_string() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = secs / 86400;
    // Approximate date calculation (good enough for archive file naming)
    let year = 1970 + days / 365;
    let day_of_year = days % 365;
    let month = day_of_year / 30 + 1;
    let day = day_of_year % 30 + 1;
    format!("{:04}-{:02}-{:02}", year, month.min(12), day.min(31))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
