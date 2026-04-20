//! Autocomplete engine for slash commands (/) and file mentions (@).
//!
//! Activates when user types `/` or `@` at the beginning of input or after space.
//! Shows a dropdown of matching candidates below the input area.

use std::path::Path;

/// An autocomplete candidate.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub text: String,
    pub description: String,
}

/// Autocomplete state.
#[derive(Debug)]
pub struct AutocompleteState {
    pub active: bool,
    pub trigger: AutocompleteTrigger,
    pub query: String,
    pub candidates: Vec<Candidate>,
    pub selected: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AutocompleteTrigger {
    Slash,
    AtFile,
}

impl AutocompleteState {
    pub fn new() -> Self {
        Self {
            active: false,
            trigger: AutocompleteTrigger::Slash,
            query: String::new(),
            candidates: Vec::new(),
            selected: 0,
        }
    }

    pub fn selected_text(&self) -> Option<&str> {
        self.candidates.get(self.selected).map(|c| c.text.as_str())
    }
}

/// All available slash commands.
pub fn slash_commands() -> Vec<Candidate> {
    vec![
        Candidate { text: "/help".into(), description: "Show keybindings and commands".into() },
        Candidate { text: "/status".into(), description: "Show provider, model, token info".into() },
        Candidate { text: "/clear".into(), description: "Clear transcript".into() },
        Candidate { text: "/export".into(), description: "Export session as markdown".into() },
        Candidate { text: "/mode".into(), description: "Cycle agent mode (agent/plan/ask)".into() },
        Candidate { text: "/quit".into(), description: "Exit theo".into() },
        Candidate { text: "/timeline".into(), description: "Show tool chain causality".into() },
        Candidate { text: "/theme".into(), description: "Change color theme".into() },
        Candidate { text: "/tab".into(), description: "Open new session tab".into() },
        Candidate { text: "/history".into(), description: "Search across all sessions".into() },
        Candidate { text: "/login".into(), description: "Show auth options".into() },
        Candidate { text: "/login https://api.opencode.ai".into(), description: "Login with your plan (device flow)".into() },
        Candidate { text: "/login sk-".into(), description: "Set API key directly".into() },
        Candidate { text: "/login device".into(), description: "OpenAI device flow".into() },
        Candidate { text: "/logout".into(), description: "Clear stored auth tokens".into() },
        Candidate { text: "/memory".into(), description: "Manage project memories (list/search/delete)".into() },
        Candidate { text: "/skills".into(), description: "List available skills".into() },
        Candidate { text: "/model".into(), description: "Switch LLM model".into() },
        Candidate { text: "/sidebar".into(), description: "Toggle sidebar panel".into() },
        Candidate { text: "/search".into(), description: "Search in transcript (Ctrl+F)".into() },
        Candidate { text: "/close".into(), description: "Close current tab".into() },
        Candidate { text: "/copy".into(), description: "Copy last response to clipboard".into() },
        Candidate { text: "/copy code".into(), description: "Copy last code block".into() },
        Candidate { text: "/select".into(), description: "Toggle mouse selection mode (Ctrl+Y)".into() },
    ]
}

/// Search for files matching a query in the project directory.
pub fn file_candidates(project_dir: &Path, query: &str) -> Vec<Candidate> {
    let mut results = Vec::new();
    let query_lower = query.to_lowercase();

    // Walk directory (max depth 4, max 50 results)
    if let Ok(entries) = walk_dir(project_dir, 4) {
        for entry in entries {
            let relative = entry.strip_prefix(project_dir).unwrap_or(&entry);
            let display = relative.to_string_lossy().to_string();

            if display.to_lowercase().contains(&query_lower) {
                results.push(Candidate {
                    text: format!("@{display}"),
                    description: String::new(),
                });
            }

            if results.len() >= 20 {
                break;
            }
        }
    }

    results
}

/// Fuzzy filter candidates by query.
pub fn filter_candidates(candidates: &[Candidate], query: &str) -> Vec<Candidate> {
    if query.is_empty() {
        return candidates.to_vec();
    }
    let q = query.to_lowercase();
    candidates
        .iter()
        .filter(|c| c.text.to_lowercase().contains(&q))
        .cloned()
        .collect()
}

/// Simple directory walker (no external deps, respects common ignores).
fn walk_dir(dir: &Path, max_depth: usize) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut results = Vec::new();
    walk_dir_inner(dir, dir, max_depth, 0, &mut results)?;
    Ok(results)
}

fn walk_dir_inner(
    root: &Path,
    dir: &Path,
    max_depth: usize,
    current_depth: usize,
    results: &mut Vec<std::path::PathBuf>,
) -> std::io::Result<()> {
    if current_depth > max_depth || results.len() >= 200 {
        return Ok(());
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden dirs, node_modules, target, .git
        if name.starts_with('.') || name == "node_modules" || name == "target" || name == "__pycache__" {
            continue;
        }

        if path.is_file() {
            results.push(path);
        } else if path.is_dir() {
            walk_dir_inner(root, &path, max_depth, current_depth + 1, results)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_commands_not_empty() {
        assert!(!slash_commands().is_empty());
    }

    #[test]
    fn filter_narrows_results() {
        let cmds = slash_commands();
        let filtered = filter_candidates(&cmds, "exp");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].text, "/export");
    }

    #[test]
    fn filter_empty_query_returns_all() {
        let cmds = slash_commands();
        let filtered = filter_candidates(&cmds, "");
        assert_eq!(filtered.len(), cmds.len());
    }

    #[test]
    fn filter_no_match_returns_empty() {
        let cmds = slash_commands();
        let filtered = filter_candidates(&cmds, "zzzzz");
        assert!(filtered.is_empty());
    }
}
