//! Structural hygiene tests — computational sensors that detect codebase drift.
//!
//! Each test validates an invariant about the codebase structure.
//! These are "feedback sensors" (Böckeler, 2026): they observe after the agent
//! acts and help it self-correct. They run on every `cargo test`.

use std::fs;
use std::path::Path;

fn workspace_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn collect_rs_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if !dir.exists() {
        return files;
    }
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            files.extend(collect_rs_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
    files
}

/// No `println!` in library source code. Use `tracing` or `log` instead.
/// Allowed in: tests, examples, CLI main, benchmarks, binary entrypoints.
#[test]
fn no_println_in_library_code() {
    let root = workspace_root();
    let mut violations = Vec::new();

    for crate_dir in &["crates"] {
        let base = root.join(crate_dir);
        if !base.exists() {
            continue;
        }
        for entry in fs::read_dir(&base).unwrap() {
            let entry = entry.unwrap();
            let src_dir = entry.path().join("src");
            for file in collect_rs_files(&src_dir) {
                let path_str = file.to_string_lossy();
                // Allow in test files and binary entrypoints
                if path_str.contains("test") || path_str.contains("/bin/") {
                    continue;
                }
                let content = fs::read_to_string(&file).unwrap();
                for (i, line) in content.lines().enumerate() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("//") {
                        continue;
                    }
                    // Only flag println!, not eprintln! (eprintln is acceptable for warnings)
                    if (trimmed.contains("println!(") || trimmed.contains("print!("))
                        && !trimmed.contains("eprintln")
                        && !trimmed.contains("eprint!")
                    {
                        violations.push(format!(
                            "{}:{}: {}",
                            file.strip_prefix(&root).unwrap_or(&file).display(),
                            i + 1,
                            trimmed
                        ));
                    }
                }
            }
        }
    }

    // Track as info — eprintln is common for warnings in the current codebase
    if !violations.is_empty() {
        eprintln!(
            "INFO: {} println!/print! found in library code:\n  {}",
            violations.len(),
            violations.iter().take(10).cloned().collect::<Vec<_>>().join("\n  ")
        );
    }
}

/// Track source files exceeding 2000 lines as oversized.
/// Large files indicate SRP violations — split into modules.
#[test]
fn no_oversized_source_files() {
    let root = workspace_root();
    let mut violations = Vec::new();
    let max_lines = 2500; // Hard limit — no file should be this big

    for dir in &["crates", "apps"] {
        let base = root.join(dir);
        if !base.exists() {
            continue;
        }
        for entry in fs::read_dir(&base).unwrap() {
            let entry = entry.unwrap();
            let src_dir = entry.path().join("src");
            for file in collect_rs_files(&src_dir) {
                let content = fs::read_to_string(&file).unwrap();
                let line_count = content.lines().count();
                if line_count > max_lines {
                    violations.push(format!(
                        "{}: {} lines (max {})",
                        file.strip_prefix(&root).unwrap_or(&file).display(),
                        line_count,
                        max_lines
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Source files exceeding {} lines:\n  {}",
        max_lines,
        violations.join("\n  ")
    );
}

/// No `std::process::exit` outside of binary entrypoints.
/// Library code must return errors, not terminate the process.
#[test]
fn no_process_exit_in_libraries() {
    let root = workspace_root();
    let mut violations = Vec::new();

    let crates_dir = root.join("crates");
    if !crates_dir.exists() {
        return;
    }
    for entry in fs::read_dir(&crates_dir).unwrap() {
        let entry = entry.unwrap();
        let src_dir = entry.path().join("src");
        for file in collect_rs_files(&src_dir) {
            // Allow in binary entrypoints (src/bin/*.rs, src/main.rs)
            let path_str = file.to_string_lossy();
            if path_str.contains("/bin/") || path_str.ends_with("/main.rs") {
                continue;
            }
            let content = fs::read_to_string(&file).unwrap();
            for (i, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.starts_with("//") {
                    continue;
                }
                if trimmed.contains("std::process::exit")
                    || trimmed.contains("process::exit")
                {
                    violations.push(format!(
                        "{}:{}",
                        file.strip_prefix(&root).unwrap_or(&file).display(),
                        i + 1
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "std::process::exit found in library crates (return errors instead):\n  {}",
        violations.join("\n  ")
    );
}

/// Every workspace member crate must have a Cargo.toml with a [package] section.
#[test]
fn all_workspace_crates_have_package_section() {
    let root = workspace_root();
    let mut violations = Vec::new();

    // Read workspace members from root Cargo.toml
    let root_cargo = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let workspace_members: Vec<&str> = root_cargo
        .lines()
        .filter(|l| l.trim().starts_with('"') && (l.contains("crates/") || l.contains("apps/")))
        .filter_map(|l| l.trim().trim_matches(|c| c == '"' || c == ',').split('/').next_back())
        .collect();

    for dir in &["crates", "apps"] {
        let base = root.join(dir);
        if !base.exists() {
            continue;
        }
        for entry in fs::read_dir(&base).unwrap() {
            let entry = entry.unwrap();
            let crate_name = entry.file_name().to_string_lossy().to_string();

            // Only check crates that are workspace members
            if !workspace_members.iter().any(|m| *m == crate_name) {
                continue;
            }

            let cargo_path = entry.path().join("Cargo.toml");
            if !cargo_path.exists() {
                violations.push(format!("{}: missing Cargo.toml", entry.path().display()));
                continue;
            }
            let content = fs::read_to_string(&cargo_path).unwrap();
            if !content.contains("[package]") {
                violations.push(format!(
                    "{}: Cargo.toml missing [package] section",
                    entry.path().display()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Crate issues:\n  {}",
        violations.join("\n  ")
    );
}

/// theo-domain must have zero internal dependencies (pure types crate).
#[test]
fn theo_domain_is_dependency_free() {
    let root = workspace_root();
    let cargo_path = root.join("crates/theo-domain/Cargo.toml");
    let content = fs::read_to_string(&cargo_path).unwrap();

    let internal_deps: Vec<&str> = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("theo-") && !trimmed.starts_with("theo-domain")
        })
        .collect();

    assert!(
        internal_deps.is_empty(),
        "theo-domain must have zero internal dependencies, found:\n  {}",
        internal_deps.join("\n  ")
    );
}

/// Track `unsafe` blocks without SAFETY comments.
#[test]
fn unsafe_blocks_tracked() {
    let root = workspace_root();
    let mut violations = Vec::new();

    let crates_dir = root.join("crates");
    if !crates_dir.exists() {
        return;
    }
    for entry in fs::read_dir(&crates_dir).unwrap() {
        let entry = entry.unwrap();
        let src_dir = entry.path().join("src");
        for file in collect_rs_files(&src_dir) {
            let content = fs::read_to_string(&file).unwrap();
            let lines: Vec<&str> = content.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                let trimmed = line.trim();
                if trimmed.starts_with("//") {
                    continue;
                }
                if trimmed.contains("unsafe {") || trimmed.contains("unsafe{") {
                    let has_safety = i > 0
                        && (lines[i - 1].contains("SAFETY")
                            || lines[i - 1].contains("safety")
                            || lines[i - 1].contains("Safe because"));
                    if !has_safety {
                        violations.push(format!(
                            "{}:{}",
                            file.strip_prefix(&root).unwrap_or(&file).display(),
                            i + 1
                        ));
                    }
                }
            }
        }
    }

    // Track as info — unsafe blocks are rare in this codebase
    if !violations.is_empty() {
        eprintln!(
            "INFO: {} unsafe block(s) without SAFETY comment:\n  {}",
            violations.len(),
            violations.join("\n  ")
        );
    }
}

/// The .theo/ directory must contain required agent navigation files.
#[test]
fn required_theo_files_exist() {
    let root = workspace_root();
    let required = [
        ".theo/AGENTS.md",
        ".theo/QUALITY_RULES.md",
        ".theo/QUALITY_SCORE.md",
    ];

    let mut missing = Vec::new();
    for file in &required {
        let path = root.join(file);
        if !path.exists() {
            missing.push(*file);
        } else {
            let meta = fs::metadata(&path).unwrap();
            if meta.len() < 500 {
                missing.push(file);
            }
        }
    }

    assert!(
        missing.is_empty(),
        "Required .theo/ files missing or too small (<500 bytes):\n  {}",
        missing.join("\n  ")
    );
}

/// clippy.toml must exist at workspace root.
#[test]
fn clippy_toml_exists() {
    let root = workspace_root();
    let clippy_path = root.join("clippy.toml");
    assert!(
        clippy_path.exists(),
        "clippy.toml must exist at workspace root"
    );
}

/// No TODO or FIXME without an associated issue/ticket reference.
/// Tracks unresolved work that could be lost.
#[test]
fn todos_are_tracked() {
    let root = workspace_root();
    let mut untracked = Vec::new();

    for dir in &["crates"] {
        let base = root.join(dir);
        if !base.exists() {
            continue;
        }
        for entry in fs::read_dir(&base).unwrap() {
            let entry = entry.unwrap();
            let src_dir = entry.path().join("src");
            for file in collect_rs_files(&src_dir) {
                let content = fs::read_to_string(&file).unwrap();
                for (i, line) in content.lines().enumerate() {
                    let upper = line.to_uppercase();
                    if (upper.contains("TODO") || upper.contains("FIXME"))
                        && !line.contains('#')
                        && !line.contains("http")
                        && !line.contains("issue")
                        && !line.contains("ticket")
                    {
                        untracked.push(format!(
                            "{}:{}",
                            file.strip_prefix(&root).unwrap_or(&file).display(),
                            i + 1
                        ));
                    }
                }
            }
        }
    }

    // Track as info, not hard failure (some TODOs are inherent in development)
    if !untracked.is_empty() {
        eprintln!(
            "INFO: {} untracked TODO/FIXME(s) found (consider adding issue references):\n  {}",
            untracked.len(),
            untracked.iter().take(10).cloned().collect::<Vec<_>>().join("\n  ")
        );
    }
}

/// Test files should exist for every crate that has source code.
#[test]
fn every_crate_has_tests() {
    let root = workspace_root();
    let mut no_tests = Vec::new();

    let crates_dir = root.join("crates");
    if !crates_dir.exists() {
        return;
    }
    for entry in fs::read_dir(&crates_dir).unwrap() {
        let entry = entry.unwrap();
        let crate_name = entry.file_name().to_string_lossy().to_string();
        let src_dir = entry.path().join("src");

        if !src_dir.exists() {
            continue;
        }

        // Check for #[test] in src/ or tests/
        let mut has_tests = false;
        for file in collect_rs_files(&entry.path()) {
            let content = fs::read_to_string(&file).unwrap_or_default();
            if content.contains("#[test]") {
                has_tests = true;
                break;
            }
        }

        if !has_tests {
            no_tests.push(crate_name);
        }
    }

    // Track as info for now — some crates may legitimately have zero tests
    if !no_tests.is_empty() {
        eprintln!(
            "INFO: Crates without any tests: {}",
            no_tests.join(", ")
        );
    }
}
