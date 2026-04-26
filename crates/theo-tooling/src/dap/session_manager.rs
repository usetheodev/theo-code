//! T13.1 — DAP session manager.
//!
//! Companion to `LspSessionManager` for debug adapters. DAP sessions
//! differ from LSP sessions in two important ways, so the API shape
//! is intentionally different:
//!
//!   1. **Per-target, not per-language.** Each debug session is a
//!      distinct process bound to one debuggee. Two concurrent
//!      debug runs of the same Python program need two adapters,
//!      not one shared like LSP would.
//!   2. **Caller-chosen identity.** The session_id is supplied by
//!      the caller (the agent's debug tool) so they can refer back
//!      to it across multiple `set_breakpoint` / `step` / `eval`
//!      calls.
//!
//! Workflow:
//!   1. `launch(session_id, language, launch_args)` — find the
//!      right adapter for the language, spawn it, run `initialize`
//!      + `launch`, cache the client by session_id.
//!   2. `session(session_id)` — fast lookup, returns
//!      `Arc<DapClient>` for follow-up requests.
//!   3. `terminate(session_id)` — drop the cached client. The
//!      underlying adapter is killed via `kill_on_drop`.
//!
//! Concurrency: cheap to clone (`Arc<Mutex<...>>` inside).

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;
use tokio::process::ChildStdin;
use tokio::sync::Mutex;

use crate::dap::client::{DapClient, DapClientError};
use crate::dap::discovery::{DiscoveredAdapter, adapter_for_language, discover};
use crate::dap::protocol::DapResponse;

/// Errors the DAP session manager surfaces.
#[derive(Debug, thiserror::Error)]
pub enum DapSessionError {
    #[error("no DAP adapter installed on PATH for language `{language}`")]
    NoAdapterForLanguage { language: String },
    #[error("session id `{id}` is already in use; terminate it first")]
    SessionAlreadyExists { id: String },
    #[error(transparent)]
    Client(#[from] DapClientError),
    #[error("initialize handshake failed: {0}")]
    InitializeFailed(String),
    #[error("launch failed: {0}")]
    LaunchFailed(String),
    #[error("attach failed: {0}")]
    AttachFailed(String),
}

/// One cached, initialised DAP client + the language label it was
/// launched for. Read-only after construction.
struct CachedSession {
    client: Arc<DapClient<ChildStdin>>,
    language: String,
}

struct SessionState {
    /// session_id → cached client.
    by_id: HashMap<String, Arc<CachedSession>>,
    /// Adapter catalogue keyed by language (lower-case ASCII).
    catalogue: HashMap<&'static str, DiscoveredAdapter>,
}

/// Public manager. Cheap to clone.
#[derive(Clone)]
pub struct DapSessionManager {
    state: Arc<Mutex<SessionState>>,
}

impl DapSessionManager {
    /// Build a manager from the system PATH discovery.
    pub fn from_path() -> Self {
        let adapters = discover();
        let catalogue = adapter_for_language(&adapters);
        Self {
            state: Arc::new(Mutex::new(SessionState {
                by_id: HashMap::new(),
                catalogue,
            })),
        }
    }

    /// Build a manager from a specific catalogue. Used by tests.
    pub fn from_catalogue(catalogue: HashMap<&'static str, DiscoveredAdapter>) -> Self {
        Self {
            state: Arc::new(Mutex::new(SessionState {
                by_id: HashMap::new(),
                catalogue,
            })),
        }
    }

    /// Languages the manager has at least one adapter for.
    pub async fn supported_languages(&self) -> Vec<String> {
        let s = self.state.lock().await;
        s.catalogue.keys().map(|k| (*k).to_string()).collect()
    }

    /// Number of currently active (cached) sessions.
    pub async fn active_count(&self) -> usize {
        self.state.lock().await.by_id.len()
    }

    /// List active session ids in arbitrary order.
    pub async fn active_sessions(&self) -> Vec<String> {
        self.state.lock().await.by_id.keys().cloned().collect()
    }

    /// Look up a previously launched session. Returns `None` when
    /// the id is unknown OR was terminated.
    pub async fn session(&self, session_id: &str) -> Option<Arc<DapClient<ChildStdin>>> {
        self.state
            .lock()
            .await
            .by_id
            .get(session_id)
            .map(|c| c.client.clone())
    }

    /// Look up the language a session was launched for. Returns
    /// `None` when unknown.
    pub async fn language_of(&self, session_id: &str) -> Option<String> {
        self.state
            .lock()
            .await
            .by_id
            .get(session_id)
            .map(|c| c.language.clone())
    }

    /// Launch a new debug session.
    ///
    /// 1. Look up the adapter for `language` in the catalogue.
    /// 2. Spawn the adapter as a subprocess.
    /// 3. Run the DAP `initialize` handshake.
    /// 4. Issue the `launch` request with `launch_args` (debugger-
    ///    specific JSON: `program`, `cwd`, `env`, etc.).
    /// 5. Cache the client by `session_id` and return it.
    pub async fn launch(
        &self,
        session_id: &str,
        language: &str,
        launch_args: Value,
    ) -> Result<Arc<DapClient<ChildStdin>>, DapSessionError> {
        self.spawn_session(session_id, language, "launch", launch_args)
            .await
    }

    /// Attach to an existing process via DAP `attach`. Same wiring
    /// as `launch` but the request command differs.
    pub async fn attach(
        &self,
        session_id: &str,
        language: &str,
        attach_args: Value,
    ) -> Result<Arc<DapClient<ChildStdin>>, DapSessionError> {
        self.spawn_session(session_id, language, "attach", attach_args)
            .await
    }

    /// Terminate a session: removes the cache entry, dropping the
    /// `DapClient` (which kills the adapter via `kill_on_drop`).
    /// Returns `true` when there was something to terminate.
    pub async fn terminate(&self, session_id: &str) -> bool {
        let mut s = self.state.lock().await;
        s.by_id.remove(session_id).is_some()
    }

    async fn spawn_session(
        &self,
        session_id: &str,
        language: &str,
        command: &str,
        args: Value,
    ) -> Result<Arc<DapClient<ChildStdin>>, DapSessionError> {
        // Reject duplicate session ids before spawning anything.
        {
            let s = self.state.lock().await;
            if s.by_id.contains_key(session_id) {
                return Err(DapSessionError::SessionAlreadyExists {
                    id: session_id.into(),
                });
            }
        }

        // Look up the adapter under read lock; clone out so we can
        // release the lock before the (slow) spawn.
        let needle = language.to_ascii_lowercase();
        let adapter = {
            let s = self.state.lock().await;
            s.catalogue
                .get(needle.as_str())
                .cloned()
                .ok_or_else(|| DapSessionError::NoAdapterForLanguage {
                    language: language.to_string(),
                })?
        };

        let cmd = adapter
            .command
            .to_str()
            .ok_or(DapSessionError::Client(
                DapClientError::StdioCaptureFailed,
            ))?
            .to_string();
        let adapter_args: Vec<&str> = adapter.args.to_vec();
        let (client, _evt_rx) = DapClient::spawn(&cmd, &adapter_args).await?;

        // DAP handshake: `initialize` then the launch/attach command.
        let init_resp = client
            .request(
                "initialize",
                Some(serde_json::json!({
                    "clientID": "theo",
                    "clientName": "Theo Code",
                    "adapterID": adapter.name,
                    "linesStartAt1": true,
                    "columnsStartAt1": true,
                    "pathFormat": "path",
                    "supportsRunInTerminalRequest": false,
                })),
            )
            .await?;
        if !init_resp.success {
            return Err(DapSessionError::InitializeFailed(
                init_resp
                    .message
                    .unwrap_or_else(|| "no message".to_string()),
            ));
        }

        let resp: DapResponse = client.request(command, Some(args)).await?;
        if !resp.success {
            let msg = resp.message.unwrap_or_else(|| "no message".to_string());
            return Err(match command {
                "launch" => DapSessionError::LaunchFailed(msg),
                "attach" => DapSessionError::AttachFailed(msg),
                _ => DapSessionError::LaunchFailed(msg),
            });
        }

        // Drain the event channel so the adapter's mpsc doesn't fill
        // up. A future iteration could surface events through a
        // callback.
        tokio::spawn(async move {
            let mut rx = _evt_rx;
            while rx.recv().await.is_some() {}
        });

        let client_arc = Arc::new(client);
        let cached = Arc::new(CachedSession {
            client: client_arc.clone(),
            language: language.to_string(),
        });
        let mut s = self.state.lock().await;
        // Race-safe: if a concurrent caller raced and inserted, keep
        // the FIRST one (via or_insert). The losing client is dropped
        // here when this scope ends, killing its adapter.
        let entry = s
            .by_id
            .entry(session_id.to_string())
            .or_insert(cached);
        Ok(entry.client.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fake_catalogue() -> HashMap<&'static str, DiscoveredAdapter> {
        // The path doesn't have to exist — tests that don't actually
        // launch the adapter just verify the catalogue lookup logic.
        let mut cat = HashMap::new();
        cat.insert(
            "python",
            DiscoveredAdapter {
                name: "debugpy",
                command: PathBuf::from("/usr/bin/debugpy-adapter"),
                args: vec![],
                languages: &["python"],
                file_extensions: &["py"],
            },
        );
        cat.insert(
            "rust",
            DiscoveredAdapter {
                name: "lldb-vscode",
                command: PathBuf::from("/usr/bin/lldb-vscode"),
                args: vec![],
                languages: &["rust", "c", "cpp"],
                file_extensions: &["rs", "c", "cpp"],
            },
        );
        cat
    }

    #[tokio::test]
    async fn t131sm_empty_catalogue_supports_zero_languages() {
        let mgr = DapSessionManager::from_catalogue(HashMap::new());
        assert!(mgr.supported_languages().await.is_empty());
        assert_eq!(mgr.active_count().await, 0);
    }

    #[tokio::test]
    async fn t131sm_supported_languages_lists_catalogue_keys() {
        let mgr = DapSessionManager::from_catalogue(fake_catalogue());
        let mut langs = mgr.supported_languages().await;
        langs.sort();
        assert_eq!(langs, vec!["python".to_string(), "rust".to_string()]);
    }

    #[tokio::test]
    async fn t131sm_launch_unknown_language_returns_typed_error() {
        let mgr = DapSessionManager::from_catalogue(fake_catalogue());
        let res = mgr
            .launch("ses-1", "haskell", serde_json::json!({"program":"a.hs"}))
            .await
            .map(|_| "unexpected_ok");
        match res {
            Err(DapSessionError::NoAdapterForLanguage { language }) => {
                assert_eq!(language, "haskell");
            }
            Err(other) => panic!("expected NoAdapterForLanguage, got {other:?}"),
            Ok(marker) => panic!("expected error, got Ok({marker})"),
        }
    }

    #[tokio::test]
    async fn t131sm_attach_unknown_language_returns_typed_error() {
        let mgr = DapSessionManager::from_catalogue(fake_catalogue());
        let res = mgr
            .attach("ses-2", "elixir", serde_json::json!({"pid": 1234}))
            .await
            .map(|_| "unexpected_ok");
        match res {
            Err(DapSessionError::NoAdapterForLanguage { language }) => {
                assert_eq!(language, "elixir");
            }
            Err(other) => panic!("expected NoAdapterForLanguage, got {other:?}"),
            Ok(marker) => panic!("expected error, got Ok({marker})"),
        }
    }

    #[tokio::test]
    async fn t131sm_terminate_unknown_session_returns_false() {
        let mgr = DapSessionManager::from_catalogue(fake_catalogue());
        assert!(!mgr.terminate("never-existed").await);
    }

    #[tokio::test]
    async fn t131sm_session_unknown_id_returns_none() {
        let mgr = DapSessionManager::from_catalogue(fake_catalogue());
        assert!(mgr.session("never-existed").await.is_none());
        assert!(mgr.language_of("never-existed").await.is_none());
    }

    #[tokio::test]
    async fn t131sm_active_sessions_empty_initially() {
        let mgr = DapSessionManager::from_catalogue(fake_catalogue());
        assert!(mgr.active_sessions().await.is_empty());
    }

    #[tokio::test]
    async fn t131sm_clone_shares_state_via_arc() {
        let mgr = DapSessionManager::from_catalogue(fake_catalogue());
        let cloned = mgr.clone();
        // Both clones see the same supported_languages because they
        // share the inner Arc<Mutex<...>>.
        let a = mgr.supported_languages().await.len();
        let b = cloned.supported_languages().await.len();
        assert_eq!(a, b);
        assert_eq!(a, 2);
    }

    #[tokio::test]
    async fn t131sm_from_path_does_not_panic_in_any_environment() {
        let _mgr = DapSessionManager::from_path();
    }

    #[tokio::test]
    async fn t131sm_concurrent_supported_languages_safe() {
        let mgr = Arc::new(DapSessionManager::from_catalogue(fake_catalogue()));
        let mut handles = Vec::new();
        for _ in 0..16 {
            let m = mgr.clone();
            handles.push(tokio::spawn(async move {
                m.supported_languages().await
            }));
        }
        for h in handles {
            let langs = h.await.unwrap();
            assert_eq!(langs.len(), 2);
        }
    }

    #[tokio::test]
    async fn t131sm_launch_unknown_language_is_case_insensitive() {
        let mgr = DapSessionManager::from_catalogue(fake_catalogue());
        // catalogue has "python" lower-case; calling with "Python"
        // (capitalised) should still hit the catalogue (the lookup
        // lowercases internally). To prove this, we exercise the
        // launch path — which will fail at spawn (binary doesn't
        // really exist at /usr/bin/debugpy-adapter on every box).
        // The point is we should NOT see NoAdapterForLanguage.
        let res = mgr
            .launch("ses-3", "Python", serde_json::json!({"program":"x.py"}))
            .await
            .map(|_| "unexpected_ok");
        match res {
            Err(DapSessionError::NoAdapterForLanguage { language }) => {
                panic!(
                    "language `{language}` should have been found via case-insensitive lookup"
                )
            }
            // Anything else is fine — the test is only about the
            // language lookup, not about whether the binary exists.
            _ => {}
        }
    }
}
