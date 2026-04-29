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
pub mod config;
mod events;
mod input;
mod markdown;
pub mod theme;
mod view;
mod widgets;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::FutureExt;
use ratatui::prelude::*;
use tokio::sync::mpsc;

// T1.2: route runtime types through the theo-application facade.
use theo_application::facade::agent::config::{AgentConfig, AgentMode, system_prompt_for_mode};
use theo_application::facade::agent::EventBus;
#[allow(deprecated)]
use theo_application::facade::agent::AgentLoop;
use theo_application::facade::llm::Message;
use theo_application::facade::tooling::create_default_registry_with_project;

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
    injections: theo_application::use_cases::run_agent_session::SubagentInjections,
) -> anyhow::Result<()> {
    let _ = std::fs::create_dir_all(dirs_path());
    tui_log("=== TUI START ===");
    let mut terminal = setup_tui_terminal()?;
    let event_bus = Arc::new(EventBus::new());
    let broadcast_rx = event_bus.subscribe_broadcast(1024);
    let (msg_tx, mut msg_rx) = mpsc::channel::<Msg>(256);
    spawn_input_and_event_tasks(msg_tx.clone(), broadcast_rx);
    let size = terminal.size()?;
    let mut state = TuiState::new(
        provider_name,
        config.llm.model.clone(),
        config.loop_cfg.max_iterations,
        size.width,
        size.height,
    );
    state.project_dir = project_dir.clone();
    let mut session_messages: Vec<Message> = Vec::new();
    let mut pending_prompt: Option<String> = initial_prompt;

    // Render loop at ~30fps
    let mut tick_interval = tokio::time::interval(std::time::Duration::from_millis(33));
    let mut cursor_interval = tokio::time::interval(std::time::Duration::from_millis(500));

    loop {
        // Drain all pending messages
        while let Ok(msg) = msg_rx.try_recv() {
            log_significant_msg(&msg);
            let Some(msg) = redirect_modal_msg(&mut state, msg) else {
                continue;
            };
            let msg = if !is_normal_mode(&state) {
                msg
            } else {
                match handle_normal_mode_msg(
                    &mut state,
                    &mut terminal,
                    &msg_tx,
                    &mut pending_prompt,
                    msg,
                )
                .await?
                {
                    Some(m) => m,
                    None => continue,
                }
            };
            // Handle IO-bound commands before update.
            dispatch_io_command(&mut state, &msg, &project_dir, &msg_tx);

            app::update(&mut state, msg);

            // Trigger autocomplete update after any input change
            if !state.search_mode && !state.show_help {
                app::update(&mut state, Msg::AutocompleteUpdate);
            }
        }

        if pending_prompt.is_some() {
            tui_log(&format!(
                "TICK: pending_prompt=Some agent_running={}",
                state.agent_running
            ));
        }
        if let Some(prompt) = pending_prompt.take() {
            launch_agent_for_prompt(
                &mut state,
                &project_dir,
                &event_bus,
                &mut session_messages,
                &injections,
                &msg_tx,
                prompt,
            )
            .await;
        }

        // Cursor blink
        if cursor_interval.tick().now_or_never().is_some() {
            app::update(&mut state, Msg::CursorBlink);
        }

        sync_mouse_capture_for_copy_mode(state.copy_mode)?;

        // Draw
        terminal.draw(|frame| {
            view::draw(frame, &state);
        })?;

        if state.should_quit {
            break;
        }

        tick_interval.tick().await;
    }

    cleanup_tui_terminal(&mut terminal)?;
    Ok(())
}

/// Enable raw mode + alternate screen + mouse capture, install a
/// panic hook that restores the terminal on unwind, and return a
/// `Terminal` ready for ratatui drawing.
fn setup_tui_terminal() -> anyhow::Result<Terminal<CrosstermBackend<std::io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        original_hook(info);
    }));
    Ok(terminal)
}

fn cleanup_tui_terminal(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
) -> anyhow::Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn spawn_input_and_event_tasks(
    msg_tx: mpsc::Sender<Msg>,
    broadcast_rx: tokio::sync::broadcast::Receiver<theo_domain::event::DomainEvent>,
) {
    let input_tx = msg_tx.clone();
    tokio::spawn(async move {
        input::input_loop(input_tx).await;
    });
    tokio::spawn(async move {
        events::event_loop(broadcast_rx, msg_tx).await;
    });
}

/// Normal-mode dispatch: when no modal is active, intercept `Submit`
/// and convert non-empty input into either a slash-command run (which
/// returns `Ok(None)` to skip the dispatch) or a `Msg::Submit(text)`
/// that records `pending_prompt` for the next iteration. Other
/// messages pass through.
async fn handle_normal_mode_msg(
    state: &mut TuiState,
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    msg_tx: &mpsc::Sender<Msg>,
    pending_prompt: &mut Option<String>,
    msg: Msg,
) -> anyhow::Result<Option<Msg>> {
    if matches!(&msg, Msg::Submit(_)) {
        let s_content = if let Msg::Submit(ref s) = msg {
            s.clone()
        } else {
            String::new()
        };
        tui_log(&format!(
            "SUBMIT in normal mode: s='{}' input_text='{}' autocomplete_active={}",
            s_content, state.input_text, state.autocomplete.active
        ));
    }
    if matches!(&msg, Msg::Submit(_)) && state.autocomplete.active {
        state.autocomplete.active = false;
        tui_log("Autocomplete was active on Submit — force closed");
    }
    match msg {
        Msg::Submit(ref s) if s.is_empty() && !state.input_text.is_empty() => {
            tui_log(&format!(
                "SUBMIT matched: will process '{}'",
                state.input_text
            ));
            let text = state.input_text.clone();
            if let Some(cmds) = commands::process_command(&text, state) {
                state.input_text.clear();
                state.input_cursor = 0;
                run_slash_command_messages(state, terminal, msg_tx, cmds).await?;
                return Ok(None);
            }
            *pending_prompt = Some(text.clone());
            tui_log(&format!(
                "pending_prompt SET to '{}'",
                &text[..text.len().min(40)]
            ));
            Ok(Some(Msg::Submit(text)))
        }
        Msg::Submit(ref s) if s.is_empty() => Ok(None),
        other => Ok(Some(other)),
    }
}

async fn run_slash_command_messages(
    state: &mut TuiState,
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    msg_tx: &mpsc::Sender<Msg>,
    cmds: Vec<Msg>,
) -> anyhow::Result<()> {
    for cmd_msg in cmds {
        if matches!(cmd_msg, Msg::ExportSession) {
            apply_export_session(state);
        } else if matches!(cmd_msg, Msg::LoginStart(_)) {
            // Returns true if the cached-token short-circuit fired; either
            // way we keep iterating the remaining slash-command msgs.
            let _ = handle_login_start_inline(state, terminal, msg_tx, cmd_msg).await?;
        } else if matches!(cmd_msg, Msg::LoginServer(_)) {
            handle_login_server_inline(state, terminal, msg_tx, cmd_msg).await?;
        } else {
            app::update(state, cmd_msg);
        }
    }
    Ok(())
}

fn apply_export_session(state: &mut TuiState) {
    let md = commands::export_transcript(state);
    let export_dir = dirs_path().join("exports");
    let _ = std::fs::create_dir_all(&export_dir);
    let filename = format!("{}.md", chrono::Utc::now().format("%Y%m%d-%H%M%S"));
    let path = export_dir.join(&filename);
    match std::fs::write(&path, &md) {
        Ok(_) => app::update(
            state,
            Msg::Notify(format!("Exported to {}", path.display())),
        ),
        Err(e) => app::update(state, Msg::Notify(format!("Export failed: {e}"))),
    }
}

/// Toggle terminal mouse-capture in sync with the user's copy-mode
/// flag. When copy mode is on, the terminal handles selection
/// natively (so we disable capture); when off, the TUI handles mouse
/// events itself.
fn sync_mouse_capture_for_copy_mode(copy_mode: bool) -> std::io::Result<()> {
    static mut LAST_COPY_MODE: bool = false;
    // SAFETY: LAST_COPY_MODE is read/written from exactly one task —
    // the single render-loop future. No concurrent access possible.
    unsafe {
        if copy_mode == LAST_COPY_MODE {
            return Ok(());
        }
        if copy_mode {
            execute!(std::io::stdout(), DisableMouseCapture)?;
        } else {
            execute!(std::io::stdout(), EnableMouseCapture)?;
        }
        LAST_COPY_MODE = copy_mode;
    }
    Ok(())
}

fn log_significant_msg(msg: &Msg) {
    if matches!(
        msg,
        Msg::Submit(_)
            | Msg::LoginStart(_)
            | Msg::LoginComplete(_)
            | Msg::LoginFailed(_)
            | Msg::AgentComplete(_, _)
            | Msg::LoginServer(_)
            | Msg::LoginWithKey(_)
    ) {
        tui_log(&format!("MSG: {:?}", std::mem::discriminant(msg)));
    }
}

/// Whether the TUI is in normal-mode dispatch (no modal active).
fn is_normal_mode(state: &TuiState) -> bool {
    !state.search_mode
        && state.pending_approval.is_none()
        && !state.show_model_picker
        && !state.show_help
        && !state.autocomplete.active
}

/// Modal redirect: when a modal (search / approval / picker / help /
/// autocomplete) is active, transform input messages into modal-
/// specific actions. Returns `None` when the message must be skipped
/// (modal-defined "ignore"), `Some(msg)` to dispatch the (possibly
/// rewritten) message. Normal mode is signalled by returning the
/// original `msg` untouched (caller checks via `is_normal_mode`).
fn redirect_modal_msg(state: &mut TuiState, msg: Msg) -> Option<Msg> {
    if state.search_mode {
        Some(match msg {
            Msg::InputChar(c) => Msg::SearchChar(c),
            Msg::InputBackspace => Msg::SearchBackspace,
            Msg::Submit(_) | Msg::ToggleHelp => Msg::SearchClose,
            other => other,
        })
    } else if state.pending_approval.is_some() {
        match msg {
            Msg::InputChar('a') | Msg::InputChar('A') => Some(Msg::ApproveDecision),
            Msg::InputChar('r') | Msg::InputChar('R') | Msg::ToggleHelp => {
                Some(Msg::RejectDecision)
            }
            Msg::Quit => Some(Msg::Quit),
            _ => None,
        }
    } else if state.show_model_picker {
        match msg {
            Msg::InputChar('j') | Msg::ScrollDown(_) => Some(Msg::ModelPickerDown),
            Msg::InputChar('k') | Msg::ScrollUp(_) => Some(Msg::ModelPickerUp),
            Msg::Submit(_) => Some(Msg::ModelPickerSelect),
            Msg::ToggleHelp | Msg::ToggleModelPicker => Some(Msg::ToggleModelPicker),
            Msg::Quit => Some(Msg::Quit),
            _ => None,
        }
    } else if state.show_help {
        Some(match msg {
            Msg::ToggleHelp => Msg::ToggleHelp,
            Msg::Quit => Msg::Quit,
            _ => Msg::ToggleHelp,
        })
    } else if state.autocomplete.active {
        Some(match msg {
            Msg::ScrollUp(_) => Msg::AutocompleteUp,
            Msg::ScrollDown(_) => Msg::AutocompleteDown,
            Msg::Submit(_) | Msg::ToggleSidebar => Msg::AutocompleteAccept,
            Msg::ToggleHelp => Msg::AutocompleteClose,
            other => other,
        })
    } else {
        // Normal mode — caller handles via is_normal_mode().
        Some(msg)
    }
}

/// Dispatch the IO-bound slash commands (`/memory`, `/skills`) that
/// can't be folded into `app::update` because they need async IO or
/// access to the project directory + sender. `LoginStart` /
/// `LoginServer` are handled separately, inline, because they need
/// `terminal.draw` between steps.
fn dispatch_io_command(
    state: &mut TuiState,
    msg: &Msg,
    project_dir: &Path,
    msg_tx: &mpsc::Sender<Msg>,
) {
    match msg {
        Msg::MemoryCommand(arg) => {
            spawn_memory_command(arg.clone(), project_dir.to_path_buf(), msg_tx.clone());
        }
        Msg::SkillsCommand => render_skills_list(state, project_dir),
        _ => {}
    }
}

fn spawn_memory_command(arg: String, project_dir: PathBuf, msg_tx: mpsc::Sender<Msg>) {
    tokio::spawn(async move {
        let memory_root = dirs_path().join("memory");
        let store =
            theo_application::facade::tooling::memory::FileMemoryStore::for_project(
                &memory_root,
                &project_dir,
            );
        let result = run_memory_command(&store, &arg).await;
        let _ = msg_tx.send(Msg::Notify(result)).await;
    });
}

async fn run_memory_command(
    store: &theo_application::facade::tooling::memory::FileMemoryStore,
    arg: &str,
) -> String {
    if arg.is_empty() || arg == "list" {
        match store.list().await {
            Ok(memories) if memories.is_empty() => {
                "No memories for this project.".to_string()
            }
            Ok(memories) => memories
                .iter()
                .map(|m| format!("  {}: {}", m.key, m.value))
                .collect::<Vec<_>>()
                .join("\n"),
            Err(e) => format!("Error: {e}"),
        }
    } else if let Some(query) = arg.strip_prefix("search ") {
        match store.search(query).await {
            Ok(results) if results.is_empty() => format!("No memories matching '{query}'"),
            Ok(results) => results
                .iter()
                .map(|m| format!("  {}: {}", m.key, m.value))
                .collect::<Vec<_>>()
                .join("\n"),
            Err(e) => format!("Error: {e}"),
        }
    } else if let Some(key) = arg.strip_prefix("delete ") {
        match store.delete(key).await {
            Ok(true) => format!("Deleted: {key}"),
            Ok(false) => format!("Not found: {key}"),
            Err(e) => format!("Error: {e}"),
        }
    } else {
        "Usage: /memory [list|search <q>|delete <key>]".to_string()
    }
}

fn render_skills_list(state: &mut TuiState, project_dir: &Path) {
    let mut registry =
        theo_application::facade::agent::skill::SkillRegistry::new();
    registry.load_bundled();
    let skills_dir = project_dir.join(".theo").join("skills");
    if skills_dir.exists() {
        registry.load_from_dir(&skills_dir);
    }
    let skills = registry.list();
    if skills.is_empty() {
        app::update(state, Msg::Notify("No skills available.".into()));
        return;
    }
    let list: Vec<String> = skills
        .iter()
        .map(|s| format!("  {} — {}", s.name, s.trigger))
        .collect();
    app::update(
        state,
        Msg::Notify(format!("{} skills:\n{}", skills.len(), list.join("\n"))),
    );
}

/// Handle the `Msg::LoginStart` slash command inline. Runs the OpenAI
/// device flow with `terminal.draw` between steps so the user sees the
/// verification code immediately. Returns `Ok(true)` when the caller
/// should `continue` (the cached-token short-circuit case), `Ok(false)`
/// when the slash-command loop should keep iterating.
async fn handle_login_start_inline(
    state: &mut TuiState,
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    msg_tx: &mpsc::Sender<Msg>,
    cmd_msg: Msg,
) -> anyhow::Result<bool> {
    app::update(state, cmd_msg); // shows "Starting..."
    let auth = theo_application::facade::auth::OpenAIAuth::with_default_store();

    if let Ok(Some(tokens)) = auth.get_tokens()
        && !tokens.is_expired()
    {
        app::update(
            state,
            Msg::LoginComplete("Already logged in (token valid)".into()),
        );
        return Ok(true);
    }

    app::update(state, Msg::Notify("Contacting auth server...".into()));
    terminal.draw(|frame| view::draw(frame, state))?;

    match auth.start_device_flow().await {
        Ok(code) => {
            app::update(state, Msg::Notify(SEPARATOR.into()));
            app::update(
                state,
                Msg::Notify(format!("1. Open: {}", code.verification_uri)),
            );
            app::update(
                state,
                Msg::Notify("2. Login to your OpenAI account if needed".into()),
            );
            app::update(
                state,
                Msg::Notify(format!("3. Enter code: {}", code.user_code)),
            );
            app::update(state, Msg::Notify("4. Click 'Authorize'".into()));
            app::update(state, Msg::Notify(SEPARATOR.into()));
            // Copy code to clipboard via OSC52.
            eprint!("\x1b]52;c;{}\x07", app::base64_encode(&code.user_code));
            app::update(
                state,
                Msg::Notify("Code copied to clipboard. Waiting...".into()),
            );
            terminal.draw(|frame| view::draw(frame, state))?;

            open_browser_silent(&code.verification_uri);

            let poll_tx = msg_tx.clone();
            tokio::spawn(async move {
                match auth.poll_device_flow(&code).await {
                    Ok(_) => {
                        let _ = poll_tx
                            .send(Msg::LoginComplete(
                                "✓ Authenticated with OpenAI!".into(),
                            ))
                            .await;
                    }
                    Err(e) => {
                        let _ = poll_tx
                            .send(Msg::LoginFailed(format!("Auth failed: {e}")))
                            .await;
                    }
                }
            });
        }
        Err(e) => {
            app::update(
                state,
                Msg::LoginFailed(format!("Device flow error: {e}")),
            );
        }
    }
    Ok(false)
}

/// Handle the `Msg::LoginServer(url)` slash command — generic
/// RFC 8628 device flow against an arbitrary server. The polling
/// future captures the access token into the `OPENAI_API_KEY` env var
/// so the next agent run picks it up.
async fn handle_login_server_inline(
    state: &mut TuiState,
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    msg_tx: &mpsc::Sender<Msg>,
    cmd_msg: Msg,
) -> anyhow::Result<()> {
    let server_url = if let Msg::LoginServer(ref u) = cmd_msg {
        u.clone()
    } else {
        unreachable!()
    };
    app::update(state, cmd_msg);
    let http = reqwest::Client::new();
    let config =
        theo_application::facade::auth::device_flow::DeviceFlowConfig::new(&server_url);

    app::update(state, Msg::Notify("Requesting device code...".into()));
    terminal.draw(|frame| view::draw(frame, state))?;

    match theo_application::facade::auth::device_flow::start_device_flow(&http, &config)
        .await
    {
        Ok(code) => {
            app::update(state, Msg::Notify(SEPARATOR.into()));
            app::update(
                state,
                Msg::Notify(format!("1. Open: {}", code.verification_url)),
            );
            app::update(
                state,
                Msg::Notify(format!("2. Enter code: {}", code.user_code)),
            );
            app::update(state, Msg::Notify("3. Authorize Theo".into()));
            app::update(state, Msg::Notify(SEPARATOR.into()));
            eprint!("\x1b]52;c;{}\x07", app::base64_encode(&code.user_code));
            app::update(
                state,
                Msg::Notify("Code copied to clipboard. Waiting...".into()),
            );
            terminal.draw(|frame| view::draw(frame, state))?;

            open_browser_silent(&code.verification_url);

            let poll_tx = msg_tx.clone();
            let poll_config = config.clone();
            let poll_code = code.clone();
            tokio::spawn(async move {
                let http = reqwest::Client::new();
                match theo_application::facade::auth::device_flow::poll_device_flow(
                    &http,
                    &poll_config,
                    &poll_code,
                )
                .await
                {
                    Ok(tokens) => {
                        // SAFETY: the TUI runtime owns the process-wide env
                        // table and this call happens on the single
                        // render-loop task; no other thread reads/writes env
                        // vars concurrently.
                        unsafe {
                            std::env::set_var("OPENAI_API_KEY", &tokens.access_token);
                        }
                        let _ = poll_tx
                            .send(Msg::LoginComplete(
                                "✓ Authenticated! Provider ready.".into(),
                            ))
                            .await;
                    }
                    Err(e) => {
                        let _ = poll_tx.send(Msg::LoginFailed(format!("{e}"))).await;
                    }
                }
            });
        }
        Err(e) => {
            app::update(state, Msg::LoginFailed(format!("Server error: {e}")));
        }
    }
    Ok(())
}

/// Open a URL in the default browser, suppressing all stdio so the
/// TUI display stays clean. No-op on unsupported platforms.
fn open_browser_silent(url: &str) {
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open")
            .arg(url)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg(url)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = url;
    }
}

const SEPARATOR: &str = "─────────────────────────────────────";

/// Spawn the agent task for a pending prompt. Re-resolves config to
/// pick up any tokens captured by an inline /login flow, attaches the
/// memory provider, snapshots the session history, and dispatches the
/// run on a tokio task. Result is forwarded to the TUI via
/// `Msg::AgentComplete`.
#[allow(clippy::too_many_arguments)]
async fn launch_agent_for_prompt(
    state: &mut TuiState,
    project_dir: &Path,
    event_bus: &Arc<EventBus>,
    session_messages: &mut Vec<Message>,
    injections: &theo_application::use_cases::run_agent_session::SubagentInjections,
    msg_tx: &mpsc::Sender<Msg>,
    prompt: String,
) {
    tui_log(&format!(
        "PROMPT TAKEN: '{}' agent_running={}",
        &prompt[..prompt.len().min(40)],
        state.agent_running
    ));
    if state.agent_running {
        return;
    }
    state.agent_running = true;
    tui_log("=== AGENT LAUNCH START ===");
    tui_log(&format!("Prompt: {}", &prompt[..prompt.len().min(80)]));

    // Re-resolve config to pick up tokens from login, then attach the
    // memory provider if memory is enabled (Phase 0 T0.2 —
    // run_agent_session's attach is on the outer config, which we
    // discard here, so redo it).
    let (mut fresh_config, fresh_provider) =
        crate::resolve_agent_config(None, None, None).await;
    theo_application::use_cases::memory_factory::attach_memory_to_config(
        &mut fresh_config,
        project_dir,
    );
    tui_log(&format!("Resolved provider: {fresh_provider}"));
    tui_log(&format!("Model: {}", fresh_config.llm.model));
    tui_log(&format!("Base URL: {}", fresh_config.llm.base_url));
    tui_log(&format!(
        "API key present: {}",
        fresh_config.llm.api_key.is_some()
    ));
    tui_log(&format!(
        "Endpoint override: {:?}",
        fresh_config.llm.endpoint_override
    ));

    if fresh_provider != "default" {
        state.status.provider = fresh_provider.clone();
        state.status.model = fresh_config.llm.model.clone();
    }

    let debug_msg = format!(
        "[debug] provider={} model={} key={} url={}",
        &state.status.provider,
        &fresh_config.llm.model,
        if fresh_config.llm.api_key.is_some() {
            "yes"
        } else {
            "NO"
        },
        &fresh_config.llm.base_url,
    );
    app::update(state, Msg::Notify(debug_msg));

    let task_config = fresh_config;
    let task_dir = project_dir.to_path_buf();
    let task_bus = event_bus.clone();
    let task_messages = session_messages.clone();
    let task_prompt = prompt.clone();
    let task_msg_tx = msg_tx.clone();
    let injections_for_task = injections.clone();

    session_messages.push(Message::user(&prompt));

    tokio::spawn(async move {
        run_agent_task(
            task_config,
            task_dir,
            task_bus,
            task_messages,
            task_prompt,
            task_msg_tx,
            injections_for_task,
        )
        .await;
    });
}

async fn run_agent_task(
    task_config: AgentConfig,
    task_dir: PathBuf,
    task_bus: Arc<EventBus>,
    task_messages: Vec<Message>,
    task_prompt: String,
    task_msg_tx: mpsc::Sender<Msg>,
    injections_for_task: theo_application::use_cases::run_agent_session::SubagentInjections,
) {
    tui_log("Agent task spawned");
    let mut cfg = task_config;
    cfg.context.system_prompt = system_prompt_for_mode(AgentMode::Agent);
    // Phase 52 (prompt-ab): operator-supplied system prompt via
    // THEO_SYSTEM_PROMPT_FILE — same fallback semantics as headless.
    if let Some(custom) = crate::prompt_override::override_from_env() {
        cfg.context.system_prompt = custom;
    }
    cfg.loop_cfg.mode = AgentMode::Agent;

    // T15.1 — populate docs_search index from project well-known locations.
    let registry = create_default_registry_with_project(&task_dir);

    // T14.1 — wire partial-progress streaming. The agent loop emits
    // envelopes through `tx`; a spawned drainer pulls them with 50 ms
    // debounce and forwards rendered lines as
    // `Msg::PartialProgressUpdate` to the TUI update path.
    let (partial_tx, partial_rx) = tokio::sync::mpsc::channel::<String>(64);
    let drainer_msg_tx = task_msg_tx.clone();
    let drainer_handle = tokio::spawn(async move {
        crate::render::partial_progress::run_drainer(partial_rx, move |lines| {
            let _ = drainer_msg_tx
                .try_send(crate::tui::app::Msg::PartialProgressUpdate(lines));
        })
        .await;
    });

    let agent = injections_for_task
        .apply_to(AgentLoop::new(cfg.clone(), registry))
        .with_partial_progress_tx(partial_tx);

    tui_log("AgentLoop created, calling run_with_history...");
    tui_log(&format!(
        "  api_key len: {}",
        cfg.llm.api_key.as_ref().map(|k| k.len()).unwrap_or(0)
    ));
    tui_log(&format!("  base_url: {}", cfg.llm.base_url));
    tui_log(&format!("  endpoint: {:?}", cfg.llm.endpoint_override));
    tui_log(&format!("  history msgs: {}", task_messages.len()));

    let result = agent
        .run_with_history(&task_prompt, &task_dir, task_messages, Some(task_bus))
        .await;

    tui_log(&format!(
        "Agent finished: success={} summary={}",
        result.success,
        &result.summary[..result.summary.len().min(100)]
    ));

    // T14.1 — agent's `partial_tx` clones drop with the AgentRunEngine;
    // the drainer sees the channel close and exits naturally. Await
    // its join handle so any in-flight final frame reaches the TUI
    // before we send AgentComplete.
    let _ = drainer_handle.await;
    // Clear the status line on agent exit.
    let _ = task_msg_tx
        .send(Msg::PartialProgressUpdate(Vec::new()))
        .await;
    let _ = task_msg_tx
        .send(Msg::AgentComplete(result.summary, result.success))
        .await;
}
