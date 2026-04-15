//! TUI module — ratatui-based terminal interface for Theo.
//!
//! Architecture: Elm/Redux pattern with 3 tokio tasks:
//! 1. Input task — crossterm EventStream → UserAction
//! 2. Event task — broadcast::Receiver<DomainEvent> → batched TuiMsg
//! 3. Render task — 30fps tick, drain messages, update state, draw

mod app;
mod commands;
mod events;
mod input;
mod markdown;
mod view;
mod widgets;

use std::path::PathBuf;
use std::sync::Arc;

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::FutureExt;
use ratatui::prelude::*;
use tokio::sync::mpsc;

use theo_agent_runtime::config::{AgentConfig, AgentMode, system_prompt_for_mode};
use theo_agent_runtime::event_bus::EventBus;
#[allow(deprecated)]
use theo_agent_runtime::events::PrintEventSink;
use theo_agent_runtime::AgentLoop;
use theo_infra_llm::types::Message;
use theo_tooling::registry::create_default_registry;

use app::{Msg, TuiState};

fn dirs_path() -> std::path::PathBuf {
    std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"))
        .join(".config")
        .join("theo")
}

/// Main entry point for TUI mode.
pub async fn run(
    config: AgentConfig,
    project_dir: PathBuf,
    provider_name: String,
    initial_prompt: Option<String>,
) -> anyhow::Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Panic hook: restore terminal even on panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        original_hook(info);
    }));

    // Create shared EventBus + broadcast bridge
    let event_bus = Arc::new(EventBus::new());
    let broadcast_rx = event_bus.subscribe_broadcast(1024);

    // Message channel: all tasks send Msg here, render loop drains
    let (msg_tx, mut msg_rx) = mpsc::channel::<Msg>(256);

    // Spawn input task
    let input_tx = msg_tx.clone();
    tokio::spawn(async move {
        input::input_loop(input_tx).await;
    });

    // Spawn event task (broadcast → batched Msg)
    let event_tx = msg_tx.clone();
    tokio::spawn(async move {
        events::event_loop(broadcast_rx, event_tx).await;
    });

    // Initialize state
    let size = terminal.size()?;
    let mut state = TuiState::new(
        provider_name,
        config.model.clone(),
        config.max_iterations,
        size.width,
        size.height,
    );

    // Session history for agent continuity
    let mut session_messages: Vec<Message> = Vec::new();

    // If initial prompt provided, queue it for execution
    let mut pending_prompt: Option<String> = initial_prompt;

    // Render loop at ~30fps
    let mut tick_interval = tokio::time::interval(std::time::Duration::from_millis(33));
    let mut cursor_interval = tokio::time::interval(std::time::Duration::from_millis(500));

    loop {
        // Drain all pending messages
        while let Ok(msg) = msg_rx.try_recv() {
            // In search mode, redirect input to search
            let msg = if state.search_mode {
                match msg {
                    Msg::InputChar(c) => Msg::SearchChar(c),
                    Msg::InputBackspace => Msg::SearchBackspace,
                    Msg::Submit(_) => Msg::SearchClose,
                    Msg::ToggleHelp => Msg::SearchClose,
                    other => other,
                }
            } else if state.show_model_picker {
                // Model picker mode
                match msg {
                    Msg::InputChar('j') | Msg::ScrollDown(_) => Msg::ModelPickerDown,
                    Msg::InputChar('k') | Msg::ScrollUp(_) => Msg::ModelPickerUp,
                    Msg::Submit(_) => Msg::ModelPickerSelect,
                    Msg::ToggleHelp | Msg::ToggleModelPicker => Msg::ToggleModelPicker,
                    Msg::Quit => Msg::Quit,
                    _ => continue,
                }
            } else if state.show_help {
                // In help mode, Esc closes help
                match msg {
                    Msg::ToggleHelp => Msg::ToggleHelp,
                    Msg::Quit => Msg::Quit,
                    _ => Msg::ToggleHelp, // any key closes help
                }
            } else {
                // Normal mode: intercept Submit
                match msg {
                    Msg::Submit(ref s) if s.is_empty() && !state.input_text.is_empty() => {
                        let text = state.input_text.clone();
                        // Check if it's a slash command
                        if let Some(cmds) = commands::process_command(&text, &state) {
                            // Clear input and process command messages
                            state.input_text.clear();
                            state.input_cursor = 0;
                            for cmd_msg in cmds {
                                // Handle ExportSession with filesystem access
                                if matches!(cmd_msg, Msg::ExportSession) {
                                    let md = commands::export_transcript(&state);
                                    let export_dir = dirs_path().join("exports");
                                    let _ = std::fs::create_dir_all(&export_dir);
                                    let filename = format!("{}.md", chrono::Utc::now().format("%Y%m%d-%H%M%S"));
                                    let path = export_dir.join(&filename);
                                    match std::fs::write(&path, &md) {
                                        Ok(_) => {
                                            app::update(&mut state, Msg::ShowToast(
                                                format!("Exported to {}", path.display()),
                                                app::ToastLevel::Info,
                                            ));
                                        }
                                        Err(e) => {
                                            app::update(&mut state, Msg::ShowToast(
                                                format!("Export failed: {e}"),
                                                app::ToastLevel::Error,
                                            ));
                                        }
                                    }
                                } else {
                                    app::update(&mut state, cmd_msg);
                                }
                            }
                            continue;
                        }
                        pending_prompt = Some(text.clone());
                        Msg::Submit(text)
                    }
                    Msg::Submit(ref s) if s.is_empty() => continue, // empty submit, skip
                    other => other,
                }
            };
            app::update(&mut state, msg);
        }

        // Launch agent for pending prompt (if not already running)
        if let Some(prompt) = pending_prompt.take() {
            if !state.agent_running {
                state.agent_running = true;

                let task_config = config.clone();
                let task_dir = project_dir.clone();
                let task_bus = event_bus.clone();
                let task_messages = session_messages.clone();
                let task_prompt = prompt.clone();
                let task_msg_tx = msg_tx.clone();

                // Record in session
                session_messages.push(Message::user(&prompt));

                tokio::spawn(async move {
                    let mut cfg = task_config;
                    cfg.system_prompt = system_prompt_for_mode(AgentMode::Agent);
                    cfg.mode = AgentMode::Agent;

                    let registry = create_default_registry();
                    #[allow(deprecated)]
                    let event_sink = Arc::new(PrintEventSink);
                    let agent = AgentLoop::new(cfg, registry, event_sink);

                    let result = agent
                        .run_with_history(
                            &task_prompt,
                            &task_dir,
                            task_messages,
                            Some(task_bus),
                        )
                        .await;

                    // Signal completion via a DomainEvent (LlmCallEnd with final summary)
                    // The result summary will be picked up by the event stream
                    let _ = task_msg_tx.send(Msg::AgentComplete(result.summary, result.success)).await;
                });
            }
        }

        // Cursor blink + toast cleanup
        if cursor_interval.tick().now_or_never().is_some() {
            app::update(&mut state, Msg::CursorBlink);
            app::update(&mut state, Msg::DismissExpiredToasts);
        }

        // Draw
        terminal.draw(|frame| {
            view::draw(frame, &state);
        })?;

        if state.should_quit {
            break;
        }

        tick_interval.tick().await;
    }

    // Cleanup terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}
