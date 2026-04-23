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
pub fn load_router(
    project_dir: &Path,
    metrics_recorder: Option<RoutingMetricsRecorder>,
) -> Option<Arc<dyn theo_domain::routing::ModelRouter>> {
    let path = project_dir.join(PROJECT_CONFIG_PATH);
    let raw = std::fs::read_to_string(&path).ok()?;
    let wrapper: Wrapper = toml::from_str(&raw).ok()?;
    let config = wrapper.routing?;
    let config = config.apply_overrides(env_disables_routing(), false);
    if !config.enabled || config.slots.is_empty() {
        return None;
    }
    let inner = RuleBasedRouter::new(config.to_pricing_table());
    let mut auto = AutomaticModelRouter::new(inner, true);
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
    fn load_router_recorder_captures_decisions() {
        use theo_domain::routing::{ModelRouter, RoutingContext, RoutingPhase};
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
