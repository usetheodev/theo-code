//! ReloadableRegistry — Arc<RwLock<SubAgentRegistry>> com helper para
//! re-build a partir de project/global dirs.
//!
//! Usado pelo CLI quando `--watch-agents` está ativo: a cada change
//! event do RegistryWatcher, chamamos `reload()` para atualizar o
//! registry compartilhado em background.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use super::approval::ApprovalMode;
use super::registry::SubAgentRegistry;

/// Wrapper compartilhável que permite swap atômico do registry interno.
#[derive(Clone)]
pub struct ReloadableRegistry {
    inner: Arc<RwLock<SubAgentRegistry>>,
    project_dir: Option<PathBuf>,
    home_dir: Option<PathBuf>,
    mode: ApprovalMode,
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
        }
    }

    /// Snapshot read-only do registry atual.
    pub fn snapshot(&self) -> SubAgentRegistry {
        self.inner.read().expect("registry rwlock poisoned").clone()
    }

    /// Re-load do registry: reconstroi com builtins + load_all do project/global.
    /// Idempotente. Atomic swap.
    pub fn reload(&self) {
        let mut new_reg = SubAgentRegistry::with_builtins();
        let _ = new_reg.load_all(
            self.project_dir.as_deref(),
            self.home_dir.as_deref(),
            self.mode,
        );
        let mut guard = self.inner.write().expect("registry rwlock poisoned");
        *guard = new_reg;
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
}
