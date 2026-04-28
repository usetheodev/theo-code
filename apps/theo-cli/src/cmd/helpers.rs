//! Helper functions used across cmd_* handlers (T5.3.b of god-files-2026-07-23-plan.md, ADR D6).

#![allow(unused_imports, dead_code)]

use std::path::{Path, PathBuf};
use std::time::Instant;

use theo_application::use_cases::pipeline::{Pipeline, PipelineConfig};

use crate::*;

pub fn resolve_dir(path: PathBuf) -> PathBuf {
    if path == Path::new(".") {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    } else {
        path
    }
}

pub fn build_fresh(
    pipeline: &mut Pipeline,
    repo_path: &Path,
    cache_dir: &Path,
    cache_path: &Path,
) -> (
    u128,
    u128,
    u128,
    Vec<theo_application::use_cases::pipeline::Community>,
) {
    let t = Instant::now();
    let (files, _) = theo_application::use_cases::extraction::extract_repo(repo_path);
    pipeline.build_graph(&files);
    let graph_ms = t.elapsed().as_millis();

    let t = Instant::now();
    let _ = pipeline.add_git_cochanges(repo_path);
    let git_ms = t.elapsed().as_millis();

    let t = Instant::now();
    pipeline.cluster();
    let communities = pipeline.communities().to_vec();
    let cluster_ms = t.elapsed().as_millis();

    if let Err(e) = std::fs::create_dir_all(cache_dir) {
        eprintln!("[cache] Cannot create dir: {}", e);
    } else {
        if let Err(e) = pipeline.save_graph(&cache_path.to_string_lossy()) {
            eprintln!("[cache] Cannot save graph: {}", e);
        }
        let cluster_cache = cache_dir.join("clusters.bin");
        if let Err(e) = pipeline.save_clusters(&cluster_cache.to_string_lossy()) {
            eprintln!("[cache] Cannot save clusters: {}", e);
        }
        let summaries_cache = cache_dir.join("summaries.bin");
        if let Err(e) = pipeline.save_summaries(&summaries_cache.to_string_lossy()) {
            eprintln!("[cache] Cannot save summaries: {}", e);
        } else {
            eprintln!(
                "[cache] Saved graph + clusters + summaries to {}",
                cache_dir.display()
            );
        }
    }

    (graph_ms, git_ms, cluster_ms, communities)
}

pub async fn resolve_agent_config(
    provider_id: Option<&str>,
    model: Option<&str>,
    max_iter: Option<usize>,
) -> (theo_application::facade::agent::AgentConfig, String) {
    use theo_application::facade::llm::provider_registry::create_default_registry
        as create_provider_registry;

    let mut config = theo_application::facade::agent::AgentConfig::default();
    // Opt-in flag for the memory subsystem (G1–G10). Default stays `false`
    // for backward-compat (test_pre5_ac_1_memory_enabled_default_false).
    // Set `THEO_MEMORY=1` (or any non-empty value) to activate every hook.
    if std::env::var("THEO_MEMORY").map(|v| !v.is_empty()).unwrap_or(false) {
        config.memory.enabled = true;
        eprintln!("[theo] THEO_MEMORY=1 detected — memory subsystem active");
    }
    let mut provider_name = "default".to_string();

    let mut api_key: Option<String> = None;
    let mut oauth_applied = false;

    if provider_id.is_none() {
        let auth = theo_application::facade::auth::OpenAIAuth::with_default_store();
        if let Ok(Some(tokens)) = auth.get_tokens()
            && !tokens.is_expired() {
                api_key = Some(tokens.access_token.clone());
                oauth_applied = true;
            }
    }

    let registry = create_provider_registry();

    if let Some(pid) = provider_id {
        if let Some(spec) = registry.get(pid) {
            config.llm.base_url = spec.base_url.to_string();
            config.llm.endpoint_override = Some(spec.endpoint_url());
            config.llm.api_key = api_key.or_else(|| {
                spec.api_key_env_var()
                    .and_then(|var| std::env::var(var).ok())
            });
            provider_name = spec.display_name.to_string();
        } else {
            eprintln!("Unknown provider: {pid}");
            std::process::exit(1);
        }
    } else if oauth_applied {
        if let Some(spec) = registry.get("chatgpt-codex") {
            config.llm.base_url = spec.base_url.to_string();
            config.llm.endpoint_override = Some(spec.endpoint_url());
            config.llm.api_key = api_key;
            provider_name = spec.display_name.to_string();

            let auth = theo_application::facade::auth::OpenAIAuth::with_default_store();
            if let Ok(Some(tokens)) = auth.get_tokens()
                && let Some(ref account_id) = tokens.account_id {
                    config
                        .llm
                        .extra_headers
                        .insert("ChatGPT-Account-Id".to_string(), account_id.clone());
                }
        }
    } else if let Ok(key) = std::env::var("OPENAI_API_KEY")
        && let Some(spec) = registry.get("openai") {
            config.llm.base_url = spec.base_url.to_string();
            config.llm.endpoint_override = Some(spec.endpoint_url());
            config.llm.api_key = Some(key);
            provider_name = "OpenAI".to_string();
        }

    if let Some(m) = model {
        config.llm.model = m.to_string();
    } else if oauth_applied && config.llm.model == "default" {
        // Default to gpt-5.4 ("current strong everyday").
        //
        // ChatGPT-account OAuth supports a SUBSET of the catalog
        // (verified live against chatgpt.com/backend-api/codex/responses
        // on 2026-04-24):
        //   ✅ gpt-5.4, gpt-5.4-mini, gpt-5.3-codex, gpt-5.2
        //   ❌ gpt-5.2-codex, gpt-5.1-codex-max, gpt-5.1-codex-mini
        //      (these return: "not supported when using Codex with a
        //       ChatGPT account" — they require API-key auth)
        //
        // See `theo_application::use_cases::router_loader::CHATGPT_OAUTH_SUPPORTED_MODELS`
        // for the canonical allowlist + startup warning when slots
        // misconfigure to an unsupported model.
        config.llm.model = "gpt-5.4".to_string();
    }

    if let Some(n) = max_iter {
        config.loop_cfg.max_iterations = n;
    }

    if config.llm.reasoning_effort.is_none() {
        config.llm.reasoning_effort = Some("medium".to_string());
    }

    (config, provider_name)
}
