//! `/skills` — list registered skills.

use std::path::Path;

use async_trait::async_trait;
use theo_agent_runtime::skill::SkillRegistry;

use crate::render::style::{accent, bold};
use crate::tty::TtyCaps;

use super::{CommandCategory, CommandContext, CommandOutcome, SlashCommand};

pub struct SkillsCommand;

#[async_trait]
impl SlashCommand for SkillsCommand {
    fn name(&self) -> &'static str {
        "skills"
    }
    fn category(&self) -> CommandCategory {
        CommandCategory::Info
    }
    async fn execute<'a>(&self, _args: &str, ctx: &CommandContext<'a>) -> CommandOutcome {
        handle_skills(ctx.project_dir);
        CommandOutcome::Continue
    }
}

fn handle_skills(project_dir: &Path) {
    let caps = TtyCaps::detect().style_caps();
    let mut registry = SkillRegistry::new();
    registry.load_bundled();

    let project_skills = project_dir.join(".theo").join("skills");
    if project_skills.exists() {
        registry.load_from_dir(&project_skills);
    }

    let user_skills = crate::config::TheoPaths::resolve().skills();
    if user_skills.exists() {
        registry.load_from_dir(&user_skills);
    }

    let skills = registry.list();
    if skills.is_empty() {
        eprintln!("  No skills available.");
        return;
    }

    eprintln!("  {}", bold("Available Skills:", caps));
    for skill in skills {
        let mode_str = match &skill.mode {
            theo_agent_runtime::skill::SkillMode::InContext => "in-context",
            theo_agent_runtime::skill::SkillMode::SubAgent { role } => match role {
                theo_agent_runtime::subagent::SubAgentRole::Explorer => "sub-agent:explorer",
                theo_agent_runtime::subagent::SubAgentRole::Implementer => "sub-agent:implementer",
                theo_agent_runtime::subagent::SubAgentRole::Verifier => "sub-agent:verifier",
                theo_agent_runtime::subagent::SubAgentRole::Reviewer => "sub-agent:reviewer",
            },
        };
        eprintln!(
            "    {} ({}) — {}",
            accent(&skill.name, caps),
            mode_str,
            skill.trigger
        );
    }
    eprintln!("  ({} total)", skills.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_is_skills() {
        assert_eq!(SkillsCommand.name(), "skills");
    }

    #[test]
    fn test_category_is_info() {
        assert_eq!(SkillsCommand.category(), CommandCategory::Info);
    }
}
