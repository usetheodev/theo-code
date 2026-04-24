//! ReloadableRegistry — Arc<RwLock<SubAgentRegistry>> com helper para
//! re-build a partir de project/global dirs.
//!
//! Usado pelo CLI quando `--watch-agents` está ativo: a cada change
//! event do RegistryWatcher, chamamos `reload()` para atualizar o
//! registry compartilhado em background.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;

use super::approval::ApprovalMode;
use super::registry::SubAgentRegistry;

/// Callback invoked after every successful `reload()`. Receives the set
/// of mcp_servers that appeared in ANY spec (old or new) — consumers
/// can use this to invalidate caches keyed by server name.
///
/// Phase 21 (sota-gaps-followup) gap #9: when a spec is hot-reloaded
/// and its `mcp_servers` list changed, the discovery cache must drop
/// stale entries so the next spawn re-fetches `tools/list`.
pub type ReloadCallback =
    Arc<dyn Fn(&std::collections::BTreeSet<String>) + Send + Sync>;

/// Wrapper compartilhável que permite swap atômico do registry interno.
#[derive(Clone)]
pub struct ReloadableRegistry {
    inner: Arc<RwLock<SubAgentRegistry>>,
    project_dir: Option<PathBuf>,
    home_dir: Option<PathBuf>,
    mode: ApprovalMode,
    /// Callbacks fired after every reload. Receive the union of mcp_servers
    /// from old + new specs so consumers can invalidate stale caches.
    on_reload: Arc<RwLock<Vec<ReloadCallback>>>,
}

impl ReloadableRegistry {
    pub fn new(
        registry: SubAgentRegistry,
        project_dir: Option<PathBuf>,
        home_dir: Option<PathBuf>,
        mode: ApprovalMode,
    ) -> Self {
        Self {
            inner: Arc::new(RwLock::new(registry)),
            project_dir,
            home_dir,
            mode,
            on_reload: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Snapshot read-only do registry atual.
    pub fn snapshot(&self) -> SubAgentRegistry {
        self.inner.read().clone()
    }

    /// Register a callback to fire after every successful `reload()`.
    /// Multiple callbacks can be registered; they are called in order.
    pub fn on_reload(&self, callback: ReloadCallback) {
        self.on_reload.write().push(callback);
    }

    /// Re-load do registry: reconstroi com builtins + load_all do project/global.
    /// Idempotente. Atomic swap.
    pub fn reload(&self) {
        // Capture mcp_servers from BEFORE swap so callbacks see what changed.
        let old_servers: std::collections::BTreeSet<String> = {
            let r = self.inner.read();
            r.iter()
                .flat_map(|s| s.mcp_servers.iter().cloned())
                .collect()
        };

        let mut new_reg = SubAgentRegistry::with_builtins();
        let _ = new_reg.load_all(
            self.project_dir.as_deref(),
            self.home_dir.as_deref(),
            self.mode,
        );
        let new_servers: std::collections::BTreeSet<String> = new_reg
            .iter()
            .flat_map(|s| s.mcp_servers.iter().cloned())
            .collect();
        let mut guard = self.inner.write();
        *guard = new_reg;
        drop(guard);

        // Compute symmetric difference: any server that appeared OR
        // disappeared between old and new. Cache entries for unchanged
        // servers stay valid; entries for changed ones must be dropped.
        let changed: std::collections::BTreeSet<String> = old_servers
            .symmetric_difference(&new_servers)
            .cloned()
            .collect();
        if !changed.is_empty() {
            let cbs = self.on_reload.read();
            for cb in cbs.iter() {
                cb(&changed);
            }
        }
    }

    /// Returns the inner Arc — for cases where consumer needs to clone the lock.
    pub fn shared(&self) -> Arc<RwLock<SubAgentRegistry>> {
        self.inner.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn new_holds_initial_registry() {
        let r = ReloadableRegistry::new(
            SubAgentRegistry::with_builtins(),
            None,
            None,
            ApprovalMode::TrustAll,
        );
        assert_eq!(r.snapshot().len(), 4);
    }

    #[test]
    fn reload_picks_up_new_project_agents() {
        let dir = TempDir::new().unwrap();
        let agents = dir.path().join(".theo").join("agents");
        std::fs::create_dir_all(&agents).unwrap();

        let r = ReloadableRegistry::new(
            SubAgentRegistry::with_builtins(),
            Some(dir.path().to_path_buf()),
            None,
            ApprovalMode::TrustAll,
        );
        // Initially: only 4 builtins
        assert_eq!(r.snapshot().len(), 4);

        // Add a project spec
        std::fs::write(
            agents.join("custom.md"),
            "---\ndescription: custom\n---\nbody",
        )
        .unwrap();

        r.reload();

        assert_eq!(r.snapshot().len(), 5);
        assert!(r.snapshot().contains("custom"));
    }

    #[test]
    fn reload_drops_removed_agents() {
        let dir = TempDir::new().unwrap();
        let agents = dir.path().join(".theo").join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        std::fs::write(
            agents.join("temp.md"),
            "---\ndescription: temp\n---\nbody",
        )
        .unwrap();

        let r = ReloadableRegistry::new(
            SubAgentRegistry::with_builtins(),
            Some(dir.path().to_path_buf()),
            None,
            ApprovalMode::TrustAll,
        );
        r.reload();
        assert!(r.snapshot().contains("temp"));

        // Delete spec + reload
        std::fs::remove_file(agents.join("temp.md")).unwrap();
        r.reload();
        assert!(!r.snapshot().contains("temp"));
        assert_eq!(r.snapshot().len(), 4); // back to builtins only
    }

    #[test]
    fn shared_returns_same_arc() {
        let r = ReloadableRegistry::new(
            SubAgentRegistry::with_builtins(),
            None,
            None,
            ApprovalMode::TrustAll,
        );
        let a = r.shared();
        let b = r.shared();
        // Both point to the same RwLock
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn clone_shares_inner_state() {
        let r = ReloadableRegistry::new(
            SubAgentRegistry::with_builtins(),
            None,
            None,
            ApprovalMode::TrustAll,
        );
        let r2 = r.clone();
        assert!(Arc::ptr_eq(&r.shared(), &r2.shared()));
    }

    // ── Phase 21 (sota-gaps-followup): mcp_servers change → callback ──

    fn write_spec_with_mcp(dir: &Path, name: &str, mcp_servers: &[&str]) {
        let agents = dir.join(".theo").join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        let frontmatter = if mcp_servers.is_empty() {
            format!("---\ndescription: x\n---\nbody")
        } else {
            let list = mcp_servers
                .iter()
                .map(|s| format!("\"{}\"", s))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "---\ndescription: x\nmcp_servers: [{}]\n---\nbody",
                list
            )
        };
        std::fs::write(agents.join(format!("{}.md", name)), frontmatter).unwrap();
    }

    use std::path::Path;
    use std::sync::Mutex;

    #[test]
    fn reloadable_invalidates_cache_when_spec_mcp_servers_changes() {
        let dir = TempDir::new().unwrap();
        write_spec_with_mcp(dir.path(), "alpha", &["github"]);

        let r = ReloadableRegistry::new(
            SubAgentRegistry::with_builtins(),
            Some(dir.path().to_path_buf()),
            None,
            ApprovalMode::TrustAll,
        );
        r.reload(); // bring `alpha` into the registry

        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_cb = captured.clone();
        r.on_reload(Arc::new(move |servers| {
            let mut g = captured_cb.lock().unwrap();
            g.extend(servers.iter().cloned());
        }));

        // Change the mcp_servers list — github → postgres
        write_spec_with_mcp(dir.path(), "alpha", &["postgres"]);
        r.reload();

        let g = captured.lock().unwrap();
        // Symmetric difference: {github, postgres} both flagged.
        assert!(
            g.contains(&"github".to_string()),
            "removed server must be flagged: {:?}",
            *g
        );
        assert!(
            g.contains(&"postgres".to_string()),
            "added server must be flagged: {:?}",
            *g
        );
    }

    #[test]
    fn reloadable_does_not_fire_callback_when_mcp_servers_unchanged() {
        let dir = TempDir::new().unwrap();
        write_spec_with_mcp(dir.path(), "alpha", &["github"]);

        let r = ReloadableRegistry::new(
            SubAgentRegistry::with_builtins(),
            Some(dir.path().to_path_buf()),
            None,
            ApprovalMode::TrustAll,
        );
        r.reload();

        let count: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
        let count_cb = count.clone();
        r.on_reload(Arc::new(move |_| {
            *count_cb.lock().unwrap() += 1;
        }));

        // Re-write the same spec — same mcp_servers list.
        write_spec_with_mcp(dir.path(), "alpha", &["github"]);
        r.reload();

        assert_eq!(
            *count.lock().unwrap(),
            0,
            "callback must not fire when mcp_servers list is unchanged"
        );
    }

    #[test]
    fn reloadable_fires_multiple_callbacks_in_order() {
        let dir = TempDir::new().unwrap();
        write_spec_with_mcp(dir.path(), "a", &["s1"]);

        let r = ReloadableRegistry::new(
            SubAgentRegistry::with_builtins(),
            Some(dir.path().to_path_buf()),
            None,
            ApprovalMode::TrustAll,
        );
        r.reload();

        let log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
        let log1 = log.clone();
        let log2 = log.clone();
        r.on_reload(Arc::new(move |_| log1.lock().unwrap().push("first")));
        r.on_reload(Arc::new(move |_| log2.lock().unwrap().push("second")));

        write_spec_with_mcp(dir.path(), "a", &["s2"]);
        r.reload();

        let g = log.lock().unwrap();
        assert_eq!(*g, vec!["first", "second"]);
    }
}
