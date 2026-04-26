//! Context building: walk the tree from the current leaf back to root and
//! assemble the ordered list of `SessionEntry` references that the LLM
//! should see, honoring the most-recent compaction summary's
//! `first_kept_entry_id` cut-off.
//!
//! Fase 4 (REMEDIATION_PLAN T4.6). Extracted from `session_tree.rs`.
//! Behavior is byte-identical; impl'd as `impl SessionTree` here so the
//! public `build_context` entry point keeps its method shape.

use super::{EntryId, SessionEntry, SessionTree};

impl SessionTree {
    /// Build the ordered list of session entries (root → leaf) that should
    /// be shown to the LLM. If a compaction entry exists in the current
    /// path, the window becomes `[compaction, first_kept .. end]`;
    /// otherwise returns the full path (header excluded).
    pub fn build_context(&self) -> Vec<&SessionEntry> {
        let leaf_id = match &self.leaf_id {
            Some(id) => id,
            None => return Vec::new(),
        };

        let path = self.walk_to_root(leaf_id);
        if path.is_empty() {
            return Vec::new();
        }

        // Find the latest compaction in the path.
        let mut compaction_idx: Option<usize> = None;
        let mut first_kept_id: Option<&str> = None;

        for (i, entry) in path.iter().enumerate() {
            if let SessionEntry::Compaction {
                first_kept_entry_id,
                ..
            } = entry
            {
                compaction_idx = Some(i);
                first_kept_id = Some(first_kept_entry_id.as_str());
            }
        }

        match (compaction_idx, first_kept_id) {
            (Some(comp_idx), Some(kept_id)) => {
                build_context_with_compaction(&path, comp_idx, kept_id)
            }
            _ => path,
        }
    }

    /// Walk from the given entry to root and return the path in root-to-leaf
    /// order, excluding `Header` entries.
    pub(super) fn walk_to_root(&self, from: &EntryId) -> Vec<&SessionEntry> {
        let mut path = Vec::new();
        let mut current_id = Some(from);

        while let Some(id) = current_id {
            if let Some(entry) = self.get(id) {
                if !entry.is_header() {
                    path.push(entry);
                }
                current_id = entry.parent_id();
            } else {
                break;
            }
        }

        path.reverse();
        path
    }
}

/// Assemble the context slice when the path contains a compaction entry:
/// `[compaction, kept_entries_before_compaction, entries_after_compaction]`.
/// The compaction entry itself carries the summary that replaces the
/// pre-kept-cut-off prefix.
fn build_context_with_compaction<'a>(
    path: &[&'a SessionEntry],
    comp_idx: usize,
    kept_id: &str,
) -> Vec<&'a SessionEntry> {
    let mut result = Vec::new();

    // 1. Emit the compaction summary entry.
    result.push(path[comp_idx]);

    // 2. Emit kept entries before the compaction (from first_kept_id on).
    let mut found_first_kept = false;
    for entry in &path[..comp_idx] {
        if entry.id().as_str() == kept_id {
            found_first_kept = true;
        }
        if found_first_kept {
            result.push(entry);
        }
    }

    // 3. Emit entries after the compaction.
    for entry in &path[comp_idx + 1..] {
        result.push(entry);
    }

    result
}
