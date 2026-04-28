//! Sibling test body of `git.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `git.rs` via `#[path = "git_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.

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
