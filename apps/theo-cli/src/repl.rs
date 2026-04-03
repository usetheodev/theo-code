//! Interactive REPL for the Theo Agent with session persistence.

use std::path::PathBuf;
use std::sync::Arc;

use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

use theo_agent_runtime::event_bus::EventBus;
#[allow(deprecated)]
use theo_agent_runtime::events::PrintEventSink;
use theo_agent_runtime::{AgentConfig, AgentLoop};
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
    /// Session history — persists between prompts in the REPL.
    session_messages: Vec<Message>,
}

impl Repl {
    pub fn new(config: AgentConfig, project_dir: PathBuf, provider_name: String) -> Self {
        let editor = DefaultEditor::new().expect("failed to create editor");
        Self {
            editor,
            config,
            project_dir,
            provider_name,
            session_messages: Vec::new(),
        }
    }

    pub async fn run(&mut self) {
        self.print_banner();

        loop {
            match self.editor.readline("\x1b[36mtheo>\x1b[0m ") {
                Ok(line) => {
                    let line = line.trim().to_string();
                    if line.is_empty() {
                        continue;
                    }

                    let _ = self.editor.add_history_entry(&line);

                    if line.starts_with('/') {
                        let should_exit = handle_command(
                            &line,
                            &self.config,
                            &self.project_dir,
                            &self.provider_name,
                        )
                        .await;
                        if should_exit {
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
                    eprintln!("Goodbye.");
                    break;
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    break;
                }
            }
        }
    }

    async fn execute_task(&mut self, task: &str) {
        eprintln!();

        // Create EventBus with CLI renderer for real-time feedback
        let event_bus = Arc::new(EventBus::new());
        let renderer = Arc::new(CliRenderer::new());
        event_bus.subscribe(renderer);

        // Create agent
        #[allow(deprecated)]
        let event_sink = Arc::new(PrintEventSink);
        let registry = create_default_registry();
        let agent = AgentLoop::new(self.config.clone(), registry, event_sink);

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
            format!(", \x1b[90m{}tokens\x1b[0m", format_tokens(result.tokens_used))
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
                result.iterations_used,
                token_str,
            );
            eprintln!();
        } else if result.tokens_used > 0 {
            eprintln!("\x1b[90m{} tokens\x1b[0m", format_tokens(result.tokens_used));
            eprintln!();
        }
    }

    fn print_banner(&self) {
        eprintln!("\x1b[1mtheo v0.1.0\x1b[0m — type /help for commands");
        eprintln!(
            "Provider: {} | Model: {}",
            self.provider_name, self.config.model
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
