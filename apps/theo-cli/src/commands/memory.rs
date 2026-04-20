//! `/memory list|search|delete` — project memory store operations.

use std::path::Path;

use async_trait::async_trait;

use crate::render::style::accent;
use crate::tty::TtyCaps;

use super::{CommandCategory, CommandContext, CommandOutcome, SlashCommand};

pub struct MemoryCommand;

#[async_trait]
impl SlashCommand for MemoryCommand {
    fn name(&self) -> &'static str {
        "memory"
    }
    fn category(&self) -> CommandCategory {
        CommandCategory::Info
    }
    async fn execute<'a>(&self, args: &str, ctx: &CommandContext<'a>) -> CommandOutcome {
        let parts: Vec<&str> = args.splitn(2, ' ').collect();
        let action = parts.first().copied().unwrap_or("list").trim();
        let arg = parts.get(1).copied().map(str::trim);
        handle(action, arg, ctx.project_dir).await;
        CommandOutcome::Continue
    }
}

async fn handle(action: &str, arg: Option<&str>, project_dir: &Path) {
    let caps = TtyCaps::detect().style_caps();
    let memory_root = memory_root();
    let store = make_store(&memory_root, project_dir);

    match action {
        "list" | "" => match store.list().await {
            Ok(memories) if memories.is_empty() => eprintln!("  No memories for this project."),
            Ok(memories) => {
                for m in &memories {
                    eprintln!("  {}: {}", accent(&m.key, caps), m.value);
                }
                eprintln!("  ({} total)", memories.len());
            }
            Err(e) => eprintln!("  Error: {e}"),
        },
        "search" => {
            let query = arg.unwrap_or("");
            if query.is_empty() {
                eprintln!("  Usage: /memory search <query>");
                return;
            }
            match store.search(query).await {
                Ok(results) if results.is_empty() => {
                    eprintln!("  No memories matching '{query}'");
                }
                Ok(results) => {
                    for m in &results {
                        eprintln!("  {}: {}", accent(&m.key, caps), m.value);
                    }
                }
                Err(e) => eprintln!("  Error: {e}"),
            }
        }
        "delete" => {
            let key = arg.unwrap_or("");
            if key.is_empty() {
                eprintln!("  Usage: /memory delete <key>");
                return;
            }
            match store.delete(key).await {
                Ok(true) => eprintln!("  Deleted: {key}"),
                Ok(false) => eprintln!("  Not found: {key}"),
                Err(e) => eprintln!("  Error: {e}"),
            }
        }
        _ => {
            eprintln!("  Usage: /memory [list|search <query>|delete <key>]");
        }
    }
}

/// Construct a FileMemoryStore. Isolated in its own function so the
/// infra import lives in one place per ADR-004.
fn make_store(
    root: &Path,
    project_dir: &Path,
) -> theo_tooling::memory::FileMemoryStore {
    theo_tooling::memory::FileMemoryStore::for_project(root, project_dir)
}

/// Root dir for memory storage. Uses `TheoPaths::memory()` which
/// respects XDG + `THEO_HOME`.
fn memory_root() -> std::path::PathBuf {
    crate::config::TheoPaths::resolve().memory()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_is_memory() {
        assert_eq!(MemoryCommand.name(), "memory");
    }

    #[test]
    fn test_category_is_info() {
        assert_eq!(MemoryCommand.category(), CommandCategory::Info);
    }

    #[test]
    fn test_memory_root_is_xdg_compliant() {
        let path = memory_root();
        let s = path.display().to_string();
        assert!(s.contains("theo"), "expected theo/ in {s}");
    }
}
