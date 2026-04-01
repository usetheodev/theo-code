use std::path::PathBuf;
use std::sync::Arc;

use theo_agent_runtime::{AgentConfig, AgentLoop};
use theo_agent_runtime::events::PrintEventSink;
use theo_infra_auth::OpenAIAuth;
use theo_tooling::registry::create_default_registry;

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  theo-agent --repo <path> --task <task> [options]");
    eprintln!("  theo-agent auth login [--device]");
    eprintln!("  theo-agent auth status");
    eprintln!("  theo-agent auth logout");
    eprintln!();
    eprintln!("Agent options:");
    eprintln!("  --repo <path>      Path to the repository to work on");
    eprintln!("  --task <task>      Task description for the agent");
    eprintln!("  --url <url>        LLM API base URL (default: http://localhost:8000)");
    eprintln!("  --model <model>    Model name (default: from env or 'default')");
    eprintln!("  --api-key <key>    API key (default: from env OPENAI_API_KEY)");
    eprintln!("  --max-iter <n>     Maximum iterations (default: 15)");
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Handle `auth` subcommand
    if args.get(1).map(|s| s.as_str()) == Some("auth") {
        handle_auth(&args[2..]).await;
        return;
    }

    let mut repo: Option<String> = None;
    let mut task: Option<String> = None;
    let mut url: Option<String> = None;
    let mut model: Option<String> = None;
    let mut api_key: Option<String> = None;
    let mut max_iter: Option<usize> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--repo" => {
                repo = args.get(i + 1).cloned();
                i += 2;
            }
            "--task" => {
                task = args.get(i + 1).cloned();
                i += 2;
            }
            "--url" => {
                url = args.get(i + 1).cloned();
                i += 2;
            }
            "--model" => {
                model = args.get(i + 1).cloned();
                i += 2;
            }
            "--api-key" => {
                api_key = args.get(i + 1).cloned();
                i += 2;
            }
            "--max-iter" => {
                max_iter = args.get(i + 1).and_then(|s| s.parse().ok());
                i += 2;
            }
            "--help" | "-h" => {
                print_usage();
                return;
            }
            _ => {
                eprintln!("Unknown argument: {}", args[i]);
                print_usage();
                std::process::exit(1);
            }
        }
    }

    let Some(repo) = repo else {
        eprintln!("Error: --repo is required");
        print_usage();
        std::process::exit(1);
    };

    let Some(task) = task else {
        eprintln!("Error: --task is required");
        print_usage();
        std::process::exit(1);
    };

    let project_dir = PathBuf::from(&repo);
    if !project_dir.exists() {
        eprintln!("Error: repo path does not exist: {repo}");
        std::process::exit(1);
    }

    // Try to get API key from OAuth if not provided
    if api_key.is_none() {
        let auth = OpenAIAuth::with_default_store();
        if let Ok(Some(tokens)) = auth.get_tokens() {
            if !tokens.is_expired() {
                api_key = Some(tokens.access_token);
            }
        }
    }

    let mut config = AgentConfig::default();
    config.base_url = url
        .or_else(|| std::env::var("VLLM_URL").ok())
        .or_else(|| std::env::var("OPENAI_BASE_URL").ok())
        .unwrap_or(config.base_url);
    config.model = model
        .or_else(|| std::env::var("MODEL_NAME").ok())
        .unwrap_or(config.model);
    config.api_key = api_key
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok());
    if let Some(n) = max_iter {
        config.max_iterations = n;
    }

    eprintln!("╔══════════════════════════════════╗");
    eprintln!("║        theo-agent v0.1.0         ║");
    eprintln!("╠══════════════════════════════════╣");
    eprintln!("║ Model: {:<24} ║", config.model);
    eprintln!("║ URL:   {:<24} ║", config.base_url);
    eprintln!("║ Repo:  {:<24} ║", repo);
    eprintln!("║ Max:   {:<24} ║", config.max_iterations);
    eprintln!("╚══════════════════════════════════╝");
    eprintln!();
    eprintln!("Task: {task}");
    eprintln!();

    let registry = create_default_registry();
    let event_sink = Arc::new(PrintEventSink);
    let agent = AgentLoop::new(config, registry, event_sink);

    let result = agent.run(&task, &project_dir).await;

    eprintln!();
    eprintln!("═══ Result ═══");
    eprintln!("Success: {}", result.success);
    eprintln!("Iterations: {}", result.iterations_used);
    eprintln!("Files edited: {}", result.files_edited.join(", "));
    eprintln!("Summary: {}", result.summary);

    std::process::exit(if result.success { 0 } else { 1 });
}

async fn handle_auth(args: &[String]) {
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    let auth = OpenAIAuth::with_default_store();

    match subcmd {
        "login" => {
            let use_device = args.iter().any(|a| a == "--device");

            if use_device {
                // Device flow
                eprintln!("Starting device authorization flow...");
                match auth.start_device_flow().await {
                    Ok(dc) => {
                        eprintln!();
                        eprintln!("╔══════════════════════════════════════════╗");
                        eprintln!("║  Open this URL in your browser:          ║");
                        eprintln!("║  {:<40} ║", dc.verification_uri);
                        eprintln!("║                                          ║");
                        eprintln!("║  Enter code: {:<27} ║", dc.user_code);
                        eprintln!("╚══════════════════════════════════════════╝");
                        eprintln!();
                        eprintln!("Waiting for authorization...");

                        match auth.poll_device_flow(&dc).await {
                            Ok(tokens) => {
                                eprintln!("Login successful!");
                                if let Some(id) = &tokens.account_id {
                                    eprintln!("Account: {id}");
                                }
                            }
                            Err(e) => {
                                eprintln!("Login failed: {e}");
                                std::process::exit(1);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to start device flow: {e}");
                        std::process::exit(1);
                    }
                }
            } else {
                // Browser flow
                eprintln!("Opening browser for OpenAI login...");
                match auth.login_browser().await {
                    Ok(tokens) => {
                        eprintln!("Login successful!");
                        if let Some(id) = &tokens.account_id {
                            eprintln!("Account: {id}");
                        }
                    }
                    Err(e) => {
                        eprintln!("Login failed: {e}");
                        eprintln!();
                        eprintln!("Tip: try `theo-agent auth login --device` for headless login.");
                        std::process::exit(1);
                    }
                }
            }
        }
        "status" => {
            match auth.get_tokens() {
                Ok(Some(tokens)) => {
                    if tokens.is_expired() {
                        eprintln!("Status: EXPIRED");
                        eprintln!("Run `theo-agent auth login` to re-authenticate.");
                    } else {
                        eprintln!("Status: AUTHENTICATED");
                        if let Some(id) = &tokens.account_id {
                            eprintln!("Account: {id}");
                        }
                        if let Some(exp) = tokens.expires_at {
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();
                            let remaining = exp.saturating_sub(now);
                            let hours = remaining / 3600;
                            let mins = (remaining % 3600) / 60;
                            eprintln!("Expires in: {hours}h {mins}m");
                        }
                        eprintln!("Has refresh token: {}", tokens.refresh_token.is_some());
                    }
                }
                Ok(None) => {
                    eprintln!("Status: NOT AUTHENTICATED");
                    eprintln!("Run `theo-agent auth login` to authenticate with OpenAI.");
                }
                Err(e) => {
                    eprintln!("Error reading auth store: {e}");
                    std::process::exit(1);
                }
            }
        }
        "logout" => {
            match auth.logout() {
                Ok(()) => eprintln!("Logged out successfully."),
                Err(e) => {
                    eprintln!("Logout failed: {e}");
                    std::process::exit(1);
                }
            }
        }
        _ => {
            eprintln!("Unknown auth command: {subcmd}");
            eprintln!("Available: login, status, logout");
            std::process::exit(1);
        }
    }
}
