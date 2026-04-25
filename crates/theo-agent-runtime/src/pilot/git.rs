//! Pilot git-progress helpers.
//!
//! Split out of `pilot/mod.rs` (REMEDIATION_PLAN T4.* — production-LOC
//! trim toward the per-file 500-line target). All helpers stay
//! `pub(super)` so the public crate surface is unchanged.

use std::path::Path;

pub(super) struct GitProgress {
    pub(super) sha_changed: bool,
    pub(super) files_changed: usize,
}

pub(super) async fn detect_git_progress(
    project_dir: &Path,
    previous_sha: &Option<String>,
) -> GitProgress {
    let current_sha = get_git_sha(project_dir).await;

    let sha_changed = match (previous_sha, &current_sha) {
        (Some(prev), Some(curr)) => prev != curr,
        _ => false,
    };

    // Count changed files (staged + unstaged + untracked)
    let files_changed = get_changed_file_count(project_dir).await;

    GitProgress {
        sha_changed,
        files_changed,
    }
}

pub(super) async fn get_git_sha(project_dir: &Path) -> Option<String> {
    let output = tokio::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(project_dir)
        .output()
        .await
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

pub(super) async fn get_changed_file_count(project_dir: &Path) -> usize {
    let output = tokio::process::Command::new("git")
        .args(["diff", "--stat"])
        .current_dir(project_dir)
        .output()
        .await;
    match output {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout);
            text.lines()
                .filter(|l| !l.trim().is_empty())
                .count()
                .saturating_sub(1) // last line is summary
        }
        Err(_) => 0,
    }
}
