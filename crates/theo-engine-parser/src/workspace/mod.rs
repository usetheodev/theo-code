//! Workspace detection for monorepo projects.
//!
//! Detects monorepo workspace layouts by reading manifest files
//! (pnpm-workspace.yaml, package.json, Cargo.toml, go.work, pyproject.toml)
//! and discovering sub-packages within the workspace.
//!
//! When a workspace is detected, each package becomes a separate `Component`
//! in the `CodeModel`, enabling per-package import resolution and module
//! boundary inference.

pub mod detect;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The kind of workspace/monorepo tool detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceKind {
    Pnpm,
    Npm,
    Cargo,
    Go,
    Uv,
}

impl std::fmt::Display for WorkspaceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pnpm => write!(f, "pnpm"),
            Self::Npm => write!(f, "npm"),
            Self::Cargo => write!(f, "Cargo"),
            Self::Go => write!(f, "Go"),
            Self::Uv => write!(f, "uv"),
        }
    }
}

/// A single package/crate/module within the workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspacePackage {
    pub name: String,
    pub root: PathBuf,
}

/// Layout of a detected monorepo workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceLayout {
    pub kind: WorkspaceKind,
    pub workspace_root: PathBuf,
    pub packages: Vec<WorkspacePackage>,
}

impl WorkspaceLayout {
    /// Find which package owns a given file path using longest-prefix matching.
    ///
    /// Returns the package name if the file path starts with any package root.
    /// Files outside all packages return `None` (they route to the default
    /// component).
    pub fn component_for_path(&self, path: &Path) -> Option<&str> {
        let mut best_match: Option<(&str, usize)> = None;

        for pkg in &self.packages {
            if path.starts_with(&pkg.root) {
                let prefix_len = pkg.root.as_os_str().len();
                match best_match {
                    Some((_, current_len)) if prefix_len > current_len => {
                        best_match = Some((&pkg.name, prefix_len));
                    }
                    None => {
                        best_match = Some((&pkg.name, prefix_len));
                    }
                    _ => {}
                }
            }
        }

        best_match.map(|(name, _)| name)
    }
}

/// Detect the workspace layout for a project root.
///
/// Tries manifest parsers in priority order: pnpm → npm → Cargo → Go → uv.
/// Returns `None` for single-project repos (no workspace manifest found)
/// or when a manifest exists but is malformed (logged as warning, graceful
/// fallback to single-project mode).
pub fn detect_workspace(project_root: &Path) -> Option<WorkspaceLayout> {
    detect::detect_pnpm(project_root)
        .or_else(|| detect::detect_npm(project_root))
        .or_else(|| detect::detect_cargo(project_root))
        .or_else(|| detect::detect_go(project_root))
        .or_else(|| detect::detect_uv(project_root))
}
