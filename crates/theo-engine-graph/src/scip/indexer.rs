//! SCIP Indexer — invokes rust-analyzer scip to generate index.scip.
//!
//! Runs in background, detects staleness, gracefully handles missing rust-analyzer.
//! No feature gate needed — this module only invokes external binaries.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Default output path for the SCIP index.
const SCIP_INDEX_FILE: &str = ".theo/index.scip";

/// Check if rust-analyzer is available on the system.
pub fn is_rust_analyzer_available() -> bool {
    Command::new("rust-analyzer")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Generate a SCIP index for a Rust project.
///
/// Invokes `rust-analyzer scip <project_dir>` and writes to `.theo/index.scip`.
/// Returns the path to the generated index, or None if generation failed.
///
/// This is a blocking operation that may take 30-90 seconds for large projects.
/// Callers should run this in `tokio::task::spawn_blocking`.
pub fn generate_scip_index(project_dir: &Path) -> Option<PathBuf> {
    // Verify rust-analyzer is available
    if !is_rust_analyzer_available() {
        eprintln!(
            "[scip] rust-analyzer not found — SCIP indexing unavailable. Falling back to Tree-Sitter."
        );
        return None;
    }

    // Verify this is a Rust project
    if !project_dir.join("Cargo.toml").exists() {
        return None; // Not a Rust project — SCIP only supports Rust via rust-analyzer
    }

    let output_dir = project_dir.join(".theo");
    let _ = std::fs::create_dir_all(&output_dir);
    let output_path = output_dir.join("index.scip");

    eprintln!("[scip] Generating SCIP index via rust-analyzer...");

    let result = Command::new("rust-analyzer")
        .arg("scip")
        .arg(project_dir)
        .arg("--output")
        .arg(&output_path)
        .current_dir(project_dir)
        .output();

    match result {
        Ok(output) if output.status.success() => {
            if let Ok(meta) = std::fs::metadata(&output_path) {
                eprintln!(
                    "[scip] Index generated: {} ({} bytes)",
                    output_path.display(),
                    meta.len()
                );
                Some(output_path)
            } else {
                eprintln!("[scip] Index file not found after generation");
                None
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!(
                "[scip] rust-analyzer scip failed: {}",
                stderr.chars().take(200).collect::<String>()
            );
            None
        }
        Err(e) => {
            eprintln!("[scip] Failed to run rust-analyzer: {}", e);
            None
        }
    }
}

/// Check if an existing SCIP index is stale (older than any source file).
pub fn is_index_stale(project_dir: &Path) -> bool {
    let index_path = project_dir.join(SCIP_INDEX_FILE);

    let index_mtime = match std::fs::metadata(&index_path).and_then(|m| m.modified()) {
        Ok(t) => t,
        Err(_) => return true, // No index = stale
    };

    // Check if any .rs file is newer than the index
    let walker = ignore::WalkBuilder::new(project_dir)
        .hidden(true)
        .git_ignore(true)
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            if let Ok(meta) = std::fs::metadata(path) {
                if let Ok(mtime) = meta.modified() {
                    if mtime > index_mtime {
                        return true; // Source newer than index
                    }
                }
            }
        }
    }

    false
}

/// Get the SCIP index path for a project.
pub fn scip_index_path(project_dir: &Path) -> PathBuf {
    project_dir.join(SCIP_INDEX_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_check_returns_true_for_nonexistent_index() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(is_index_stale(tmp.path()));
    }

    #[test]
    fn scip_index_path_is_correct() {
        let path = scip_index_path(Path::new("/project"));
        assert_eq!(path.to_string_lossy(), "/project/.theo/index.scip");
    }
}
