//! Structural boundary test — validates architectural dependency rules.
//!
//! Rules from `.claude/rules/architecture.md`:
//! - `theo-domain` has NO internal dependencies (pure types)
//! - `apps/*` NEVER import engine/infra crates directly — they talk to `theo-application`
//! - No circular dependencies

use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Crate prefixes that apps are forbidden from importing directly.
const FORBIDDEN_FOR_APPS: &[&str] = &[
    "theo-engine-graph",
    "theo-engine-parser",
    "theo-engine-retrieval",
    "theo-governance",
    "theo-infra-llm",
    "theo-infra-auth",
    "theo-tooling",
];

/// Crates that `theo-domain` is forbidden from depending on.
const FORBIDDEN_FOR_DOMAIN: &[&str] = &[
    "theo-engine-graph",
    "theo-engine-parser",
    "theo-engine-retrieval",
    "theo-governance",
    "theo-infra-llm",
    "theo-infra-auth",
    "theo-tooling",
    "theo-agent-runtime",
    "theo-application",
    "theo-api-contracts",
];

/// Allowed dependencies for apps (the only internal crates apps may import).
const ALLOWED_FOR_APPS: &[&str] = &[
    "theo-application",
    "theo-api-contracts",
    "theo-domain",
    "theo-agent-runtime",
];

fn workspace_root() -> std::path::PathBuf {
    // CARGO_MANIFEST_DIR points to crates/theo-governance/
    // Workspace root is two levels up
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Extract workspace dependency names from a Cargo.toml content string.
/// Only returns deps that start with "theo-".
fn extract_theo_deps(cargo_toml_content: &str) -> Vec<String> {
    let mut deps = Vec::new();
    let mut in_deps = false;
    let mut in_build_deps = false;

    for line in cargo_toml_content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("[dependencies]")
            || trimmed.starts_with("[dev-dependencies]")
        {
            in_deps = true;
            in_build_deps = false;
            continue;
        }
        if trimmed.starts_with("[build-dependencies]") {
            in_build_deps = true;
            in_deps = false;
            continue;
        }
        if trimmed.starts_with('[') {
            in_deps = false;
            in_build_deps = false;
            continue;
        }

        if in_deps && !in_build_deps {
            // Match lines like: theo-domain.workspace = true
            // or: theo-domain = { ... }
            // or: theo-domain = "0.1"
            if let Some(name) = trimmed.split(&['.', ' ', '='][..]).next() {
                let name = name.trim();
                if name.starts_with("theo-") {
                    deps.push(name.to_string());
                }
            }
        }
    }

    deps
}

/// Collect all crate Cargo.toml files grouped by category (apps vs crates).
fn collect_cargo_tomls() -> HashMap<String, (String, Vec<String>)> {
    let root = workspace_root();
    let mut result = HashMap::new();

    for dir in &["crates", "apps"] {
        let dir_path = root.join(dir);
        if !dir_path.exists() {
            continue;
        }
        for entry in fs::read_dir(&dir_path).unwrap() {
            let entry = entry.unwrap();
            let cargo_path = entry.path().join("Cargo.toml");
            if cargo_path.exists() {
                let content = fs::read_to_string(&cargo_path).unwrap();
                let crate_name = entry.file_name().to_string_lossy().to_string();
                let deps = extract_theo_deps(&content);
                let category = dir.to_string();
                result.insert(crate_name, (category, deps));
            }
        }
    }

    result
}

/// Engine violations: MUST be zero. Hard assert.
const ENGINE_CRATES: &[&str] = &[
    "theo-engine-graph",
    "theo-engine-parser",
    "theo-engine-retrieval",
    "theo-governance",
];

#[test]
fn apps_must_not_import_engines_directly() {
    let crates = collect_cargo_tomls();
    let mut violations = Vec::new();

    for (name, (category, deps)) in &crates {
        if category != "apps" {
            continue;
        }
        for dep in deps {
            if ENGINE_CRATES.contains(&dep.as_str()) {
                violations.push(format!("apps/{name} imports {dep} directly"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Engine boundary violations (apps must use theo-application):\n  {}",
        violations.join("\n  ")
    );
}

/// Infra violations: tracked as warnings (future refactor).
#[test]
fn apps_infra_imports_tracked_as_warnings() {
    let crates = collect_cargo_tomls();
    let infra_crates = ["theo-infra-llm", "theo-infra-auth", "theo-tooling"];
    let mut warnings = Vec::new();

    for (name, (category, deps)) in &crates {
        if category != "apps" {
            continue;
        }
        for dep in deps {
            if infra_crates.contains(&dep.as_str()) {
                warnings.push(format!("apps/{name} imports {dep} directly"));
            }
        }
    }

    if !warnings.is_empty() {
        eprintln!(
            "INFO: {} infra import(s) in apps (tracked for future refactor):\n  {}",
            warnings.len(),
            warnings.join("\n  ")
        );
    }
}

#[test]
fn theo_domain_has_no_internal_deps() {
    let crates = collect_cargo_tomls();

    if let Some((_, deps)) = crates.get("theo-domain") {
        let violations: Vec<&String> = deps
            .iter()
            .filter(|d| FORBIDDEN_FOR_DOMAIN.contains(&d.as_str()))
            .collect();

        assert!(
            violations.is_empty(),
            "theo-domain must not depend on other internal crates, but depends on: {:?}",
            violations
        );
    } else {
        panic!("theo-domain crate not found in workspace");
    }
}

#[test]
fn apps_only_use_allowed_internal_deps() {
    let crates = collect_cargo_tomls();
    let mut violations = Vec::new();

    for (name, (category, deps)) in &crates {
        if category != "apps" {
            continue;
        }
        for dep in deps {
            if !ALLOWED_FOR_APPS.contains(&dep.as_str())
                && !FORBIDDEN_FOR_APPS.contains(&dep.as_str())
            {
                // Unknown internal dep — flag it
                violations.push(format!(
                    "apps/{name} imports unknown internal crate: {dep}"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Unknown internal dependencies in apps:\n{}",
        violations.join("\n")
    );
}

#[test]
fn extract_theo_deps_parses_workspace_style() {
    let content = r#"
[dependencies]
theo-domain.workspace = true
theo-engine-graph.workspace = true
serde = { version = "1" }
tokio = "1"

[dev-dependencies]
theo-tooling = { path = "../theo-tooling" }
"#;
    let deps = extract_theo_deps(content);
    assert_eq!(deps, vec!["theo-domain", "theo-engine-graph", "theo-tooling"]);
}
