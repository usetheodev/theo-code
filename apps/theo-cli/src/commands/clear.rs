//! `/clear` — clear the terminal screen and move cursor home.

use async_trait::async_trait;

use super::{CommandCategory, CommandContext, CommandOutcome, SlashCommand};

pub struct ClearCommand;

#[async_trait]
impl SlashCommand for ClearCommand {
    fn name(&self) -> &'static str {
        "clear"
    }
    fn category(&self) -> CommandCategory {
        CommandCategory::Session
    }
    async fn execute<'a>(&self, _args: &str, _ctx: &CommandContext<'a>) -> CommandOutcome {
        use std::io::Write;
        let _ = crossterm::execute!(
            std::io::stderr(),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
            crossterm::cursor::MoveTo(0, 0),
        );
        let _ = std::io::stderr().flush();
        CommandOutcome::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_is_clear() {
        assert_eq!(ClearCommand.name(), "clear");
    }

    #[test]
    fn test_category_is_session() {
        assert_eq!(ClearCommand.category(), CommandCategory::Session);
    }
}
