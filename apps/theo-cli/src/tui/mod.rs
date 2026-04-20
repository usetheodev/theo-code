//! TUI module — ratatui-based terminal interface for Theo.
//!
//! Architecture: Elm/Redux pattern with 3 tokio tasks:
//! 1. Input task — crossterm EventStream → UserAction
//! 2. Event task — broadcast::Receiver<DomainEvent> → batched TuiMsg
//! 3. Render task — 30fps tick, drain messages, update state, draw

mod app;
mod autocomplete;
mod bench;
mod commands;
mod events;
mod input;
mod markdown;
pub mod theme;
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
use theo_agent_runtime::AgentLoop;
use theo_infra_llm::types::Message;
use theo_tooling::registry::create_default_registry;

use app::{Msg, TuiState};

/// Write debug log to ~/.config/theo/tui.log (visible outside the TUI)
fn tui_log(msg: &str) {
    use std::io::Write;
    let path = dirs_path().join("tui.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let ts = chrono::Utc::now().format("%H:%M:%S%.3f");
        let _ = writeln!(f, "[{ts}] {msg}");
    }
}

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
    // Ensure log directory exists
    let _ = std::fs::create_dir_all(dirs_path());
    tui_log("=== TUI START ===");

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
    state.project_dir = project_dir.clone();

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
            // Log non-trivial messages
            match &msg {
                Msg::Submit(_) | Msg::LoginStart(_) | Msg::LoginComplete(_) | Msg::LoginFailed(_)
                | Msg::AgentComplete(_, _) | Msg::LoginServer(_) | Msg::LoginWithKey(_) => {
                    tui_log(&format!("MSG: {:?}", std::mem::discriminant(&msg)));
                }
                _ => {}
            }
            // In search mode, redirect input to search
            let msg = if state.search_mode {
                match msg {
                    Msg::InputChar(c) => Msg::SearchChar(c),
                    Msg::InputBackspace => Msg::SearchBackspace,
                    Msg::Submit(_) => Msg::SearchClose,
                    Msg::ToggleHelp => Msg::SearchClose,
                    other => other,
                }
            } else if state.pending_approval.is_some() {
                // Approval modal — only a/r/Esc
                match msg {
                    Msg::InputChar('a') | Msg::InputChar('A') => Msg::ApproveDecision,
                    Msg::InputChar('r') | Msg::InputChar('R') => Msg::RejectDecision,
                    Msg::ToggleHelp => Msg::RejectDecision, // Esc = reject
                    Msg::Quit => Msg::Quit,
                    _ => continue, // ignore all other input
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
            } else if state.autocomplete.active {
                // Autocomplete mode
                match msg {
                    Msg::ScrollUp(_) => Msg::AutocompleteUp,
                    Msg::ScrollDown(_) => Msg::AutocompleteDown,
                    Msg::Submit(_) => Msg::AutocompleteAccept, // Enter accepts
                    Msg::ToggleHelp => Msg::AutocompleteClose, // Esc closes
                    Msg::ToggleSidebar => Msg::AutocompleteAccept, // Tab accepts
                    other => other, // Other keys pass through (typing continues)
                }
            } else {
                // Normal mode: intercept Submit
                // Debug: log every message type
                if matches!(&msg, Msg::Submit(_)) {
                    let s_content = if let Msg::Submit(ref s) = msg { s.clone() } else { String::new() };
                    tui_log(&format!("SUBMIT in normal mode: s='{}' input_text='{}' autocomplete_active={}", s_content, state.input_text, state.autocomplete.active));
                }
                // Force close autocomplete on Submit so it doesn't intercept
                if matches!(&msg, Msg::Submit(_)) && state.autocomplete.active {
                    state.autocomplete.active = false;
                    tui_log("Autocomplete was active on Submit — force closed");
                }
                match msg {
                    Msg::Submit(ref s) if s.is_empty() && !state.input_text.is_empty() => {
                        tui_log(&format!("SUBMIT matched: will process '{}'", state.input_text));
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
                                            app::update(&mut state, Msg::Notify(
                                                format!("Exported to {}", path.display()),
                                            ));
                                        }
                                        Err(e) => {
                                            app::update(&mut state, Msg::Notify(
                                                format!("Export failed: {e}"),
                                            ));
                                        }
                                    }
                                } else if matches!(cmd_msg, Msg::LoginStart(_)) {
                                    // Login must run inline (not via app::update)
                                    // because it needs async IO + force draw
                                    app::update(&mut state, cmd_msg); // shows "Starting..."
                                    let auth = theo_infra_auth::OpenAIAuth::with_default_store();

                                    if let Ok(Some(tokens)) = auth.get_tokens() {
                                        if !tokens.is_expired() {
                                            app::update(&mut state, Msg::LoginComplete(
                                                "Already logged in (token valid)".into()
                                            ));
                                            continue;
                                        }
                                    }

                                    app::update(&mut state, Msg::Notify(
                                        "Contacting auth server...".into()
                                    ));
                                    terminal.draw(|frame| view::draw(frame, &state))?;

                                    match auth.start_device_flow().await {
                                        Ok(code) => {
                                            app::update(&mut state, Msg::Notify("─────────────────────────────────────".into()));
                                            app::update(&mut state, Msg::Notify(format!("1. Open: {}", code.verification_uri)));
                                            app::update(&mut state, Msg::Notify("2. Login to your OpenAI account if needed".into()));
                                            app::update(&mut state, Msg::Notify(format!("3. Enter code: {}", code.user_code)));
                                            app::update(&mut state, Msg::Notify("4. Click 'Authorize'".into()));
                                            app::update(&mut state, Msg::Notify("─────────────────────────────────────".into()));
                                            // Copy code to clipboard via OSC52
                                            eprint!("\x1b]52;c;{}\x07", app::base64_encode(&code.user_code));
                                            app::update(&mut state, Msg::Notify("Code copied to clipboard. Waiting...".into()));
                                            terminal.draw(|frame| view::draw(frame, &state))?;

                                            // Open browser silently (redirect output to /dev/null)
                                            #[cfg(target_os = "linux")]
                                            {
                                                let _ = std::process::Command::new("xdg-open")
                                                    .arg(&code.verification_uri)
                                                    .stdout(std::process::Stdio::null())
                                                    .stderr(std::process::Stdio::null())
                                                    .spawn();
                                            }
                                            #[cfg(target_os = "macos")]
                                            {
                                                let _ = std::process::Command::new("open")
                                                    .arg(&code.verification_uri)
                                                    .stdout(std::process::Stdio::null())
                                                    .stderr(std::process::Stdio::null())
                                                    .spawn();
                                            }

                                            let poll_tx = msg_tx.clone();
                                            tokio::spawn(async move {
                                                match auth.poll_device_flow(&code).await {
                                                    Ok(_) => { let _ = poll_tx.send(Msg::LoginComplete("✓ Authenticated with OpenAI!".into())).await; }
                                                    Err(e) => { let _ = poll_tx.send(Msg::LoginFailed(format!("Auth failed: {e}"))).await; }
                                                }
                                            });
                                        }
                                        Err(e) => {
                                            app::update(&mut state, Msg::LoginFailed(
                                                format!("Device flow error: {e}")
                                            ));
                                        }
                                    }
                                } else if matches!(cmd_msg, Msg::LoginServer(_)) {
                                    // Generic device flow against any RFC 8628 server
                                    let server_url = if let Msg::LoginServer(ref u) = cmd_msg { u.clone() } else { unreachable!() };
                                    app::update(&mut state, cmd_msg);
                                    let http = reqwest::Client::new();
                                    let config = theo_infra_auth::device_flow::DeviceFlowConfig::new(&server_url);

                                    app::update(&mut state, Msg::Notify("Requesting device code...".into()));
                                    terminal.draw(|frame| view::draw(frame, &state))?;

                                    match theo_infra_auth::device_flow::start_device_flow(&http, &config).await {
                                        Ok(code) => {
                                            app::update(&mut state, Msg::Notify("─────────────────────────────────────".into()));
                                            app::update(&mut state, Msg::Notify(format!("1. Open: {}", code.verification_url)));
                                            app::update(&mut state, Msg::Notify(format!("2. Enter code: {}", code.user_code)));
                                            app::update(&mut state, Msg::Notify("3. Authorize Theo".into()));
                                            app::update(&mut state, Msg::Notify("─────────────────────────────────────".into()));
                                            eprint!("\x1b]52;c;{}\x07", app::base64_encode(&code.user_code));
                                            app::update(&mut state, Msg::Notify("Code copied to clipboard. Waiting...".into()));
                                            terminal.draw(|frame| view::draw(frame, &state))?;

                                            // Open browser silently
                                            #[cfg(target_os = "linux")]
                                            { let _ = std::process::Command::new("xdg-open").arg(&code.verification_url).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn(); }
                                            #[cfg(target_os = "macos")]
                                            { let _ = std::process::Command::new("open").arg(&code.verification_url).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn(); }

                                            // Poll in background
                                            let poll_tx = msg_tx.clone();
                                            let poll_config = config.clone();
                                            let poll_code = code.clone();
                                            tokio::spawn(async move {
                                                let http = reqwest::Client::new();
                                                match theo_infra_auth::device_flow::poll_device_flow(&http, &poll_config, &poll_code).await {
                                                    Ok(tokens) => {
                                                        // Set the access token as env var for the agent
                                                        unsafe { std::env::set_var("OPENAI_API_KEY", &tokens.access_token); }
                                                        let _ = poll_tx.send(Msg::LoginComplete("✓ Authenticated! Provider ready.".into())).await;
                                                    }
                                                    Err(e) => {
                                                        let _ = poll_tx.send(Msg::LoginFailed(format!("{e}"))).await;
                                                    }
                                                }
                                            });
                                        }
                                        Err(e) => {
                                            app::update(&mut state, Msg::LoginFailed(format!("Server error: {e}")));
                                        }
                                    }
                                } else {
                                    app::update(&mut state, cmd_msg);
                                }
                            }
                            continue;
                        }
                        pending_prompt = Some(text.clone());
                        tui_log(&format!("pending_prompt SET to '{}'", &text[..text.len().min(40)]));
                        Msg::Submit(text)
                    }
                    Msg::Submit(ref s) if s.is_empty() => continue, // empty submit, skip
                    other => other,
                }
            };
            // Handle IO-bound commands before update
            match &msg {
                // LoginStart is handled inline in the slash command loop above
                Msg::MemoryCommand(arg) => {
                    let tx = msg_tx.clone();
                    let project_dir = project_dir.clone();
                    let arg = arg.clone();
                    tokio::spawn(async move {
                        let memory_root = dirs_path().join("memory");
                        let store = theo_tooling::memory::FileMemoryStore::for_project(&memory_root, &project_dir);
                        let result = if arg.is_empty() || arg == "list" {
                            match store.list().await {
                                Ok(memories) if memories.is_empty() => "No memories for this project.".to_string(),
                                Ok(memories) => {
                                    memories.iter()
                                        .map(|m| format!("  {}: {}", m.key, m.value))
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                }
                                Err(e) => format!("Error: {e}"),
                            }
                        } else if arg.starts_with("search ") {
                            let query = arg.strip_prefix("search ").unwrap_or("");
                            match store.search(query).await {
                                Ok(results) if results.is_empty() => format!("No memories matching '{query}'"),
                                Ok(results) => results.iter()
                                    .map(|m| format!("  {}: {}", m.key, m.value))
                                    .collect::<Vec<_>>()
                                    .join("\n"),
                                Err(e) => format!("Error: {e}"),
                            }
                        } else if arg.starts_with("delete ") {
                            let key = arg.strip_prefix("delete ").unwrap_or("");
                            match store.delete(key).await {
                                Ok(true) => format!("Deleted: {key}"),
                                Ok(false) => format!("Not found: {key}"),
                                Err(e) => format!("Error: {e}"),
                            }
                        } else {
                            "Usage: /memory [list|search <q>|delete <key>]".to_string()
                        };
                        let _ = tx.send(Msg::Notify(result)).await;
                    });
                }
                Msg::SkillsCommand => {
                    let project_dir = project_dir.clone();
                    let mut registry = theo_agent_runtime::skill::SkillRegistry::new();
                    registry.load_bundled();
                    let skills_dir = project_dir.join(".theo").join("skills");
                    if skills_dir.exists() {
                        registry.load_from_dir(&skills_dir);
                    }
                    let skills = registry.list();
                    if skills.is_empty() {
                        app::update(&mut state, Msg::Notify("No skills available.".into()));
                    } else {
                        let list: Vec<String> = skills.iter()
                            .map(|s| format!("  {} — {}", s.name, s.trigger))
                            .collect();
                        app::update(&mut state, Msg::Notify(
                            format!("{} skills:\n{}", skills.len(), list.join("\n")),
                        ));
                    }
                }
                _ => {}
            }

            app::update(&mut state, msg);

            // Trigger autocomplete update after any input change
            if !state.search_mode && !state.show_help {
                app::update(&mut state, Msg::AutocompleteUpdate);
            }
        }

        // Launch agent for pending prompt (if not already running)
        if pending_prompt.is_some() {
            tui_log(&format!("TICK: pending_prompt=Some agent_running={}", state.agent_running));
        }
        if let Some(prompt) = pending_prompt.take() {
            tui_log(&format!("PROMPT TAKEN: '{}' agent_running={}", &prompt[..prompt.len().min(40)], state.agent_running));
            if !state.agent_running {
                state.agent_running = true;
                tui_log("=== AGENT LAUNCH START ===");
                tui_log(&format!("Prompt: {}", &prompt[..prompt.len().min(80)]));

                // Re-resolve config to pick up tokens from login
                let (fresh_config, fresh_provider) = crate::resolve_agent_config(None, None, None).await;
                tui_log(&format!("Resolved provider: {fresh_provider}"));
                tui_log(&format!("Model: {}", fresh_config.model));
                tui_log(&format!("Base URL: {}", fresh_config.base_url));
                tui_log(&format!("API key present: {}", fresh_config.api_key.is_some()));
                tui_log(&format!("Endpoint override: {:?}", fresh_config.endpoint_override));

                if fresh_provider != "default" {
                    state.status.provider = fresh_provider.clone();
                    state.status.model = fresh_config.model.clone();
                }

                // Show debug info in transcript
                let debug_msg = format!(
                    "[debug] provider={} model={} key={} url={}",
                    &state.status.provider,
                    &fresh_config.model,
                    if fresh_config.api_key.is_some() { "yes" } else { "NO" },
                    &fresh_config.base_url,
                );
                app::update(&mut state, Msg::Notify(debug_msg));

                let task_config = fresh_config;
                let task_dir = project_dir.clone();
                let task_bus = event_bus.clone();
                let task_messages = session_messages.clone();
                let task_prompt = prompt.clone();
                let task_msg_tx = msg_tx.clone();

                // Record in session
                session_messages.push(Message::user(&prompt));

                tokio::spawn(async move {
                    tui_log("Agent task spawned");
                    let mut cfg = task_config;
                    cfg.system_prompt = system_prompt_for_mode(AgentMode::Agent);
                    cfg.mode = AgentMode::Agent;

                    let registry = create_default_registry();
                    #[allow(deprecated)]
                    let agent = AgentLoop::new(cfg.clone(), registry);

                    tui_log(&format!("AgentLoop created, calling run_with_history..."));
                    tui_log(&format!("  api_key len: {}", cfg.api_key.as_ref().map(|k| k.len()).unwrap_or(0)));
                    tui_log(&format!("  base_url: {}", cfg.base_url));
                    tui_log(&format!("  endpoint: {:?}", cfg.endpoint_override));
                    tui_log(&format!("  history msgs: {}", task_messages.len()));

                    let result = agent
                        .run_with_history(
                            &task_prompt,
                            &task_dir,
                            task_messages,
                            Some(task_bus),
                        )
                        .await;

                    tui_log(&format!("Agent finished: success={} summary={}", result.success, &result.summary[..result.summary.len().min(100)]));
                    let _ = task_msg_tx.send(Msg::AgentComplete(result.summary, result.success)).await;
                });
            }
        }

        // Cursor blink
        if cursor_interval.tick().now_or_never().is_some() {
            app::update(&mut state, Msg::CursorBlink);
        }

        // Toggle mouse capture for copy mode
        // When copy_mode is on, disable mouse capture so terminal handles selection
        static mut LAST_COPY_MODE: bool = false;
        unsafe {
            if state.copy_mode != LAST_COPY_MODE {
                if state.copy_mode {
                    execute!(std::io::stdout(), DisableMouseCapture)?;
                } else {
                    execute!(std::io::stdout(), EnableMouseCapture)?;
                }
                LAST_COPY_MODE = state.copy_mode;
            }
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
