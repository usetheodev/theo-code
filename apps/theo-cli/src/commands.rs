//! Slash command handlers for the REPL.

use std::path::Path;

use theo_agent_runtime::AgentConfig;
use theo_agent_runtime::skill::SkillRegistry;
use theo_tooling::memory::FileMemoryStore;

/// Handle a slash command. Returns true if the REPL should exit.
pub async fn handle_command(
    input: &str,
    config: &AgentConfig,
    project_dir: &Path,
    provider_name: &str,
) -> bool {
    let parts: Vec<&str> = input.splitn(3, ' ').collect();
    let cmd = parts[0];

    match cmd {
        "/exit" | "/quit" | "/q" => {
            eprintln!("Goodbye.");
            return true;
        }
        "/help" | "/h" => {
            print_help();
        }
        "/clear" => {
            // ANSI escape to clear terminal
            eprint!("\x1b[2J\x1b[1;1H");
        }
        "/status" => {
            eprintln!("  Provider: {provider_name}");
            eprintln!("  Model: {}", config.model);
            eprintln!("  Project: {}", project_dir.display());
            eprintln!("  Max iterations: {}", config.max_iterations);

            // Memory count
            let memory_root = memory_root();
            let store = FileMemoryStore::for_project(&memory_root, project_dir);
            match store.list().await {
                Ok(memories) => eprintln!("  Memories: {}", memories.len()),
                Err(_) => eprintln!("  Memories: (error reading)"),
            }
        }
        "/memory" => {
            let action = parts.get(1).copied().unwrap_or("list");
            handle_memory(action, parts.get(2).copied(), project_dir).await;
        }
        "/skills" => {
            handle_skills(project_dir);
        }
        _ => {
            eprintln!("  Unknown command: {cmd}");
            eprintln!("  Type /help for available commands.");
        }
    }

    false
}

async fn handle_memory(action: &str, arg: Option<&str>, project_dir: &Path) {
    let memory_root = memory_root();
    let store = FileMemoryStore::for_project(&memory_root, project_dir);

    match action {
        "list" => match store.list().await {
            Ok(memories) if memories.is_empty() => {
                eprintln!("  No memories for this project.");
            }
            Ok(memories) => {
                for m in &memories {
                    eprintln!("  \x1b[36m{}\x1b[0m: {}", m.key, m.value);
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
                        eprintln!("  \x1b[36m{}\x1b[0m: {}", m.key, m.value);
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

fn handle_skills(project_dir: &Path) {
    let mut registry = SkillRegistry::new();
    registry.load_bundled();

    let project_skills = project_dir.join(".theo").join("skills");
    if project_skills.exists() {
        registry.load_from_dir(&project_skills);
    }

    let user_skills = std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"))
        .join(".config")
        .join("theo")
        .join("skills");
    if user_skills.exists() {
        registry.load_from_dir(&user_skills);
    }

    let skills = registry.list();
    if skills.is_empty() {
        eprintln!("  No skills available.");
        return;
    }

    eprintln!("  \x1b[1mAvailable Skills:\x1b[0m");
    for skill in skills {
        let mode_str = match &skill.mode {
            theo_agent_runtime::skill::SkillMode::InContext => "in-context",
            theo_agent_runtime::skill::SkillMode::SubAgent { role } => {
                // Can't easily return &str from format!, use leak-free approach
                match role {
                    theo_agent_runtime::subagent::SubAgentRole::Explorer => "sub-agent:explorer",
                    theo_agent_runtime::subagent::SubAgentRole::Implementer => "sub-agent:implementer",
                    theo_agent_runtime::subagent::SubAgentRole::Verifier => "sub-agent:verifier",
                    theo_agent_runtime::subagent::SubAgentRole::Reviewer => "sub-agent:reviewer",
                }
            }
        };
        eprintln!(
            "    \x1b[36m{}\x1b[0m ({}) — {}",
            skill.name, mode_str, skill.trigger
        );
    }
    eprintln!("  ({} total)", skills.len());
}

fn print_help() {
    eprintln!("  \x1b[1mCommands:\x1b[0m");
    eprintln!("    /help              Show this help");
    eprintln!("    /exit              Exit the REPL");
    eprintln!("    /clear             Clear the terminal");
    eprintln!("    /status            Show provider, model, project info");
    eprintln!("    /memory list       List all memories for this project");
    eprintln!("    /memory search Q   Search memories by keyword");
    eprintln!("    /memory delete K   Delete a memory by key");
    eprintln!("    /skills            List available skills");
    eprintln!("    /mode              Show current mode");
    eprintln!("    /mode <agent|plan|ask>  Change interaction mode");
    eprintln!();
    eprintln!("  \x1b[1mModes:\x1b[0m");
    eprintln!("    agent              Full autonomy — act immediately (default)");
    eprintln!("    plan               Create plan first, wait for approval");
    eprintln!("    ask                Ask clarifying questions first");
    eprintln!();
    eprintln!("  \x1b[1mAgent:\x1b[0m");
    eprintln!("    Type any text to send a task to the agent.");
    eprintln!("    The agent will read, edit, and verify files in the project.");
}

fn memory_root() -> std::path::PathBuf {
    std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"))
        .join(".config")
        .join("theo")
        .join("memory")
}
