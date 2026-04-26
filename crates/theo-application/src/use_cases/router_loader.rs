//! Phase 27 follow-up (sota-gaps-followup gap #4): build a production
//! `AutomaticModelRouter` from `.theo/config.toml`.
//!
//! Loose contract: missing config OR missing `[routing]` block returns
//! `None`. The caller falls back to the legacy "no_router" path
//! (model from AgentConfig.model). When a router IS built, every
//! routing decision is recorded via the supplied callback so the CLI
//! can aggregate them into `MetricsCollector` post-run.

use std::path::Path;
use std::sync::Arc;

use theo_infra_llm::routing::{
    auto::RoutingMetricsRecorder, AutomaticModelRouter, RoutingConfig, RuleBasedRouter,
    env_disables_routing,
};

const PROJECT_CONFIG_PATH: &str = ".theo/config.toml";

/// Phase 29 follow-up — gap discovered during real OAuth probing.
/// The ChatGPT-account OAuth endpoint
/// (`chatgpt.com/backend-api/codex/responses`) only serves a subset of
/// the catalog. API-key-only models return:
///   `"detail":"The 'X' model is not supported when using Codex with a
///    ChatGPT account."`
/// This list is the empirical allowlist verified against the real
/// endpoint on 2026-04-24. Models outside the list trigger a startup
/// warning so operators discover the misconfiguration BEFORE the first
/// agent run instead of mid-conversation.
pub const CHATGPT_OAUTH_SUPPORTED_MODELS: &[&str] = &[
    "gpt-5.4",
    "gpt-5.4-mini",
    "gpt-5.3-codex",
    "gpt-5.2",
];

/// True when `model` is known to work with ChatGPT-account OAuth.
pub fn is_chatgpt_oauth_supported(model: &str) -> bool {
    CHATGPT_OAUTH_SUPPORTED_MODELS.contains(&model)
}

#[derive(serde::Deserialize)]
struct Wrapper {
    #[serde(default)]
    routing: Option<RoutingConfig>,
}

/// Build an `AutomaticModelRouter` (with optional metrics recorder)
/// from `<project_dir>/.theo/config.toml`. Returns `None` when:
/// - The file is absent or unreadable
/// - The `[routing]` block is missing
/// - `routing.enabled = false`
/// - `THEO_ROUTING_DISABLED=1` is set in the env
/// - The slot map is empty (nothing to route to)
///
/// T10.1 — The returned router honours `routing.cost_aware`:
///   - `cost_aware = true`  → classifier picks Cheap/Default/Strong
///                            (SOTA-default; ≥20% cost reduction A/B)
///   - `cost_aware = false` → always picks the default slot
///   The flag also reads `THEO_ROUTING_COST_AWARE` (0/false/off/no →
///   classifier off) so operators can disable per-shell without
///   editing config.
pub fn load_router(
    project_dir: &Path,
    metrics_recorder: Option<RoutingMetricsRecorder>,
) -> Option<Arc<dyn theo_domain::routing::ModelRouter>> {
    let path = project_dir.join(PROJECT_CONFIG_PATH);
    let raw = std::fs::read_to_string(&path).ok()?;
    let wrapper: Wrapper = toml::from_str(&raw).ok()?;
    let config = wrapper.routing?;
    let config = config
        .apply_overrides(env_disables_routing(), false)
        .apply_cost_aware_override(theo_infra_llm::routing::env_disables_cost_aware());
    if !config.enabled || config.slots.is_empty() {
        return None;
    }
    // Validate slot models against the ChatGPT-account OAuth allowlist.
    // Print a one-time startup warning per unsupported slot so the
    // operator notices BEFORE the first agent run blows up mid-stream.
    for (alias, slot) in &config.slots {
        if slot.provider == "chatgpt-codex" && !is_chatgpt_oauth_supported(&slot.model)
        {
            eprintln!(
                "[theo:router] WARNING: slot '{}' uses model '{}' which is NOT supported \
                 with ChatGPT-account OAuth. Supported: {:?}. The slot will route to a \
                 server_error response. Switch to one of the supported models.",
                alias, slot.model, CHATGPT_OAUTH_SUPPORTED_MODELS
            );
        }
    }

    let inner = RuleBasedRouter::new(config.to_pricing_table());
    // T10.1 — pass `cost_aware` (was hard-coded `true`). When false,
    // AutomaticModelRouter passes through to the inner RuleBasedRouter
    // without classification, so callers always get the default slot.
    let mut auto = AutomaticModelRouter::new(inner, config.cost_aware);
    if let Some(recorder) = metrics_recorder {
        auto = auto.with_metrics(recorder);
    }
    Some(Arc::new(auto))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::TempDir;

    fn fixture(content: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        let theo = dir.path().join(".theo");
        std::fs::create_dir_all(&theo).unwrap();
        std::fs::write(theo.join("config.toml"), content).unwrap();
        dir
    }

    #[test]
    fn load_router_returns_none_when_no_config() {
        let dir = TempDir::new().unwrap();
        assert!(load_router(dir.path(), None).is_none());
    }

    #[test]
    fn load_router_returns_none_when_no_routing_block() {
        let dir = fixture("# project config without [routing]\n");
        assert!(load_router(dir.path(), None).is_none());
    }

    #[test]
    fn load_router_returns_none_when_routing_disabled() {
        let dir = fixture(
            r#"
            [routing]
            enabled = false
            [routing.slots.cheap]
            model = "haiku"
            provider = "anthropic"
            "#,
        );
        assert!(load_router(dir.path(), None).is_none());
    }

    #[test]
    fn load_router_returns_none_when_slot_map_empty() {
        let dir = fixture(
            r#"
            [routing]
            enabled = true
            "#,
        );
        assert!(load_router(dir.path(), None).is_none());
    }

    #[test]
    fn load_router_returns_some_when_slots_present() {
        let dir = fixture(
            r#"
            [routing]
            enabled = true
            strategy = "rules"
            [routing.slots.cheap]
            model = "haiku"
            provider = "anthropic"
            [routing.slots.default]
            model = "sonnet"
            provider = "anthropic"
            [routing.slots.strong]
            model = "opus"
            provider = "anthropic"
            "#,
        );
        assert!(load_router(dir.path(), None).is_some());
    }

    #[test]
    fn is_chatgpt_oauth_supported_matches_known_models() {
        assert!(is_chatgpt_oauth_supported("gpt-5.4"));
        assert!(is_chatgpt_oauth_supported("gpt-5.4-mini"));
        assert!(is_chatgpt_oauth_supported("gpt-5.3-codex"));
        assert!(is_chatgpt_oauth_supported("gpt-5.2"));
    }

    #[test]
    fn is_chatgpt_oauth_supported_rejects_api_only_models() {
        // These return "not supported when using Codex with ChatGPT account"
        assert!(!is_chatgpt_oauth_supported("gpt-5.2-codex"));
        assert!(!is_chatgpt_oauth_supported("gpt-5.1-codex-max"));
        assert!(!is_chatgpt_oauth_supported("gpt-5.1-codex-mini"));
    }

    #[test]
    fn is_chatgpt_oauth_supported_rejects_random_garbage() {
        assert!(!is_chatgpt_oauth_supported(""));
        assert!(!is_chatgpt_oauth_supported("gpt-99"));
        assert!(!is_chatgpt_oauth_supported("claude-opus"));
    }

    // ── T10.1 — cost_aware runtime gate end-to-end ────────────────

    fn three_slot_config(extra: &str) -> String {
        format!(
            r#"
            [routing]
            enabled = true
            strategy = "rules"
            {extra}
            [routing.slots.cheap]
            model = "haiku"
            provider = "anthropic"
            [routing.slots.default]
            model = "sonnet"
            provider = "anthropic"
            [routing.slots.strong]
            model = "opus"
            provider = "anthropic"
            "#
        )
    }

    #[test]
    fn t101_router_loader_default_is_cost_aware_picks_strong_for_complex_tasks() {
        // No `cost_aware` line → defaults to true → classifier runs →
        // an Analysis-keyword'd objective routes to Strong/opus.
        use theo_domain::routing::{RoutingContext, RoutingPhase};
        let dir = fixture(&three_slot_config(""));
        let router = load_router(dir.path(), None).expect("router built");
        let mut ctx = RoutingContext::new(RoutingPhase::Normal);
        ctx.latest_user_message = Some("audit security analysis");
        let choice = router.route(&ctx);
        assert_eq!(choice.model_id, "opus");
    }

    #[test]
    fn t101_router_loader_with_cost_aware_false_always_picks_default() {
        // `cost_aware = false` → classifier bypassed → ALL routes
        // collapse to the `default` slot regardless of complexity.
        use theo_domain::routing::{RoutingContext, RoutingPhase};
        let dir = fixture(&three_slot_config("cost_aware = false"));
        let router = load_router(dir.path(), None).expect("router built");
        let mut ctx = RoutingContext::new(RoutingPhase::Normal);
        // Same complex objective that would route to Strong with
        // cost-aware ON now collapses to Default.
        ctx.latest_user_message = Some("audit security analysis");
        let choice = router.route(&ctx);
        assert_eq!(choice.model_id, "sonnet", "cost_aware=false → default slot");
    }

    #[test]
    fn t101_router_loader_with_cost_aware_true_picks_cheap_for_simple_tasks() {
        // Explicit cost_aware=true must still classify low-complexity
        // tasks down to the cheap slot.
        use theo_domain::routing::{RoutingContext, RoutingPhase};
        let dir = fixture(&three_slot_config("cost_aware = true"));
        let router = load_router(dir.path(), None).expect("router built");
        let mut ctx = RoutingContext::new(RoutingPhase::Normal);
        // Trivial retrieval keyword + tiny prompt → Cheap tier.
        ctx.latest_user_message = Some("read this file");
        let choice = router.route(&ctx);
        assert_eq!(choice.model_id, "haiku", "cheap slot for trivial task");
    }

    #[test]
    fn load_router_recorder_captures_decisions() {
        use theo_domain::routing::{RoutingContext, RoutingPhase};
        let dir = fixture(
            r#"
            [routing]
            enabled = true
            strategy = "rules"
            [routing.slots.cheap]
            model = "haiku"
            provider = "anthropic"
            [routing.slots.default]
            model = "sonnet"
            provider = "anthropic"
            [routing.slots.strong]
            model = "opus"
            provider = "anthropic"
            "#,
        );
        let captured: Arc<Mutex<Vec<(String, String, String)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let cap = captured.clone();
        let recorder: RoutingMetricsRecorder = Arc::new(move |t, ti, m| {
            cap.lock()
                .unwrap()
                .push((t.to_string(), ti.to_string(), m.to_string()));
        });
        let router = load_router(dir.path(), Some(recorder)).unwrap();
        let mut ctx = RoutingContext::new(RoutingPhase::Normal);
        ctx.latest_user_message = Some("audit security analysis");
        let _ = router.route(&ctx);
        let g = captured.lock().unwrap();
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].0, "Analysis");
        assert_eq!(g[0].1, "Strong");
        assert_eq!(g[0].2, "opus");
    }
}
