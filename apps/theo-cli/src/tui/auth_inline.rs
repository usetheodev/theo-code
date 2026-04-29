//! Inline `/login` and `/login server <url>` slash-command handlers.
//!
//! Extracted from `tui/mod.rs` (size-budget split — keeps `mod.rs` ≤ 800 LOC).
//! Both handlers run with `terminal.draw` between Notify events so the
//! verification code reaches the user mid-flow. The polling future is
//! always spawned onto the tokio runtime and reports back via
//! `Msg::LoginComplete` / `Msg::LoginFailed`.

use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use super::app::{self, Msg, TuiState};
use super::view;

/// Visual divider used between the device-flow steps in the transcript.
const SEPARATOR: &str = "─────────────────────────────────────";

/// Handle the `Msg::LoginStart` slash command inline. Runs the OpenAI
/// device flow with `terminal.draw` between steps so the user sees the
/// verification code immediately. Returns `Ok(true)` when the caller
/// should `continue` (the cached-token short-circuit case), `Ok(false)`
/// when the slash-command loop should keep iterating.
pub(super) async fn handle_login_start_inline(
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
            print_openai_device_steps(state, terminal, &code)?;
            spawn_openai_poll(auth, code, msg_tx.clone());
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

fn print_openai_device_steps(
    state: &mut TuiState,
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    code: &theo_application::facade::auth::openai::DeviceCode,
) -> anyhow::Result<()> {
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
    Ok(())
}

fn spawn_openai_poll(
    auth: theo_application::facade::auth::OpenAIAuth,
    code: theo_application::facade::auth::openai::DeviceCode,
    poll_tx: mpsc::Sender<Msg>,
) {
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

/// Handle the `Msg::LoginServer(url)` slash command — generic
/// RFC 8628 device flow against an arbitrary server. The polling
/// future captures the access token into the `OPENAI_API_KEY` env var
/// so the next agent run picks it up.
pub(super) async fn handle_login_server_inline(
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
            print_generic_device_steps(state, terminal, &code)?;
            spawn_generic_poll(config.clone(), code.clone(), msg_tx.clone());
        }
        Err(e) => {
            app::update(state, Msg::LoginFailed(format!("Server error: {e}")));
        }
    }
    Ok(())
}

fn print_generic_device_steps(
    state: &mut TuiState,
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    code: &theo_application::facade::auth::device_flow::DeviceFlowCode,
) -> anyhow::Result<()> {
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
    Ok(())
}

fn spawn_generic_poll(
    poll_config: theo_application::facade::auth::device_flow::DeviceFlowConfig,
    poll_code: theo_application::facade::auth::device_flow::DeviceFlowCode,
    poll_tx: mpsc::Sender<Msg>,
) {
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

/// Open a URL in the default browser, suppressing all stdio so the
/// TUI display stays clean. No-op on unsupported platforms.
pub(super) fn open_browser_silent(url: &str) {
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
