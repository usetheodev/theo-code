/// Git log parser for co-change extraction.
///
/// Parses `git log --name-only` output to identify files that changed together
/// in the same commit, then feeds those pairs into `update_cochanges()` with
/// temporal decay based on commit age.
use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::cochange::update_cochanges;
use crate::model::CodeGraph;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum GitError {
    /// The given path is not inside a git repository.
    NotAGitRepo,
    /// `git` command exited with a non-zero status.
    GitCommandFailed(String),
    /// Could not parse the git log output.
    ParseError(String),
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitError::NotAGitRepo => write!(f, "path is not inside a git repository"),
            GitError::GitCommandFailed(msg) => write!(f, "git command failed: {msg}"),
            GitError::ParseError(msg) => write!(f, "git log parse error: {msg}"),
        }
    }
}

impl std::error::Error for GitError {}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

/// Summary statistics returned after processing git history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoChangeStats {
    /// Number of commits that contributed co-change edges.
    pub commits_processed: usize,
    /// Commits skipped (too many files, or no matching graph nodes).
    pub commits_skipped: usize,
    /// Total number of co-change edges added or updated.
    pub edges_added: usize,
}

// ---------------------------------------------------------------------------
// Internal: parsed commit
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct ParsedCommit {
    _hash: String,
    timestamp: u64,
    files: Vec<String>,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse raw `git log --name-only --format='COMMIT:%H %at' --no-merges` output
/// into a list of `ParsedCommit`.
///
/// This is a pure function with no I/O — testable independently of git.
fn parse_git_log(raw: &str) -> Result<Vec<ParsedCommit>, GitError> {
    let mut commits: Vec<ParsedCommit> = Vec::new();
    let mut current: Option<ParsedCommit> = None;

    for line in raw.lines() {
        let line = line.trim();

        if let Some(rest) = line.strip_prefix("COMMIT:") {
            // Flush previous commit.
            if let Some(c) = current.take()
                && !c.files.is_empty() {
                    commits.push(c);
                }

            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() < 2 {
                return Err(GitError::ParseError(format!(
                    "expected 'COMMIT:<hash> <timestamp>', got: {line}"
                )));
            }

            let hash = parts[0].to_string();
            let timestamp: u64 = parts[1]
                .parse()
                .map_err(|_| GitError::ParseError(format!("invalid timestamp in line: {line}")))?;

            current = Some(ParsedCommit {
                _hash: hash,
                timestamp,
                files: Vec::new(),
            });
        } else if !line.is_empty() {
            // Non-empty, non-header line → file path.
            if let Some(ref mut c) = current {
                c.files.push(line.to_string());
            }
            // Lines before the first COMMIT header are ignored.
        }
    }

    // Flush last commit.
    if let Some(c) = current
        && !c.files.is_empty() {
            commits.push(c);
        }

    Ok(commits)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse git history and populate co-change edges in the graph.
///
/// Runs `git log` on the given repository, extracts file pairs that changed
/// together per commit, and feeds them into [`update_cochanges()`] with
/// temporal decay based on commit age.
///
/// # Arguments
/// * `repo_path` — path to the git repository root
/// * `graph` — the code graph to update (file nodes must already exist)
/// * `max_commits` — maximum number of commits to process (0 = all)
/// * `max_files_per_commit` — skip commits that touch more than N files
///   (noise filter; e.g. 20). Pass 0 to disable the filter.
///
/// # Errors
/// Returns [`GitError`] if the path is not a git repo, the git command fails,
/// or the output cannot be parsed.
pub fn populate_cochanges_from_git(
    repo_path: &Path,
    graph: &mut CodeGraph,
    max_commits: usize,
    max_files_per_commit: usize,
) -> Result<CoChangeStats, GitError> {
    let raw_log = run_git_log(repo_path, max_commits)?;
    let commits = parse_git_log(&raw_log)?;

    populate_from_parsed_commits(graph, &commits, max_files_per_commit)
}

/// Run the git log command and return its stdout as a string.
fn run_git_log(repo_path: &Path, max_commits: usize) -> Result<String, GitError> {
    // Verify the path is a git repo first.
    let check = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(repo_path)
        .output()
        .map_err(|e| GitError::GitCommandFailed(format!("failed to execute git: {e}")))?;

    if !check.status.success() {
        return Err(GitError::NotAGitRepo);
    }

    let mut args = vec![
        "log".to_string(),
        "--name-only".to_string(),
        "--format=COMMIT:%H %at".to_string(),
        "--no-merges".to_string(),
    ];

    if max_commits > 0 {
        args.push(format!("-{max_commits}"));
    }

    let output = Command::new("git")
        .args(&args)
        .current_dir(repo_path)
        .output()
        .map_err(|e| GitError::GitCommandFailed(format!("failed to execute git log: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GitError::GitCommandFailed(format!(
            "git log exited with status {}: {stderr}",
            output.status
        )));
    }

    String::from_utf8(output.stdout)
        .map_err(|e| GitError::ParseError(format!("git log output is not valid UTF-8: {e}")))
}

/// Core logic: process parsed commits and feed co-change pairs into the graph.
///
/// Separated from I/O so unit tests can call this directly.
fn populate_from_parsed_commits(
    graph: &mut CodeGraph,
    commits: &[ParsedCommit],
    max_files_per_commit: usize,
) -> Result<CoChangeStats, GitError> {
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| GitError::ParseError(format!("system clock error: {e}")))?
        .as_secs();

    let initial_edge_count = graph.edge_count();
    let mut stats = CoChangeStats {
        commits_processed: 0,
        commits_skipped: 0,
        edges_added: 0,
    };

    for commit in commits {
        // Filter: too many files (noise — e.g. large refactors, renames).
        if max_files_per_commit > 0 && commit.files.len() > max_files_per_commit {
            stats.commits_skipped += 1;
            continue;
        }

        // Map git file paths to graph node IDs, keeping only files that
        // already exist as nodes in the graph.
        let file_ids: Vec<String> = commit
            .files
            .iter()
            .map(|f| format!("file:{f}"))
            .filter(|id| graph.get_node(id).is_some())
            .collect();

        if file_ids.len() < 2 {
            stats.commits_skipped += 1;
            continue;
        }

        // Compute how many days ago this commit happened.
        let days_since = if commit.timestamp <= now_secs {
            (now_secs - commit.timestamp) as f64 / 86_400.0
        } else {
            0.0 // Future timestamp — treat as "now".
        };

        update_cochanges(graph, &file_ids, days_since);
        stats.commits_processed += 1;
    }

    // Count how many edges were added (new edges, not updates to existing ones).
    let final_edge_count = graph.edge_count();
    stats.edges_added = final_edge_count - initial_edge_count;

    Ok(stats)
}

// ---------------------------------------------------------------------------
// File commit messages
// ---------------------------------------------------------------------------

/// Recent commit message for a file.
#[derive(Debug, Clone)]
pub struct FileCommitInfo {
    /// File path (relative to repo root)
    pub file_path: String,
    /// Most recent commit message (first line only)
    pub last_commit_message: String,
    /// Most recent commit date (days since epoch)
    pub last_commit_days_ago: f64,
    /// Up to 5 most recent commit messages (first line only)
    pub recent_messages: Vec<String>,
}

/// Internal parsed commit with message (for commit-message extraction).
#[derive(Debug, Clone)]
struct ParsedCommitWithMessage {
    timestamp: u64,
    message: String,
    files: Vec<String>,
}

/// Parse raw `git log --name-only --format='COMMIT:%at %s' --no-merges` output
/// into a list of `ParsedCommitWithMessage`.
///
/// This is a pure function with no I/O — testable independently of git.
fn parse_git_log_with_messages(raw: &str) -> Result<Vec<ParsedCommitWithMessage>, GitError> {
    let mut commits: Vec<ParsedCommitWithMessage> = Vec::new();
    let mut current: Option<ParsedCommitWithMessage> = None;

    for line in raw.lines() {
        let line = line.trim();

        if let Some(rest) = line.strip_prefix("COMMIT:") {
            // Flush previous commit.
            if let Some(c) = current.take()
                && !c.files.is_empty() {
                    commits.push(c);
                }

            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() < 2 {
                return Err(GitError::ParseError(format!(
                    "expected 'COMMIT:<timestamp> <message>', got: {line}"
                )));
            }

            let timestamp: u64 = parts[0]
                .parse()
                .map_err(|_| GitError::ParseError(format!("invalid timestamp in line: {line}")))?;
            let message = parts[1].to_string();

            current = Some(ParsedCommitWithMessage {
                timestamp,
                message,
                files: Vec::new(),
            });
        } else if !line.is_empty()
            && let Some(ref mut c) = current {
                c.files.push(line.to_string());
            }
    }

    // Flush last commit.
    if let Some(c) = current
        && !c.files.is_empty() {
            commits.push(c);
        }

    Ok(commits)
}

/// Build the file → FileCommitInfo map from parsed commits.
///
/// Separated from I/O so unit tests can call this directly.
fn build_file_commit_map(
    commits: &[ParsedCommitWithMessage],
    now_secs: u64,
) -> HashMap<String, FileCommitInfo> {
    // Accumulate (timestamp, message) per file, newest first
    // (git log output is already newest-first by default).
    let mut file_entries: HashMap<String, Vec<(u64, String)>> = HashMap::new();

    for commit in commits {
        for file in &commit.files {
            file_entries
                .entry(file.clone())
                .or_default()
                .push((commit.timestamp, commit.message.clone()));
        }
    }

    let mut result = HashMap::new();
    for (file_path, entries) in file_entries {
        // entries are already in git-log order (newest first), keep up to 5.
        let capped: Vec<(u64, String)> = entries.into_iter().take(5).collect();

        let (newest_ts, ref newest_msg) = capped[0];
        let days_ago = if newest_ts <= now_secs {
            (now_secs - newest_ts) as f64 / 86_400.0
        } else {
            0.0
        };

        let recent_messages: Vec<String> = capped.iter().map(|(_, m)| m.clone()).collect();

        result.insert(
            file_path.clone(),
            FileCommitInfo {
                file_path,
                last_commit_message: newest_msg.clone(),
                last_commit_days_ago: days_ago,
                recent_messages,
            },
        );
    }

    result
}

/// Extract recent commit messages for each file in the repository.
///
/// Runs `git log --name-only --format='COMMIT:%at %s' --no-merges -n {max_commits}`
/// to get all commits with their files in one pass.
///
/// Returns a map: file_path -> FileCommitInfo
pub fn extract_file_commit_messages(
    repo_path: &Path,
    max_commits: usize,
) -> Result<HashMap<String, FileCommitInfo>, GitError> {
    let raw_log = run_git_log_with_messages(repo_path, max_commits)?;
    let commits = parse_git_log_with_messages(&raw_log)?;

    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| GitError::ParseError(format!("system clock error: {e}")))?
        .as_secs();

    Ok(build_file_commit_map(&commits, now_secs))
}

/// Run the git log command with message format and return its stdout.
fn run_git_log_with_messages(repo_path: &Path, max_commits: usize) -> Result<String, GitError> {
    let check = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(repo_path)
        .output()
        .map_err(|e| GitError::GitCommandFailed(format!("failed to execute git: {e}")))?;

    if !check.status.success() {
        return Err(GitError::NotAGitRepo);
    }

    let mut args = vec![
        "log".to_string(),
        "--name-only".to_string(),
        "--format=COMMIT:%at %s".to_string(),
        "--no-merges".to_string(),
    ];

    if max_commits > 0 {
        args.push(format!("-{max_commits}"));
    }

    let output = Command::new("git")
        .args(&args)
        .current_dir(repo_path)
        .output()
        .map_err(|e| GitError::GitCommandFailed(format!("failed to execute git log: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GitError::GitCommandFailed(format!(
            "git log exited with status {}: {stderr}",
            output.status
        )));
    }

    String::from_utf8(output.stdout)
        .map_err(|e| GitError::ParseError(format!("git log output is not valid UTF-8: {e}")))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "git_tests.rs"]
mod tests;
