//! TOML configuration surface for the smart-model router.
//!
//! Parses a `[routing]` block with nested `[routing.slots.<alias>]`
//! entries. Env override `THEO_ROUTING_DISABLED=1` forces the loader
//! to return a disabled config regardless of file contents. A CLI flag
//! (`--router off`) calls `disable_via_cli()` on the resolved config.
//!
//! Plan ref: outputs/smart-model-routing-plan.md §R4 + §4.6.

use serde::{Deserialize, Serialize};
use theo_domain::routing::ModelChoice;

use super::pricing::PricingTable;

/// Top-level `[routing]` section.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoutingConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_strategy")]
    pub strategy: String,
    /// T10.1 — Cost-aware routing master switch.
    ///
    /// When `true` (the SOTA-default), the runtime wraps the
    /// rule-based router in an `AutomaticModelRouter` that runs the
    /// `ComplexityClassifier` over each task and downgrades to
    /// `cheap` (Haiku) when complexity allows. The plan's A/B target
    /// is ≥20% cost reduction at unchanged success rate.
    ///
    /// When `false`, the runtime uses the plain rule-based router
    /// and always picks the configured default slot (Sonnet by
    /// convention) — matches pre-T10.1 behaviour for callers that
    /// want predictable per-call costs.
    #[serde(default = "default_cost_aware")]
    pub cost_aware: bool,
    #[serde(default)]
    pub slots: std::collections::BTreeMap<String, SlotConfig>,
}

fn default_enabled() -> bool {
    true
}

fn default_strategy() -> String {
    "rules".to_string()
}

fn default_cost_aware() -> bool {
    // SOTA-default is cost-aware ON. Operators who need predictable
    // per-call costs can flip via `[routing] cost_aware = false` or
    // `THEO_ROUTING_COST_AWARE=0`.
    true
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strategy: "rules".to_string(),
            cost_aware: true,
            slots: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SlotConfig {
    pub model: String,
    pub provider: String,
    #[serde(default = "default_max_output_tokens")]
    pub max_output_tokens: u32,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

fn default_max_output_tokens() -> u32 {
    8192
}

impl RoutingConfig {
    /// Apply env / CLI overrides on top of the parsed config.
    ///
    /// - `THEO_ROUTING_DISABLED=1` forces `enabled=false` (any truthy
    ///   value other than an empty string counts).
    /// - `disable_via_cli` is a callback the CLI flag layer invokes;
    ///   when true, `enabled` is cleared.
    pub fn apply_overrides(mut self, env_disabled: bool, cli_disabled: bool) -> Self {
        if env_disabled || cli_disabled {
            self.enabled = false;
        }
        self
    }

    /// T10.1 — Apply env override for the cost-aware switch.
    ///
    /// `THEO_ROUTING_COST_AWARE=0` (or `false`) forces classification
    /// off so the rule-based router always returns the default slot.
    /// Any other value (including unset) keeps the configured value.
    pub fn apply_cost_aware_override(mut self, env_disabled: bool) -> Self {
        if env_disabled {
            self.cost_aware = false;
        }
        self
    }

    /// Build a `PricingTable` from the slot map. Empty map returns an
    /// empty table — callers can mix in `insert()` calls if they want
    /// defaults.
    pub fn to_pricing_table(&self) -> PricingTable {
        let mut table = PricingTable::new();
        for (alias, slot) in &self.slots {
            let mut choice =
                ModelChoice::new(slot.provider.clone(), slot.model.clone(), slot.max_output_tokens);
            choice.reasoning_effort = slot.reasoning_effort.clone();
            table.insert(alias.clone(), choice);
        }
        table
    }
}

/// Read `THEO_ROUTING_DISABLED` from the environment once. A non-empty
/// value is considered truthy.
pub fn env_disables_routing() -> bool {
    match std::env::var("THEO_ROUTING_DISABLED") {
        Ok(v) => !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"),
        Err(_) => false,
    }
}

/// T10.1 — Read `THEO_ROUTING_COST_AWARE` from the environment.
/// Returns `true` (i.e. "disable cost-aware") when the var is `"0"`,
/// `"false"`, `"off"`, or `"no"` (case-insensitive). Unset / any
/// other value → `false` (i.e. cost-aware stays at its config value).
pub fn env_disables_cost_aware() -> bool {
    match std::env::var("THEO_ROUTING_COST_AWARE") {
        Ok(v) => {
            let lower = v.to_ascii_lowercase();
            matches!(lower.as_str(), "0" | "false" | "off" | "no")
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_TOML: &str = r#"
        [routing]
        enabled = true
        strategy = "rules"

        [routing.slots.cheap]
        model = "haiku"
        provider = "anthropic"
        max_output_tokens = 2048

        [routing.slots.default]
        model = "sonnet"
        provider = "anthropic"

        [routing.slots.strong]
        model = "opus"
        provider = "anthropic"
        reasoning_effort = "high"
    "#;

    #[derive(Deserialize)]
    struct Wrapper {
        routing: RoutingConfig,
    }

    // ── R4-AC-5 ─────────────────────────────────────────────────
    #[test]
    fn test_r4_ac_5_toml_routing_block_parses() {
        let wrapper: Wrapper = toml::from_str(SAMPLE_TOML).expect("parse");
        assert!(wrapper.routing.enabled);
        assert_eq!(wrapper.routing.strategy, "rules");
        assert_eq!(wrapper.routing.slots.len(), 3);
        let cheap = wrapper.routing.slots.get("cheap").unwrap();
        assert_eq!(cheap.model, "haiku");
        assert_eq!(cheap.max_output_tokens, 2048);
    }

    // ── R4-AC-6 ─────────────────────────────────────────────────
    #[test]
    fn test_r4_ac_6_toml_routing_disabled_builds_empty_table() {
        let cfg = RoutingConfig {
            enabled: false,
            ..Default::default()
        };
        // Caller wraps the disabled flag as a NullRouter outside this
        // module. Here we just verify the plumbing: when disabled the
        // config is honest about it.
        assert!(!cfg.enabled);
        let table = cfg.to_pricing_table();
        assert!(!table.is_minimally_configured());
    }

    // ── R4-AC-7 ─────────────────────────────────────────────────
    #[test]
    fn test_r4_ac_7_env_var_overrides_enabled_config() {
        let cfg = RoutingConfig {
            enabled: true,
            ..Default::default()
        };
        let overridden = cfg.apply_overrides(/*env_disabled=*/ true, /*cli_disabled=*/ false);
        assert!(!overridden.enabled);
    }

    // ── R4-AC-8 ─────────────────────────────────────────────────
    #[test]
    fn test_r4_ac_8_cli_flag_router_off_disables() {
        let cfg = RoutingConfig {
            enabled: true,
            ..Default::default()
        };
        let overridden = cfg.apply_overrides(/*env_disabled=*/ false, /*cli_disabled=*/ true);
        assert!(!overridden.enabled);
    }

    #[test]
    fn reasoning_effort_roundtrips_through_pricing_table() {
        let wrapper: Wrapper = toml::from_str(SAMPLE_TOML).unwrap();
        let table = wrapper.routing.to_pricing_table();
        let strong = table.resolve("strong").unwrap();
        assert_eq!(strong.reasoning_effort.as_deref(), Some("high"));
    }

    #[test]
    fn missing_slots_block_defaults_to_empty() {
        let toml_str = r#"
            [routing]
            enabled = true
            strategy = "rules"
        "#;
        let wrapper: Wrapper = toml::from_str(toml_str).unwrap();
        assert!(wrapper.routing.slots.is_empty());
    }

    // ── T10.1 — cost_aware runtime gate ────────────────────────────

    #[test]
    fn t101_default_routing_config_is_cost_aware() {
        // SOTA-default invariant: a fresh RoutingConfig must enable
        // cost-aware classification. Regression here would silently
        // drop the +20% cost reduction A/B target.
        let cfg = RoutingConfig::default();
        assert!(cfg.cost_aware, "Default::default() must enable cost_aware");
    }

    #[test]
    fn t101_cost_aware_parses_from_toml() {
        let toml_str = r#"
            [routing]
            enabled = true
            strategy = "rules"
            cost_aware = false
        "#;
        let wrapper: Wrapper = toml::from_str(toml_str).unwrap();
        assert!(!wrapper.routing.cost_aware);
    }

    #[test]
    fn t101_cost_aware_omitted_defaults_to_true() {
        // Backward compatibility: existing configs without the new
        // field still get the SOTA-default (cost-aware ON).
        let toml_str = r#"
            [routing]
            enabled = true
            strategy = "rules"
        "#;
        let wrapper: Wrapper = toml::from_str(toml_str).unwrap();
        assert!(wrapper.routing.cost_aware);
    }

    #[test]
    fn t101_apply_cost_aware_override_disables_when_env_set() {
        let cfg = RoutingConfig::default();
        let overridden = cfg.apply_cost_aware_override(true);
        assert!(!overridden.cost_aware);
    }

    #[test]
    fn t101_apply_cost_aware_override_keeps_value_when_env_unset() {
        let cfg = RoutingConfig::default();
        let overridden = cfg.apply_cost_aware_override(false);
        assert!(overridden.cost_aware);
    }

    #[test]
    fn t101_env_disables_cost_aware_recognises_falsy_values() {
        // We can't pollute the process env from a unit test (other
        // tests may race), so exercise the parsing logic by mutating
        // and then restoring.
        unsafe {
            // Ensure we start clean.
            std::env::remove_var("THEO_ROUTING_COST_AWARE");
            assert!(!env_disables_cost_aware(), "unset → false");

            for falsy in ["0", "false", "FALSE", "off", "OFF", "no", "No"] {
                std::env::set_var("THEO_ROUTING_COST_AWARE", falsy);
                assert!(
                    env_disables_cost_aware(),
                    "value `{falsy}` should be recognised as disabling"
                );
            }

            for truthy in ["1", "true", "yes", "on", ""] {
                std::env::set_var("THEO_ROUTING_COST_AWARE", truthy);
                assert!(
                    !env_disables_cost_aware(),
                    "value `{truthy}` should NOT be recognised as disabling"
                );
            }

            std::env::remove_var("THEO_ROUTING_COST_AWARE");
        }
    }

    #[test]
    fn t101_clone_and_partialeq_round_trip() {
        let a = RoutingConfig::default();
        let b = a.clone();
        assert_eq!(a, b);
        let c = RoutingConfig {
            cost_aware: false,
            ..Default::default()
        };
        assert_ne!(a, c);
    }
}
