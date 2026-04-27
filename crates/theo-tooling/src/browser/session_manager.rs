//! T2.1 — Browser session manager.
//!
//! Lazy-spawn wrapper around `BrowserClient`. The browser tool
//! family (`browser_open`, `browser_click`, ...) shares one Arc'd
//! manager; the underlying sidecar is spawned only when the agent
//! calls `browser_open` for the first time. Subsequent tool calls
//! reuse the same Playwright session so navigation state is
//! preserved across calls.
//!
//! Single-session by design: one Playwright sidecar per manager.
//! Multi-tab / multi-context support can come later via DAP-style
//! caller-keyed session ids if needed.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::browser::client::{BrowserClient, BrowserClientError};
use crate::browser::protocol::{BrowserAction, BrowserResult};

/// Errors the session manager surfaces.
#[derive(Debug, thiserror::Error)]
pub enum BrowserSessionError {
    #[error("Node not found at `{program}` — install Node.js + run `npx playwright install chromium`")]
    NodeMissing { program: String },
    #[error("Playwright sidecar script not found at `{path}` — expected next to the theo binary")]
    ScriptMissing { path: String },
    #[error(transparent)]
    Client(#[from] BrowserClientError),
}

struct State {
    client: Option<Arc<BrowserClient>>,
    node_program: String,
    script_path: PathBuf,
}

/// Snapshot of `BrowserSessionManager` for the `browser_status` tool.
/// Pure data — safe to serialise into a tool result without leaking
/// the session manager's internal lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserStatus {
    /// The Node.js binary the manager is configured to spawn.
    pub node_program: String,
    /// Configured Playwright sidecar script path.
    pub script_path: PathBuf,
    /// `true` when the configured script exists on disk at probe time.
    pub script_present: bool,
    /// `true` when a sidecar has been spawned and is alive.
    pub session_active: bool,
}

/// Cheap to clone — internal state lives behind `Arc<Mutex<...>>`.
#[derive(Clone)]
pub struct BrowserSessionManager {
    state: Arc<Mutex<State>>,
}

impl BrowserSessionManager {
    /// Build a manager that will spawn `node <script_path>` on
    /// first use. The defaults locate `playwright_sidecar.js` next
    /// to the `theo-tooling` crate directory — the actual script
    /// shipping path is wired by the registry constructor.
    pub fn new(node_program: impl Into<String>, script_path: impl Into<PathBuf>) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                client: None,
                node_program: node_program.into(),
                script_path: script_path.into(),
            })),
        }
    }

    /// Returns true when a sidecar has been spawned and is alive.
    pub async fn is_active(&self) -> bool {
        self.state.lock().await.client.is_some()
    }

    /// Snapshot of the manager state for `browser_status`. The
    /// `script_present` flag is probed under the same lock so the
    /// snapshot is internally consistent (no TOCTOU between
    /// `script_path` and `script_present`).
    pub async fn status(&self) -> BrowserStatus {
        let s = self.state.lock().await;
        BrowserStatus {
            node_program: s.node_program.clone(),
            script_path: s.script_path.clone(),
            script_present: s.script_path.exists(),
            session_active: s.client.is_some(),
        }
    }

    /// Lazy spawn: returns the existing client or creates one.
    pub async fn ensure_client(
        &self,
    ) -> Result<Arc<BrowserClient>, BrowserSessionError> {
        // Fast path.
        {
            let s = self.state.lock().await;
            if let Some(c) = &s.client {
                return Ok(c.clone());
            }
        }
        // Slow path: spawn under the lock. We hold it across the
        // spawn so two concurrent callers don't double-spawn.
        let mut s = self.state.lock().await;
        if let Some(c) = &s.client {
            return Ok(c.clone());
        }
        if !s.script_path.exists() {
            return Err(BrowserSessionError::ScriptMissing {
                path: s.script_path.display().to_string(),
            });
        }
        let client = BrowserClient::spawn(&s.node_program, &s.script_path)
            .await
            .map_err(|e| match &e {
                BrowserClientError::Spawn { source, .. }
                    if source.kind() == std::io::ErrorKind::NotFound =>
                {
                    BrowserSessionError::NodeMissing {
                        program: s.node_program.clone(),
                    }
                }
                _ => BrowserSessionError::Client(e),
            })?;
        let arc = Arc::new(client);
        s.client = Some(arc.clone());
        Ok(arc)
    }

    /// Send an action through the lazy-spawned client.
    pub async fn request(
        &self,
        action: BrowserAction,
    ) -> Result<BrowserResult, BrowserSessionError> {
        let client = self.ensure_client().await?;
        client.request(action).await.map_err(BrowserSessionError::from)
    }

    /// Close the session if any: best-effort shutdown + drop the
    /// cached client so the next call respawns fresh.
    pub async fn terminate(&self) -> bool {
        let client = {
            let mut s = self.state.lock().await;
            s.client.take()
        };
        match client {
            Some(c) => {
                let _ = c.shutdown().await;
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn t21bsm_is_active_false_before_first_call() {
        let mgr = BrowserSessionManager::new(
            "node",
            PathBuf::from("/nonexistent/playwright_sidecar.js"),
        );
        assert!(!mgr.is_active().await);
    }

    #[tokio::test]
    async fn t21bsm_ensure_client_with_missing_script_returns_typed_error() {
        let mgr = BrowserSessionManager::new(
            "node",
            PathBuf::from("/nonexistent/playwright_sidecar.js"),
        );
        let res = mgr.ensure_client().await.map(|_| "unexpected_ok");
        match res {
            Err(BrowserSessionError::ScriptMissing { path }) => {
                assert!(path.contains("playwright_sidecar.js"));
            }
            Err(other) => panic!("expected ScriptMissing, got {other:?}"),
            Ok(marker) => panic!("expected error, got Ok({marker})"),
        }
    }

    #[tokio::test]
    async fn t21bsm_terminate_no_session_returns_false() {
        let mgr = BrowserSessionManager::new(
            "node",
            PathBuf::from("/nonexistent.js"),
        );
        assert!(!mgr.terminate().await);
    }

    #[tokio::test]
    async fn t21bsm_clone_shares_state_via_arc() {
        let mgr = BrowserSessionManager::new("node", PathBuf::from("/x"));
        let cloned = mgr.clone();
        // Both clones see the same is_active result because they
        // share the inner Arc<Mutex<...>>.
        assert_eq!(mgr.is_active().await, cloned.is_active().await);
    }

    // ── browser_status snapshot ──────────────────────────────────

    #[tokio::test]
    async fn t21bsm_status_reports_missing_script_when_path_does_not_exist() {
        let mgr = BrowserSessionManager::new(
            "node",
            PathBuf::from("/__theo_no_browser__/playwright_sidecar.js"),
        );
        let status = mgr.status().await;
        assert_eq!(status.node_program, "node");
        assert!(!status.script_present);
        assert!(!status.session_active);
        assert!(status.script_path.to_string_lossy().contains("playwright_sidecar.js"));
    }

    #[tokio::test]
    async fn t21bsm_status_reports_present_script_when_file_exists() {
        // Create a temp file so script_present == true.
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("playwright_sidecar.js");
        std::fs::write(&script, b"// stub sidecar - never executed").unwrap();
        let mgr = BrowserSessionManager::new("node", script.clone());
        let status = mgr.status().await;
        assert!(status.script_present);
        assert!(!status.session_active, "no spawn yet → session_active=false");
        assert_eq!(status.script_path, script);
    }
}
