//! Interactive REPL for the Theo Agent with session persistence.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;

use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;

use theo_agent_runtime::config::{AgentMode, system_prompt_for_mode};
use theo_agent_runtime::event_bus::EventBus;
use theo_agent_runtime::{AgentConfig, AgentLoop};
use theo_domain::graph_context::GraphContextProvider;
use theo_infra_llm::types::Message;
use theo_tooling::registry::create_default_registry;

use crate::commands::handle_command;
use crate::renderer::CliRenderer;

/// Maximum number of messages to keep in session history.
/// Prevents unbounded memory growth. Oldest messages are dropped.
const MAX_SESSION_MESSAGES: usize = 100;

pub struct Repl {
    editor: DefaultEditor,
    config: AgentConfig,
    project_dir: PathBuf,
    provider_name: String,
    /// Current interaction mode (Agent, Plan, Ask).
    mode: AgentMode,
    /// Session history — persists between prompts in the REPL.
    session_messages: Vec<Message>,
    /// GRAPHCTX — initialized once, shared across all turns (read-only after init).
    graph_context: Option<Arc<dyn GraphContextProvider>>,
}

impl Repl {
    pub fn new(config: AgentConfig, project_dir: PathBuf, provider_name: String) -> Self {
        let editor = DefaultEditor::new().expect("failed to create editor");
        // Restore session from disk (graceful — empty on missing/corrupt).
        let session_messages = load_session(&project_dir);
        if !session_messages.is_empty() {
            eprintln!(
                "[theo] Restored session ({} messages)",
                session_messages.len()
            );
        }
        Self {
            editor,
            config,
            project_dir,
            provider_name,
            mode: AgentMode::default(),
            session_messages,
            graph_context: None,
        }
    }

    /// Create REPL with a specific initial mode.
    pub fn with_mode(mut self, mode: AgentMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set the current interaction mode. Updates system prompt.
    pub fn set_mode(&mut self, mode: AgentMode) {
        self.mode = mode;
        self.config.system_prompt = system_prompt_for_mode(mode);
        eprintln!("  Mode: \x1b[36m{}\x1b[0m", mode);
    }

    /// Initialize GRAPHCTX once for the session (fire-and-forget background build).
    /// Disabled entirely when THEO_NO_GRAPHCTX=1.
    async fn ensure_graph_context(&mut self) {
        if self.graph_context.is_some() {
            return;
        }
        if std::env::var("THEO_NO_GRAPHCTX").is_ok() {
            return; // Enabled by default. Set THEO_NO_GRAPHCTX=1 to disable.
        }
        let service = Arc::new(
            theo_application::use_cases::graph_context_service::GraphContextService::new(),
        );
        let _ = service.initialize(&self.project_dir).await;
        eprintln!("[theo] GRAPHCTX building in background");
        self.graph_context = Some(service);
    }

    /// Execute a single prompt and exit (no REPL loop).
    /// Used for `theo agent "task here"` single-shot mode.
    pub async fn execute_single(&mut self, prompt: &str) {
        self.print_banner();
        self.ensure_graph_context().await;
        self.execute_task(prompt).await;
    }

    pub async fn run(&mut self) {
        self.print_banner();
        self.ensure_graph_context().await;

        loop {
            match self.editor.readline("\x1b[36mtheo>\x1b[0m ") {
                Ok(line) => {
                    let line = line.trim().to_string();
                    if line.is_empty() {
                        continue;
                    }

                    let _ = self.editor.add_history_entry(&line);

                    if line.starts_with('/') {
                        // Handle /mode locally (needs mutable self)
                        if line.starts_with("/mode") {
                            let parts: Vec<&str> = line.splitn(2, ' ').collect();
                            if let Some(mode_str) = parts.get(1) {
                                if let Some(mode) = AgentMode::from_str(mode_str.trim()) {
                                    self.set_mode(mode);
                                } else {
                                    eprintln!(
                                        "  Unknown mode: {}. Use: agent, plan, ask",
                                        mode_str
                                    );
                                }
                            } else {
                                eprintln!("  Current mode: \x1b[36m{}\x1b[0m", self.mode);
                                eprintln!("  Usage: /mode <agent|plan|ask>");
                            }
                            continue;
                        }

                        let should_exit = handle_command(
                            &line,
                            &self.config,
                            &self.project_dir,
                            &self.provider_name,
                        )
                        .await;
                        if should_exit {
                            save_session(&self.project_dir, &self.session_messages);
                            break;
                        }
                    } else {
                        self.execute_task(&line).await;
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    eprintln!("  ^C — type /exit to quit");
                }
                Err(ReadlineError::Eof) => {
                    save_session(&self.project_dir, &self.session_messages);
                    eprintln!("Goodbye.");
                    break;
                }
                Err(e) => {
                    save_session(&self.project_dir, &self.session_messages);
                    eprintln!("Error: {e}");
                    break;
                }
            }
        }
    }

    async fn execute_task(&mut self, task: &str) {
        eprintln!();

        // Apply current mode's system prompt + mode field before each execution
        self.config.system_prompt = system_prompt_for_mode(self.mode);
        self.config.mode = self.mode;

        // Create EventBus with CLI renderer for real-time feedback
        let event_bus = Arc::new(EventBus::new());
        let renderer = Arc::new(CliRenderer::new());
        event_bus.subscribe(renderer);

        // Create agent with GRAPHCTX if available
        let registry = create_default_registry();
        let mut agent = AgentLoop::new(self.config.clone(), registry);
        if let Some(ref gc) = self.graph_context {
            agent = agent.with_graph_context(gc.clone());
        }

        // Execute with session history + external EventBus
        let result = agent
            .run_with_history(
                task,
                &self.project_dir,
                self.session_messages.clone(),
                Some(event_bus),
            )
            .await;

        // Record this exchange in session history
        self.session_messages.push(Message::user(task));
        self.session_messages
            .push(Message::assistant(&result.summary));

        // Trim session if too large
        if self.session_messages.len() > MAX_SESSION_MESSAGES {
            let excess = self.session_messages.len() - MAX_SESSION_MESSAGES;
            self.session_messages.drain(..excess);
        }

        // Persist session after each task (crash safety).
        save_session(&self.project_dir, &self.session_messages);

        // Show LLM response (only if not already displayed via streaming).
        // Text-only responses are streamed via ContentDelta in real-time,
        // so re-printing the summary would duplicate the output.
        eprintln!();
        if !result.summary.is_empty() && !result.was_streamed {
            eprintln!("{}", result.summary);
            eprintln!();
        }

        // Result status with token usage
        let token_str = if result.tokens_used > 0 {
            format!(
                ", \x1b[90m{} tokens\x1b[0m",
                format_tokens(result.tokens_used)
            )
        } else {
            String::new()
        };

        if result.success && !result.files_edited.is_empty() {
            eprintln!(
                "\x1b[32m✓ Done\x1b[0m — {} iterations, {} files: {}{}",
                result.iterations_used,
                result.files_edited.len(),
                result.files_edited.join(", "),
                token_str,
            );
            eprintln!();
        } else if !result.success {
            eprintln!(
                "\x1b[31m✗ Failed\x1b[0m — {} iterations{}",
                result.iterations_used, token_str,
            );
            eprintln!();
        } else if result.tokens_used > 0 {
            eprintln!(
                "\x1b[90m{} tokens\x1b[0m",
                format_tokens(result.tokens_used)
            );
            eprintln!();
        }
    }

    fn print_banner(&self) {
        eprintln!("\x1b[1mtheo v0.1.0\x1b[0m — type /help for commands");
        eprintln!(
            "Provider: {} | Model: {} | Mode: \x1b[36m{}\x1b[0m",
            self.provider_name, self.config.model, self.mode
        );
        eprintln!("Project: {}", self.project_dir.display());
        eprintln!();
    }
}

fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        format!("{}", tokens)
    }
}

// ---------------------------------------------------------------------------
// Session persistence
// ---------------------------------------------------------------------------

/// Compute a stable hash of the project dir for session file naming.
fn project_hash(project_dir: &std::path::Path) -> String {
    let mut hasher = DefaultHasher::new();
    project_dir.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Path to the session file for a given project.
fn session_path(project_dir: &std::path::Path) -> PathBuf {
    let base = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
        .join(".config")
        .join("theo")
        .join("sessions");
    base.join(format!("{}.json", project_hash(project_dir)))
}

/// Load session messages from disk. Returns empty vec on any error (graceful).
fn load_session(project_dir: &std::path::Path) -> Vec<Message> {
    let path = session_path(project_dir);
    let Ok(data) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    match serde_json::from_str::<Vec<Message>>(&data) {
        Ok(mut messages) => {
            // Cap at MAX_SESSION_MESSAGES.
            if messages.len() > MAX_SESSION_MESSAGES {
                let excess = messages.len() - MAX_SESSION_MESSAGES;
                messages.drain(..excess);
            }
            messages
        }
        Err(_) => {
            // Corrupt JSON — start fresh, don't crash.
            Vec::new()
        }
    }
}

/// Save session messages to disk. Best-effort, failures are silent.
fn save_session(project_dir: &std::path::Path, messages: &[Message]) {
    let path = session_path(project_dir);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Only save the last MAX_SESSION_MESSAGES.
    let to_save = if messages.len() > MAX_SESSION_MESSAGES {
        &messages[messages.len() - MAX_SESSION_MESSAGES..]
    } else {
        messages
    };
    if let Ok(json) = serde_json::to_string_pretty(to_save) {
        let _ = std::fs::write(&path, json);
    }
}

/// Clear the persisted session for a project (used by /clear command).
#[allow(dead_code)]
pub fn clear_session(project_dir: &std::path::Path) {
    let path = session_path(project_dir);
    let _ = std::fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_hash_is_deterministic() {
        let h1 = project_hash(std::path::Path::new("/tmp/project-a"));
        let h2 = project_hash(std::path::Path::new("/tmp/project-a"));
        assert_eq!(h1, h2);
    }

    #[test]
    fn project_hash_differs_for_different_dirs() {
        let h1 = project_hash(std::path::Path::new("/tmp/project-a"));
        let h2 = project_hash(std::path::Path::new("/tmp/project-b"));
        assert_ne!(h1, h2);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("my-project");
        std::fs::create_dir_all(&project).unwrap();

        let messages = vec![Message::user("hello"), Message::assistant("world")];
        save_session(&project, &messages);

        let loaded = load_session(&project);
        assert_eq!(loaded.len(), 2);
    }

    #[test]
    fn load_returns_empty_on_missing_file() {
        let loaded = load_session(std::path::Path::new("/tmp/nonexistent-project-xyz"));
        assert!(loaded.is_empty());
    }

    #[test]
    fn load_returns_empty_on_corrupt_json() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("corrupt-project");
        std::fs::create_dir_all(&project).unwrap();

        // Write corrupt JSON to session path.
        let path = session_path(&project);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "NOT VALID JSON {{{").unwrap();

        let loaded = load_session(&project);
        assert!(loaded.is_empty());
    }

    #[test]
    fn save_caps_at_max_session_messages() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("big-session");
        std::fs::create_dir_all(&project).unwrap();

        // Create 120 messages (above MAX_SESSION_MESSAGES=100).
        let messages: Vec<Message> = (0..120)
            .map(|i| Message::user(&format!("msg {i}")))
            .collect();
        save_session(&project, &messages);

        let loaded = load_session(&project);
        assert_eq!(loaded.len(), MAX_SESSION_MESSAGES);
    }
}
