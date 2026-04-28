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
#[path = "mod_tests.rs"]
mod tests;
