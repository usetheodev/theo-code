//! `/help` — grouped list of all commands.

use async_trait::async_trait;

use crate::render::style::bold;
use crate::tty::TtyCaps;

use super::{CommandCategory, CommandContext, CommandOutcome, SlashCommand};

pub struct HelpCommand;

#[async_trait]
impl SlashCommand for HelpCommand {
    fn name(&self) -> &'static str {
        "help"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["h"]
    }
    fn description(&self) -> &'static str {
        "Show available commands grouped by category"
    }
    fn category(&self) -> CommandCategory {
        CommandCategory::Info
    }
    async fn execute<'a>(&self, _args: &str, _ctx: &CommandContext<'a>) -> CommandOutcome {
        print_help();
        CommandOutcome::Continue
    }
}

fn print_help() {
    let c = TtyCaps::detect().style_caps();
    eprintln!("  {}", bold("Session", c));
    eprintln!("    /exit /quit /q        Exit the REPL");
    eprintln!("    /clear                Clear the terminal");
    eprintln!();
    eprintln!("  {}", bold("Info", c));
    eprintln!("    /help /h              Show this help");
    eprintln!("    /status               Provider, model, project info");
    eprintln!("    /cost                 Token and cost tracking");
    eprintln!("    /skills               List available skills");
    eprintln!("    /memory list|search|delete");
    eprintln!();
    eprintln!("  {}", bold("Config", c));
    eprintln!("    /model [name]         Show or switch LLM model");
    eprintln!("    /mode agent|plan|ask  Change interaction mode");
    eprintln!();
    eprintln!("  {}", bold("Action", c));
    eprintln!("    /doctor               Environment diagnostics");
    eprintln!();
    eprintln!("  Type any text to send a task to the agent.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_is_help() {
        assert_eq!(HelpCommand.name(), "help");
    }

    #[test]
    fn test_h_is_alias() {
        assert!(HelpCommand.aliases().contains(&"h"));
    }

    #[test]
    fn test_category_is_info() {
        assert_eq!(HelpCommand.category(), CommandCategory::Info);
    }
}
