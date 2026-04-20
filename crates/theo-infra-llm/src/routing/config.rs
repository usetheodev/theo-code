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
    #[serde(default)]
    pub slots: std::collections::BTreeMap<String, SlotConfig>,
}

fn default_enabled() -> bool {
    true
}

fn default_strategy() -> String {
    "rules".to_string()
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strategy: "rules".to_string(),
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
        Ok(v) => !v.is_empty() && v != "0" && v.to_ascii_lowercase() != "false",
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
}
