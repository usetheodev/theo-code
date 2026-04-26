//! Session tree: append-only JSONL-backed tree structure for conversation sessions.
//!
//! Each session entry has an `id` and `parent_id`, forming a tree. The "leaf"
//! pointer tracks the current position. Appending creates a child of the current
//! leaf. Branching moves the leaf to an earlier entry, allowing new branches
//! without modifying history.
//!
//! Inspired by pi-mono's `SessionManager` (see `referencias/pi-mono/packages/coding-agent/src/core/session-manager.ts`).

mod context_builder;
mod types;
pub use types::{EntryId, SessionEntry, SessionTreeError};

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Current session format version.
pub const CURRENT_SESSION_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// SessionTree
// ---------------------------------------------------------------------------

/// In-memory session tree backed by an append-only JSONL file.
///
/// The tree is a directed acyclic graph where each entry points to its parent
/// via `parent_id`. A `leaf_id` pointer tracks the current tip of the
/// conversation. Appending a new entry makes it a child of the current leaf
/// and advances the leaf pointer.
///
/// Branching moves `leaf_id` to an earlier entry without modifying the file,
/// so the next append creates a new branch.
pub struct SessionTree {
    /// All entries in insertion order (including the header).
    entries: Vec<SessionEntry>,
    /// Index from entry ID string → position in `entries` vec.
    index: HashMap<String, usize>,
    /// Current leaf pointer (tip of active branch).
    leaf_id: Option<EntryId>,
    /// Path to the backing JSONL file.
    file_path: PathBuf,
}

impl std::fmt::Debug for SessionTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionTree")
            .field("entries", &self.entries.len())
            .field("leaf_id", &self.leaf_id)
            .field("file_path", &self.file_path)
            .finish()
    }
}

impl SessionTree {
    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    /// Create a new session tree, writing the header to a new JSONL file.
    pub fn create(path: impl Into<PathBuf>, cwd: &str) -> Result<Self, SessionTreeError> {
        let file_path = path.into();
        let header = SessionEntry::Header {
            id: EntryId::generate(),
            version: CURRENT_SESSION_VERSION,
            timestamp: now_iso(),
            cwd: cwd.to_owned(),
        };

        let mut file = File::create(&file_path)?;
        let line = serde_json::to_string(&header)?;
        writeln!(file, "{line}")?;
        file.flush()?;
        // T3.6 / find_p5_004 — `flush()` only drains userspace buffers
        // to the kernel page cache. `sync_data()` issues `fdatasync(2)`
        // so a host crash does not lose appended entries that resume
        // logic depends on. Cost: ~1-5 ms per append on rotational
        // disks; negligible on SSDs.
        file.sync_data()?;

        let mut index = HashMap::new();
        index.insert(header.id().to_string(), 0);

        Ok(Self {
            entries: vec![header],
            index,
            leaf_id: None,
            file_path,
        })
    }

    /// Load an existing session tree from a JSONL file.
    ///
    /// Returns an error if the file is empty or the first line is not a valid
    /// `Header` entry.
    pub fn load(path: impl Into<PathBuf>) -> Result<Self, SessionTreeError> {
        let file_path = path.into();
        let file = File::open(&file_path)?;
        let reader = BufReader::new(file);

        let mut entries = Vec::new();
        let mut index = HashMap::new();
        let mut leaf_id: Option<EntryId> = None;

        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<SessionEntry>(trimmed) {
                Ok(entry) => {
                    let pos = entries.len();
                    index.insert(entry.id().to_string(), pos);
                    if !entry.is_header() {
                        leaf_id = Some(entry.id().clone());
                    }
                    entries.push(entry);
                }
                Err(_) => {
                    // Skip malformed lines (matches pi-mono behavior).
                }
            }
        }

        // Validate: first entry must be a header.
        if entries.is_empty() {
            return Err(SessionTreeError::InvalidFile {
                reason: "file is empty".into(),
            });
        }
        if !entries[0].is_header() {
            return Err(SessionTreeError::InvalidFile {
                reason: "first entry is not a session header".into(),
            });
        }

        Ok(Self {
            entries,
            index,
            leaf_id,
            file_path,
        })
    }

    // -----------------------------------------------------------------------
    // Mutation
    // -----------------------------------------------------------------------

    /// Append an entry to the tree.
    ///
    /// The entry is written as a new line in the JSONL file and added to the
    /// in-memory index. The leaf pointer advances to the new entry.
    ///
    /// Returns a reference to the new entry's ID.
    pub fn append(&mut self, entry: SessionEntry) -> Result<&EntryId, SessionTreeError> {
        // Write to file first (fail-fast on IO errors).
        let line = serde_json::to_string(&entry)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)?;
        writeln!(file, "{line}")?;
        file.flush()?;
        // T3.6 / find_p5_004 — see header creation comment above.
        file.sync_data()?;

        // Update in-memory state.
        let pos = self.entries.len();
        self.index.insert(entry.id().to_string(), pos);
        if !entry.is_header() {
            self.leaf_id = Some(entry.id().clone());
        }
        self.entries.push(entry);

        Ok(self.entries.last().expect("just pushed").id())
    }

    /// Move the leaf pointer to an existing entry (branching).
    ///
    /// No file modification occurs — the JSONL file is append-only. The next
    /// `append` call will create a child of this entry, forming a new branch.
    pub fn branch(&mut self, from_id: &EntryId) -> Result<(), SessionTreeError> {
        if !self.index.contains_key(from_id.as_str()) {
            return Err(SessionTreeError::EntryNotFound(from_id.to_string()));
        }
        self.leaf_id = Some(from_id.clone());
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// Current leaf ID (tip of the active branch).
    pub fn leaf(&self) -> Option<&EntryId> {
        self.leaf_id.as_ref()
    }

    /// Total number of entries (including the header).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the tree contains only the header (or nothing).
    pub fn is_empty(&self) -> bool {
        self.entries.len() <= 1
    }

    /// Get an entry by its ID.
    pub fn get(&self, id: &EntryId) -> Option<&SessionEntry> {
        self.index
            .get(id.as_str())
            .map(|&pos| &self.entries[pos])
    }

    /// Get all entries (including the header) in insertion order.
    pub fn entries(&self) -> &[SessionEntry] {
        &self.entries
    }

    /// Path to the backing JSONL file.
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    // `build_context` + `walk_to_root` (which built the LLM context by
    // walking from root to the current leaf, applying any `Compaction`
    // entry's `first_kept_entry_id` to drop earlier messages) moved to
    // `context_builder.rs` as `impl SessionTree` methods. See that
    // file for docs.

    // -----------------------------------------------------------------------
    // Convenience helpers for appending specific entry types
    // -----------------------------------------------------------------------

    /// Append a message entry as a child of the current leaf.
    pub fn append_message(
        &mut self,
        role: &str,
        content: &str,
    ) -> Result<&EntryId, SessionTreeError> {
        let entry = SessionEntry::Message {
            id: EntryId::generate_unique(&self.index),
            parent_id: self.leaf_id.clone(),
            role: role.to_owned(),
            content: content.to_owned(),
        };
        self.append(entry)
    }

    /// Append a compaction entry as a child of the current leaf.
    pub fn append_compaction(
        &mut self,
        summary: &str,
        first_kept_entry_id: EntryId,
        tokens_before: usize,
    ) -> Result<&EntryId, SessionTreeError> {
        let entry = SessionEntry::Compaction {
            id: EntryId::generate_unique(&self.index),
            parent_id: self.leaf_id.clone(),
            summary: summary.to_owned(),
            first_kept_entry_id,
            tokens_before,
        };
        self.append(entry)
    }

    /// Append a model change entry as a child of the current leaf.
    pub fn append_model_change(
        &mut self,
        provider: &str,
        model_id: &str,
    ) -> Result<&EntryId, SessionTreeError> {
        let entry = SessionEntry::ModelChange {
            id: EntryId::generate_unique(&self.index),
            parent_id: self.leaf_id.clone(),
            provider: provider.to_owned(),
            model_id: model_id.to_owned(),
        };
        self.append(entry)
    }

    /// Append a branch summary entry as a child of the current leaf.
    pub fn append_branch_summary(
        &mut self,
        summary: &str,
        from_branch_id: EntryId,
    ) -> Result<&EntryId, SessionTreeError> {
        let entry = SessionEntry::BranchSummary {
            id: EntryId::generate_unique(&self.index),
            parent_id: self.leaf_id.clone(),
            summary: summary.to_owned(),
            from_branch_id,
        };
        self.append(entry)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return the current time as an ISO-8601 string (UTC-like, not timezone-aware).
fn now_iso() -> String {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    // Simple ISO-ish timestamp without pulling in chrono.
    let secs = t.as_secs();
    // Approximate: good enough for ordering. Production should use chrono or time crate.
    format!("{secs}")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
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
}
