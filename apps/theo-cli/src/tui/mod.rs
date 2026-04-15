//! TUI module — ratatui-based terminal interface for Theo.
//!
//! Architecture: Elm/Redux pattern with 3 tokio tasks:
//! 1. Input task — crossterm EventStream → UserAction
//! 2. Event task — broadcast::Receiver<DomainEvent> → batched TuiMsg
//! 3. Render task — 30fps tick, drain messages, update state, draw

mod app;
mod events;
mod input;
mod markdown;
mod view;

use std::path::PathBuf;

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::FutureExt;
use ratatui::prelude::*;
use tokio::sync::mpsc;

use theo_agent_runtime::config::AgentConfig;

use app::{Msg, TuiState};

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
    let event_bus = std::sync::Arc::new(theo_agent_runtime::event_bus::EventBus::new());
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

    // If initial prompt provided, submit it
    if let Some(prompt) = initial_prompt {
        app::update(&mut state, Msg::Submit(prompt));
    }

    // Render loop at ~30fps
    let mut tick_interval = tokio::time::interval(std::time::Duration::from_millis(33));
    let mut cursor_interval = tokio::time::interval(std::time::Duration::from_millis(500));

    loop {
        // Drain all pending messages
        while let Ok(msg) = msg_rx.try_recv() {
            // Intercept empty Submit (from input loop) and fill with actual text
            let msg = match msg {
                Msg::Submit(ref s) if s.is_empty() && !state.input_text.is_empty() => {
                    Msg::Submit(state.input_text.clone())
                }
                other => other,
            };
            app::update(&mut state, msg);
        }

        // Cursor blink
        if cursor_interval.tick().now_or_never().is_some() {
            app::update(&mut state, Msg::CursorBlink);
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
