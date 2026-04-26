//! Pi-Mono parallel-agent safety rules — injected into sub-agent system prompts
//! when worktree isolation is active.
//!
//! Reference: `referencias/pi-mono/AGENTS.md:194-233`. The rules forbid
//! destructive git operations that could clobber other agents' work.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IsolationMode {
    /// Sub-agent shares the parent's CWD (legacy / default).
    #[default]
    Shared,
    /// Sub-agent runs in an isolated git worktree.
    Worktree,
}

/// Returns the safety rules text to be injected into the sub-agent's system
/// prompt when `isolation: worktree` is active.
///
/// The rules are EXPLICIT (named in the prompt) so they can be referenced in
/// hook responses (PreToolUse {block, reason: "violates parallel-agent rule X"}).
pub fn safety_rules() -> &'static str {
    "PARALLEL-SAFETY RULES (active because you run in an isolated worktree):\n\
     - ONLY commit files you yourself created/modified in this worktree.\n\
     - NEVER run: git reset, git checkout (other branches), git stash pop, \
       git add -A.\n\
     - Use safe rebase only — `git rebase --abort` if conflicts arise.\n\
     - If you need files from another worktree, ASK the parent — do not \
       pull/fetch.\n\
     - Do NOT push to shared remotes without explicit parent approval."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolation_mode_default_is_shared() {
        assert_eq!(IsolationMode::default(), IsolationMode::Shared);
    }

    #[test]
    fn safety_rules_mention_forbidden_git_commands() {
        let rules = safety_rules();
        assert!(rules.contains("git reset"));
        assert!(rules.contains("git checkout"));
        assert!(rules.contains("git stash pop"));
        assert!(rules.contains("git add -A"));
    }

    #[test]
    fn safety_rules_mention_only_your_files() {
        assert!(safety_rules().contains("ONLY commit files you yourself"));
    }

    #[test]
    fn isolation_mode_serde_roundtrip() {
        for mode in [IsolationMode::Shared, IsolationMode::Worktree] {
            let s = serde_json::to_string(&mode).unwrap();
            let back: IsolationMode = serde_json::from_str(&s).unwrap();
            assert_eq!(back, mode);
        }
    }
}
