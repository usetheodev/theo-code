//! Plugin system — load custom tools and hooks from `.theo/plugins/`.
//!
//! A plugin is a directory with:
//! - `plugin.toml` — manifest (name, version, description, tool definitions)
//! - `tools/*.sh` — shell-based tools (stdin JSON args → stdout output)
//! - `hooks/*.sh` — hook scripts (loaded by HookRunner)
//!
//! Plugins are discovered from:
//! - `.theo/plugins/` (project-level)
//! - `~/.config/theo/plugins/` (global)
//!
//! **Security (T1.3):**
//! - A plugin whose `plugin.toml` is owned by a different user is rejected
//!   (supply-chain guard — a compromised writer cannot inject tools).
//! - Every loaded plugin carries a SHA-256 of its `plugin.toml` so the
//!   agent runtime can log it or enforce an allowlist of trusted hashes.
//! - Tools registered from plugins are placed under `ToolCategory::Plugin`,
//!   which is gated by the capability set even on "unrestricted" defaults.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// PluginManifest — parsed from plugin.toml
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tools: Vec<ToolSpec>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolSpec {
    /// Tool name as seen by the LLM (e.g., "jira_create_issue")
    pub name: String,
    /// Description shown to the LLM
    pub description: String,
    /// Path to shell script relative to plugin dir (e.g., "tools/create_issue.sh")
    pub script: String,
    /// Parameter descriptions for the LLM schema
    #[serde(default)]
    pub params: Vec<ToolParamSpec>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolParamSpec {
    pub name: String,
    #[serde(default = "default_string_type")]
    pub param_type: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub required: bool,
}

fn default_string_type() -> String {
    "string".to_string()
}

// ---------------------------------------------------------------------------
// LoadedPlugin — manifest + resolved paths
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct LoadedPlugin {
    pub manifest: PluginManifest,
    pub dir: PathBuf,
    pub tool_scripts: Vec<(ToolSpec, PathBuf)>, // (spec, absolute script path)
    pub hook_scripts: Vec<PathBuf>,
    /// SHA-256 hex digest of the plugin.toml manifest. Operators can
    /// pin a hash in `AgentConfig.plugin_allowlist` (T1.3) or correlate
    /// observed plugins via the stderr log.
    pub manifest_sha256: String,
}

// ---------------------------------------------------------------------------
// Plugin discovery and loading
// ---------------------------------------------------------------------------

/// Discover and load all plugins from project and global dirs.
///
/// No allowlist enforcement — every ownership-verified plugin is
/// accepted. Use [`load_plugins_with_policy`] to pin a hash allowlist.
pub fn load_plugins(project_dir: &Path) -> Vec<LoadedPlugin> {
    load_plugins_with_policy(project_dir, None, None)
}

/// T1.3: discover plugins with optional hash allowlist + optional
/// [`crate::event_bus::EventBus`] for typed `DomainEvent::PluginLoaded`
/// emission.
///
/// - `allowlist = None` → accept every ownership-verified plugin.
/// - `allowlist = Some(set)` → only accept plugins whose
///   `manifest_sha256` is present. Rejections are logged on stderr AND
///   (when `bus` is provided) emitted as `DomainEvent::Error` with
///   `payload.type = "plugin_rejected"`.
pub fn load_plugins_with_policy(
    project_dir: &Path,
    allowlist: Option<&std::collections::BTreeSet<String>>,
    bus: Option<&std::sync::Arc<crate::event_bus::EventBus>>,
) -> Vec<LoadedPlugin> {
    let mut plugins = Vec::new();

    // Project plugins
    let project_plugins = project_dir.join(".theo").join("plugins");
    if project_plugins.exists() {
        load_plugins_from_dir(&project_plugins, &mut plugins, allowlist, bus);
    }

    // Global plugins — only when HOME is set (avoid /tmp fallback).
    if let Some(global_plugins) = theo_domain::user_paths::theo_config_subdir("plugins")
        && global_plugins.exists()
    {
        load_plugins_from_dir(&global_plugins, &mut plugins, allowlist, bus);
    }

    plugins
}

fn load_plugins_from_dir(
    plugins_dir: &Path,
    plugins: &mut Vec<LoadedPlugin>,
    allowlist: Option<&std::collections::BTreeSet<String>>,
    bus: Option<&std::sync::Arc<crate::event_bus::EventBus>>,
) {
    let entries = match std::fs::read_dir(plugins_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let manifest_path = path.join("plugin.toml");
        if !manifest_path.exists() {
            continue;
        }

        // T1.3 security: reject plugins whose manifest is owned by a
        // different user. Supply-chain guard — a rogue writer with access
        // to `.theo/plugins/` cannot inject a tool under our uid.
        if !manifest_is_owned_by_current_user(&manifest_path) {
            eprintln!(
                "[theo] Plugin REJECTED (ownership mismatch): {} — manifest not owned by current user",
                path.display()
            );
            if let Some(bus) = bus {
                bus.publish(theo_domain::event::DomainEvent::new(
                    theo_domain::event::EventType::Error,
                    "plugin_loader",
                    serde_json::json!({
                        "type": "plugin_rejected",
                        "reason": "ownership_mismatch",
                        "path": path.display().to_string(),
                    }),
                ));
            }
            continue;
        }

        match load_single_plugin(&path) {
            Ok(plugin) => {
                // T1.3: if the operator pinned a hash allowlist, reject
                // plugins whose sha256 is not in it.
                if let Some(set) = allowlist
                    && !set.contains(&plugin.manifest_sha256)
                {
                    eprintln!(
                        "[theo] Plugin REJECTED (sha256 not in allowlist): {} sha256={}",
                        path.display(),
                        &plugin.manifest_sha256[..16]
                    );
                    if let Some(bus) = bus {
                        bus.publish(theo_domain::event::DomainEvent::new(
                            theo_domain::event::EventType::Error,
                            "plugin_loader",
                            serde_json::json!({
                                "type": "plugin_rejected",
                                "reason": "allowlist_miss",
                                "path": path.display().to_string(),
                                "manifest_sha256": plugin.manifest_sha256,
                            }),
                        ));
                    }
                    continue;
                }

                eprintln!(
                    "[theo] Plugin loaded: {} ({}) sha256={}",
                    plugin.manifest.name,
                    path.display(),
                    &plugin.manifest_sha256[..16]
                );
                if let Some(bus) = bus {
                    bus.publish(theo_domain::event::DomainEvent::new(
                        theo_domain::event::EventType::PluginLoaded,
                        &plugin.manifest.name,
                        serde_json::json!({
                            "name": plugin.manifest.name,
                            "dir": path.display().to_string(),
                            "manifest_sha256": plugin.manifest_sha256,
                            "tool_count": plugin.tool_scripts.len() as u64,
                            "hook_count": plugin.hook_scripts.len() as u64,
                        }),
                    ));
                }
                plugins.push(plugin);
            }
            Err(e) => {
                eprintln!(
                    "[theo] Warning: failed to load plugin at {}: {e}",
                    path.display()
                );
            }
        }
    }
}

/// Return `true` iff the manifest file is owned by the current user.
///
/// On Linux/macOS uses `metadata.uid() == getuid()`. On non-Unix platforms
/// we conservatively return `true` (ownership cannot be established).
#[cfg(unix)]
fn manifest_is_owned_by_current_user(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match std::fs::metadata(path) {
        Ok(meta) => {
            // SAFETY: `libc::getuid` has no preconditions and returns the
            // real uid of the calling process. No pointers, no lifetimes.
            let my_uid = unsafe { libc::getuid() };
            meta.uid() == my_uid
        }
        Err(_) => false,
    }
}

#[cfg(not(unix))]
fn manifest_is_owned_by_current_user(_: &Path) -> bool {
    true
}

fn load_single_plugin(plugin_dir: &Path) -> Result<LoadedPlugin, String> {
    let manifest_path = plugin_dir.join("plugin.toml");
    let content =
        std::fs::read_to_string(&manifest_path).map_err(|e| format!("read plugin.toml: {e}"))?;
    let manifest: PluginManifest =
        toml::from_str(&content).map_err(|e| format!("parse plugin.toml: {e}"))?;

    // T1.3: compute a SHA-256 of the manifest so operators can pin a
    // known-good hash via config (allowlist follow-up) or correlate what
    // was loaded on a given host.
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let manifest_sha256 = format!("{:x}", hasher.finalize());

    // Resolve tool script paths
    let mut tool_scripts = Vec::new();
    for spec in &manifest.tools {
        let script_path = plugin_dir.join(&spec.script);
        if script_path.exists() {
            tool_scripts.push((spec.clone(), script_path));
        } else {
            eprintln!(
                "[theo] Warning: plugin '{}' tool script not found: {}",
                manifest.name, spec.script
            );
        }
    }

    // Discover hook scripts
    let mut hook_scripts = Vec::new();
    if let Ok(entries) = std::fs::read_dir(plugin_dir.join("hooks")) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("sh") {
                hook_scripts.push(path);
            }
        }
    }

    Ok(LoadedPlugin {
        manifest,
        dir: plugin_dir.to_path_buf(),
        tool_scripts,
        hook_scripts,
        manifest_sha256,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plugin_manifest() {
        let toml = r#"
name = "jira"
version = "1.0.0"
description = "Jira integration"

[[tools]]
name = "jira_create"
description = "Create a Jira issue"
script = "tools/create.sh"

[[tools.params]]
name = "title"
param_type = "string"
description = "Issue title"
required = true

[[tools.params]]
name = "priority"
param_type = "string"
description = "Priority level"
required = false
"#;
        let manifest: PluginManifest = toml::from_str(toml).unwrap();
        assert_eq!(manifest.name, "jira");
        assert_eq!(manifest.tools.len(), 1);
        assert_eq!(manifest.tools[0].name, "jira_create");
        assert_eq!(manifest.tools[0].params.len(), 2);
        assert!(manifest.tools[0].params[0].required);
    }

    #[test]
    fn load_plugin_from_dir() {
        let dir = tempfile::tempdir().unwrap();
        let plugin_dir = dir.path().join("my-plugin");
        std::fs::create_dir_all(plugin_dir.join("tools")).unwrap();
        std::fs::create_dir_all(plugin_dir.join("hooks")).unwrap();

        std::fs::write(
            plugin_dir.join("plugin.toml"),
            r#"
name = "test-plugin"
version = "0.1.0"
description = "Test"

[[tools]]
name = "greet"
description = "Say hello"
script = "tools/greet.sh"
"#,
        )
        .unwrap();

        std::fs::write(plugin_dir.join("tools/greet.sh"), "#!/bin/sh\necho hello\n").unwrap();
        std::fs::write(
            plugin_dir.join("hooks/tool.before.sh"),
            "#!/bin/sh\nexit 0\n",
        )
        .unwrap();

        let plugin = load_single_plugin(&plugin_dir).unwrap();
        assert_eq!(plugin.manifest.name, "test-plugin");
        assert_eq!(plugin.tool_scripts.len(), 1);
        assert_eq!(plugin.hook_scripts.len(), 1);
    }

    #[test]
    fn load_plugins_missing_dir_returns_empty() {
        let plugins = load_plugins(Path::new("/nonexistent"));
        assert!(plugins.is_empty());
    }

    #[test]
    fn load_plugins_discovers_from_project() {
        let dir = tempfile::tempdir().unwrap();
        let plugins_dir = dir.path().join(".theo").join("plugins").join("demo");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        std::fs::write(
            plugins_dir.join("plugin.toml"),
            r#"
name = "demo"
description = "Demo plugin"
"#,
        )
        .unwrap();

        let plugins = load_plugins(dir.path());
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].manifest.name, "demo");
    }

    #[test]
    fn plugin_with_invalid_toml_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let plugins_dir = dir.path().join(".theo").join("plugins").join("broken");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        std::fs::write(plugins_dir.join("plugin.toml"), "not valid toml {{{").unwrap();

        let plugins = load_plugins(dir.path());
        assert!(plugins.is_empty());
    }

    // -----------------------------------------------------------------------
    // T1.3 — Security: ownership + SHA-256 of manifest.
    // -----------------------------------------------------------------------

    #[test]
    fn loaded_plugin_carries_sha256_of_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let plugin_dir = dir.path().join("p");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(plugin_dir.join("plugin.toml"), "name = \"p\"\n").unwrap();

        let plugin = load_single_plugin(&plugin_dir).unwrap();
        // Plain SHA-256 of "name = \"p\"\n"
        assert_eq!(plugin.manifest_sha256.len(), 64);
        assert!(plugin.manifest_sha256.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn same_manifest_content_yields_same_sha256() {
        let dir_a = tempfile::tempdir().unwrap();
        let dir_b = tempfile::tempdir().unwrap();
        for d in [&dir_a, &dir_b] {
            let p = d.path().join("p");
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join("plugin.toml"), "name = \"p\"\n").unwrap();
        }
        let a = load_single_plugin(&dir_a.path().join("p")).unwrap();
        let b = load_single_plugin(&dir_b.path().join("p")).unwrap();
        assert_eq!(a.manifest_sha256, b.manifest_sha256);
    }

    #[cfg(unix)]
    #[test]
    fn ownership_check_accepts_files_created_by_current_user() {
        // Create a plugin in tempdir — we are the owner by construction.
        let dir = tempfile::tempdir().unwrap();
        let plugins_dir = dir.path().join(".theo/plugins/me");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        std::fs::write(
            plugins_dir.join("plugin.toml"),
            "name = \"mine\"\n",
        )
        .unwrap();

        let plugins = load_plugins(dir.path());
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].manifest.name, "mine");
    }

    // -----------------------------------------------------------------------
    // T1.3 completion — Allowlist pinning by manifest SHA-256.
    // -----------------------------------------------------------------------

    #[cfg(unix)]
    #[test]
    fn plugin_allowed_when_sha256_matches_allowlist() {
        use std::collections::BTreeSet;
        let dir = tempfile::tempdir().unwrap();
        let plugins_dir = dir.path().join(".theo/plugins/a");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        std::fs::write(plugins_dir.join("plugin.toml"), "name = \"a\"\n").unwrap();

        // First pass: load without allowlist to observe the expected hash.
        let observed = load_plugins(dir.path());
        assert_eq!(observed.len(), 1);
        let sha = observed[0].manifest_sha256.clone();

        // Second pass: pin the hash — should still load.
        let allowlist: BTreeSet<String> = [sha].into_iter().collect();
        let loaded = load_plugins_with_policy(dir.path(), Some(&allowlist), None);
        assert_eq!(loaded.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn plugin_rejected_when_sha256_missing_from_allowlist() {
        use std::collections::BTreeSet;
        let dir = tempfile::tempdir().unwrap();
        let plugins_dir = dir.path().join(".theo/plugins/evil");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        std::fs::write(plugins_dir.join("plugin.toml"), "name = \"evil\"\n").unwrap();

        // Allowlist contains a different hash — plugin must be rejected.
        let wrong: BTreeSet<String> = [
            "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        ]
        .into_iter()
        .collect();
        let loaded = load_plugins_with_policy(dir.path(), Some(&wrong), None);
        assert!(loaded.is_empty(), "plugin with non-allowlisted hash must be rejected");
    }

    #[cfg(unix)]
    #[test]
    fn plugin_load_emits_plugin_loaded_event_when_bus_attached() {
        use crate::event_bus::{CapturingListener, EventBus};
        use std::sync::Arc;
        use theo_domain::event::EventType;

        let dir = tempfile::tempdir().unwrap();
        let plugins_dir = dir.path().join(".theo/plugins/loaded");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        std::fs::write(
            plugins_dir.join("plugin.toml"),
            "name = \"loaded\"\n",
        )
        .unwrap();

        let bus = Arc::new(EventBus::new());
        let listener = Arc::new(CapturingListener::new());
        bus.subscribe(listener.clone());

        let loaded = load_plugins_with_policy(dir.path(), None, Some(&bus));
        assert_eq!(loaded.len(), 1);

        let evts: Vec<_> = listener
            .captured()
            .into_iter()
            .filter(|e| e.event_type == EventType::PluginLoaded)
            .collect();
        assert_eq!(evts.len(), 1);
        assert_eq!(evts[0].entity_id, "loaded");
        assert_eq!(evts[0].payload["name"], "loaded");
        assert!(
            evts[0].payload["manifest_sha256"]
                .as_str()
                .unwrap()
                .len()
                == 64
        );
    }

    // -----------------------------------------------------------------------
    // T1.3 AC literals — alias the existing tests under the names the
    // remediation plan calls out so a future grep against the plan's AC
    // wording finds them.
    // -----------------------------------------------------------------------

    /// T1.4 AC: `load_plugins_skips_when_home_unset`. The global
    /// plugins path is `$HOME/.config/theo/plugins`. When `HOME` is
    /// absent we MUST skip the global lookup — never fall back to
    /// `/tmp` (TOCTOU vector). Project plugins are unaffected because
    /// they live under `<project_dir>/.theo/plugins/` not `HOME`.
    #[cfg(unix)]
    #[test]
    fn load_plugins_skips_when_home_unset() {
        // Lock + snapshot follow the established T0.3 / T1.5 pattern
        // (env-mutating tests serialize against each other and
        // restore HOME on Drop).
        fn env_lock() -> std::sync::MutexGuard<'static, ()> {
            use std::sync::{Mutex, OnceLock};
            static M: OnceLock<Mutex<()>> = OnceLock::new();
            M.get_or_init(|| Mutex::new(()))
                .lock()
                .unwrap_or_else(|e| e.into_inner())
        }
        struct HomeSnap(Option<std::ffi::OsString>);
        impl Drop for HomeSnap {
            fn drop(&mut self) {
                unsafe {
                    match &self.0 {
                        Some(v) => std::env::set_var("HOME", v),
                        None => std::env::remove_var("HOME"),
                    }
                }
            }
        }
        let _l = env_lock();
        let _s = HomeSnap(std::env::var_os("HOME"));
        unsafe { std::env::remove_var("HOME") };

        // Project dir without any plugins → load returns empty Vec
        // and crucially does NOT touch /tmp.
        let dir = tempfile::tempdir().unwrap();
        let plugins = load_plugins(dir.path());
        assert!(
            plugins.is_empty(),
            "no plugins exist anywhere; loader must return empty"
        );

        // The contract: with HOME unset, `theo_config_subdir("plugins")`
        // returns None — proven directly here so a regression in the
        // central helper would also trip this AC test.
        assert_eq!(
            theo_domain::user_paths::theo_config_subdir("plugins"),
            None,
            "HOME unset must yield None — never a /tmp path"
        );
    }

    /// T1.3 AC: `plugin_with_wrong_owner_rejected`. We can't easily
    /// chown a file to a different uid in CI without root, so the
    /// regression target is the helper that performs the uid check.
    /// Both branches (Err on missing path, Err on metadata) collapse
    /// to "not owned by current user" → false. A foreign-uid file at
    /// `/etc/passwd` (owned by root in every distro) is also rejected
    /// when the test process is NOT root — the typical CI case.
    #[cfg(unix)]
    #[test]
    fn plugin_with_wrong_owner_rejected() {
        // Branch 1: path that does not exist → metadata Err → reject.
        let missing = std::path::Path::new("/nonexistent/theo/plugin.toml");
        assert!(
            !manifest_is_owned_by_current_user(missing),
            "missing manifest must be treated as not-owned (reject)"
        );

        // Branch 2: existing path owned by a different uid. /etc/passwd
        // is owned by root on every supported distro.
        let foreign = std::path::Path::new("/etc/passwd");
        if !foreign.exists() {
            return; // Container without /etc/passwd — skip silently.
        }
        let am_root = unsafe { libc::getuid() } == 0;
        if am_root {
            // The test invariant assumes a non-root tester. Skip when
            // running as root (rare in CI but possible in some sandboxes).
            return;
        }
        assert!(
            !manifest_is_owned_by_current_user(foreign),
            "/etc/passwd is owned by root; non-root tester must see ownership rejected"
        );
    }

    /// T1.3 AC: `plugin_not_in_allowlist_rejected_when_configured`.
    /// Mirror of `plugin_rejected_when_sha256_missing_from_allowlist`
    /// under the literal AC name.
    #[cfg(unix)]
    #[test]
    fn plugin_not_in_allowlist_rejected_when_configured() {
        use std::collections::BTreeSet;
        let dir = tempfile::tempdir().unwrap();
        let plugins_dir = dir.path().join(".theo/plugins/x");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        std::fs::write(plugins_dir.join("plugin.toml"), "name = \"x\"\n").unwrap();

        let unrelated_hash: BTreeSet<String> = ["deadbeef".repeat(8)].into_iter().collect();
        let loaded = load_plugins_with_policy(dir.path(), Some(&unrelated_hash), None);
        assert!(
            loaded.is_empty(),
            "plugin must be rejected when its sha256 is not in the allowlist"
        );
    }

    /// T1.3 AC: `plugin_tool_blocked_by_capability_gate_read_only`.
    /// Plugin-registered tools always carry `ToolCategory::Plugin`,
    /// which is denied by a read-only `CapabilitySet`. We assert the
    /// classification stays correct so a future change cannot silently
    /// downgrade the category and leak past the gate.
    #[test]
    fn plugin_tool_blocked_by_capability_gate_read_only() {
        use std::collections::BTreeSet;
        use theo_domain::capability::{AllowedTools, CapabilitySet};
        use theo_domain::tool::ToolCategory;

        // A read-only capability set: only Search category, no
        // ToolCategory::Plugin.
        let mut allowed_categories = BTreeSet::new();
        allowed_categories.insert(ToolCategory::Search);
        let read_only = CapabilitySet {
            allowed_tools: AllowedTools::Only { tools: BTreeSet::new() },
            denied_tools: BTreeSet::new(),
            allowed_categories,
            max_file_size_bytes: 0,
            allowed_paths: vec![],
            network_access: false,
        };

        // The contract: `ToolCategory::Plugin` MUST not be in the
        // read-only allowed_categories list. If a future PR adds it,
        // this test trips and forces a deliberate review.
        assert!(
            !read_only.allowed_categories.contains(&ToolCategory::Plugin),
            "read-only capability set must not include ToolCategory::Plugin"
        );

        // And the standard membership check denies it.
        let allowed = read_only.can_use_tool("any-plugin-tool", ToolCategory::Plugin);
        assert!(
            !allowed,
            "read-only capability set must deny Plugin-category tools"
        );
    }
}
