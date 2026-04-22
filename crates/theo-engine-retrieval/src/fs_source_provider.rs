//! Filesystem adapter for `inline_builder::SourceProvider`.
//!
//! PLAN_CONTEXT_WIRING Phase 3 — the inline builder needs line ranges
//! from disk. This module provides the `FsSourceProvider` that reads
//! source files from a workspace root and hands back the requested
//! line slice. Failures (missing file, I/O error) degrade gracefully
//! to an empty string so `build_inline_slices` can fall back to
//! signature-only snippets.

use std::path::Path;

use crate::inline_builder::SourceProvider;

/// `SourceProvider` implementation that reads files from a workspace
/// root on disk. Zero caching — the agent-runtime typically invokes
/// retrieve_files at most a few times per turn, and source reads are
/// cheap compared to LLM latency.
pub struct FsSourceProvider<'a> {
    /// Absolute or project-relative root under which relative paths
    /// supplied to `get_lines` are resolved.
    pub root: &'a Path,
}

impl<'a> FsSourceProvider<'a> {
    pub fn new(root: &'a Path) -> Self {
        Self { root }
    }
}

impl<'a> SourceProvider for FsSourceProvider<'a> {
    fn get_lines(&self, file_path: &str, start_line: usize, end_line: usize) -> String {
        if start_line == 0 || end_line == 0 || end_line < start_line {
            return String::new();
        }
        let full = self.root.join(file_path);
        let Ok(source) = std::fs::read_to_string(&full) else {
            return String::new();
        };
        source
            .lines()
            .skip(start_line.saturating_sub(1))
            .take(end_line.saturating_sub(start_line) + 1)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_file(dir: &Path, name: &str, contents: &str) -> String {
        let p = dir.join(name);
        let mut f = std::fs::File::create(&p).expect("create");
        f.write_all(contents.as_bytes()).expect("write");
        name.to_string()
    }

    #[test]
    fn fs_source_provider_returns_exact_line_range() {
        // AAA: arrange a file with known lines, act on the middle range,
        // assert the slice matches.
        let dir = tempfile::tempdir().expect("tmpdir");
        let name = make_file(
            dir.path(),
            "sample.rs",
            "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n",
        );

        let provider = FsSourceProvider::new(dir.path());
        let got = provider.get_lines(&name, 2, 3);

        assert_eq!(got, "fn b() {}\nfn c() {}");
    }

    #[test]
    fn fs_source_provider_missing_file_returns_empty() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let provider = FsSourceProvider::new(dir.path());

        let got = provider.get_lines("does-not-exist.rs", 1, 5);

        assert_eq!(got, "");
    }

    #[test]
    fn fs_source_provider_invalid_range_returns_empty() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let name = make_file(dir.path(), "x.rs", "fn one() {}\n");
        let provider = FsSourceProvider::new(dir.path());

        // start_line == 0 (1-based invalid)
        assert_eq!(provider.get_lines(&name, 0, 1), "");
        // end_line < start_line
        assert_eq!(provider.get_lines(&name, 5, 2), "");
    }

    #[test]
    fn fs_source_provider_out_of_bounds_range_is_clamped() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let name = make_file(dir.path(), "short.rs", "fn one() {}\nfn two() {}\n");
        let provider = FsSourceProvider::new(dir.path());

        // Request 1..100 — iterator takes what exists and stops.
        let got = provider.get_lines(&name, 1, 100);
        assert!(got.starts_with("fn one()"));
        assert!(got.contains("fn two()"));
    }
}
