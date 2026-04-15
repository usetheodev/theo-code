//! Session management slash commands: /sessions, /tree, /fork, /compact.
//!
//! These are parsed from user input and dispatched by the REPL.
//! Actual execution logic lives in the application layer; this module
//! handles parsing and command identification only.
//!
//! Pi-mono ref: `packages/coding-agent/src/core/slash-commands.ts`

/// Available session commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionCommand {
    /// List recent sessions.
    Sessions,
    /// Show conversation tree with branches.
    Tree,
    /// Create a branch from a specific entry.
    Fork { entry_id: Option<String> },
    /// Force context compaction.
    Compact,
}

impl SessionCommand {
    /// Parse from slash command input.
    pub fn parse(input: &str) -> Option<Self> {
        let parts: Vec<&str> = input.trim().splitn(2, ' ').collect();
        let command = parts.first()?;
        match *command {
            "/sessions" => Some(SessionCommand::Sessions),
            "/tree" => Some(SessionCommand::Tree),
            "/fork" => Some(SessionCommand::Fork {
                entry_id: parts.get(1).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            }),
            "/compact" => Some(SessionCommand::Compact),
            _ => None,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            SessionCommand::Sessions => "/sessions",
            SessionCommand::Tree => "/tree",
            SessionCommand::Fork { .. } => "/fork",
            SessionCommand::Compact => "/compact",
        }
    }

    /// Help text for all session commands.
    pub fn help() -> &'static str {
        "/sessions  \u{2014} List recent sessions\n\
         /tree      \u{2014} Show conversation tree with branches\n\
         /fork [id] \u{2014} Branch from entry (or current position)\n\
         /compact   \u{2014} Force context compaction"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sessions() {
        let cmd = SessionCommand::parse("/sessions");
        assert_eq!(cmd, Some(SessionCommand::Sessions));
    }

    #[test]
    fn test_parse_tree() {
        let cmd = SessionCommand::parse("/tree");
        assert_eq!(cmd, Some(SessionCommand::Tree));
    }

    #[test]
    fn test_parse_fork_without_arg() {
        let cmd = SessionCommand::parse("/fork");
        assert_eq!(cmd, Some(SessionCommand::Fork { entry_id: None }));
    }

    #[test]
    fn test_parse_fork_with_arg() {
        let cmd = SessionCommand::parse("/fork abc123");
        assert_eq!(
            cmd,
            Some(SessionCommand::Fork {
                entry_id: Some("abc123".to_string())
            })
        );
    }

    #[test]
    fn test_parse_compact() {
        let cmd = SessionCommand::parse("/compact");
        assert_eq!(cmd, Some(SessionCommand::Compact));
    }

    #[test]
    fn test_parse_unknown_returns_none() {
        assert_eq!(SessionCommand::parse("/unknown"), None);
        assert_eq!(SessionCommand::parse("sessions"), None);
        assert_eq!(SessionCommand::parse(""), None);
    }

    #[test]
    fn test_parse_with_whitespace() {
        let cmd = SessionCommand::parse("  /sessions  ");
        assert_eq!(cmd, Some(SessionCommand::Sessions));
    }

    #[test]
    fn test_name_returns_slash_prefixed() {
        assert_eq!(SessionCommand::Sessions.name(), "/sessions");
        assert_eq!(SessionCommand::Tree.name(), "/tree");
        assert_eq!(SessionCommand::Compact.name(), "/compact");
        assert_eq!(
            SessionCommand::Fork { entry_id: None }.name(),
            "/fork"
        );
    }

    #[test]
    fn test_help_contains_all_commands() {
        let help = SessionCommand::help();
        assert!(help.contains("/sessions"));
        assert!(help.contains("/tree"));
        assert!(help.contains("/fork"));
        assert!(help.contains("/compact"));
    }
}
