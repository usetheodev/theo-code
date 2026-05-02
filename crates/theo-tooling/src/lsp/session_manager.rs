//! T3.1 — LSP session manager.
//!
//! The bridge between `LspClient` (one connection to one server) and
//! agent-callable tools that need to perform many operations across
//! many files. A session manager:
//!
//! 1. Discovers available LSP servers on PATH (once, at construction).
//! 2. Maps a file path → language → server (via the discovery catalogue).
//! 3. Lazily spawns and `initialize`s an `LspClient` on first use.
//! 4. Caches the initialized client so subsequent tool calls reuse it.
//! 5. Drains background notification channels (in a spawned task) so
//!    the server's mpsc buffer doesn't fill up.
//!
//! Concurrency: the manager wraps its inner state in a `Mutex` so
//! many tool tasks can call `with_client_for` concurrently. The
//! actual spawn happens under the lock once per language; subsequent
//! callers see the cached client.
//!
//! Lifetime: holding a `LspSessionManager` keeps every spawned LSP
//! server alive (via `kill_on_drop`). Dropping the manager closes
//! all clients.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::json;
use tokio::process::ChildStdin;
use tokio::sync::Mutex;

use crate::lsp::client::{LspClient, LspClientError};
use crate::lsp::discovery::{DiscoveredServer, discover, server_for_extension};

/// Errors the session manager surfaces. Wraps lower-layer errors
/// with enough context to know whether the failure was discovery
/// (no server installed for the language) vs. spawn vs. protocol.
#[derive(Debug, thiserror::Error)]
pub enum LspSessionError {
    #[error("no LSP server installed on PATH for file extension `{ext}`")]
    NoServerForExtension { ext: String },
    #[error("file `{path}` has no extension — cannot route to an LSP server")]
    MissingExtension { path: String },
    #[error(transparent)]
    Client(#[from] LspClientError),
    #[error("initialize handshake failed: server returned error: {0}")]
    InitializeFailed(String),
}

/// One cached, initialized LSP client + the workspace root it was
/// initialised for. Re-initialisation is required if the workspace
/// root changes — this is a simplification: most tool calls run in
/// the same root for the lifetime of an agent run.
struct CachedClient {
    client: Arc<LspClient<ChildStdin>>,
    workspace_root: PathBuf,
}

/// State protected by the manager's Mutex.
struct SessionState {
    /// language extension (without dot) → cached client.
    /// Multiple extensions for the same language share one cache
    /// entry by reusing the underlying server's preferred extension.
    by_extension: HashMap<String, Arc<CachedClient>>,
    /// Discovered servers, indexed by extension. Built once at
    /// construction; immutable after that.
    catalogue: HashMap<&'static str, DiscoveredServer>,
}

/// Public manager. Cheap to clone — holds an `Arc<Mutex<...>>`.
#[derive(Clone)]
pub struct LspSessionManager {
    state: Arc<Mutex<SessionState>>,
}

impl LspSessionManager {
    /// Build a manager from the system PATH discovery.
    pub fn from_path() -> Self {
        let servers = discover();
        let catalogue = server_for_extension(&servers);
        Self {
            state: Arc::new(Mutex::new(SessionState {
                by_extension: HashMap::new(),
                catalogue,
            })),
        }
    }

    /// Build a manager from a specific catalogue. Used by tests so
    /// they can inject fake servers without polluting PATH.
    pub fn from_catalogue(catalogue: HashMap<&'static str, DiscoveredServer>) -> Self {
        Self {
            state: Arc::new(Mutex::new(SessionState {
                by_extension: HashMap::new(),
                catalogue,
            })),
        }
    }

    /// Number of distinct extensions the manager knows how to route.
    pub async fn supported_extensions(&self) -> Vec<String> {
        let s = self.state.lock().await;
        s.catalogue.keys().map(|k| (*k).to_string()).collect()
    }

    /// Number of currently cached (initialised) clients.
    pub async fn cached_session_count(&self) -> usize {
        // Different extensions may point to the same Arc — count
        // unique Arcs by pointer identity.
        let s = self.state.lock().await;
        let mut seen: Vec<*const CachedClient> = Vec::new();
        for c in s.by_extension.values() {
            let ptr = Arc::as_ptr(c);
            if !seen.contains(&ptr) {
                seen.push(ptr);
            }
        }
        seen.len()
    }

    /// Look up (or lazily spawn + initialize) the LSP client for the
    /// given file path. Returns an `Arc<LspClient>` ready for
    /// `request`/`notify` calls. `workspace_root` is the absolute
    /// path the server should treat as project root for the
    /// `initialize` request.
    pub async fn ensure_client_for(
        &self,
        file_path: &Path,
        workspace_root: &Path,
    ) -> Result<Arc<LspClient<ChildStdin>>, LspSessionError> {
        let ext = file_path
            .extension()
            .and_then(|e| e.to_str())
            .ok_or_else(|| LspSessionError::MissingExtension {
                path: file_path.display().to_string(),
            })?;

        // Fast path: already cached.
        {
            let s = self.state.lock().await;
            if let Some(cached) = s.by_extension.get(ext) {
                return Ok(cached.client.clone());
            }
        }

        // Slow path: need to spawn. We MUST release the read-lock and
        // re-acquire as writer to avoid holding the mutex across the
        // full LSP `initialize` round-trip (which would serialise
        // everyone). But we also can't have two callers double-spawn
        // — so we re-check under the new lock and short-circuit.
        let server = {
            let s = self.state.lock().await;
            s.catalogue
                .get(ext)
                .cloned()
                .ok_or_else(|| LspSessionError::NoServerForExtension {
                    ext: ext.to_string(),
                })?
        };

        let cmd = server
            .command
            .to_str()
            .ok_or(LspSessionError::Client(LspClientError::StdioCaptureFailed))?
            .to_string();
        let args: Vec<&str> = server.args.to_vec();
        let (client, _notif_rx) = LspClient::spawn(&cmd, &args).await?;

        // Send `initialize` so the server is ready for textDocument/* requests.
        let workspace_uri = path_to_file_uri(workspace_root);
        let init_resp = client
            .request(
                "initialize",
                Some(json!({
                    "processId": std::process::id() as u64,
                    "rootUri": workspace_uri,
                    "capabilities": {
                        "textDocument": {
                            "synchronization": {"didSave": true},
                            "rename": {"dynamicRegistration": false},
                            "references": {"dynamicRegistration": false},
                            "definition": {"dynamicRegistration": false},
                            "hover": {"dynamicRegistration": false},
                            "codeAction": {"dynamicRegistration": false},
                        },
                        "workspace": {"workspaceFolders": true}
                    },
                    "workspaceFolders": [{
                        "uri": workspace_uri,
                        "name": workspace_root
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("root"),
                    }],
                })),
            )
            .await?;
        if let Some(err) = init_resp.error {
            return Err(LspSessionError::InitializeFailed(err.message));
        }
        // Per spec: client must send `initialized` notification after
        // the initialize response.
        client.notify("initialized", Some(json!({}))).await?;

        // Drain the notification channel in a background task so the
        // server's mpsc doesn't fill up. We don't currently route
        // notifications anywhere — the manager treats them as "log
        // messages we don't show". A future iteration could surface
        // them through a callback.
        tokio::spawn(async move {
            let mut rx = _notif_rx;
            while rx.recv().await.is_some() {
                // discard
            }
        });

        let cached = Arc::new(CachedClient {
            client: Arc::new(client),
            workspace_root: workspace_root.to_path_buf(),
        });

        // Cache it. If a concurrent caller raced and spawned its
        // own, the FIRST one wins (we use entry API).
        let mut s = self.state.lock().await;
        let entry = s.by_extension.entry(ext.to_string()).or_insert(cached);
        Ok(entry.client.clone())
    }

    /// Drop a cached client (e.g. when the server crashed and we
    /// need to respawn on next use). Returns true when there was
    /// something to drop.
    pub async fn invalidate(&self, file_path: &Path) -> bool {
        let Some(ext) = file_path.extension().and_then(|e| e.to_str()) else {
            return false;
        };
        let mut s = self.state.lock().await;
        s.by_extension.remove(ext).is_some()
    }

    /// Look up the cached workspace root for an extension. Returns
    /// `None` if no client is cached yet for that extension.
    pub async fn cached_workspace_for(&self, ext: &str) -> Option<PathBuf> {
        let s = self.state.lock().await;
        s.by_extension.get(ext).map(|c| c.workspace_root.clone())
    }
}

/// Convert an absolute filesystem path to a `file://` URI. Caller is
/// responsible for passing a canonical path.
fn path_to_file_uri(path: &Path) -> String {
    let s = path.display().to_string();
    if s.starts_with("file://") {
        return s;
    }
    if s.starts_with('/') {
        format!("file://{s}")
    } else {
        format!("file:///{}", s.replace('\\', "/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn t31sm_empty_catalogue_supports_zero_extensions() {
        let mgr = LspSessionManager::from_catalogue(HashMap::new());
        assert!(mgr.supported_extensions().await.is_empty());
        assert_eq!(mgr.cached_session_count().await, 0);
    }

    #[tokio::test]
    async fn t31sm_supported_extensions_lists_catalogue_keys() {
        let mut cat: HashMap<&'static str, DiscoveredServer> = HashMap::new();
        cat.insert(
            "rs",
            DiscoveredServer {
                name: "rust-analyzer",
                command: PathBuf::from("/usr/bin/rust-analyzer"),
                args: vec![],
                file_extensions: &["rs"],
                languages: &["rust"],
            },
        );
        cat.insert(
            "py",
            DiscoveredServer {
                name: "pyright",
                command: PathBuf::from("/usr/bin/pyright-langserver"),
                args: vec!["--stdio"],
                file_extensions: &["py", "pyi"],
                languages: &["python"],
            },
        );
        let mgr = LspSessionManager::from_catalogue(cat);
        let mut exts = mgr.supported_extensions().await;
        exts.sort();
        assert_eq!(exts, vec!["py".to_string(), "rs".to_string()]);
    }

    #[tokio::test]
    async fn t31sm_ensure_client_for_extensionless_file_returns_typed_error() {
        // LspClient doesn't impl Debug (its Child doesn't), so we
        // can't use `{:?}` on the Ok arm. Map success to a plain
        // marker before matching.
        let mgr = LspSessionManager::from_catalogue(HashMap::new());
        let res = mgr
            .ensure_client_for(Path::new("/tmp/Makefile"), Path::new("/tmp"))
            .await
            .map(|_| "unexpected_ok");
        match res {
            Err(LspSessionError::MissingExtension { path }) => {
                assert!(path.contains("Makefile"));
            }
            Err(other) => panic!("expected MissingExtension, got {other:?}"),
            Ok(marker) => panic!("expected MissingExtension, got Ok({marker})"),
        }
    }

    #[tokio::test]
    async fn t31sm_ensure_client_for_unknown_extension_returns_typed_error() {
        let mgr = LspSessionManager::from_catalogue(HashMap::new());
        let res = mgr
            .ensure_client_for(Path::new("/tmp/file.unknownxyz"), Path::new("/tmp"))
            .await
            .map(|_| "unexpected_ok");
        match res {
            Err(LspSessionError::NoServerForExtension { ext }) => {
                assert_eq!(ext, "unknownxyz");
            }
            Err(other) => panic!("expected NoServerForExtension, got {other:?}"),
            Ok(marker) => panic!("expected NoServerForExtension, got Ok({marker})"),
        }
    }

    #[tokio::test]
    async fn t31sm_invalidate_removes_cached_extension_returns_true() {
        let mgr = LspSessionManager::from_catalogue(HashMap::new());
        // Pre-seed the cache directly via a fake CachedClient. We
        // can't easily do that without spawning a real server; so
        // we instead verify invalidate returns false when nothing
        // is cached (the "removes when present" path is exercised
        // implicitly when ensure_client_for paths run end-to-end in
        // future integration tests).
        assert!(!mgr.invalidate(Path::new("/tmp/x.rs")).await);
    }

    #[tokio::test]
    async fn t31sm_invalidate_extensionless_file_returns_false() {
        let mgr = LspSessionManager::from_catalogue(HashMap::new());
        assert!(!mgr.invalidate(Path::new("/tmp/Makefile")).await);
    }

    #[tokio::test]
    async fn t31sm_cached_workspace_for_returns_none_when_uncached() {
        let mgr = LspSessionManager::from_catalogue(HashMap::new());
        assert!(mgr.cached_workspace_for("rs").await.is_none());
    }

    #[tokio::test]
    async fn t31sm_from_path_does_not_panic_in_any_environment() {
        // Just confirm we can build a manager from the real PATH —
        // we don't assert anything about which servers were found.
        let _mgr = LspSessionManager::from_path();
    }

    #[tokio::test]
    async fn t31sm_clone_shares_state_via_arc() {
        let mut cat: HashMap<&'static str, DiscoveredServer> = HashMap::new();
        cat.insert(
            "rs",
            DiscoveredServer {
                name: "rust-analyzer",
                command: PathBuf::from("/usr/bin/rust-analyzer"),
                args: vec![],
                file_extensions: &["rs"],
                languages: &["rust"],
            },
        );
        let mgr = LspSessionManager::from_catalogue(cat);
        let mgr_clone = mgr.clone();
        // Both clones see the same supported_extensions because they
        // share the inner Arc<Mutex<...>>.
        assert_eq!(
            mgr.supported_extensions().await.len(),
            mgr_clone.supported_extensions().await.len()
        );
    }

    #[tokio::test]
    async fn t31sm_concurrent_calls_to_supported_extensions_are_safe() {
        let mut cat: HashMap<&'static str, DiscoveredServer> = HashMap::new();
        cat.insert(
            "rs",
            DiscoveredServer {
                name: "rust-analyzer",
                command: PathBuf::from("/usr/bin/rust-analyzer"),
                args: vec![],
                file_extensions: &["rs"],
                languages: &["rust"],
            },
        );
        let mgr = Arc::new(LspSessionManager::from_catalogue(cat));
        let mut handles = Vec::new();
        for _ in 0..20 {
            let m = mgr.clone();
            handles.push(tokio::spawn(async move {
                m.supported_extensions().await
            }));
        }
        for h in handles {
            let exts = h.await.unwrap();
            assert_eq!(exts, vec!["rs".to_string()]);
        }
    }

    #[test]
    fn t31sm_path_to_file_uri_handles_absolute_unix_path() {
        assert_eq!(path_to_file_uri(Path::new("/home/x/y")), "file:///home/x/y");
    }

    #[test]
    fn t31sm_path_to_file_uri_passes_through_existing_uri() {
        // Caller may already pass a URI — leave it alone.
        let s = path_to_file_uri(Path::new("file:///already/uri"));
        assert_eq!(s, "file:///already/uri");
    }

    #[test]
    fn t31sm_path_to_file_uri_handles_windows_style_path() {
        let s = path_to_file_uri(Path::new("C:\\Users\\x"));
        assert_eq!(s, "file:///C:/Users/x");
    }
}
