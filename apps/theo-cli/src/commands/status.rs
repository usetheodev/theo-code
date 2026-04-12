//! `/status` — provider, model, project info as an aligned table.

use async_trait::async_trait;

use crate::render::table::{kv_table, render_table};
use crate::tty::TtyCaps;

use super::{CommandCategory, CommandContext, CommandOutcome, SlashCommand};

pub struct StatusCommand;

#[async_trait]
impl SlashCommand for StatusCommand {
    fn name(&self) -> &'static str {
        "status"
    }
    fn description(&self) -> &'static str {
        "Show provider, model, and project info"
    }
    fn category(&self) -> CommandCategory {
        CommandCategory::Info
    }
    async fn execute<'a>(&self, _args: &str, ctx: &CommandContext<'a>) -> CommandOutcome {
        let caps = TtyCaps::detect().style_caps();
        let rows = build_rows(ctx);
        let table = kv_table(&rows, caps);
        eprintln!("{}", render_table(&table));
        CommandOutcome::Continue
    }
}

pub(super) fn build_rows(ctx: &CommandContext<'_>) -> Vec<(String, String)> {
    vec![
        ("Provider".to_string(), ctx.provider_name.to_string()),
        ("Model".to_string(), ctx.config.model.clone()),
        (
            "Project".to_string(),
            ctx.project_dir.display().to_string(),
        ),
        (
            "Max iterations".to_string(),
            ctx.config.max_iterations.to_string(),
        ),
        ("Mode".to_string(), format!("{}", ctx.config.mode)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use theo_agent_runtime::AgentConfig;

    fn ctx<'a>(cfg: &'a AgentConfig, dir: &'a Path) -> CommandContext<'a> {
        CommandContext {
            config: cfg,
            project_dir: dir,
            provider_name: "openai",
        }
    }

    #[test]
    fn test_name_is_status() {
        assert_eq!(StatusCommand.name(), "status");
    }

    #[test]
    fn test_build_rows_contains_expected_keys() {
        let cfg = AgentConfig::default();
        let dir = Path::new("/tmp/test");
        let c = ctx(&cfg, dir);
        let rows = build_rows(&c);
        let keys: Vec<&str> = rows.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"Provider"));
        assert!(keys.contains(&"Model"));
        assert!(keys.contains(&"Project"));
        assert!(keys.contains(&"Max iterations"));
        assert!(keys.contains(&"Mode"));
    }

    #[test]
    fn test_build_rows_provider_matches_ctx() {
        let cfg = AgentConfig::default();
        let dir = Path::new("/tmp");
        let c = ctx(&cfg, dir);
        let rows = build_rows(&c);
        let provider = rows
            .iter()
            .find(|(k, _)| k == "Provider")
            .map(|(_, v)| v.as_str())
            .unwrap_or("");
        assert_eq!(provider, "openai");
    }
}
