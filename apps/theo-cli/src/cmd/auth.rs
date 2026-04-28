//! Single-cmd slice extracted from `cmd.rs` (T5.3.b of god-files-2026-07-23-plan.md, ADR D6).

#![allow(unused_imports, dead_code)]

use std::path::{Path, PathBuf};
use std::time::Instant;

use theo_application::use_cases::pipeline::{Pipeline, PipelineConfig};

use crate::*;
use super::helpers::*;

pub fn cmd_login(key: Option<String>, server: Option<String>, no_browser: bool) -> i32 {
    use theo_application::use_cases::auth;

    // Path 1: API key direct persistence.
    if let Some(raw) = key {
        let store = theo_application::facade::auth::AuthStore::open();
        match auth::save_api_key(&store, &raw) {
            Ok(_) => {
                eprintln!("✓ Saved API key: {}", auth::mask_key(raw.trim()));
                0
            }
            Err(e) => {
                eprintln!("✗ save failed: {e}");
                1
            }
        }
    } else if let Some(url) = server {
        eprintln!("✗ `--server {url}` is not yet wired in the headless CLI.");
        eprintln!("  Use the TUI `/login {url}` (Ctrl+C then run `theo`) for the generic RFC 8628 flow.");
        1
    } else {
        // Path 2: OpenAI OAuth device flow.
        let rt = match tokio::runtime::Runtime::new() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("✗ failed to create tokio runtime: {e}");
                return 1;
            }
        };
        rt.block_on(async { run_oauth_device_flow(no_browser).await })
    }
}

/// Run the OpenAI device-flow end-to-end, printing UX prompts to stderr.
pub async fn run_oauth_device_flow(no_browser: bool) -> i32 {
    let auth_client = theo_application::facade::auth::OpenAIAuth::with_default_store();
    eprintln!("Contacting OpenAI authorization server...");
    let code = match auth_client.start_device_flow().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("✗ device flow failed: {e}");
            return 1;
        }
    };
    eprintln!();
    eprintln!("─────────────────────────────────────");
    eprintln!("1. Open:  {}", code.verification_uri);
    eprintln!("2. Enter code:  {}", code.user_code);
    eprintln!("3. Authorize the Theo application.");
    eprintln!("─────────────────────────────────────");
    eprintln!();
    if !no_browser {
        let _ = open_browser(&code.verification_uri);
    }
    eprintln!("Waiting for authorization…");
    match auth_client.poll_device_flow(&code).await {
        Ok(_) => {
            eprintln!("✓ Authenticated with OpenAI. Tokens saved.");
            0
        }
        Err(e) => {
            eprintln!("✗ authorization failed: {e}");
            1
        }
    }
}

/// Best-effort browser opener for the device-flow URL. Linux uses
/// `xdg-open`, macOS uses `open`. Failures are silent.
pub fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    let program = "xdg-open";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(not(target_os = "linux"), not(target_os = "macos")))]
    let program = "true"; // noop on unsupported platforms
    std::process::Command::new(program)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
}

/// `theo logout` — clear saved OpenAI credentials.
pub fn cmd_logout() -> i32 {
    use theo_application::use_cases::auth;
    let store = theo_application::facade::auth::AuthStore::open();
    match auth::logout(&store) {
        Ok(true) => {
            eprintln!("✓ Logged out. Saved credentials cleared.");
            0
        }
        Ok(false) => {
            eprintln!("Nothing to log out of — no OpenAI credentials were saved.");
            0
        }
        Err(e) => {
            eprintln!("✗ logout failed: {e}");
            1
        }
    }
}

