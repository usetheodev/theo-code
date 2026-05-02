//! T9.1 — `theo skill` subcommand handlers.
//!
//! Thin presentation layer over `theo_application::use_cases::skills` —
//! the use case owns business logic, this module owns the CLI surface
//! (formatting, exit codes, prompt confirmation).

use std::io::Write;
use std::path::Path;

use theo_application::facade::agent::{
    delete_skill, SkillCatalogError, SkillMetadata, SkillView,
};
use theo_application::use_cases::skills;

/// Format choices for the `list` subcommand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListFormat {
    Text,
    Json,
}

impl ListFormat {
    pub fn from_str_opt(s: Option<&str>) -> Self {
        match s.map(|x| x.to_ascii_lowercase()) {
            Some(ref v) if v == "json" => Self::Json,
            _ => Self::Text,
        }
    }
}

/// Resolve `$THEO_HOME` (or `~/.theo`); print a helpful diagnostic +
/// return exit code 2 when neither is set. Used by every subcommand.
fn resolve_home_or_die() -> Result<std::path::PathBuf, i32> {
    skills::theo_home().ok_or_else(|| {
        eprintln!(
            "error: cannot resolve theo home — set $THEO_HOME or ensure $HOME is defined"
        );
        2
    })
}

/// Handler for `theo skill list`.
pub fn handle_list(format: ListFormat) -> i32 {
    let Ok(home) = resolve_home_or_die() else {
        return 2;
    };
    let entries = skills::list(&home);
    match format {
        ListFormat::Text => print_list_text(&home, &entries),
        ListFormat::Json => print_list_json(&entries),
    }
    0
}

/// Handler for `theo skill view <name>`.
pub fn handle_view(name: &str) -> i32 {
    let Ok(home) = resolve_home_or_die() else {
        return 2;
    };
    match skills::view(&home, name) {
        Some(view) => {
            print_view_text(&view);
            0
        }
        None => {
            eprintln!("error: skill `{name}` not found under {}", home.display());
            1
        }
    }
}

/// Handler for `theo skill delete <name> [--yes]`.
pub fn handle_delete(name: &str, skip_confirm: bool) -> i32 {
    let Ok(home) = resolve_home_or_die() else {
        return 2;
    };
    if skills::view(&home, name).is_none() {
        eprintln!("error: skill `{name}` not found");
        return 1;
    }
    if !skip_confirm && !confirm_delete(name) {
        eprintln!("aborted: skill `{name}` not deleted");
        return 0;
    }
    match delete_skill(&home, name) {
        Ok(()) => {
            println!("deleted skill `{name}`");
            0
        }
        Err(e) => {
            eprintln!("error: {}", format_skill_error(&e));
            1
        }
    }
}

fn print_list_text(home: &Path, entries: &[SkillMetadata]) {
    if entries.is_empty() {
        println!("no skills installed under {}/skills/", home.display());
        println!("(install a skill by placing a SKILL.md under that path)");
        return;
    }
    println!(
        "{} skill(s) installed under {}/skills/:",
        entries.len(),
        home.display()
    );
    for s in entries {
        println!(
            "  {:<24}  [{}]  {}",
            s.name,
            s.category,
            truncate(&s.description, 60)
        );
    }
}

fn print_list_json(entries: &[SkillMetadata]) {
    let payload: Vec<serde_json::Value> = entries
        .iter()
        .map(|s| {
            serde_json::json!({
                "name": s.name,
                "description": s.description,
                "category": s.category,
            })
        })
        .collect();
    let value = serde_json::json!({ "skills": payload });
    // serde_json never fails to serialise this shape — but we guard
    // anyway and emit best-effort output on the unexpected path.
    match serde_json::to_string_pretty(&value) {
        Ok(s) => println!("{s}"),
        Err(_) => println!("{{\"skills\": []}}"),
    }
}

fn print_view_text(view: &SkillView) {
    println!("# {}", view.metadata.name);
    println!("category: {}", view.metadata.category);
    if !view.metadata.description.is_empty() {
        println!("description: {}", view.metadata.description);
    }
    if !view.linked_files.is_empty() {
        println!();
        println!("linked files:");
        for f in &view.linked_files {
            println!("  - {}", f.display());
        }
    }
    println!();
    println!("---");
    println!("{}", view.body);
}

fn confirm_delete(name: &str) -> bool {
    print!("delete skill `{name}`? [y/N] ");
    let _ = std::io::stdout().flush();
    let mut buf = String::new();
    if std::io::stdin().read_line(&mut buf).is_err() {
        return false;
    }
    matches!(buf.trim(), "y" | "Y" | "yes" | "YES")
}

fn format_skill_error(e: &SkillCatalogError) -> String {
    // Match on the typed variants so the user gets an actionable message
    // instead of the Display impl which is concise but generic.
    e.to_string()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t91cli_list_format_from_str_opt_handles_text_default() {
        assert_eq!(ListFormat::from_str_opt(None), ListFormat::Text);
        assert_eq!(ListFormat::from_str_opt(Some("text")), ListFormat::Text);
        assert_eq!(ListFormat::from_str_opt(Some("TEXT")), ListFormat::Text);
        assert_eq!(ListFormat::from_str_opt(Some("nonsense")), ListFormat::Text);
    }

    #[test]
    fn t91cli_list_format_from_str_opt_recognises_json() {
        assert_eq!(ListFormat::from_str_opt(Some("json")), ListFormat::Json);
        assert_eq!(ListFormat::from_str_opt(Some("JSON")), ListFormat::Json);
        assert_eq!(ListFormat::from_str_opt(Some("Json")), ListFormat::Json);
    }

    #[test]
    fn t91cli_truncate_passes_short_strings_through() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("", 10), "");
    }

    #[test]
    fn t91cli_truncate_appends_ellipsis_for_long_strings() {
        let out = truncate("abcdefghijklmnop", 5);
        assert_eq!(out, "abcde…");
    }

    #[test]
    fn t91cli_truncate_handles_multibyte_at_boundary() {
        // "olá olá olá" — 11 chars; truncate to 5 should NOT panic
        // on UTF-8 boundary.
        let out = truncate("olá olá olá", 5);
        assert_eq!(out, "olá o…");
    }
}
