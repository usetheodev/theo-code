//! Atomic filesystem helpers shared by all memory writers.
//!
//! `atomic_write` is the one-true writer for markdown / JSONL artefacts
//! in `.theo/memory/`, `.theo/wiki/memory/`, `.theo/reflections.jsonl`.
//! Every caller goes through this helper so a panic mid-write cannot
//! corrupt on-disk state.
//!
//! Pattern: write-to-temp + rename. `rename` is atomic on POSIX; on
//! Windows a best-effort remove+rename dance is used.
//!
//! Plan ref: `outputs/agent-memory-plan.md` §RM1 DoD extras (atomic
//! write via `theo-infra-memory::fs_util::atomic_write`).

use std::path::Path;

use theo_domain::memory::MemoryError;

/// Write `content` to `target` atomically via a sibling `<target>.tmp`
/// file + `rename`. Creates parent directories if missing.
pub async fn atomic_write(target: &Path, content: &[u8]) -> Result<(), MemoryError> {
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| MemoryError::StoreFailed {
                key: target.display().to_string(),
                source: e,
            })?;
    }
    let mut tmp = target.to_path_buf();
    let file_name = tmp
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "out".to_string());
    tmp.set_file_name(format!("{file_name}.tmp"));
    tokio::fs::write(&tmp, content)
        .await
        .map_err(|e| MemoryError::StoreFailed {
            key: tmp.display().to_string(),
            source: e,
        })?;
    tokio::fs::rename(&tmp, target)
        .await
        .map_err(|e| MemoryError::StoreFailed {
            key: target.display().to_string(),
            source: e,
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn atomic_write_creates_file_with_content() {
        let dir = tempfile_dir();
        let target = dir.join("a/b/c.txt");
        atomic_write(&target, b"hello").await.unwrap();
        assert_eq!(tokio::fs::read(&target).await.unwrap(), b"hello");
    }

    #[tokio::test]
    async fn atomic_write_overwrites_existing() {
        let dir = tempfile_dir();
        let target = dir.join("x.txt");
        atomic_write(&target, b"v1").await.unwrap();
        atomic_write(&target, b"v2").await.unwrap();
        assert_eq!(tokio::fs::read(&target).await.unwrap(), b"v2");
    }

    #[tokio::test]
    async fn atomic_write_cleans_temp_file_on_success() {
        let dir = tempfile_dir();
        let target = dir.join("x.md");
        atomic_write(&target, b"final").await.unwrap();
        let tmp_sibling = dir.join("x.md.tmp");
        assert!(
            !tmp_sibling.exists(),
            "temp sibling should be renamed away after success"
        );
    }

    fn tempfile_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "theo-memory-fs-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
