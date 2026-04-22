//! Session-scoped ACL that remembers `Always` / `Deny always` decisions.
//!
//! The session is keyed by `(tool_name, summary_key)`. Any request
//! matching a persistent decision skips the prompt entirely.

#![allow(dead_code)] // Scaffolded helpers — kept for upcoming TUI features.
use std::collections::HashMap;
use std::sync::RwLock;

use super::prompt::{PermissionDecision, PermissionRequest};

/// Outcome of a session permission check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionOutcome {
    /// Previously approved — allow without prompting.
    Allow,
    /// Previously denied — deny without prompting.
    Deny,
    /// No persistent decision exists — caller must prompt.
    NeedsPrompt,
}

#[derive(Debug, Default)]
pub struct PermissionSession {
    acl: RwLock<HashMap<String, PermissionDecision>>,
}

impl PermissionSession {
    pub fn new() -> Self {
        Self {
            acl: RwLock::new(HashMap::new()),
        }
    }

    /// Produce a key that groups similar tool calls.
    ///
    /// For `bash` we key on the first word of the summary so that
    /// `bash: ls -la` and `bash: ls /etc` share a decision.
    pub fn key_for(req: &PermissionRequest) -> String {
        let action_seed = req
            .summary
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string();
        format!("{}:{}", req.tool, action_seed)
    }

    /// Check for a previous decision. Does NOT prompt.
    pub fn check(&self, req: &PermissionRequest) -> SessionOutcome {
        let key = Self::key_for(req);
        let acl = self.acl.read().expect("poisoned acl lock");
        match acl.get(&key) {
            Some(PermissionDecision::Always) => SessionOutcome::Allow,
            Some(PermissionDecision::DenyAlways) => SessionOutcome::Deny,
            _ => SessionOutcome::NeedsPrompt,
        }
    }

    /// Remember a decision if it is persistent.
    pub fn remember(&self, req: &PermissionRequest, decision: PermissionDecision) {
        if !decision.is_persistent() {
            return;
        }
        let key = Self::key_for(req);
        let mut acl = self.acl.write().expect("poisoned acl lock");
        acl.insert(key, decision);
    }

    /// Total number of persistent decisions stored.
    pub fn size(&self) -> usize {
        self.acl.read().map(|a| a.len()).unwrap_or(0)
    }

    /// Clear all persistent decisions.
    pub fn clear(&self) {
        if let Ok(mut acl) = self.acl.write() {
            acl.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(tool: &str, summary: &str) -> PermissionRequest {
        PermissionRequest {
            tool: tool.to_string(),
            summary: summary.to_string(),
        }
    }

    #[test]
    fn test_new_session_is_empty() {
        let s = PermissionSession::new();
        assert_eq!(s.size(), 0);
    }

    #[test]
    fn test_fresh_request_needs_prompt() {
        let s = PermissionSession::new();
        assert_eq!(
            s.check(&req("bash", "ls /tmp")),
            SessionOutcome::NeedsPrompt
        );
    }

    #[test]
    fn test_remember_always_returns_allow() {
        let s = PermissionSession::new();
        let r = req("bash", "ls /tmp");
        s.remember(&r, PermissionDecision::Always);
        assert_eq!(s.check(&r), SessionOutcome::Allow);
    }

    #[test]
    fn test_remember_deny_always_returns_deny() {
        let s = PermissionSession::new();
        let r = req("bash", "rm /etc/passwd");
        s.remember(&r, PermissionDecision::DenyAlways);
        assert_eq!(s.check(&r), SessionOutcome::Deny);
    }

    #[test]
    fn test_yes_decision_is_not_persisted() {
        let s = PermissionSession::new();
        let r = req("bash", "ls /tmp");
        s.remember(&r, PermissionDecision::Yes);
        assert_eq!(s.check(&r), SessionOutcome::NeedsPrompt);
    }

    #[test]
    fn test_no_decision_is_not_persisted() {
        let s = PermissionSession::new();
        let r = req("bash", "ls /tmp");
        s.remember(&r, PermissionDecision::No);
        assert_eq!(s.check(&r), SessionOutcome::NeedsPrompt);
    }

    #[test]
    fn test_same_tool_same_first_word_shares_decision() {
        let s = PermissionSession::new();
        s.remember(&req("bash", "ls -la"), PermissionDecision::Always);
        // Different args but same first word → same ACL entry.
        assert_eq!(
            s.check(&req("bash", "ls /etc")),
            SessionOutcome::Allow
        );
    }

    #[test]
    fn test_different_tool_has_independent_decision() {
        let s = PermissionSession::new();
        s.remember(&req("bash", "ls -la"), PermissionDecision::Always);
        assert_eq!(
            s.check(&req("read", "ls -la")),
            SessionOutcome::NeedsPrompt
        );
    }

    #[test]
    fn test_key_format() {
        let r = req("bash", "rm -rf /tmp/foo");
        assert_eq!(PermissionSession::key_for(&r), "bash:rm");
    }

    #[test]
    fn test_clear_removes_all_decisions() {
        let s = PermissionSession::new();
        s.remember(&req("bash", "ls"), PermissionDecision::Always);
        assert_eq!(s.size(), 1);
        s.clear();
        assert_eq!(s.size(), 0);
    }

    #[test]
    fn test_size_reflects_persistent_count() {
        let s = PermissionSession::new();
        s.remember(&req("bash", "ls"), PermissionDecision::Always);
        s.remember(&req("bash", "rm foo"), PermissionDecision::DenyAlways);
        s.remember(&req("read", "x"), PermissionDecision::Yes); // not persisted
        assert_eq!(s.size(), 2);
    }
}
