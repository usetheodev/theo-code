//! Slash command registry and dispatcher.
//!
//! The registry pattern lets each command live in its own file and
//! be added with a single line in [`build_registry`]. Adding a new
//! command is: 1) create a file, 2) register it here, 3) write tests.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use theo_agent_runtime::AgentConfig;

mod clear;
mod cost;
mod doctor;
mod help;
mod memory;
mod model;
mod skills;
mod status;

pub use clear::ClearCommand;
pub use cost::CostCommand;
pub use doctor::DoctorCommand;
pub use help::HelpCommand;
pub use memory::MemoryCommand;
pub use model::ModelCommand;
pub use skills::SkillsCommand;
pub use status::StatusCommand;

/// Category used to group commands in `/help`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandCategory {
    Session,
    Info,
    Config,
    Action,
}

impl CommandCategory {
    pub fn label(&self) -> &'static str {
        match self {
            CommandCategory::Session => "Session",
            CommandCategory::Info => "Info",
            CommandCategory::Config => "Config",
            CommandCategory::Action => "Action",
        }
    }
}

/// Contextual runtime state passed to each command.
pub struct CommandContext<'a> {
    pub config: &'a AgentConfig,
    pub project_dir: &'a Path,
    pub provider_name: &'a str,
}

/// Outcome of dispatching a slash command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandOutcome {
    /// Command handled, REPL continues.
    Continue,
    /// REPL should exit.
    Exit,
    /// Command not recognized.
    Unknown,
}

/// A slash command. Implementations live in this module's children.
#[async_trait]
pub trait SlashCommand: Send + Sync {
    /// Primary command name (without the leading `/`).
    fn name(&self) -> &'static str;
    /// Additional aliases (without the leading `/`).
    fn aliases(&self) -> &'static [&'static str] {
        &[]
    }
    /// Short description for `/help`.
    fn description(&self) -> &'static str;
    /// Category used to group commands in `/help`.
    fn category(&self) -> CommandCategory;
    /// Execute the command with its arguments (excluding the name).
    async fn execute<'a>(&self, args: &str, ctx: &CommandContext<'a>) -> CommandOutcome;
}

/// Command registry — built once, reused per REPL turn.
pub struct CommandRegistry {
    commands: Vec<Arc<dyn SlashCommand>>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub fn register(&mut self, cmd: Arc<dyn SlashCommand>) {
        self.commands.push(cmd);
    }

    /// All commands in registration order.
    pub fn all(&self) -> &[Arc<dyn SlashCommand>] {
        &self.commands
    }

    /// Look up a command by name or alias (input already stripped of `/`).
    pub fn find(&self, name: &str) -> Option<&Arc<dyn SlashCommand>> {
        self.commands
            .iter()
            .find(|c| c.name() == name || c.aliases().contains(&name))
    }

    /// Dispatch a user line like `/status`. Returns the outcome.
    /// Input should include the leading slash; it is stripped internally.
    pub async fn dispatch<'a>(
        &self,
        line: &str,
        ctx: &CommandContext<'a>,
    ) -> CommandOutcome {
        let line = line.strip_prefix('/').unwrap_or(line);
        let (name, args) = match line.find(' ') {
            Some(i) => (&line[..i], line[i + 1..].trim()),
            None => (line.trim(), ""),
        };
        match self.find(name) {
            Some(cmd) => cmd.execute(args, ctx).await,
            None => CommandOutcome::Unknown,
        }
    }

    /// Return names sorted, for tab completion.
    pub fn names(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .commands
            .iter()
            .map(|c| format!("/{}", c.name()))
            .collect();
        out.sort();
        out
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the default command registry.
pub fn build_registry() -> CommandRegistry {
    let mut r = CommandRegistry::new();
    r.register(Arc::new(HelpCommand));
    r.register(Arc::new(StatusCommand));
    r.register(Arc::new(ClearCommand));
    r.register(Arc::new(ModelCommand));
    r.register(Arc::new(CostCommand));
    r.register(Arc::new(DoctorCommand));
    r.register(Arc::new(MemoryCommand));
    r.register(Arc::new(SkillsCommand));
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_ctx<'a>(
        config: &'a AgentConfig,
        dir: &'a Path,
    ) -> CommandContext<'a> {
        CommandContext {
            config,
            project_dir: dir,
            provider_name: "test",
        }
    }

    struct Stub(&'static str);

    #[async_trait]
    impl SlashCommand for Stub {
        fn name(&self) -> &'static str {
            self.0
        }
        fn description(&self) -> &'static str {
            "stub"
        }
        fn category(&self) -> CommandCategory {
            CommandCategory::Action
        }
        async fn execute<'a>(&self, _args: &str, _ctx: &CommandContext<'a>) -> CommandOutcome {
            CommandOutcome::Continue
        }
    }

    #[test]
    fn test_empty_registry_find_returns_none() {
        let r = CommandRegistry::new();
        assert!(r.find("status").is_none());
    }

    #[test]
    fn test_register_and_find_by_name() {
        let mut r = CommandRegistry::new();
        r.register(Arc::new(Stub("foo")));
        assert!(r.find("foo").is_some());
    }

    #[test]
    fn test_find_returns_none_for_unknown() {
        let mut r = CommandRegistry::new();
        r.register(Arc::new(Stub("foo")));
        assert!(r.find("bar").is_none());
    }

    #[tokio::test]
    async fn test_dispatch_unknown_returns_unknown() {
        let r = CommandRegistry::new();
        let cfg = AgentConfig::default();
        let dir = Path::new(".");
        let ctx = dummy_ctx(&cfg, dir);
        assert_eq!(r.dispatch("/xyzzy", &ctx).await, CommandOutcome::Unknown);
    }

    #[tokio::test]
    async fn test_dispatch_known_returns_continue() {
        let mut r = CommandRegistry::new();
        r.register(Arc::new(Stub("foo")));
        let cfg = AgentConfig::default();
        let dir = Path::new(".");
        let ctx = dummy_ctx(&cfg, dir);
        assert_eq!(r.dispatch("/foo", &ctx).await, CommandOutcome::Continue);
    }

    #[tokio::test]
    async fn test_dispatch_strips_slash() {
        let mut r = CommandRegistry::new();
        r.register(Arc::new(Stub("foo")));
        let cfg = AgentConfig::default();
        let dir = Path::new(".");
        let ctx = dummy_ctx(&cfg, dir);
        // Without slash should also work (permissive).
        assert_eq!(r.dispatch("foo", &ctx).await, CommandOutcome::Continue);
    }

    #[tokio::test]
    async fn test_dispatch_with_args_splits_correctly() {
        struct Echo;
        #[async_trait::async_trait]
        impl SlashCommand for Echo {
            fn name(&self) -> &'static str {
                "echo"
            }
            fn description(&self) -> &'static str {
                "echo"
            }
            fn category(&self) -> CommandCategory {
                CommandCategory::Info
            }
            async fn execute<'a>(
                &self,
                args: &str,
                _ctx: &CommandContext<'a>,
            ) -> CommandOutcome {
                if args == "hello world" {
                    CommandOutcome::Continue
                } else {
                    CommandOutcome::Unknown
                }
            }
        }
        let mut r = CommandRegistry::new();
        r.register(Arc::new(Echo));
        let cfg = AgentConfig::default();
        let dir = Path::new(".");
        let ctx = dummy_ctx(&cfg, dir);
        assert_eq!(
            r.dispatch("/echo hello world", &ctx).await,
            CommandOutcome::Continue
        );
    }

    #[test]
    fn test_names_are_sorted() {
        let mut r = CommandRegistry::new();
        r.register(Arc::new(Stub("zeta")));
        r.register(Arc::new(Stub("alpha")));
        r.register(Arc::new(Stub("mike")));
        let names = r.names();
        assert_eq!(names, vec!["/alpha", "/mike", "/zeta"]);
    }

    #[test]
    fn test_build_registry_contains_core_commands() {
        let r = build_registry();
        assert!(r.find("help").is_some());
        assert!(r.find("status").is_some());
        assert!(r.find("clear").is_some());
        assert!(r.find("model").is_some());
        assert!(r.find("cost").is_some());
        assert!(r.find("doctor").is_some());
        assert!(r.find("memory").is_some());
        assert!(r.find("skills").is_some());
    }

    #[test]
    fn test_aliases_are_recognized() {
        struct WithAlias;
        #[async_trait::async_trait]
        impl SlashCommand for WithAlias {
            fn name(&self) -> &'static str {
                "exit"
            }
            fn aliases(&self) -> &'static [&'static str] {
                &["quit", "q"]
            }
            fn description(&self) -> &'static str {
                "exit"
            }
            fn category(&self) -> CommandCategory {
                CommandCategory::Session
            }
            async fn execute<'a>(
                &self,
                _args: &str,
                _ctx: &CommandContext<'a>,
            ) -> CommandOutcome {
                CommandOutcome::Exit
            }
        }
        let mut r = CommandRegistry::new();
        r.register(Arc::new(WithAlias));
        assert!(r.find("exit").is_some());
        assert!(r.find("quit").is_some());
        assert!(r.find("q").is_some());
    }
}
