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

        if line.starts_with("COMMIT:") {
            // Flush previous commit.
            if let Some(c) = current.take() {
                if !c.files.is_empty() {
                    commits.push(c);
                }
            }

            let rest = &line["COMMIT:".len()..];
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() < 2 {
                return Err(GitError::ParseError(format!(
                    "expected 'COMMIT:<hash> <timestamp>', got: {line}"
                )));
            }

            let hash = parts[0].to_string();
            let timestamp: u64 = parts[1].parse().map_err(|_| {
                GitError::ParseError(format!("invalid timestamp in line: {line}"))
            })?;

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
    if let Some(c) = current {
        if !c.files.is_empty() {
            commits.push(c);
        }
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

        if line.starts_with("COMMIT:") {
            // Flush previous commit.
            if let Some(c) = current.take() {
                if !c.files.is_empty() {
                    commits.push(c);
                }
            }

            let rest = &line["COMMIT:".len()..];
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() < 2 {
                return Err(GitError::ParseError(format!(
                    "expected 'COMMIT:<timestamp> <message>', got: {line}"
                )));
            }

            let timestamp: u64 = parts[0].parse().map_err(|_| {
                GitError::ParseError(format!("invalid timestamp in line: {line}"))
            })?;
            let message = parts[1].to_string();

            current = Some(ParsedCommitWithMessage {
                timestamp,
                message,
                files: Vec::new(),
            });
        } else if !line.is_empty() {
            if let Some(ref mut c) = current {
                c.files.push(line.to_string());
            }
        }
    }

    // Flush last commit.
    if let Some(c) = current {
        if !c.files.is_empty() {
            commits.push(c);
        }
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
mod tests {
    use super::*;
    use crate::model::{CodeGraph, Node, NodeType};

    /// Helper: create a graph with file nodes for the given relative paths.
    fn graph_with_files(paths: &[&str]) -> CodeGraph {
        let mut g = CodeGraph::new();
        for path in paths {
            g.add_node(Node {
                id: format!("file:{path}"),
                node_type: NodeType::File,
                name: path.to_string(),
                file_path: Some(path.to_string()),
                signature: None,
                kind: None,
                line_start: None,
                line_end: None,
                last_modified: 0.0,
                doc: None,
            });
        }
        g
    }

    /// Synthesize a realistic `git log --name-only --format='COMMIT:%H %at'` output.
    fn fake_git_log() -> String {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let one_day_ago = now - 86_400;
        let ten_days_ago = now - 86_400 * 10;
        let hundred_days_ago = now - 86_400 * 100;

        format!(
            "\
COMMIT:aaaa1111 {one_day_ago}

src/main.rs
src/lib.rs

COMMIT:bbbb2222 {ten_days_ago}

src/lib.rs
src/model.rs
src/utils.rs

COMMIT:cccc3333 {hundred_days_ago}

src/main.rs
src/model.rs
"
        )
    }

    // --- parse_git_log tests ------------------------------------------------

    #[test]
    fn test_parse_empty_log() {
        let commits = parse_git_log("").unwrap();
        assert!(commits.is_empty());
    }

    #[test]
    fn test_parse_single_commit() {
        let log = "COMMIT:abc123 1700000000\n\nsrc/main.rs\nsrc/lib.rs\n";
        let commits = parse_git_log(log).unwrap();

        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0]._hash, "abc123");
        assert_eq!(commits[0].timestamp, 1_700_000_000);
        assert_eq!(commits[0].files, vec!["src/main.rs", "src/lib.rs"]);
    }

    #[test]
    fn test_parse_multiple_commits() {
        let log = fake_git_log();
        let commits = parse_git_log(&log).unwrap();

        assert_eq!(commits.len(), 3);
        assert_eq!(commits[0].files, vec!["src/main.rs", "src/lib.rs"]);
        assert_eq!(
            commits[1].files,
            vec!["src/lib.rs", "src/model.rs", "src/utils.rs"]
        );
        assert_eq!(commits[2].files, vec!["src/main.rs", "src/model.rs"]);
    }

    #[test]
    fn test_parse_commit_with_no_files_is_skipped() {
        let log = "COMMIT:abc123 1700000000\n\nCOMMIT:def456 1700000001\n\nsrc/a.rs\n";
        let commits = parse_git_log(log).unwrap();

        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0]._hash, "def456");
    }

    #[test]
    fn test_parse_invalid_timestamp() {
        let log = "COMMIT:abc123 not_a_number\n\nsrc/a.rs\n";
        let result = parse_git_log(log);

        assert!(result.is_err());
        match result.unwrap_err() {
            GitError::ParseError(msg) => assert!(msg.contains("invalid timestamp")),
            other => panic!("expected ParseError, got: {other:?}"),
        }
    }

    #[test]
    fn test_parse_missing_timestamp() {
        let log = "COMMIT:abc123\n\nsrc/a.rs\n";
        let result = parse_git_log(log);

        assert!(result.is_err());
        match result.unwrap_err() {
            GitError::ParseError(msg) => assert!(msg.contains("expected")),
            other => panic!("expected ParseError, got: {other:?}"),
        }
    }

    // --- populate_from_parsed_commits tests ---------------------------------

    #[test]
    fn test_populate_basic_cochanges() {
        let mut graph = graph_with_files(&["src/main.rs", "src/lib.rs", "src/model.rs"]);
        let log = fake_git_log();
        let commits = parse_git_log(&log).unwrap();

        let stats = populate_from_parsed_commits(&mut graph, &commits, 0).unwrap();

        // Commit 1: main+lib (2 files, 1 pair)
        // Commit 2: lib+model+utils — utils not in graph, so lib+model (1 pair)
        // Commit 3: main+model (1 pair)
        // That's 3 unique pairs -> 3 edges
        assert_eq!(stats.commits_processed, 3);
        assert_eq!(stats.commits_skipped, 0);
        assert_eq!(stats.edges_added, 3);
    }

    #[test]
    fn test_skip_commits_with_too_many_files() {
        let mut graph =
            graph_with_files(&["src/a.rs", "src/b.rs", "src/c.rs", "src/d.rs", "src/e.rs"]);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let log = format!(
            "\
COMMIT:aaa {now}

src/a.rs
src/b.rs
src/c.rs
src/d.rs
src/e.rs

COMMIT:bbb {now}

src/a.rs
src/b.rs
"
        );
        let commits = parse_git_log(&log).unwrap();

        // max_files_per_commit = 3 → first commit (5 files) is skipped
        let stats = populate_from_parsed_commits(&mut graph, &commits, 3).unwrap();

        assert_eq!(stats.commits_processed, 1);
        assert_eq!(stats.commits_skipped, 1);
        assert_eq!(stats.edges_added, 1); // a+b pair only
    }

    #[test]
    fn test_files_not_in_graph_are_ignored() {
        let mut graph = graph_with_files(&["src/a.rs"]);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let log = format!("COMMIT:aaa {now}\n\nsrc/a.rs\nsrc/unknown.rs\n");
        let commits = parse_git_log(&log).unwrap();

        // Only 1 file matches graph → fewer than 2 → commit skipped
        let stats = populate_from_parsed_commits(&mut graph, &commits, 0).unwrap();

        assert_eq!(stats.commits_processed, 0);
        assert_eq!(stats.commits_skipped, 1);
        assert_eq!(stats.edges_added, 0);
    }

    #[test]
    fn test_duplicate_commit_updates_weight_not_duplicates_edge() {
        let mut graph = graph_with_files(&["src/a.rs", "src/b.rs"]);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let old = now - 86_400 * 30; // 30 days ago
        let recent = now - 86_400; // 1 day ago

        // Older commit first, then more recent one with the same files.
        let log = format!(
            "\
COMMIT:old1 {old}

src/a.rs
src/b.rs

COMMIT:new1 {recent}

src/a.rs
src/b.rs
"
        );
        let commits = parse_git_log(&log).unwrap();

        let stats = populate_from_parsed_commits(&mut graph, &commits, 0).unwrap();

        assert_eq!(stats.commits_processed, 2);
        // Only 1 edge added (the first commit adds it, the second updates in place).
        assert_eq!(stats.edges_added, 1);
    }

    #[test]
    fn test_file_id_format() {
        // Verify that file IDs use the "file:" prefix.
        let graph = graph_with_files(&["src/foo.rs"]);

        assert!(graph.get_node("file:src/foo.rs").is_some());
        assert!(graph.get_node("src/foo.rs").is_none());
    }

    #[test]
    fn test_empty_commits_list() {
        let mut graph = graph_with_files(&["src/a.rs"]);
        let stats = populate_from_parsed_commits(&mut graph, &[], 0).unwrap();

        assert_eq!(stats.commits_processed, 0);
        assert_eq!(stats.commits_skipped, 0);
        assert_eq!(stats.edges_added, 0);
    }

    // --- GitError Display ---------------------------------------------------

    #[test]
    fn test_error_display() {
        assert_eq!(
            format!("{}", GitError::NotAGitRepo),
            "path is not inside a git repository"
        );
        assert!(format!("{}", GitError::GitCommandFailed("boom".into())).contains("boom"));
        assert!(format!("{}", GitError::ParseError("bad".into())).contains("bad"));
    }

    // --- parse_git_log_with_messages tests ----------------------------------

    #[test]
    fn test_parse_messages_basic() {
        let log = "\
COMMIT:1700000000 Add user authentication
src/auth.rs
src/main.rs

COMMIT:1699900000 Fix database connection pool
src/db.rs
";
        let commits = parse_git_log_with_messages(log).unwrap();

        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].timestamp, 1_700_000_000);
        assert_eq!(commits[0].message, "Add user authentication");
        assert_eq!(commits[0].files, vec!["src/auth.rs", "src/main.rs"]);
        assert_eq!(commits[1].timestamp, 1_699_900_000);
        assert_eq!(commits[1].message, "Fix database connection pool");
        assert_eq!(commits[1].files, vec!["src/db.rs"]);
    }

    #[test]
    fn test_parse_messages_empty_log() {
        let commits = parse_git_log_with_messages("").unwrap();
        assert!(commits.is_empty());
    }

    #[test]
    fn test_parse_messages_commit_no_files_skipped() {
        let log = "\
COMMIT:1700000000 Empty commit
COMMIT:1699900000 Real commit
src/a.rs
";
        let commits = parse_git_log_with_messages(log).unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].message, "Real commit");
    }

    #[test]
    fn test_parse_messages_invalid_timestamp() {
        let log = "COMMIT:not_a_number Some message\nsrc/a.rs\n";
        let result = parse_git_log_with_messages(log);
        assert!(result.is_err());
        match result.unwrap_err() {
            GitError::ParseError(msg) => assert!(msg.contains("invalid timestamp")),
            other => panic!("expected ParseError, got: {other:?}"),
        }
    }

    // --- build_file_commit_map tests ----------------------------------------

    #[test]
    fn test_build_map_most_recent_first() {
        let now_secs: u64 = 1_700_000_000;
        let one_day_ago = now_secs - 86_400;
        let ten_days_ago = now_secs - 86_400 * 10;

        let commits = vec![
            ParsedCommitWithMessage {
                timestamp: one_day_ago,
                message: "Recent change".to_string(),
                files: vec!["src/lib.rs".to_string()],
            },
            ParsedCommitWithMessage {
                timestamp: ten_days_ago,
                message: "Older change".to_string(),
                files: vec!["src/lib.rs".to_string()],
            },
        ];

        let map = build_file_commit_map(&commits, now_secs);

        assert_eq!(map.len(), 1);
        let info = map.get("src/lib.rs").unwrap();
        assert_eq!(info.last_commit_message, "Recent change");
        assert_eq!(info.recent_messages.len(), 2);
        assert_eq!(info.recent_messages[0], "Recent change");
        assert_eq!(info.recent_messages[1], "Older change");

        // days_ago should be ~1.0
        assert!((info.last_commit_days_ago - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_build_map_caps_at_five_messages() {
        let now_secs: u64 = 1_700_000_000;

        let commits: Vec<ParsedCommitWithMessage> = (0..8)
            .map(|i| ParsedCommitWithMessage {
                timestamp: now_secs - 86_400 * (i + 1),
                message: format!("Commit {i}"),
                files: vec!["src/busy.rs".to_string()],
            })
            .collect();

        let map = build_file_commit_map(&commits, now_secs);

        let info = map.get("src/busy.rs").unwrap();
        assert_eq!(info.recent_messages.len(), 5);
        assert_eq!(info.recent_messages[0], "Commit 0");
        assert_eq!(info.recent_messages[4], "Commit 4");
    }

    #[test]
    fn test_build_map_empty_commits_returns_empty() {
        let map = build_file_commit_map(&[], 1_700_000_000);
        assert!(map.is_empty());
    }

    #[test]
    fn test_build_map_multiple_files() {
        let now_secs: u64 = 1_700_000_000;

        let commits = vec![ParsedCommitWithMessage {
            timestamp: now_secs - 86_400 * 3,
            message: "Refactor modules".to_string(),
            files: vec!["src/a.rs".to_string(), "src/b.rs".to_string()],
        }];

        let map = build_file_commit_map(&commits, now_secs);

        assert_eq!(map.len(), 2);
        assert_eq!(
            map.get("src/a.rs").unwrap().last_commit_message,
            "Refactor modules"
        );
        assert_eq!(
            map.get("src/b.rs").unwrap().last_commit_message,
            "Refactor modules"
        );
        assert!((map.get("src/a.rs").unwrap().last_commit_days_ago - 3.0).abs() < 0.01);
    }

    #[test]
    fn test_build_map_future_timestamp_treated_as_now() {
        let now_secs: u64 = 1_700_000_000;

        let commits = vec![ParsedCommitWithMessage {
            timestamp: now_secs + 86_400, // 1 day in the future
            message: "Future commit".to_string(),
            files: vec!["src/time_travel.rs".to_string()],
        }];

        let map = build_file_commit_map(&commits, now_secs);

        let info = map.get("src/time_travel.rs").unwrap();
        assert_eq!(info.last_commit_days_ago, 0.0);
    }
}
