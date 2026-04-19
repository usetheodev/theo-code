//! JIT (just-in-time) loader for per-subdirectory instruction files.
//!
//! When a tool accesses `packages/foo/bar.rs`, the loader walks upward
//! looking for `CLAUDE.md` or `THEO.md` and queues them for injection
//! on the next turn. Already-loaded paths are deduped so docs appear at
//! most once per session.
//!
//! Reference: `referencias/gemini-cli/packages/core/src/context/memoryContextManager.ts:49-159`
//!
//! The C12 criterion in `.theo/evolution_criteria.md` requires: (a) discover
//! on tool access, (b) no re-loading of the same path, (c) respect workspace
//! root boundary.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Filenames recognized as subdir instructions, in priority order.
pub const INSTRUCTION_FILENAMES: &[&str] = &["CLAUDE.md", "THEO.md", "AGENTS.md"];

/// Tracks which instruction files have already been injected, so the
/// loader emits each path at most once per session.
#[derive(Debug, Default, Clone)]
pub struct JitInstructionLoader {
    workspace_root: Option<PathBuf>,
    loaded: HashSet<PathBuf>,
    pending: Vec<PathBuf>,
}

impl JitInstructionLoader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the workspace root boundary — the loader never walks above it.
    pub fn with_workspace_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.workspace_root = Some(root.into());
        self
    }

    /// Mark a path as already-loaded (e.g., the root `CLAUDE.md` injected
    /// at session boot) so it won't be re-enqueued.
    pub fn mark_loaded(&mut self, path: impl Into<PathBuf>) {
        self.loaded.insert(path.into());
    }

    /// Called when the agent accesses a file. Enqueues any unseen
    /// instruction file found on the upward traversal from that file's
    /// parent directory up to the workspace root.
    pub fn discover_on_access(&mut self, accessed: &Path) {
        let mut current = accessed.parent();
        while let Some(dir) = current {
            if let Some(root) = &self.workspace_root {
                if !dir.starts_with(root) {
                    break;
                }
            }
            for name in INSTRUCTION_FILENAMES {
                let candidate = dir.join(name);
                if self.loaded.contains(&candidate) {
                    continue;
                }
                if candidate.is_file() {
                    self.loaded.insert(candidate.clone());
                    self.pending.push(candidate);
                }
            }
            current = dir.parent();
        }
    }

    /// Drain the queue — returns paths enqueued since the last call.
    /// Callers are responsible for reading the files and injecting content.
    pub fn take_pending(&mut self) -> Vec<PathBuf> {
        std::mem::take(&mut self.pending)
    }

    /// Total number of paths already injected.
    pub fn loaded_count(&self) -> usize {
        self.loaded.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, body).expect("write test fixture");
        path
    }

    #[test]
    fn discovers_claude_md_in_parent_directory() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let pkg = root.join("pkg");
        fs::create_dir_all(&pkg).unwrap();
        let _claude = write(&pkg, "CLAUDE.md", "pkg rules");
        let target = write(&pkg, "code.rs", "fn main() {}");

        let mut loader = JitInstructionLoader::new().with_workspace_root(root);
        loader.discover_on_access(&target);

        let pending = loader.take_pending();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].ends_with("CLAUDE.md"));
    }

    #[test]
    fn does_not_enqueue_same_path_twice() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let _claude = write(root, "CLAUDE.md", "root rules");
        let target = write(root, "file.rs", "");

        let mut loader = JitInstructionLoader::new().with_workspace_root(root);
        loader.discover_on_access(&target);
        loader.discover_on_access(&target);

        assert_eq!(loader.take_pending().len(), 1);
        assert_eq!(loader.take_pending().len(), 0);
    }

    #[test]
    fn respects_workspace_root_boundary() {
        let tmp = TempDir::new().unwrap();
        let outer = tmp.path();
        let workspace = outer.join("ws");
        fs::create_dir_all(&workspace).unwrap();
        // CLAUDE.md OUTSIDE the workspace root — must be ignored.
        let _outside = write(outer, "CLAUDE.md", "must be ignored");
        let target = write(&workspace, "file.rs", "");

        let mut loader = JitInstructionLoader::new().with_workspace_root(&workspace);
        loader.discover_on_access(&target);
        assert!(loader.take_pending().is_empty());
    }

    #[test]
    fn finds_multiple_files_along_path() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let sub = root.join("a").join("b");
        fs::create_dir_all(&sub).unwrap();
        let _r = write(root, "CLAUDE.md", "root");
        let _s = write(&sub, "CLAUDE.md", "sub");
        let target = write(&sub, "file.rs", "");

        let mut loader = JitInstructionLoader::new().with_workspace_root(root);
        loader.discover_on_access(&target);
        let pending = loader.take_pending();
        assert_eq!(pending.len(), 2);
    }

    #[test]
    fn mark_loaded_prevents_future_enqueue() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let claude = write(root, "CLAUDE.md", "root");
        let target = write(root, "file.rs", "");

        let mut loader = JitInstructionLoader::new().with_workspace_root(root);
        loader.mark_loaded(&claude);
        loader.discover_on_access(&target);
        assert!(loader.take_pending().is_empty());
    }

    #[test]
    fn take_pending_drains_queue() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let _c = write(root, "CLAUDE.md", "root");
        let target = write(root, "file.rs", "");

        let mut loader = JitInstructionLoader::new().with_workspace_root(root);
        loader.discover_on_access(&target);
        assert_eq!(loader.take_pending().len(), 1);
        assert_eq!(loader.take_pending().len(), 0);
    }

    #[test]
    fn loaded_count_reflects_unique_injections() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let _c = write(root, "CLAUDE.md", "root");
        let target = write(root, "file.rs", "");

        let mut loader = JitInstructionLoader::new().with_workspace_root(root);
        loader.discover_on_access(&target);
        assert_eq!(loader.loaded_count(), 1);
    }
}
