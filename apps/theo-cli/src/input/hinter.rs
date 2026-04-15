//! Rustyline-style hints based on command history + known commands.

/// Suggest a completion hint for a partial input line.
///
/// Returns the **tail** that would complete the input (the portion
/// the user has not yet typed). If no unique suggestion is possible,
/// returns `None`.
pub fn hint_for(line: &str, commands: &[String]) -> Option<String> {
    if line.is_empty() {
        return None;
    }
    let matches: Vec<&String> = commands.iter().filter(|c| c.starts_with(line)).collect();
    if matches.len() == 1 && matches[0].len() > line.len() {
        Some(matches[0][line.len()..].to_string())
    } else {
        None
    }
}

/// Suggest a hint using a recent history as an additional source.
///
/// History entries that start with the current line are scored
/// before command matches. Commands act as a fallback.
pub fn hint_with_history(
    line: &str,
    history: &[String],
    commands: &[String],
) -> Option<String> {
    if line.is_empty() {
        return None;
    }
    // History priority: most recent first, still prefix match.
    for past in history.iter().rev() {
        if past.starts_with(line) && past.len() > line.len() {
            return Some(past[line.len()..].to_string());
        }
    }
    hint_for(line, commands)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmds() -> Vec<String> {
        vec![
            "/status".to_string(),
            "/skills".to_string(),
            "/help".to_string(),
            "/clear".to_string(),
        ]
    }

    #[test]
    fn test_empty_input_returns_none() {
        assert!(hint_for("", &cmds()).is_none());
    }

    #[test]
    fn test_unique_prefix_returns_tail() {
        let h = hint_for("/h", &cmds());
        assert_eq!(h, Some("elp".to_string()));
    }

    #[test]
    fn test_ambiguous_prefix_returns_none() {
        // /s matches both /status and /skills
        assert!(hint_for("/s", &cmds()).is_none());
    }

    #[test]
    fn test_no_match_returns_none() {
        assert!(hint_for("/xyz", &cmds()).is_none());
    }

    #[test]
    fn test_exact_match_returns_none() {
        // Nothing left to complete.
        assert!(hint_for("/clear", &cmds()).is_none());
    }

    #[test]
    fn test_history_match_takes_priority() {
        let history = vec!["fix bug in auth.rs".to_string()];
        let h = hint_with_history("fix bug", &history, &cmds());
        assert_eq!(h, Some(" in auth.rs".to_string()));
    }

    #[test]
    fn test_history_newest_wins() {
        let history = vec![
            "older command".to_string(),
            "new prompt text".to_string(),
        ];
        let h = hint_with_history("new", &history, &cmds());
        assert_eq!(h, Some(" prompt text".to_string()));
    }

    #[test]
    fn test_history_miss_falls_back_to_commands() {
        let history = vec!["unrelated".to_string()];
        let h = hint_with_history("/he", &history, &cmds());
        assert_eq!(h, Some("lp".to_string()));
    }

    #[test]
    fn test_history_empty_falls_back_to_commands() {
        let h = hint_with_history("/he", &[], &cmds());
        assert_eq!(h, Some("lp".to_string()));
    }

    #[test]
    fn test_empty_input_with_history_returns_none() {
        assert!(hint_with_history("", &["foo".into()], &cmds()).is_none());
    }
}
