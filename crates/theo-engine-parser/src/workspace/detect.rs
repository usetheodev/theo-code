//! Manifest-based workspace detection for 5 monorepo formats.
//!
//! Each detector reads a specific manifest file, parses workspace member
//! patterns, expands globs to discover package directories, and returns
//! a [`WorkspaceLayout`] with the detected packages.
//!
//! Detection priority: pnpm → npm → Cargo → Go → uv.
//! Each detector returns `None` if the manifest file doesn't exist or
//! is malformed (logged as a warning for graceful fallback).

use std::path::{Path, PathBuf};


use super::{WorkspaceKind, WorkspaceLayout, WorkspacePackage};

/// Derive a package name from a directory path relative to the workspace root.
///
/// For example, `packages/auth` → `"auth"`, `apps/web/frontend` → `"web/frontend"`.
/// Used when the manifest doesn't contain explicit package names (pnpm, npm).
fn name_from_relative_path(workspace_root: &Path, package_dir: &Path) -> String {
    package_dir
        .strip_prefix(workspace_root)
        .unwrap_or(package_dir)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Expand a set of glob patterns relative to a root directory into concrete
/// directory paths. Each pattern is joined with the root before expansion.
///
/// Invalid patterns or I/O errors are logged and skipped.
fn expand_globs(root: &Path, patterns: &[String]) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for pattern in patterns {
        let full_pattern = root.join(pattern);
        let pattern_str = full_pattern.to_string_lossy();
        match glob::glob(&pattern_str) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    if entry.is_dir() {
                        dirs.push(entry);
                    }
                }
            }
            Err(e) => {
                eprintln!("[warn] pattern={}, error={}: invalid glob pattern in workspace manifest", pattern, e);
            }
        }
    }
    dirs
}

/// Read package name from a `package.json` file in the given directory.
///
/// Returns the `"name"` field if present, otherwise derives from the path.
fn read_npm_package_name(package_dir: &Path, workspace_root: &Path) -> String {
    let pkg_json_path = package_dir.join("package.json");
    if let Ok(contents) = std::fs::read_to_string(&pkg_json_path) {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&contents) {
            if let Some(name) = parsed.get("name").and_then(|v| v.as_str()) {
                return name.to_string();
            }
        }
    }
    name_from_relative_path(workspace_root, package_dir)
}

// ---------------------------------------------------------------------------
// pnpm workspace detection
// ---------------------------------------------------------------------------

/// pnpm workspace layout from `pnpm-workspace.yaml`.
///
/// Expected format:
/// ```yaml
/// packages:
///   - "packages/*"
///   - "apps/*"
/// ```
#[derive(serde::Deserialize)]
struct PnpmWorkspaceYaml {
    packages: Option<Vec<String>>,
}

pub(crate) fn detect_pnpm(root: &Path) -> Option<WorkspaceLayout> {
    let manifest_path = root.join("pnpm-workspace.yaml");
    let contents = std::fs::read_to_string(&manifest_path).ok()?;

    let parsed: PnpmWorkspaceYaml = match serde_yaml::from_str(&contents) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[warn] path={}, error={}: malformed pnpm-workspace.yaml, falling back to single-project mode", manifest_path.display(), e);
            return None;
        }
    };

    let patterns = parsed.packages?;
    if patterns.is_empty() {
        return None;
    }

    let dirs = expand_globs(root, &patterns);
    if dirs.is_empty() {
        return None;
    }

    let packages: Vec<WorkspacePackage> = dirs
        .into_iter()
        .map(|dir| {
            let name = read_npm_package_name(&dir, root);
            WorkspacePackage { name, root: dir }
        })
        .collect();

    Some(WorkspaceLayout {
        kind: WorkspaceKind::Pnpm,
        workspace_root: root.to_path_buf(),
        packages,
    })
}

// ---------------------------------------------------------------------------
// npm/yarn workspace detection
// ---------------------------------------------------------------------------

pub(crate) fn detect_npm(root: &Path) -> Option<WorkspaceLayout> {
    let manifest_path = root.join("package.json");
    let contents = std::fs::read_to_string(&manifest_path).ok()?;

    let parsed: serde_json::Value = match serde_json::from_str(&contents) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[warn] path={}, error={}: malformed package.json, falling back to single-project mode", manifest_path.display(), e);
            return None;
        }
    };

    // "workspaces" can be:
    //  - an array: ["packages/*", "apps/*"]
    //  - an object: { "packages": ["packages/*"] }  (yarn classic)
    let patterns: Vec<String> = match parsed.get("workspaces") {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        Some(serde_json::Value::Object(obj)) => obj
            .get("packages")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        _ => return None,
    };

    if patterns.is_empty() {
        return None;
    }

    let dirs = expand_globs(root, &patterns);
    if dirs.is_empty() {
        return None;
    }

    let packages: Vec<WorkspacePackage> = dirs
        .into_iter()
        .map(|dir| {
            let name = read_npm_package_name(&dir, root);
            WorkspacePackage { name, root: dir }
        })
        .collect();

    Some(WorkspaceLayout {
        kind: WorkspaceKind::Npm,
        workspace_root: root.to_path_buf(),
        packages,
    })
}

// ---------------------------------------------------------------------------
// Cargo workspace detection
// ---------------------------------------------------------------------------

pub(crate) fn detect_cargo(root: &Path) -> Option<WorkspaceLayout> {
    let manifest_path = root.join("Cargo.toml");
    let contents = std::fs::read_to_string(&manifest_path).ok()?;

    let parsed: toml::Value = match contents.parse() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[warn] path={}, error={}: malformed Cargo.toml, falling back to single-project mode", manifest_path.display(), e);
            return None;
        }
    };

    let members = parsed
        .get("workspace")
        .and_then(|ws| ws.get("members"))
        .and_then(|m| m.as_array())?;

    let patterns: Vec<String> = members
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    if patterns.is_empty() {
        return None;
    }

    let dirs = expand_globs(root, &patterns);
    if dirs.is_empty() {
        return None;
    }

    let packages: Vec<WorkspacePackage> = dirs
        .into_iter()
        .map(|dir| {
            let name = read_cargo_package_name(&dir, root);
            WorkspacePackage { name, root: dir }
        })
        .collect();

    if packages.is_empty() {
        return None;
    }

    Some(WorkspaceLayout {
        kind: WorkspaceKind::Cargo,
        workspace_root: root.to_path_buf(),
        packages,
    })
}

/// Read crate name from a member's `Cargo.toml` `[package] name` field.
///
/// Falls back to deriving the name from the directory path.
fn read_cargo_package_name(member_dir: &Path, workspace_root: &Path) -> String {
    let cargo_path = member_dir.join("Cargo.toml");
    if let Ok(contents) = std::fs::read_to_string(&cargo_path) {
        if let Ok(parsed) = contents.parse::<toml::Value>() {
            if let Some(name) = parsed
                .get("package")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
            {
                return name.to_string();
            }
        }
    }
    name_from_relative_path(workspace_root, member_dir)
}

// ---------------------------------------------------------------------------
// Go workspace detection (go.work)
// ---------------------------------------------------------------------------

pub(crate) fn detect_go(root: &Path) -> Option<WorkspaceLayout> {
    let work_path = root.join("go.work");
    let contents = std::fs::read_to_string(&work_path).ok()?;

    // go.work format is NOT TOML/YAML. It looks like:
    //   go 1.22
    //   use (
    //       ./cmd/api
    //       ./pkg/auth
    //   )
    // We parse the `use (...)` block line-by-line.
    let modules = parse_go_work_use_block(&contents);
    if modules.is_empty() {
        return None;
    }

    let packages: Vec<WorkspacePackage> = modules
        .into_iter()
        .filter_map(|rel_path| {
            let dir = root.join(&rel_path);
            if !dir.is_dir() {
                return None;
            }
            let name = read_go_module_name(&dir, root);
            Some(WorkspacePackage { name, root: dir })
        })
        .collect();

    if packages.is_empty() {
        return None;
    }

    Some(WorkspaceLayout {
        kind: WorkspaceKind::Go,
        workspace_root: root.to_path_buf(),
        packages,
    })
}

/// Parse `use (...)` blocks from go.work content.
///
/// Returns the relative paths listed inside the use block.
fn parse_go_work_use_block(contents: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut inside_use_block = false;

    for line in contents.lines() {
        let trimmed = line.trim();

        if trimmed == "use (" {
            inside_use_block = true;
            continue;
        }

        if inside_use_block {
            if trimmed == ")" {
                inside_use_block = false;
                continue;
            }
            // Each line inside `use (...)` is a relative directory path
            if !trimmed.is_empty() && !trimmed.starts_with("//") {
                paths.push(trimmed.to_string());
            }
        }

        // Single-line form: `use ./some/path`
        if trimmed.starts_with("use ") && !trimmed.contains('(') {
            let path_part = trimmed.strip_prefix("use ").unwrap_or("").trim();
            if !path_part.is_empty() {
                paths.push(path_part.to_string());
            }
        }
    }

    paths
}

/// Read Go module name from `go.mod` in the given directory.
///
/// Falls back to deriving from the directory path.
fn read_go_module_name(module_dir: &Path, workspace_root: &Path) -> String {
    let go_mod_path = module_dir.join("go.mod");
    if let Ok(contents) = std::fs::read_to_string(&go_mod_path) {
        for line in contents.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("module ") {
                let module_name = trimmed.strip_prefix("module ").unwrap_or("").trim();
                if !module_name.is_empty() {
                    return module_name.to_string();
                }
            }
        }
    }
    name_from_relative_path(workspace_root, module_dir)
}

// ---------------------------------------------------------------------------
// uv workspace detection (pyproject.toml)
// ---------------------------------------------------------------------------

pub(crate) fn detect_uv(root: &Path) -> Option<WorkspaceLayout> {
    let manifest_path = root.join("pyproject.toml");
    let contents = std::fs::read_to_string(&manifest_path).ok()?;

    let parsed: toml::Value = match contents.parse() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[warn] path={}, error={}: malformed pyproject.toml, falling back to single-project mode", manifest_path.display(), e);
            return None;
        }
    };

    let members = parsed
        .get("tool")
        .and_then(|t| t.get("uv"))
        .and_then(|uv| uv.get("workspace"))
        .and_then(|ws| ws.get("members"))
        .and_then(|m| m.as_array())?;

    let patterns: Vec<String> = members
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    if patterns.is_empty() {
        return None;
    }

    let dirs = expand_globs(root, &patterns);
    if dirs.is_empty() {
        return None;
    }

    let packages: Vec<WorkspacePackage> = dirs
        .into_iter()
        .map(|dir| {
            let name = read_python_package_name(&dir, root);
            WorkspacePackage { name, root: dir }
        })
        .collect();

    if packages.is_empty() {
        return None;
    }

    Some(WorkspaceLayout {
        kind: WorkspaceKind::Uv,
        workspace_root: root.to_path_buf(),
        packages,
    })
}

/// Read Python package name from a member's `pyproject.toml` `[project] name` field.
///
/// Falls back to deriving from the directory path.
fn read_python_package_name(member_dir: &Path, workspace_root: &Path) -> String {
    let pyproject_path = member_dir.join("pyproject.toml");
    if let Ok(contents) = std::fs::read_to_string(&pyproject_path) {
        if let Ok(parsed) = contents.parse::<toml::Value>() {
            if let Some(name) = parsed
                .get("project")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
            {
                return name.to_string();
            }
        }
    }
    name_from_relative_path(workspace_root, member_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Helper to create a temp directory tree from path-content pairs.
    fn create_tree(entries: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (path, content) in entries {
            let full_path = dir.path().join(path);
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&full_path, content).unwrap();
        }
        dir
    }

    // --- pnpm ---

    #[test]
    fn detect_pnpm_workspace() {
        let dir = create_tree(&[
            ("pnpm-workspace.yaml", "packages:\n  - \"packages/*\"\n"),
            ("packages/api/package.json", r#"{"name": "@mono/api"}"#),
            ("packages/auth/package.json", r#"{"name": "@mono/auth"}"#),
        ]);

        let layout = detect_pnpm(dir.path()).expect("should detect pnpm workspace");

        assert_eq!(layout.kind, WorkspaceKind::Pnpm);
        assert_eq!(layout.packages.len(), 2);

        let names: Vec<&str> = layout.packages.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"@mono/api"));
        assert!(names.contains(&"@mono/auth"));
    }

    #[test]
    fn pnpm_without_package_json_uses_dir_name() {
        let dir = create_tree(&[
            ("pnpm-workspace.yaml", "packages:\n  - \"packages/*\"\n"),
            ("packages/api/src/index.ts", "// api code"),
        ]);

        let layout = detect_pnpm(dir.path()).expect("should detect pnpm workspace");

        assert_eq!(layout.packages.len(), 1);
        assert_eq!(layout.packages[0].name, "packages/api");
    }

    // --- npm ---

    #[test]
    fn detect_npm_workspace_array_format() {
        let dir = create_tree(&[
            (
                "package.json",
                r#"{"name": "root", "workspaces": ["packages/*"]}"#,
            ),
            ("packages/ui/package.json", r#"{"name": "@mono/ui"}"#),
        ]);

        let layout = detect_npm(dir.path()).expect("should detect npm workspace");

        assert_eq!(layout.kind, WorkspaceKind::Npm);
        assert_eq!(layout.packages.len(), 1);
        assert_eq!(layout.packages[0].name, "@mono/ui");
    }

    #[test]
    fn detect_npm_workspace_object_format() {
        let dir = create_tree(&[
            (
                "package.json",
                r#"{"name": "root", "workspaces": {"packages": ["packages/*"]}}"#,
            ),
            ("packages/core/package.json", r#"{"name": "@mono/core"}"#),
        ]);

        let layout = detect_npm(dir.path()).expect("should detect npm workspace (yarn format)");

        assert_eq!(layout.kind, WorkspaceKind::Npm);
        assert_eq!(layout.packages.len(), 1);
        assert_eq!(layout.packages[0].name, "@mono/core");
    }

    #[test]
    fn npm_no_workspaces_field_returns_none() {
        let dir = create_tree(&[("package.json", r#"{"name": "single-app"}"#)]);

        assert!(detect_npm(dir.path()).is_none());
    }

    // --- Cargo ---

    #[test]
    fn detect_cargo_workspace() {
        let dir = create_tree(&[
            ("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n"),
            (
                "crates/core/Cargo.toml",
                "[package]\nname = \"mono-core\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            (
                "crates/api/Cargo.toml",
                "[package]\nname = \"mono-api\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
        ]);

        let layout = detect_cargo(dir.path()).expect("should detect Cargo workspace");

        assert_eq!(layout.kind, WorkspaceKind::Cargo);
        assert_eq!(layout.packages.len(), 2);

        let names: Vec<&str> = layout.packages.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"mono-core"));
        assert!(names.contains(&"mono-api"));
    }

    #[test]
    fn cargo_no_workspace_section_returns_none() {
        let dir = create_tree(&[(
            "Cargo.toml",
            "[package]\nname = \"single-crate\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )]);

        assert!(detect_cargo(dir.path()).is_none());
    }

    // --- Go ---

    #[test]
    fn detect_go_workspace() {
        let dir = create_tree(&[
            (
                "go.work",
                "go 1.22\n\nuse (\n\t./cmd/api\n\t./pkg/auth\n)\n",
            ),
            (
                "cmd/api/go.mod",
                "module example.com/mono/cmd/api\n\ngo 1.22\n",
            ),
            ("cmd/api/main.go", "package main"),
            (
                "pkg/auth/go.mod",
                "module example.com/mono/pkg/auth\n\ngo 1.22\n",
            ),
            ("pkg/auth/auth.go", "package auth"),
        ]);

        let layout = detect_go(dir.path()).expect("should detect Go workspace");

        assert_eq!(layout.kind, WorkspaceKind::Go);
        assert_eq!(layout.packages.len(), 2);

        let names: Vec<&str> = layout.packages.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"example.com/mono/cmd/api"));
        assert!(names.contains(&"example.com/mono/pkg/auth"));
    }

    #[test]
    fn go_single_use_line() {
        let dir = create_tree(&[
            ("go.work", "go 1.22\n\nuse ./cmd/api\n"),
            ("cmd/api/go.mod", "module example.com/api\n\ngo 1.22\n"),
            ("cmd/api/main.go", "package main"),
        ]);

        let layout = detect_go(dir.path()).expect("should detect single-use Go workspace");

        assert_eq!(layout.packages.len(), 1);
        assert_eq!(layout.packages[0].name, "example.com/api");
    }

    // --- uv ---

    #[test]
    fn detect_uv_workspace() {
        let dir = create_tree(&[
            (
                "pyproject.toml",
                "[tool.uv.workspace]\nmembers = [\"packages/*\"]\n",
            ),
            (
                "packages/core/pyproject.toml",
                "[project]\nname = \"mono-core\"\nversion = \"0.1.0\"\n",
            ),
            (
                "packages/api/pyproject.toml",
                "[project]\nname = \"mono-api\"\nversion = \"0.1.0\"\n",
            ),
        ]);

        let layout = detect_uv(dir.path()).expect("should detect uv workspace");

        assert_eq!(layout.kind, WorkspaceKind::Uv);
        assert_eq!(layout.packages.len(), 2);

        let names: Vec<&str> = layout.packages.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"mono-core"));
        assert!(names.contains(&"mono-api"));
    }

    #[test]
    fn uv_no_workspace_section_returns_none() {
        let dir = create_tree(&[(
            "pyproject.toml",
            "[project]\nname = \"single-app\"\nversion = \"0.1.0\"\n",
        )]);

        assert!(detect_uv(dir.path()).is_none());
    }

    // --- Edge cases ---

    #[test]
    fn detect_no_workspace() {
        let dir = create_tree(&[("src/main.rs", "fn main() {}")]);

        assert!(super::super::detect_workspace(dir.path()).is_none());
    }

    #[test]
    fn malformed_pnpm_manifest_returns_none() {
        let dir = create_tree(&[("pnpm-workspace.yaml", "this is: not: valid: yaml: [")]);

        assert!(detect_pnpm(dir.path()).is_none());
    }

    #[test]
    fn malformed_cargo_manifest_returns_none() {
        let dir = create_tree(&[("Cargo.toml", "this is not valid toml {{{{")]);

        assert!(detect_cargo(dir.path()).is_none());
    }

    // --- component_for_path ---

    #[test]
    fn component_for_path_longest_prefix() {
        let layout = WorkspaceLayout {
            kind: WorkspaceKind::Pnpm,
            workspace_root: PathBuf::from("/repo"),
            packages: vec![
                WorkspacePackage {
                    name: "api".into(),
                    root: PathBuf::from("/repo/packages/api"),
                },
                WorkspacePackage {
                    name: "auth".into(),
                    root: PathBuf::from("/repo/packages/auth"),
                },
            ],
        };

        assert_eq!(
            layout.component_for_path(Path::new("/repo/packages/api/src/index.ts")),
            Some("api")
        );
        assert_eq!(
            layout.component_for_path(Path::new("/repo/packages/auth/src/middleware.ts")),
            Some("auth")
        );
        // File outside all packages
        assert_eq!(
            layout.component_for_path(Path::new("/repo/scripts/deploy.sh")),
            None
        );
    }

    #[test]
    fn component_for_path_nested_packages() {
        let layout = WorkspaceLayout {
            kind: WorkspaceKind::Npm,
            workspace_root: PathBuf::from("/repo"),
            packages: vec![
                WorkspacePackage {
                    name: "outer".into(),
                    root: PathBuf::from("/repo/packages"),
                },
                WorkspacePackage {
                    name: "inner".into(),
                    root: PathBuf::from("/repo/packages/inner"),
                },
            ],
        };

        // Should match the more specific (longer) prefix
        assert_eq!(
            layout.component_for_path(Path::new("/repo/packages/inner/src/lib.ts")),
            Some("inner")
        );
        // Falls back to outer for files directly in packages/
        assert_eq!(
            layout.component_for_path(Path::new("/repo/packages/utils.ts")),
            Some("outer")
        );
    }

    // --- go.work parser ---

    #[test]
    fn parse_go_work_use_block_multi_module() {
        let content = "go 1.22\n\nuse (\n\t./cmd/api\n\t./pkg/auth\n\t./internal/db\n)\n";
        let paths = parse_go_work_use_block(content);
        assert_eq!(paths, vec!["./cmd/api", "./pkg/auth", "./internal/db"]);
    }

    #[test]
    fn parse_go_work_use_block_with_comments() {
        let content = "go 1.22\n\nuse (\n\t// main service\n\t./cmd/api\n)\n";
        let paths = parse_go_work_use_block(content);
        assert_eq!(paths, vec!["./cmd/api"]);
    }

    #[test]
    fn parse_go_work_single_use() {
        let content = "go 1.22\n\nuse ./cmd/api\n";
        let paths = parse_go_work_use_block(content);
        assert_eq!(paths, vec!["./cmd/api"]);
    }
}
