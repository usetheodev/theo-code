//! Cooperative cancellation tree — parent → child token propagation.
//!
//! Track B — 
//!
//! When the parent agent cancels (Ctrl+C, timeout, programmatic abort),
//! all child sub-agents cancel cooperatively. Cancellation is non-blocking:
//! tokens are checked at well-defined points in the agent loop.
//!
//! Reference: opendev/docs/subagent-execution-model.md:75 documents
//! "Each subagent has its own cancellation token". Tokio's CancellationToken
//! supports child tokens that propagate parent cancellation automatically.

use std::sync::Arc;

use dashmap::DashMap;
use tokio_util::sync::CancellationToken;

/// A cancellation tree where the root cancels propagates to all children.
/// Specific agents can also be cancelled individually without affecting siblings.
///
/// Cheap to clone (`Arc` internally).
#[derive(Clone, Default)]
pub struct CancellationTree {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    root: CancellationToken,
    /// Per-agent tokens, keyed by `agent_run_id` (or `agent_name` for ad-hoc lookup).
    children: DashMap<String, CancellationToken>,
}

impl CancellationTree {
    /// Create a new tree with a fresh root token.
    pub fn new() -> Self {
        Self::default()
    }

    /// The root token (cancelling this cancels every child).
    pub fn root(&self) -> CancellationToken {
        self.inner.root.clone()
    }

    /// True if the root has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.inner.root.is_cancelled()
    }

    /// Create or retrieve a child token for `agent_id`. The child propagates
    /// root cancellation automatically (via `child_token`).
    pub fn child(&self, agent_id: &str) -> CancellationToken {
        if let Some(existing) = self.inner.children.get(agent_id) {
            return existing.clone();
        }
        let token = self.inner.root.child_token();
        self.inner
            .children
            .insert(agent_id.to_string(), token.clone());
        token
    }

    /// Cancel a specific agent (does not affect siblings or root).
    ///
    /// Returns `true` if the agent was registered and cancelled, `false` if
    /// the name was not in the tree.
    pub fn cancel_agent(&self, agent_id: &str) -> bool {
        match self.inner.children.get(agent_id) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    /// Cancel the root → propagates to all child tokens.
    pub fn cancel_all(&self) {
        self.inner.root.cancel();
    }

    /// Number of registered child tokens.
    pub fn child_count(&self) -> usize {
        self.inner.children.len()
    }

    /// Forget a child token (for cleanup post-completion). Idempotent.
    pub fn forget(&self, agent_id: &str) {
        self.inner.children.remove(agent_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn root_token_cancels_propagates_to_all_children() {
        let tree = CancellationTree::new();
        let c1 = tree.child("a");
        let c2 = tree.child("b");
        assert!(!c1.is_cancelled());
        assert!(!c2.is_cancelled());

        tree.cancel_all();

        assert!(tree.is_cancelled());
        assert!(c1.is_cancelled());
        assert!(c2.is_cancelled());
    }

    #[tokio::test]
    async fn cancel_specific_agent_does_not_affect_siblings_or_root() {
        let tree = CancellationTree::new();
        let c1 = tree.child("a");
        let c2 = tree.child("b");

        let cancelled = tree.cancel_agent("a");
        assert!(cancelled);

        assert!(c1.is_cancelled());
        assert!(!c2.is_cancelled());
        assert!(!tree.is_cancelled());
    }

    #[tokio::test]
    async fn cancel_unknown_agent_returns_false() {
        let tree = CancellationTree::new();
        assert!(!tree.cancel_agent("nonexistent"));
    }

    #[tokio::test]
    async fn child_for_same_agent_returns_same_token() {
        let tree = CancellationTree::new();
        let t1 = tree.child("a");
        let t2 = tree.child("a");
        // Same backing token — cancelling t1 also cancels t2
        t1.cancel();
        assert!(t2.is_cancelled());
        // Tree count is 1, not 2
        assert_eq!(tree.child_count(), 1);
    }

    #[tokio::test]
    async fn forget_removes_child() {
        let tree = CancellationTree::new();
        tree.child("a");
        tree.child("b");
        assert_eq!(tree.child_count(), 2);
        tree.forget("a");
        assert_eq!(tree.child_count(), 1);
    }

    #[tokio::test]
    async fn cancellation_token_select_aborts_long_future() {
        // Verifies the cooperative-cancellation pattern: select! between the
        // token and a long-running task makes the token win.
        let tree = CancellationTree::new();
        let token = tree.child("worker");
        let token_for_task = token.clone();
        // Spawn the cancellation in the background
        let cancel_handle = tokio::spawn({
            let tree = tree.clone();
            async move {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                tree.cancel_all();
            }
        });

        let result = tokio::select! {
            _ = token_for_task.cancelled() => "cancelled",
            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => "long-running",
        };

        cancel_handle.await.unwrap();
        assert_eq!(result, "cancelled");
    }

    #[tokio::test]
    async fn root_does_not_lose_cancellation_after_creating_more_children() {
        let tree = CancellationTree::new();
        let c1 = tree.child("a");
        tree.cancel_all();
        // Now create another child AFTER cancellation
        let c2 = tree.child("late");
        // Late child should already be cancelled (parent already cancelled)
        assert!(c1.is_cancelled());
        assert!(c2.is_cancelled());
    }

    #[tokio::test]
    async fn tree_is_clone_cheap() {
        let tree = CancellationTree::new();
        let clone = tree.clone();
        let _c = tree.child("x");
        // Clones share state
        assert_eq!(clone.child_count(), 1);
    }
}
