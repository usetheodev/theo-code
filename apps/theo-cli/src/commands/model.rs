//! `/model [name]` — show current model or switch.

use async_trait::async_trait;

use super::{CommandCategory, CommandContext, CommandOutcome, SlashCommand};

pub struct ModelCommand;

#[async_trait]
impl SlashCommand for ModelCommand {
    fn name(&self) -> &'static str {
        "model"
    }
    fn category(&self) -> CommandCategory {
        CommandCategory::Config
    }
    async fn execute<'a>(&self, args: &str, ctx: &CommandContext<'a>) -> CommandOutcome {
        let trimmed = args.trim();
        if trimmed.is_empty() {
            eprintln!("  Current model: {}", ctx.config.model);
            eprintln!("  Usage: /model <name>");
            eprintln!("  Note: runtime switching requires REPL restart in this build.");
        } else {
            // Full runtime switching is in T2.2b / F2 polish. For now we
            // surface the intent and tell the user to restart.
            eprintln!(
                "  Requested model: {trimmed} (current: {})",
                ctx.config.model
            );
            eprintln!("  Set $THEO_MODEL={trimmed} and restart to apply.");
        }
        CommandOutcome::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_is_model() {
        assert_eq!(ModelCommand.name(), "model");
    }

    #[test]
    fn test_category_is_config() {
        assert_eq!(ModelCommand.category(), CommandCategory::Config);
    }
}
