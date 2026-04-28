//! Sibling test body of `session_tree/mod.rs` (T3.x of god-files-2026-07-23-plan.md).

#![allow(unused_imports)]

use super::*;

    use super::*;
    use std::io::Read;

    /// Helper: create a SessionTree in a temp dir.
    fn create_temp_tree() -> (SessionTree, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join("session.jsonl");
        let tree = SessionTree::create(&path, "/home/user/project")
            .expect("failed to create session tree");
        (tree, dir)
    }

    // -- Creation -----------------------------------------------------------

    #[test]
    fn test_create_writes_header_to_file() {
        // Arrange
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("test.jsonl");

        // Act
        let tree = SessionTree::create(&path, "/tmp/cwd").expect("create");

        // Assert
        assert_eq!(tree.len(), 1); // header only
        assert!(tree.is_empty()); // no non-header entries
        assert!(tree.leaf().is_none());

        // File should have exactly one line.
        let mut content = String::new();
        File::open(&path)
            .expect("open")
            .read_to_string(&mut content)
            .expect("read");
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 1);

        // Verify the header deserializes correctly.
        let entry: SessionEntry = serde_json::from_str(lines[0]).expect("parse header");
        assert!(entry.is_header());
        if let SessionEntry::Header { version, cwd, .. } = &entry {
            assert_eq!(*version, CURRENT_SESSION_VERSION);
            assert_eq!(cwd, "/tmp/cwd");
        }
    }

    // -- Append messages ----------------------------------------------------

    #[test]
    fn test_append_messages_written_to_file() {
        // Arrange
        let (mut tree, _dir) = create_temp_tree();

        // Act
        tree.append_message("user", "Hello").expect("append user");
        tree.append_message("assistant", "Hi there!")
            .expect("append assistant");

        // Assert
        assert_eq!(tree.len(), 3); // header + 2 messages
        assert!(!tree.is_empty());
        assert!(tree.leaf().is_some());

        // File should have 3 lines.
        let mut content = String::new();
        File::open(tree.file_path())
            .expect("open")
            .read_to_string(&mut content)
            .expect("read");
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn test_append_message_parent_chain() {
        // Arrange
        let (mut tree, _dir) = create_temp_tree();

        // Act
        let id1 = tree.append_message("user", "msg1").expect("m1").clone();
        let id2 = tree.append_message("assistant", "msg2").expect("m2").clone();
        let id3 = tree.append_message("user", "msg3").expect("m3").clone();

        // Assert: each message's parent is the previous one.
        let e1 = tree.get(&id1).expect("get e1");
        assert!(e1.parent_id().is_none()); // first message has no parent

        let e2 = tree.get(&id2).expect("get e2");
        assert_eq!(e2.parent_id(), Some(&id1));

        let e3 = tree.get(&id3).expect("get e3");
        assert_eq!(e3.parent_id(), Some(&id2));

        assert_eq!(tree.leaf(), Some(&id3));
    }

    // -- Load from file -----------------------------------------------------

    #[test]
    fn test_load_from_file_matches_entries() {
        // Arrange
        let (mut tree, _dir) = create_temp_tree();
        tree.append_message("user", "Hello").expect("append");
        tree.append_message("assistant", "World").expect("append");
        let original_len = tree.len();
        let original_leaf = tree.leaf().cloned();
        let path = tree.file_path().to_owned();

        // Act
        let loaded = SessionTree::load(&path).expect("load");

        // Assert
        assert_eq!(loaded.len(), original_len);
        assert_eq!(loaded.leaf().cloned(), original_leaf);
        assert_eq!(loaded.entries().len(), original_len);
    }

    #[test]
    fn test_load_empty_file_returns_error() {
        // Arrange
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("empty.jsonl");
        File::create(&path).expect("create empty file");

        // Act
        let result = SessionTree::load(&path);

        // Assert
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, SessionTreeError::InvalidFile { .. }),
            "expected InvalidFile, got: {err:?}"
        );
    }

    // -- Build context (root-to-leaf) ----------------------------------------

    #[test]
    fn test_build_context_returns_root_to_leaf_path() {
        // Arrange
        let (mut tree, _dir) = create_temp_tree();
        tree.append_message("user", "first").expect("m1");
        tree.append_message("assistant", "second").expect("m2");
        tree.append_message("user", "third").expect("m3");

        // Act
        let ctx = tree.build_context();

        // Assert
        assert_eq!(ctx.len(), 3);
        // Verify ordering: first → second → third.
        let contents: Vec<&str> = ctx
            .iter()
            .filter_map(|e| match e {
                SessionEntry::Message { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(contents, vec!["first", "second", "third"]);
    }

    #[test]
    fn test_build_context_empty_session() {
        // Arrange
        let (tree, _dir) = create_temp_tree();

        // Act
        let ctx = tree.build_context();

        // Assert
        assert!(ctx.is_empty());
    }

    // -- Branching -----------------------------------------------------------

    #[test]
    fn test_branch_changes_leaf_pointer() {
        // Arrange
        let (mut tree, _dir) = create_temp_tree();
        let id1 = tree.append_message("user", "msg1").expect("m1").clone();
        let _id2 = tree.append_message("assistant", "msg2").expect("m2").clone();

        // Act
        tree.branch(&id1).expect("branch");

        // Assert
        assert_eq!(tree.leaf(), Some(&id1));
    }

    #[test]
    fn test_branch_to_nonexistent_entry_fails() {
        // Arrange
        let (mut tree, _dir) = create_temp_tree();
        let fake_id = EntryId::from_raw("nonexistent");

        // Act
        let result = tree.branch(&fake_id);

        // Assert
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SessionTreeError::EntryNotFound(_)));
    }

    #[test]
    fn test_branch_creates_fork_in_context() {
        // Arrange
        let (mut tree, _dir) = create_temp_tree();
        let id1 = tree.append_message("user", "root msg").expect("m1").clone();
        tree.append_message("assistant", "branch A reply").expect("m2");

        // Act: branch back to id1 and create a new branch.
        tree.branch(&id1).expect("branch");
        tree.append_message("assistant", "branch B reply").expect("m3");

        // Assert: context should follow the new branch (root → B).
        let ctx = tree.build_context();
        let contents: Vec<&str> = ctx
            .iter()
            .filter_map(|e| match e {
                SessionEntry::Message { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(contents, vec!["root msg", "branch B reply"]);
    }

    // -- Compaction in context -----------------------------------------------

    #[test]
    fn test_compaction_entry_in_context_replaces_older_messages() {
        // Arrange
        let (mut tree, _dir) = create_temp_tree();
        let _id1 = tree.append_message("user", "old msg 1").expect("m1").clone();
        let _id2 = tree
            .append_message("assistant", "old msg 2")
            .expect("m2")
            .clone();
        let id3 = tree.append_message("user", "kept msg").expect("m3").clone();

        // Act: insert a compaction that keeps from id3 onward.
        tree.append_compaction("summary of old messages", id3.clone(), 500)
            .expect("compaction");
        tree.append_message("user", "new msg after compaction")
            .expect("m4");

        // Assert: context should be [compaction, kept msg, new msg].
        let ctx = tree.build_context();
        assert_eq!(ctx.len(), 3);

        // First entry is the compaction summary.
        assert!(ctx[0].is_compaction());

        // Second entry is the kept message.
        if let SessionEntry::Message { content, .. } = ctx[1] {
            assert_eq!(content, "kept msg");
        } else {
            panic!("expected Message, got: {:?}", ctx[1]);
        }

        // Third entry is the new message.
        if let SessionEntry::Message { content, .. } = ctx[2] {
            assert_eq!(content, "new msg after compaction");
        } else {
            panic!("expected Message, got: {:?}", ctx[2]);
        }
    }

    // -- Model change -------------------------------------------------------

    #[test]
    fn test_append_model_change() {
        // Arrange
        let (mut tree, _dir) = create_temp_tree();
        tree.append_message("user", "hello").expect("m1");

        // Act
        let id = tree
            .append_model_change("openai", "gpt-4o")
            .expect("model change")
            .clone();

        // Assert
        let entry = tree.get(&id).expect("get model change");
        if let SessionEntry::ModelChange {
            provider, model_id, ..
        } = entry
        {
            assert_eq!(provider, "openai");
            assert_eq!(model_id, "gpt-4o");
        } else {
            panic!("expected ModelChange");
        }
    }

    // -- Branch summary -----------------------------------------------------

    #[test]
    fn test_append_branch_summary() {
        // Arrange
        let (mut tree, _dir) = create_temp_tree();
        let id1 = tree.append_message("user", "msg").expect("m1").clone();

        // Act
        tree.branch(&id1).expect("branch");
        let bs_id = tree
            .append_branch_summary("summary of abandoned path", id1.clone())
            .expect("branch summary")
            .clone();

        // Assert
        let entry = tree.get(&bs_id).expect("get");
        if let SessionEntry::BranchSummary {
            summary,
            from_branch_id,
            ..
        } = entry
        {
            assert_eq!(summary, "summary of abandoned path");
            assert_eq!(from_branch_id, &id1);
        } else {
            panic!("expected BranchSummary");
        }
    }

    // -- Persistence round-trip ---------------------------------------------

    #[test]
    fn test_load_preserves_all_entry_types() {
        // Arrange
        let (mut tree, _dir) = create_temp_tree();
        tree.append_message("user", "hello").expect("msg");
        let kept_id = tree.leaf().cloned().expect("leaf");
        tree.append_model_change("anthropic", "claude-4")
            .expect("model");
        tree.append_compaction("compacted", kept_id.clone(), 1000)
            .expect("comp");
        tree.append_branch_summary("branch ctx", kept_id)
            .expect("bs");
        tree.append_message("assistant", "world").expect("msg2");
        let path = tree.file_path().to_owned();
        let original_len = tree.len();

        // Act
        let loaded = SessionTree::load(&path).expect("load");

        // Assert
        assert_eq!(loaded.len(), original_len);
        // Verify each entry type is present.
        let has_message = loaded.entries().iter().any(|e| e.is_message());
        let has_compaction = loaded.entries().iter().any(|e| e.is_compaction());
        let has_model = loaded
            .entries()
            .iter()
            .any(|e| matches!(e, SessionEntry::ModelChange { .. }));
        let has_branch = loaded
            .entries()
            .iter()
            .any(|e| matches!(e, SessionEntry::BranchSummary { .. }));
        assert!(has_message);
        assert!(has_compaction);
        assert!(has_model);
        assert!(has_branch);
    }

    /// Backward-compat regression guard for the `sota-tier1-tier2-plan`
    /// global DoD: a `.theo/state/<run_id>/session.jsonl` written by an
    /// earlier theo build MUST still parse under the current
    /// `#[non_exhaustive]` `SessionEntry` enum. The 5 original variants
    /// (`header`/`message`/`compaction`/`model_change`/`branch_summary`)
    /// were not modified by the SOTA Tier 1 + Tier 2 work; this test
    /// locks the wire-format contract so a future bump that breaks
    /// state v1 transcripts surfaces immediately.
    #[test]
    fn pre_sota_legacy_session_entry_jsonl_loads_each_original_variant() {
        // Canonical pre-SOTA wire shapes for each variant. Note the
        // `type` discriminator + snake_case rename is part of the
        // contract — these strings are what older theo runs wrote to
        // disk.
        let cases: &[(&str, fn(&SessionEntry) -> bool)] = &[
            (
                r#"{"type":"header","id":"deadbeefcafebabe",
                    "version":1,
                    "timestamp":"2025-01-15T12:00:00Z",
                    "cwd":"/home/user/project"}"#,
                |e| e.is_header(),
            ),
            (
                r#"{"type":"message","id":"00000000aaaaaaaa",
                    "parent_id":"deadbeefcafebabe",
                    "role":"user","content":"hello"}"#,
                |e| e.is_message(),
            ),
            (
                r#"{"type":"compaction","id":"00000000bbbbbbbb",
                    "parent_id":"00000000aaaaaaaa",
                    "summary":"older turns folded","first_kept_entry_id":
                    "00000000cccccccc","tokens_before":1024}"#,
                |e| e.is_compaction(),
            ),
            (
                r#"{"type":"model_change","id":"00000000dddddddd",
                    "parent_id":"00000000aaaaaaaa",
                    "provider":"anthropic","model_id":"claude-sonnet-4-6"}"#,
                |e| matches!(e, SessionEntry::ModelChange { .. }),
            ),
            (
                r#"{"type":"branch_summary","id":"00000000eeeeeeee",
                    "parent_id":"00000000aaaaaaaa",
                    "summary":"abandoned exploration",
                    "from_branch_id":"00000000ffffffff"}"#,
                |e| matches!(e, SessionEntry::BranchSummary { .. }),
            ),
        ];
        for (json, predicate) in cases {
            let entry: SessionEntry = serde_json::from_str(json).unwrap_or_else(|err| {
                panic!("legacy SessionEntry line failed to parse:\n{json}\n→ {err}")
            });
            assert!(
                predicate(&entry),
                "deserialised SessionEntry did not match expected variant for line:\n{json}"
            );
            // Roundtrip: writing and re-reading the modern form must
            // still classify correctly under the same predicate.
            let s = serde_json::to_string(&entry).expect("entry serialises");
            let back: SessionEntry =
                serde_json::from_str(&s).expect("entry round-trips");
            assert!(predicate(&back));
        }
    }
