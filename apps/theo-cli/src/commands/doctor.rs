//! `/doctor` — environment and dependency diagnostics.

use async_trait::async_trait;

use crate::render::style::{StyleCaps, cross_symbol, error, success};
use crate::tty::TtyCaps;

use super::{CommandCategory, CommandContext, CommandOutcome, SlashCommand};

pub struct DoctorCommand;

#[async_trait]
impl SlashCommand for DoctorCommand {
    fn name(&self) -> &'static str {
        "doctor"
    }
    fn description(&self) -> &'static str {
        "Run environment and configuration diagnostics"
    }
    fn category(&self) -> CommandCategory {
        CommandCategory::Action
    }
    async fn execute<'a>(&self, _args: &str, ctx: &CommandContext<'a>) -> CommandOutcome {
        let caps = TtyCaps::detect().style_caps();
        let checks = run_checks(ctx);
        print_checks(&checks, caps);
        CommandOutcome::Continue
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub details: String,
}

pub fn run_checks(ctx: &CommandContext<'_>) -> Vec<Check> {
    let mut checks = Vec::new();

    // Project dir exists
    let dir = ctx.project_dir;
    checks.push(Check {
        name: "Project dir",
        ok: dir.exists() && dir.is_dir(),
        details: dir.display().to_string(),
    });

    // Writable?
    let probe = dir.join(".theo-doctor-probe");
    let writable = std::fs::write(&probe, b"theo").is_ok();
    let _ = std::fs::remove_file(&probe);
    checks.push(Check {
        name: "Writable",
        ok: writable,
        details: if writable {
            "yes".to_string()
        } else {
            "read-only".to_string()
        },
    });

    // HOME env set
    checks.push(Check {
        name: "HOME env",
        ok: std::env::var_os("HOME").is_some(),
        details: std::env::var("HOME").unwrap_or_else(|_| "(unset)".to_string()),
    });

    // Provider configured
    checks.push(Check {
        name: "Provider",
        ok: !ctx.provider_name.is_empty(),
        details: ctx.provider_name.to_string(),
    });

    // Model set
    checks.push(Check {
        name: "Model",
        ok: !ctx.config.model.is_empty(),
        details: ctx.config.model.clone(),
    });

    // Sandbox capability probed via env var hint
    let sandbox_hint = if cfg!(target_os = "linux") {
        "linux (bwrap/landlock)".to_string()
    } else {
        "non-linux (noop sandbox)".to_string()
    };
    checks.push(Check {
        name: "Sandbox",
        ok: true,
        details: sandbox_hint,
    });

    checks
}

pub fn print_checks(checks: &[Check], caps: StyleCaps) {
    for c in checks {
        let glyph = if c.ok {
            success(crate::render::style::check_symbol(caps), caps).to_string()
        } else {
            error(cross_symbol(caps), caps).to_string()
        };
        eprintln!("  {glyph}  {:<14} {}", c.name, c.details);
    }
    let failing = checks.iter().filter(|c| !c.ok).count();
    if failing == 0 {
        eprintln!("\n  {} All checks passed.", success("✓", caps));
    } else {
        eprintln!(
            "\n  {} {failing} check(s) failed.",
            error("✗", caps)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use theo_agent_runtime::AgentConfig;

    #[test]
    fn test_name_is_doctor() {
        assert_eq!(DoctorCommand.name(), "doctor");
    }

    #[test]
    fn test_category_is_action() {
        assert_eq!(DoctorCommand.category(), CommandCategory::Action);
    }

    #[test]
    fn test_run_checks_returns_all_named_checks() {
        let cfg = AgentConfig::default();
        let dir = Path::new(".");
        let ctx = CommandContext {
            config: &cfg,
            project_dir: dir,
            provider_name: "openai",
        };
        let checks = run_checks(&ctx);
        let names: Vec<&str> = checks.iter().map(|c| c.name).collect();
        assert!(names.contains(&"Project dir"));
        assert!(names.contains(&"Writable"));
        assert!(names.contains(&"HOME env"));
        assert!(names.contains(&"Provider"));
        assert!(names.contains(&"Model"));
        assert!(names.contains(&"Sandbox"));
    }

    #[test]
    fn test_run_checks_passes_for_valid_project() {
        let cfg = AgentConfig::default();
        let dir = Path::new(".");
        let ctx = CommandContext {
            config: &cfg,
            project_dir: dir,
            provider_name: "openai",
        };
        let checks = run_checks(&ctx);
        // Project dir and provider should pass
        let by_name: std::collections::HashMap<&str, bool> =
            checks.iter().map(|c| (c.name, c.ok)).collect();
        assert_eq!(by_name.get("Project dir"), Some(&true));
        assert_eq!(by_name.get("Provider"), Some(&true));
    }

    #[test]
    fn test_empty_provider_fails_provider_check() {
        let cfg = AgentConfig::default();
        let dir = Path::new(".");
        let ctx = CommandContext {
            config: &cfg,
            project_dir: dir,
            provider_name: "",
        };
        let checks = run_checks(&ctx);
        let prov = checks.iter().find(|c| c.name == "Provider").unwrap();
        assert!(!prov.ok);
    }
}
