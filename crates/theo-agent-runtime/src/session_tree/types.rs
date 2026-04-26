//! Domain types for the session tree: `EntryId`, `SessionEntry`,
//! `SessionTreeError`. Split out of `mod.rs` to keep the main file
//! focused on the tree-ops implementation.
//!
//! Fase 4 (REMEDIATION_PLAN T4.6). Extracted from `session_tree.rs`.
//! Behavior is byte-identical; these are the same public types in the
//! same public paths (re-exported from `mod.rs`).

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// EntryId
// ---------------------------------------------------------------------------

/// Unique ID for a session entry (8-char hex).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntryId(String);

impl EntryId {
    /// Generate a new random ID (16-hex form derived from
    /// `theo_domain::identifiers::random_u64`).
    ///
    /// T4.6 / find_p5_008 — replaces the previous 32-bit nanosecond-XOR
    /// form which could collide on fast hardware in parallel spawns.
    /// `random_u64` mixes wall-clock nanoseconds, thread id, and a
    /// stack-pointer-derived value for collision-resistance equivalent
    /// to the other identifiers in `theo-domain`.
    pub fn generate() -> Self {
        Self(format!(
            "{:016x}",
            theo_domain::identifiers::random_u64()
        ))
    }

    /// Generate an ID that does not collide with existing entries.
    pub fn generate_unique(existing: &HashMap<String, usize>) -> Self {
        for _ in 0..100 {
            let id = Self::generate();
            if !existing.contains_key(id.as_str()) {
                return id;
            }
            // Tiny sleep to change nanos on collision — extremely unlikely path.
            std::thread::yield_now();
        }
        // Fallback: append counter suffix.
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Self(format!("{:016x}", t.as_nanos()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Create from a raw string (useful in tests and deserialization).
    pub fn from_raw(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::fmt::Display for EntryId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ---------------------------------------------------------------------------
// SessionEntry
// ---------------------------------------------------------------------------

/// Type of session entry stored in the JSONL file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum SessionEntry {
    /// Session header — always the first line in the JSONL file.
    Header {
        id: EntryId,
        version: u32,
        timestamp: String,
        cwd: String,
    },
    /// A conversation message (user, assistant, tool, system).
    Message {
        id: EntryId,
        parent_id: Option<EntryId>,
        role: String,
        content: String,
    },
    /// Compaction summary replacing older messages.
    Compaction {
        id: EntryId,
        parent_id: Option<EntryId>,
        summary: String,
        first_kept_entry_id: EntryId,
        tokens_before: usize,
    },
    /// Model change event.
    ModelChange {
        id: EntryId,
        parent_id: Option<EntryId>,
        provider: String,
        model_id: String,
    },
    /// Branch summary — context about an abandoned branch.
    BranchSummary {
        id: EntryId,
        parent_id: Option<EntryId>,
        summary: String,
        from_branch_id: EntryId,
    },
}

impl SessionEntry {
    /// Returns the entry's unique ID.
    pub fn id(&self) -> &EntryId {
        match self {
            Self::Header { id, .. }
            | Self::Message { id, .. }
            | Self::Compaction { id, .. }
            | Self::ModelChange { id, .. }
            | Self::BranchSummary { id, .. } => id,
        }
    }

    /// Returns the parent ID (None for Header and root entries).
    pub fn parent_id(&self) -> Option<&EntryId> {
        match self {
            Self::Header { .. } => None,
            Self::Message { parent_id, .. }
            | Self::Compaction { parent_id, .. }
            | Self::ModelChange { parent_id, .. }
            | Self::BranchSummary { parent_id, .. } => parent_id.as_ref(),
        }
    }

    /// Returns `true` if this is a `Header` variant.
    pub fn is_header(&self) -> bool {
        matches!(self, Self::Header { .. })
    }

    /// Returns `true` if this is a `Message` variant.
    pub fn is_message(&self) -> bool {
        matches!(self, Self::Message { .. })
    }

    /// Returns `true` if this is a `Compaction` variant.
    pub fn is_compaction(&self) -> bool {
        matches!(self, Self::Compaction { .. })
    }
}

// ---------------------------------------------------------------------------
// SessionTreeError
// ---------------------------------------------------------------------------

/// Errors that can occur when operating on a session tree.
#[derive(Debug, thiserror::Error)]
pub enum SessionTreeError {
    #[error("entry not found: {0}")]
    EntryNotFound(String),

    #[error("invalid session file: {reason}")]
    InvalidFile { reason: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}
