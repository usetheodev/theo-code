//! T15.1 — `MarkdownDirSource`: filesystem-backed `DocSource`.
//!
//! Scans a directory recursively for `.md` files and yields one
//! [`DocEntry`] per file. Useful out of the box for any project that
//! ships local Markdown documentation (e.g. `docs/`, `wiki/`,
//! `~/.cache/theo/docs/<lang>/`).
//!
//! No network, no third-party crates beyond stdlib + `walkdir` (which
//! the workspace already pulls in elsewhere). The Markdown parsing is
//! deliberately minimal — enough to extract a sensible title (first
//! `# heading` or filename stem) and a body suitable for TF-IDF
//! tokenisation. Full HTML rendering is the consumer's concern.

use std::fs;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::docs_search::index::DocEntry;
use crate::docs_search::source::DocSource;

/// Maximum size (bytes) of a single Markdown file we will read into
/// memory. Files larger than this are skipped — they're almost
/// certainly not real docs (test fixtures, generated artefacts).
const MAX_FILE_SIZE: u64 = 4 * 1024 * 1024; // 4 MiB

/// Documentation source backed by a directory of `.md` files.
pub struct MarkdownDirSource {
    /// Short label that ends up in `DocEntry.source` (used by the
    /// tool's `source=` filter).
    name: String,
    /// Root directory scanned recursively for `.md` files.
    root: PathBuf,
    /// Optional language tag added to every emitted `DocEntry`.
    /// Defaults to `"markdown"` so consumers can still distinguish
    /// from code-language docs.
    language: String,
}

impl MarkdownDirSource {
    /// Build a source over `root`. The `name` becomes the source
    /// label visible to the LLM. `language` defaults to "markdown".
    pub fn new(name: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            root: root.into(),
            language: "markdown".into(),
        }
    }

    /// Override the language tag (e.g. "rust" if the directory is a
    /// curated Rust-API doc dump).
    #[must_use]
    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = language.into();
        self
    }

    /// Iterate the root directory, returning one [`DocEntry`] per
    /// `.md` file. Hidden directories (starting with `.`) and files
    /// larger than [`MAX_FILE_SIZE`] are skipped. IO errors on
    /// individual files are skipped silently — a corrupted file
    /// shouldn't kill the whole scan.
    fn scan(&self) -> Vec<DocEntry> {
        let mut out = Vec::new();
        if !self.root.exists() {
            return out;
        }
        let root = self.root.clone();
        for entry in WalkDir::new(&self.root)
            .into_iter()
            // Allow the root through unconditionally — its basename
            // may legitimately start with `.` (e.g. tempfile::tempdir
            // creates `/tmp/.tmpXXX`, or a user passes `.theo/wiki`).
            // Only descendants are filtered for hidden segments.
            .filter_entry(|e| e.path() == root.as_path() || !is_hidden(e.path()))
        {
            let Ok(entry) = entry else { continue };
            if !entry.file_type().is_file() {
                continue;
            }
            if entry.path().extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            if let Ok(meta) = entry.metadata()
                && meta.len() > MAX_FILE_SIZE
            {
                continue;
            }
            if let Some(doc) = self.read_entry(entry.path()) {
                out.push(doc);
            }
        }
        out
    }

    fn read_entry(&self, path: &Path) -> Option<DocEntry> {
        let body = fs::read_to_string(path).ok()?;
        let title = match extract_title(&body) {
            Some(t) => t,
            None => path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "untitled".to_string()),
        };
        let rel = path
            .strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        let url = format!("file://{}", path.display());
        Some(DocEntry {
            id: format!("{}::{rel}", self.name),
            source: self.name.clone(),
            language: self.language.clone(),
            title,
            url,
            body,
        })
    }
}

impl DocSource for MarkdownDirSource {
    fn name(&self) -> &str {
        &self.name
    }

    /// Fetch by relative path inside `root` (with or without `.md`
    /// suffix). Used by `docs_search`'s `source=` filter as a quick
    /// lookup; bulk indexing goes through [`Self::list_all`].
    fn fetch(&self, key: &str) -> Option<DocEntry> {
        let direct = self.root.join(key);
        let with_ext = if direct.extension().is_some() {
            direct.clone()
        } else {
            self.root.join(format!("{key}.md"))
        };
        let candidate = if direct.is_file() { direct } else { with_ext };
        if !candidate.is_file() {
            return None;
        }
        self.read_entry(&candidate)
    }

    fn list_all(&self) -> Vec<DocEntry> {
        self.scan()
    }
}

/// Skip hidden dirs/files (those starting with `.`). Always allows
/// the root itself even if its basename starts with `.` (e.g. user
/// passed a `.theo/wiki/` root explicitly).
fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.starts_with('.') && s != ".")
        .unwrap_or(false)
}

/// Extract the first `# heading` line from a Markdown body. Returns
/// `None` when no top-level heading exists.
fn extract_title(body: &str) -> Option<String> {
    for line in body.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("# ")
            && !rest.trim().is_empty()
        {
            return Some(rest.trim().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_root_with_files(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        for (rel_path, body) in files {
            let abs = dir.path().join(rel_path);
            if let Some(parent) = abs.parent() {
                fs::create_dir_all(parent).expect("mkdir -p");
            }
            let mut f = fs::File::create(&abs).expect("create md");
            f.write_all(body.as_bytes()).expect("write md");
        }
        dir
    }

    #[test]
    fn t151md_scan_empty_directory_yields_empty_vec() {
        let dir = tempfile::tempdir().unwrap();
        let src = MarkdownDirSource::new("local", dir.path().to_path_buf());
        assert!(src.list_all().is_empty());
    }

    #[test]
    fn t151md_scan_nonexistent_root_yields_empty_vec() {
        let src = MarkdownDirSource::new(
            "local",
            PathBuf::from("/nonexistent/path/should/not/exist/xyzz"),
        );
        assert!(src.list_all().is_empty());
    }

    #[test]
    fn t151md_scan_finds_md_files_recursively() {
        let dir = make_root_with_files(&[
            ("a.md", "# A\nbody a"),
            ("nested/b.md", "# B\nbody b"),
            ("deeply/nested/c.md", "# C\nbody c"),
        ]);
        let src = MarkdownDirSource::new("local", dir.path().to_path_buf());
        let entries = src.list_all();
        assert_eq!(entries.len(), 3);
        let mut titles: Vec<_> = entries.iter().map(|d| d.title.clone()).collect();
        titles.sort();
        assert_eq!(titles, vec!["A".to_string(), "B".to_string(), "C".to_string()]);
    }

    #[test]
    fn t151md_scan_skips_non_md_files() {
        let dir = make_root_with_files(&[
            ("real.md", "# real"),
            ("readme.txt", "not markdown"),
            ("script.sh", "#!/bin/sh"),
        ]);
        let src = MarkdownDirSource::new("local", dir.path().to_path_buf());
        let entries = src.list_all();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "real");
    }

    #[test]
    fn t151md_scan_skips_hidden_directories() {
        // `.git/` is the canonical case — must NOT be indexed even
        // when it contains .md files (e.g. .git/COMMIT_EDITMSG.md).
        let dir = make_root_with_files(&[
            ("visible.md", "# visible"),
            (".git/secret.md", "# do not index"),
            (".cache/cached.md", "# also no"),
        ]);
        let src = MarkdownDirSource::new("local", dir.path().to_path_buf());
        let titles: Vec<_> = src
            .list_all()
            .into_iter()
            .map(|d| d.title)
            .collect();
        assert_eq!(titles, vec!["visible"]);
    }

    #[test]
    fn t151md_extract_title_uses_first_h1() {
        let body = "intro paragraph\n\n# Real Title\nrest";
        assert_eq!(extract_title(body), Some("Real Title".to_string()));
    }

    #[test]
    fn t151md_extract_title_ignores_h2_h3() {
        let body = "## not h1\n### also not h1\n# Yes h1";
        assert_eq!(extract_title(body), Some("Yes h1".to_string()));
    }

    #[test]
    fn t151md_extract_title_returns_none_when_no_h1() {
        let body = "## sub-heading only\nsome text\n";
        assert!(extract_title(body).is_none());
    }

    #[test]
    fn t151md_extract_title_handles_empty_h1_gracefully() {
        // `# ` followed by nothing — falls through.
        let body = "# \n# Real";
        assert_eq!(extract_title(body), Some("Real".to_string()));
    }

    #[test]
    fn t151md_doc_entry_falls_back_to_filename_when_no_h1() {
        let dir = make_root_with_files(&[("no_title.md", "just body, no heading")]);
        let src = MarkdownDirSource::new("local", dir.path().to_path_buf());
        let entries = src.list_all();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "no_title");
    }

    #[test]
    fn t151md_doc_entry_id_uses_source_name_and_relative_path() {
        let dir = make_root_with_files(&[("nested/page.md", "# page")]);
        let src = MarkdownDirSource::new("project-docs", dir.path().to_path_buf());
        let entries = src.list_all();
        assert_eq!(entries.len(), 1);
        // Cross-platform path separator handled by display lossy.
        let id = &entries[0].id;
        assert!(id.starts_with("project-docs::"));
        assert!(id.contains("page.md"));
    }

    #[test]
    fn t151md_doc_entry_url_is_file_protocol_absolute() {
        let dir = make_root_with_files(&[("a.md", "# A")]);
        let src = MarkdownDirSource::new("local", dir.path().to_path_buf());
        let entries = src.list_all();
        assert!(entries[0].url.starts_with("file://"));
        assert!(entries[0].url.ends_with("a.md"));
    }

    #[test]
    fn t151md_with_language_overrides_default_markdown() {
        let dir = make_root_with_files(&[("a.md", "# A")]);
        let src = MarkdownDirSource::new("rust-std", dir.path().to_path_buf())
            .with_language("rust");
        assert_eq!(src.list_all()[0].language, "rust");
    }

    #[test]
    fn t151md_default_language_is_markdown() {
        let dir = make_root_with_files(&[("a.md", "# A")]);
        let src = MarkdownDirSource::new("local", dir.path().to_path_buf());
        assert_eq!(src.list_all()[0].language, "markdown");
    }

    #[test]
    fn t151md_fetch_by_relative_path_with_extension() {
        let dir = make_root_with_files(&[("a.md", "# A")]);
        let src = MarkdownDirSource::new("local", dir.path().to_path_buf());
        let d = src.fetch("a.md").expect("found by full name");
        assert_eq!(d.title, "A");
    }

    #[test]
    fn t151md_fetch_by_relative_path_without_extension() {
        let dir = make_root_with_files(&[("a.md", "# A")]);
        let src = MarkdownDirSource::new("local", dir.path().to_path_buf());
        let d = src.fetch("a").expect("found by stem");
        assert_eq!(d.title, "A");
    }

    #[test]
    fn t151md_fetch_returns_none_for_missing() {
        let dir = make_root_with_files(&[("a.md", "# A")]);
        let src = MarkdownDirSource::new("local", dir.path().to_path_buf());
        assert!(src.fetch("does-not-exist").is_none());
    }

    #[test]
    fn t151md_skips_oversized_file() {
        let dir = tempfile::tempdir().unwrap();
        let big = dir.path().join("huge.md");
        let mut f = fs::File::create(&big).unwrap();
        // Write MAX_FILE_SIZE + 1 bytes.
        let chunk = vec![b'a'; 1024 * 1024];
        for _ in 0..5 {
            f.write_all(&chunk).unwrap();
        }
        // Add a small file that SHOULD be indexed.
        let small = dir.path().join("ok.md");
        fs::write(&small, b"# ok").unwrap();

        let src = MarkdownDirSource::new("local", dir.path().to_path_buf());
        let entries = src.list_all();
        assert_eq!(entries.len(), 1, "huge.md should be skipped");
        assert_eq!(entries[0].title, "ok");
    }

    #[test]
    fn t151md_name_round_trips() {
        let src = MarkdownDirSource::new("my-source", PathBuf::from("/tmp"));
        assert_eq!(src.name(), "my-source");
    }
}
