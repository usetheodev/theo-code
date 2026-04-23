//! `SubAgentRegistry` — central lookup for all sub-agent specs.
//!
//! Loads from three sources with priority (project > global > built-in) and
//! supports runtime registration for on-demand agents.
//!
//! Uses `IndexMap` to preserve insertion order — guarantees deterministic
//! ordering in `build_tool_description()` regardless of insertion ordering.
//!
//! Track A — Phase 1 + Phase 2.

use std::fs;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;

use theo_domain::agent_spec::{AgentSpec, AgentSpecSource};

use super::approval::{
    compute_current_manifest, diff_unapproved, load_approved, ApprovalMode, ApprovedEntry,
};
use super::parser::{parse_agent_spec, ParseError};

/// Warning emitted when a custom agent overrides a builtin or fails to load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryWarning {
    pub kind: WarningKind,
    pub message: String,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningKind {
    /// A spec file failed to parse and was skipped.
    InvalidSpec,
    /// A custom spec overrode a builtin name (capabilities are intersected via S2).
    BuiltinOverride,
    /// A spec file was unreadable (I/O error).
    ReadError,
    /// A project spec was found but not in the approval manifest (S3 / G1).
    PendingApproval,
    /// A project spec dir was loaded with `TrustAll` mode (no approval check).
    TrustedProject,
}

/// Outcome of `load_all`. Reports both successes and pending approvals.
#[derive(Debug, Clone, Default)]
pub struct LoadOutcome {
    /// Specs successfully loaded into the registry.
    pub loaded: Vec<String>,
    /// Project specs that need user approval before they can be loaded.
    pub pending_approval: Vec<ApprovedEntry>,
    /// Diagnostic warnings (parse failures, builtin overrides, I/O errors).
    pub warnings: Vec<RegistryWarning>,
}

#[derive(Debug, Clone, Default)]
pub struct SubAgentRegistry {
    specs: IndexMap<String, AgentSpec>,
}

impl SubAgentRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pre-loaded with the 4 built-in agents (explorer, implementer, verifier, reviewer).
    pub fn with_builtins() -> Self {
        let mut reg = Self::new();
        for spec in super::builtins::all_builtins() {
            reg.specs.insert(spec.name.clone(), spec);
        }
        reg
    }

    /// Register an agent (used for on-demand or programmatic registration).
    pub fn register(&mut self, spec: AgentSpec) {
        self.specs.insert(spec.name.clone(), spec);
    }

    /// Lookup by name.
    pub fn get(&self, name: &str) -> Option<&AgentSpec> {
        self.specs.get(name)
    }

    /// True if a spec with this name exists.
    pub fn contains(&self, name: &str) -> bool {
        self.specs.contains_key(name)
    }

    /// Number of registered specs.
    pub fn len(&self) -> usize {
        self.specs.len()
    }

    /// True if no specs are registered.
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }

    /// Names of all registered agents (preserves insertion order).
    pub fn names(&self) -> Vec<&str> {
        self.specs.keys().map(|s| s.as_str()).collect()
    }

    /// Iterate over all specs in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = &AgentSpec> {
        self.specs.values()
    }

    /// Generate the description for the `delegate_task` tool schema.
    /// Deterministic: same registry → same output (IndexMap preserves order).
    pub fn build_tool_description(&self) -> String {
        let agents: Vec<String> = self
            .specs
            .values()
            .map(|s| format!("- {}: {}", s.name, s.description))
            .collect();
        format!(
            "Delegate work to a specialized sub-agent.\n\nAvailable agents:\n{}\n\n\
             Use any name not listed above to create an on-demand read-only agent.",
            agents.join("\n")
        )
    }

    /// Insert a spec, applying S2 intersection if it overrides a builtin.
    /// Returns a warning if the override happened.
    fn insert_with_s2(&mut self, mut spec: AgentSpec) -> Option<RegistryWarning> {
        let warning = if let Some(existing) = self.specs.get(&spec.name) {
            if existing.source == AgentSpecSource::Builtin && spec.source != AgentSpecSource::Builtin {
                // S2: intersect capabilities — never escalate
                spec.capability_set = existing.capability_set.intersect(&spec.capability_set);
                Some(RegistryWarning {
                    kind: WarningKind::BuiltinOverride,
                    message: format!(
                        "custom agent '{}' overrides a builtin; capabilities intersected (S2)",
                        spec.name
                    ),
                    path: None,
                })
            } else {
                None
            }
        } else {
            None
        };
        self.specs.insert(spec.name.clone(), spec);
        warning
    }

    /// Load custom agents from a directory (e.g. `.theo/agents/`).
    ///
    /// Specs with invalid frontmatter or I/O errors are skipped with warnings.
    /// Specs that override a builtin name have their capabilities intersected (S2).
    ///
    /// `source` controls the AgentSpecSource of loaded specs (Project or Global).
    pub fn load_from_dir(&mut self, dir: &Path, source: AgentSpecSource) -> Vec<RegistryWarning> {
        let mut warnings = Vec::new();
        if !dir.exists() {
            return warnings;
        }
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(err) => {
                warnings.push(RegistryWarning {
                    kind: WarningKind::ReadError,
                    message: format!("cannot read directory: {}", err),
                    path: Some(dir.to_path_buf()),
                });
                return warnings;
            }
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("md"))
            .collect();
        paths.sort(); // deterministic order

        for path in paths {
            let fallback_name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unnamed")
                .to_string();

            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(err) => {
                    warnings.push(RegistryWarning {
                        kind: WarningKind::ReadError,
                        message: format!("cannot read file: {}", err),
                        path: Some(path.clone()),
                    });
                    continue;
                }
            };
            match parse_agent_spec(&content, &fallback_name, source) {
                Ok(spec) => {
                    if let Some(w) = self.insert_with_s2(spec) {
                        warnings.push(RegistryWarning {
                            path: Some(path),
                            ..w
                        });
                    }
                }
                Err(err) => {
                    warnings.push(RegistryWarning {
                        kind: WarningKind::InvalidSpec,
                        message: format!("parse error: {}", err),
                        path: Some(path),
                    });
                }
            }
        }
        warnings
    }

    /// Convenience: load from a directory without applying S3 approval (for global agents).
    fn load_global(&mut self, global_dir: &Path) -> Vec<RegistryWarning> {
        self.load_from_dir(global_dir, AgentSpecSource::Global)
    }

    /// Load all sources (built-in must be pre-loaded; global; project with S3).
    ///
    /// Resolution order: built-in (lowest) → global → project (highest).
    /// Project overrides global which overrides built-in. For builtins that
    /// are overridden, capabilities are intersected via S2.
    ///
    /// Project specs respect S3 approval policy:
    /// - `Interactive`: pending specs are NOT loaded; reported in `pending_approval`
    /// - `NonInteractive`: pending specs are silently skipped (CI mode)
    /// - `TrustAll`: all specs loaded without checking approval (CLI flag override)
    pub fn load_all(
        &mut self,
        project_dir: Option<&Path>,
        global_dir: Option<&Path>,
        mode: ApprovalMode,
    ) -> LoadOutcome {
        let mut outcome = LoadOutcome::default();

        // 1. Global (no approval needed; user controls their own home)
        if let Some(global) = global_dir {
            let warnings = self.load_global(global);
            outcome.warnings.extend(warnings);
        }

        // 2. Project (S3 approval gating)
        if let Some(project) = project_dir {
            let agents_dir = project.join(".theo").join("agents");
            if !agents_dir.exists() {
                return outcome;
            }

            let current = match compute_current_manifest(&agents_dir) {
                Ok(c) => c,
                Err(err) => {
                    outcome.warnings.push(RegistryWarning {
                        kind: WarningKind::ReadError,
                        message: format!("cannot scan project agents dir: {}", err),
                        path: Some(agents_dir.clone()),
                    });
                    return outcome;
                }
            };

            let approved = match load_approved(project) {
                Ok(a) => a,
                Err(err) => {
                    outcome.warnings.push(RegistryWarning {
                        kind: WarningKind::ReadError,
                        message: format!("cannot read approval manifest: {}", err),
                        path: None,
                    });
                    return outcome;
                }
            };

            let pending = diff_unapproved(&current, &approved);

            // Determine which files are "approved-as-current": present in the
            // current manifest AND their SHA matches the approved manifest.
            let approved_current_files: std::collections::HashSet<String> = current
                .iter()
                .filter(|c| approved.is_approved(&c.file, &c.sha256))
                .map(|c| c.file.clone())
                .collect();

            match mode {
                ApprovalMode::Interactive => {
                    // Pending specs are NOT loaded. Caller must prompt + persist + retry.
                    if !pending.is_empty() {
                        outcome.pending_approval = pending.clone();
                    }
                    self.load_filtered(&agents_dir, &approved_current_files, &mut outcome);
                }
                ApprovalMode::NonInteractive => {
                    if !pending.is_empty() {
                        for p in &pending {
                            outcome.warnings.push(RegistryWarning {
                                kind: WarningKind::PendingApproval,
                                message: format!(
                                    "project agent '{}' not approved; skipped (CI mode)",
                                    p.file
                                ),
                                path: Some(agents_dir.join(&p.file)),
                            });
                        }
                    }
                    self.load_filtered(&agents_dir, &approved_current_files, &mut outcome);
                }
                ApprovalMode::TrustAll => {
                    if !current.is_empty() {
                        outcome.warnings.push(RegistryWarning {
                            kind: WarningKind::TrustedProject,
                            message: format!(
                                "loading {} project agent(s) without approval (TrustAll mode)",
                                current.len()
                            ),
                            path: Some(agents_dir.clone()),
                        });
                    }
                    let warnings = self.load_from_dir(&agents_dir, AgentSpecSource::Project);
                    outcome.warnings.extend(warnings);
                }
            }
        }

        // Track which specs were actually loaded
        outcome.loaded = self.names().iter().map(|s| s.to_string()).collect();
        outcome
    }

    fn load_filtered(
        &mut self,
        agents_dir: &Path,
        approved_files: &std::collections::HashSet<String>,
        outcome: &mut LoadOutcome,
    ) {
        let entries = match fs::read_dir(agents_dir) {
            Ok(e) => e,
            Err(err) => {
                outcome.warnings.push(RegistryWarning {
                    kind: WarningKind::ReadError,
                    message: format!("{}", err),
                    path: Some(agents_dir.to_path_buf()),
                });
                return;
            }
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension().and_then(|s| s.to_str()) == Some("md")
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| approved_files.contains(n))
                        .unwrap_or(false)
            })
            // ^ NOTE: approved_files here contains ONLY files whose current
            // SHA matches the approved manifest; mismatched SHAs are pending.
            .collect();
        paths.sort();

        for path in paths {
            let fallback_name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unnamed")
                .to_string();
            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(err) => {
                    outcome.warnings.push(RegistryWarning {
                        kind: WarningKind::ReadError,
                        message: format!("{}", err),
                        path: Some(path.clone()),
                    });
                    continue;
                }
            };
            match parse_agent_spec(&content, &fallback_name, AgentSpecSource::Project) {
                Ok(spec) => {
                    if let Some(w) = self.insert_with_s2(spec) {
                        outcome.warnings.push(RegistryWarning {
                            path: Some(path),
                            ..w
                        });
                    }
                }
                Err(err) => {
                    outcome.warnings.push(RegistryWarning {
                        kind: WarningKind::InvalidSpec,
                        message: format!("{}", err),
                        path: Some(path),
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn registry_new_is_empty() {
        let reg = SubAgentRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn registry_with_builtins_has_4_agents() {
        let reg = SubAgentRegistry::with_builtins();
        assert_eq!(reg.len(), 4);
    }

    #[test]
    fn registry_get_returns_some_for_existing() {
        let reg = SubAgentRegistry::with_builtins();
        assert!(reg.get("explorer").is_some());
        assert!(reg.get("implementer").is_some());
        assert!(reg.get("verifier").is_some());
        assert!(reg.get("reviewer").is_some());
    }

    #[test]
    fn registry_get_returns_none_for_missing() {
        let reg = SubAgentRegistry::with_builtins();
        assert!(reg.get("non-existent").is_none());
    }

    #[test]
    fn registry_register_adds_agent() {
        let mut reg = SubAgentRegistry::new();
        let spec = AgentSpec::on_demand("custom", "do x");
        reg.register(spec.clone());
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.get("custom").unwrap().name, "custom");
    }

    #[test]
    fn registry_register_replaces_same_name() {
        let mut reg = SubAgentRegistry::with_builtins();
        let mut spec = AgentSpec::on_demand("explorer", "redefined");
        spec.description = "redefined desc".to_string();
        reg.register(spec);
        assert_eq!(reg.len(), 4);
        assert_eq!(reg.get("explorer").unwrap().description, "redefined desc");
    }

    #[test]
    fn registry_names_preserves_insertion_order() {
        let reg = SubAgentRegistry::with_builtins();
        assert_eq!(
            reg.names(),
            vec!["explorer", "implementer", "verifier", "reviewer"]
        );
    }

    #[test]
    fn registry_contains_returns_correct_bool() {
        let reg = SubAgentRegistry::with_builtins();
        assert!(reg.contains("explorer"));
        assert!(!reg.contains("missing"));
    }

    #[test]
    fn registry_build_tool_description_is_deterministic() {
        let reg1 = SubAgentRegistry::with_builtins();
        let reg2 = SubAgentRegistry::with_builtins();
        assert_eq!(reg1.build_tool_description(), reg2.build_tool_description());
    }

    #[test]
    fn registry_build_tool_description_lists_all_agents() {
        let reg = SubAgentRegistry::with_builtins();
        let desc = reg.build_tool_description();
        assert!(desc.contains("explorer"));
        assert!(desc.contains("implementer"));
        assert!(desc.contains("verifier"));
        assert!(desc.contains("reviewer"));
        assert!(desc.contains("on-demand"));
    }

    #[test]
    fn registry_iter_returns_specs_in_order() {
        let reg = SubAgentRegistry::with_builtins();
        let names: Vec<&str> = reg.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["explorer", "implementer", "verifier", "reviewer"]);
    }

    // ── Phase 2: load_from_dir ─────────────────────────────────────────

    fn make_dir_with_files(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (name, content) in files {
            fs::write(dir.path().join(name), content).unwrap();
        }
        dir
    }

    #[test]
    fn load_from_dir_finds_md_files() {
        let dir = make_dir_with_files(&[
            (
                "custom.md",
                "---\ndescription: a custom agent\n---\nbody",
            ),
            ("readme.txt", "ignored"),
        ]);
        let mut reg = SubAgentRegistry::new();
        let warnings = reg.load_from_dir(dir.path(), AgentSpecSource::Project);
        assert_eq!(warnings.len(), 0);
        assert!(reg.contains("custom"));
    }

    #[test]
    fn load_from_dir_skips_invalid_with_warning() {
        let dir = make_dir_with_files(&[
            ("good.md", "---\ndescription: good\n---\nbody"),
            ("bad.md", "no frontmatter at all"),
        ]);
        let mut reg = SubAgentRegistry::new();
        let warnings = reg.load_from_dir(dir.path(), AgentSpecSource::Project);
        assert!(reg.contains("good"));
        assert!(!reg.contains("bad"));
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].kind, WarningKind::InvalidSpec);
    }

    #[test]
    fn load_from_dir_nonexistent_dir_returns_empty_warnings() {
        let mut reg = SubAgentRegistry::new();
        let warnings =
            reg.load_from_dir(Path::new("/nonexistent/dir"), AgentSpecSource::Project);
        assert!(warnings.is_empty());
    }

    #[test]
    fn load_from_dir_builtin_override_intersects_capabilities() {
        // A custom "explorer" tries to GRANT bash (which builtin denies)
        let dir = make_dir_with_files(&[(
            "explorer.md",
            "---\ndescription: rogue explorer with bash\nnetwork_access: true\n---\nbody",
        )]);
        let mut reg = SubAgentRegistry::with_builtins();
        // builtin explorer denies bash + no network
        let warnings = reg.load_from_dir(dir.path(), AgentSpecSource::Project);

        // Override warning emitted
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].kind, WarningKind::BuiltinOverride);

        // S2 invariant: capabilities NEVER escalate
        let after = reg.get("explorer").unwrap();
        assert!(
            !after
                .capability_set
                .can_use_tool("bash", theo_domain::tool::ToolCategory::Execution),
            "S2 violated: builtin explorer denies bash but custom granted it"
        );
        assert!(
            !after.capability_set.network_access,
            "S2 violated: builtin denies network but custom enabled it"
        );
    }

    #[test]
    fn load_all_resolution_order_project_overrides_global() {
        let global = TempDir::new().unwrap();
        fs::write(
            global.path().join("foo.md"),
            "---\nname: foo\ndescription: from-global\n---\nbody",
        )
        .unwrap();

        let project = TempDir::new().unwrap();
        let agents = project.path().join(".theo").join("agents");
        fs::create_dir_all(&agents).unwrap();
        fs::write(
            agents.join("foo.md"),
            "---\nname: foo\ndescription: from-project\n---\nbody",
        )
        .unwrap();

        let mut reg = SubAgentRegistry::new();
        // Use TrustAll to skip approval for this test (test focuses on resolution order)
        let outcome = reg.load_all(
            Some(project.path()),
            Some(global.path()),
            ApprovalMode::TrustAll,
        );

        // Project wins over global
        assert_eq!(reg.get("foo").unwrap().description, "from-project");
        assert_eq!(reg.get("foo").unwrap().source, AgentSpecSource::Project);
        // TrustAll warning
        assert!(outcome
            .warnings
            .iter()
            .any(|w| matches!(w.kind, WarningKind::TrustedProject)));
    }

    #[test]
    fn load_all_interactive_pending_specs_not_loaded() {
        let project = TempDir::new().unwrap();
        let agents = project.path().join(".theo").join("agents");
        fs::create_dir_all(&agents).unwrap();
        fs::write(
            agents.join("custom.md"),
            "---\ndescription: pending\n---\nbody",
        )
        .unwrap();

        let mut reg = SubAgentRegistry::new();
        let outcome = reg.load_all(Some(project.path()), None, ApprovalMode::Interactive);

        assert!(!reg.contains("custom"), "unapproved spec should NOT load");
        assert_eq!(outcome.pending_approval.len(), 1);
        assert_eq!(outcome.pending_approval[0].file, "custom.md");
    }

    #[test]
    fn load_all_interactive_approved_specs_load() {
        use super::super::approval::{persist_approved, sha256_hex, ApprovalManifest};

        let project = TempDir::new().unwrap();
        let agents = project.path().join(".theo").join("agents");
        fs::create_dir_all(&agents).unwrap();
        let content = "---\ndescription: good\n---\nbody";
        fs::write(agents.join("custom.md"), content).unwrap();

        let manifest = ApprovalManifest {
            approved: vec![ApprovedEntry {
                file: "custom.md".into(),
                sha256: sha256_hex(content),
            }],
        };
        persist_approved(project.path(), &manifest).unwrap();

        let mut reg = SubAgentRegistry::new();
        let outcome = reg.load_all(Some(project.path()), None, ApprovalMode::Interactive);

        assert!(reg.contains("custom"), "approved spec should load");
        assert!(outcome.pending_approval.is_empty());
    }

    #[test]
    fn load_all_non_interactive_skips_unapproved_with_warning() {
        let project = TempDir::new().unwrap();
        let agents = project.path().join(".theo").join("agents");
        fs::create_dir_all(&agents).unwrap();
        fs::write(
            agents.join("ci-blocked.md"),
            "---\ndescription: pending\n---\nbody",
        )
        .unwrap();

        let mut reg = SubAgentRegistry::new();
        let outcome = reg.load_all(Some(project.path()), None, ApprovalMode::NonInteractive);

        assert!(!reg.contains("ci-blocked"));
        assert!(outcome
            .warnings
            .iter()
            .any(|w| matches!(w.kind, WarningKind::PendingApproval)));
    }

    #[test]
    fn load_all_trustall_loads_unapproved() {
        let project = TempDir::new().unwrap();
        let agents = project.path().join(".theo").join("agents");
        fs::create_dir_all(&agents).unwrap();
        fs::write(
            agents.join("untrusted.md"),
            "---\ndescription: ignored-trust\n---\nbody",
        )
        .unwrap();

        let mut reg = SubAgentRegistry::new();
        let outcome = reg.load_all(Some(project.path()), None, ApprovalMode::TrustAll);

        assert!(reg.contains("untrusted"));
        assert!(outcome
            .warnings
            .iter()
            .any(|w| matches!(w.kind, WarningKind::TrustedProject)));
    }

    #[test]
    fn load_all_modified_spec_invalidates_approval() {
        use super::super::approval::{persist_approved, sha256_hex, ApprovalManifest};

        let project = TempDir::new().unwrap();
        let agents = project.path().join(".theo").join("agents");
        fs::create_dir_all(&agents).unwrap();
        let original = "---\ndescription: original\n---\nbody";
        fs::write(agents.join("x.md"), original).unwrap();

        // Approve original
        let manifest = ApprovalManifest {
            approved: vec![ApprovedEntry {
                file: "x.md".into(),
                sha256: sha256_hex(original),
            }],
        };
        persist_approved(project.path(), &manifest).unwrap();

        // Modify spec
        fs::write(
            agents.join("x.md"),
            "---\ndescription: MODIFIED\n---\nnew body",
        )
        .unwrap();

        // Re-load: should be pending again
        let mut reg = SubAgentRegistry::new();
        let outcome = reg.load_all(Some(project.path()), None, ApprovalMode::Interactive);
        assert!(!reg.contains("x"));
        assert_eq!(outcome.pending_approval.len(), 1);
    }
}
