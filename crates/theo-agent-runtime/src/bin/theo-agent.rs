use std::path::PathBuf;
use std::sync::Arc;

use theo_agent_runtime::event_bus::PrintEventListener;
use theo_agent_runtime::{AgentConfig, AgentLoop};
use theo_infra_auth::OpenAIAuth;
use theo_infra_llm::provider::registry::create_default_registry as create_provider_registry;
use theo_tooling::registry::create_default_registry;

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  theo-agent --repo <path> --task <task> [options]");
    eprintln!("  theo-agent auth login [--device]");
    eprintln!("  theo-agent auth status");
    eprintln!("  theo-agent auth logout");
    eprintln!();
    eprintln!("Agent options:");
    eprintln!("  --repo <path>        Path to the repository to work on");
    eprintln!("  --task <task>        Task description for the agent");
    eprintln!(
        "  --provider <id>      LLM provider (openai, ollama, groq, anthropic, chatgpt-codex, ...)"
    );
    eprintln!("  --url <url>          LLM API base URL (legacy, overrides --provider)");
    eprintln!("  --model <model>      Model name (default: provider-specific)");
    eprintln!("  --api-key <key>      API key (default: from env or OAuth)");
    eprintln!("  --max-iter <n>       Maximum iterations (default: 15)");
    eprintln!();
    eprintln!("Provider auto-detection (when --provider and --url omitted):");
    eprintln!("  1. OAuth token present → chatgpt-codex");
    eprintln!("  2. OPENAI_API_KEY set → openai");
    eprintln!("  3. ANTHROPIC_API_KEY set → anthropic");
    eprintln!("  4. Ollama running (localhost:11434) → ollama");
    eprintln!("  5. vLLM running (localhost:8000) → vllm");
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
    let mut provider_id: Option<String> = None;

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
            "--provider" => {
                provider_id = args.get(i + 1).cloned();
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

    // ── Provider Resolution ──
    // Priority: --url (legacy) > --provider (explicit) > auto-detect
    let mut config = AgentConfig::default();

    if let Some(ref raw_url) = url {
        // Legacy mode: raw URL
        config.llm.base_url = raw_url.clone();
        config.llm.model = model.unwrap_or_else(|| std::env::var("MODEL_NAME").unwrap_or(config.llm.model));
        config.llm.api_key = api_key
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok());
        eprintln!("[theo] Using legacy URL: {}", config.llm.base_url);
    } else {
        // Provider-based resolution
        let resolved = resolve_provider(provider_id.as_deref(), &api_key).await;
        let provider_registry = create_provider_registry();

        match provider_registry.get(&resolved.provider_id) {
            Some(spec) => {
                config.llm.base_url = spec.base_url.to_string();
                config.llm.endpoint_override = Some(spec.endpoint_url());
                config.llm.api_key = resolved.api_key;
                config.llm.model = model.unwrap_or(resolved.default_model);

                // Dynamic headers (e.g., ChatGPT-Account-Id for Codex)
                for (k, v) in resolved.extra_headers {
                    config.llm.extra_headers.insert(k, v);
                }

                eprintln!("[theo] Provider: {} ({})", spec.display_name, spec.id);
            }
            None => {
                eprintln!("Error: unknown provider '{}'", resolved.provider_id);
                eprintln!("Available providers:");
                for pid in provider_registry.list() {
                    eprintln!("  - {}", pid);
                }
                std::process::exit(1);
            }
        }
    }

    if let Some(n) = max_iter {
        config.max_iterations = n;
    }

    eprintln!("╔══════════════════════════════════╗");
    eprintln!("║        theo-agent v0.1.0         ║");
    eprintln!("╠══════════════════════════════════╣");
    eprintln!("║ Model: {:<24} ║", config.llm.model);
    eprintln!(
        "║ URL:   {:<24} ║",
        config
            .llm
            .endpoint_override
            .as_deref()
            .unwrap_or(&config.llm.base_url)
    );
    eprintln!("║ Repo:  {:<24} ║", repo);
    eprintln!("║ Max:   {:<24} ║", config.max_iterations);
    eprintln!("╚══════════════════════════════════╝");
    eprintln!();
    eprintln!("Task: {task}");
    eprintln!();

    let registry = create_default_registry();
    let event_listener = Arc::new(PrintEventListener);
    let agent = AgentLoop::new(config, registry).with_event_listener(event_listener);
    let result = agent.run(&task, &project_dir).await;

    eprintln!();
    eprintln!("═══ Result ═══");
    eprintln!("Success: {}", result.success);
    eprintln!("Iterations: {}", result.iterations_used);
    eprintln!("Files edited: {}", result.files_edited.join(", "));
    eprintln!("Summary: {}", result.summary);

    std::process::exit(if result.success { 0 } else { 1 });
}

/// Resolved provider configuration.
struct ResolvedProvider {
    provider_id: String,
    api_key: Option<String>,
    default_model: String,
    extra_headers: Vec<(String, String)>,
}

/// Resolves which provider to use based on explicit flag or auto-detection.
async fn resolve_provider(
    explicit_provider: Option<&str>,
    explicit_api_key: &Option<String>,
) -> ResolvedProvider {
    // 1. Explicit --provider
    if let Some(pid) = explicit_provider {
        return resolve_explicit_provider(pid, explicit_api_key);
    }

    // 2. Auto-detect: OAuth → env vars → local servers
    // Check OAuth first
    if let Some(resolved) = try_oauth_provider().await {
        return resolved;
    }

    // Check env vars
    if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        eprintln!("[theo] Auto-detected: OPENAI_API_KEY → provider openai");
        return ResolvedProvider {
            provider_id: "openai".to_string(),
            api_key: Some(key),
            default_model: "gpt-4o-mini".to_string(),
            extra_headers: vec![],
        };
    }

    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        eprintln!("[theo] Auto-detected: ANTHROPIC_API_KEY → provider anthropic");
        return ResolvedProvider {
            provider_id: "anthropic".to_string(),
            api_key: Some(key),
            default_model: "claude-sonnet-4-20250514".to_string(),
            extra_headers: vec![],
        };
    }

    // Check local servers (with timeout)
    if check_local_server("localhost:11434").await {
        eprintln!("[theo] Auto-detected: Ollama running → provider ollama");
        return ResolvedProvider {
            provider_id: "ollama".to_string(),
            api_key: None,
            default_model: "qwen2.5:0.5b".to_string(),
            extra_headers: vec![],
        };
    }

    if check_local_server("localhost:8000").await {
        eprintln!("[theo] Auto-detected: vLLM running → provider vllm");
        return ResolvedProvider {
            provider_id: "vllm".to_string(),
            api_key: None,
            default_model: "default".to_string(),
            extra_headers: vec![],
        };
    }

    // Nothing found
    eprintln!("Error: no LLM provider detected.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  theo-agent auth login                    # OAuth with OpenAI");
    eprintln!("  theo-agent --provider ollama             # Local Ollama");
    eprintln!("  OPENAI_API_KEY=sk-... theo-agent ...     # OpenAI API key");
    eprintln!("  theo-agent --url http://... --model ...  # Custom endpoint");
    std::process::exit(1);
}

/// Resolve explicit --provider flag.
fn resolve_explicit_provider(pid: &str, explicit_api_key: &Option<String>) -> ResolvedProvider {
    let api_key = explicit_api_key.clone().or_else(|| {
        let registry = create_provider_registry();
        registry.get(pid).and_then(|spec| {
            spec.api_key_env_var()
                .and_then(|var| std::env::var(var).ok())
        })
    });

    let default_model = match pid {
        "openai" => "gpt-4o-mini".to_string(),
        "anthropic" => "claude-sonnet-4-20250514".to_string(),
        "ollama" => "qwen2.5:0.5b".to_string(),
        "groq" => "llama-3.3-70b-versatile".to_string(),
        "chatgpt-codex" => "gpt-5.3-codex".to_string(),
        _ => "default".to_string(),
    };

    ResolvedProvider {
        provider_id: pid.to_string(),
        api_key,
        default_model,
        extra_headers: vec![],
    }
}

/// Try to use OAuth token for chatgpt-codex provider.
async fn try_oauth_provider() -> Option<ResolvedProvider> {
    let auth = OpenAIAuth::with_default_store();
    let tokens = match auth.get_tokens() {
        Ok(Some(t)) if !t.is_expired() => t,
        _ => return None,
    };

    let mut extra_headers = vec![];
    if let Some(ref account_id) = tokens.account_id {
        extra_headers.push(("ChatGPT-Account-Id".to_string(), account_id.clone()));
    }

    eprintln!("[theo] Auto-detected: OAuth token → provider chatgpt-codex");

    Some(ResolvedProvider {
        provider_id: "chatgpt-codex".to_string(),
        api_key: Some(tokens.access_token),
        default_model: "gpt-5.3-codex".to_string(),
        extra_headers,
    })
}

/// Check if a local server is responding (1s timeout).
async fn check_local_server(addr: &str) -> bool {
    // Parse host:port from URL-like string
    let addr = addr
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or(addr);

    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        tokio::net::TcpStream::connect(addr),
    )
    .await
    .is_ok_and(|r| r.is_ok())
}

// ── Auth Subcommands ──

async fn handle_auth(args: &[String]) {
    let subcommand = args.first().map(|s| s.as_str()).unwrap_or("status");

    match subcommand {
        "login" => {
            let device_flow = args.get(1).map(|s| s.as_str()) == Some("--device");
            let auth = OpenAIAuth::with_default_store();

            if device_flow {
                eprintln!("Starting device flow...");
                match auth.start_device_flow().await {
                    Ok(device) => {
                        eprintln!("Please visit: {}", device.verification_uri);
                        eprintln!("Enter code: {}", device.user_code);
                        eprintln!();
                        eprintln!("Waiting for authorization...");

                        match auth.poll_device_flow(&device).await {
                            Ok(tokens) => {
                                eprintln!("Login successful!");
                                if let Some(ref id) = tokens.account_id {
                                    eprintln!("Account: {id}");
                                }
                            }
                            Err(e) => {
                                eprintln!("Device flow failed: {e}");
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
                eprintln!("Opening browser for OpenAI login...");
                match auth.login_browser().await {
                    Ok(tokens) => {
                        eprintln!("Login successful!");
                        if let Some(ref id) = tokens.account_id {
                            eprintln!("Account: {id}");
                        }
                    }
                    Err(e) => {
                        eprintln!("Login failed: {e}");
                        std::process::exit(1);
                    }
                }
            }
        }
        "status" => {
            let auth = OpenAIAuth::with_default_store();
            match auth.get_tokens() {
                Ok(Some(tokens)) => {
                    if tokens.is_expired() {
                        eprintln!("Status: EXPIRED");
                        eprintln!("Run `theo-agent auth login` to re-authenticate.");
                    } else {
                        eprintln!("Status: AUTHENTICATED");
                        if let Some(ref id) = tokens.account_id {
                            eprintln!("Account: {id}");
                        }
                        if let Some(exp) = tokens.expires_at {
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_secs();
                            if exp > now {
                                let remaining = exp - now;
                                let hours = remaining / 3600;
                                let minutes = (remaining % 3600) / 60;
                                eprintln!("Expires in: {hours}h {minutes}m");
                            }
                        }
                        eprintln!("Has refresh token: {}", tokens.refresh_token.is_some());
                    }
                }
                Ok(None) => {
                    eprintln!("Status: NOT AUTHENTICATED");
                    eprintln!("Run `theo-agent auth login` to authenticate with OpenAI.");
                }
                Err(e) => {
                    eprintln!("Error reading auth state: {e}");
                }
            }
        }
        "logout" => {
            let auth = OpenAIAuth::with_default_store();
            match auth.logout() {
                Ok(()) => eprintln!("Logged out successfully."),
                Err(e) => eprintln!("Logout error: {e}"),
            }
        }
        _ => {
            eprintln!("Unknown auth command: {subcommand}");
            eprintln!("Usage: theo-agent auth [login|status|logout]");
        }
    }
}
