//! Tier-aliased model pricing table.
//!
//! The `RuleBasedRouter` uses aliases (`cheap`, `default`, `strong`,
//! `vision`, `compact`, plus per-subagent slot ids) so concrete model IDs
//! can be swapped via config without editing the router logic. The table
//! is constructed from config; no vendor names are hard-coded in
//! `rules.rs`.

use std::collections::HashMap;

use theo_domain::routing::ModelChoice;

/// Lookup by slot name -> ModelChoice.
#[derive(Debug, Clone, Default)]
pub struct PricingTable {
    slots: HashMap<String, ModelChoice>,
}

#[derive(Debug, thiserror::Error)]
pub enum PricingError {
    #[error("unknown pricing slot `{0}`; configure it in [routing.slots]")]
    UnknownSlot(String),
}

impl PricingTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace a slot entry.
    pub fn insert(&mut self, slot: impl Into<String>, choice: ModelChoice) {
        self.slots.insert(slot.into(), choice);
    }

    /// Resolve a slot to a `ModelChoice`, returning an error if the slot
    /// is not configured.
    pub fn resolve(&self, slot: &str) -> Result<ModelChoice, PricingError> {
        self.slots
            .get(slot)
            .cloned()
            .ok_or_else(|| PricingError::UnknownSlot(slot.to_string()))
    }

    /// Resolve a slot with a fallback to the `default` slot if the
    /// specific alias isn't configured. Returns `PricingError` only if
    /// `default` itself is missing.
    pub fn resolve_or_default(&self, slot: &str) -> Result<ModelChoice, PricingError> {
        if let Some(hit) = self.slots.get(slot) {
            return Ok(hit.clone());
        }
        self.resolve("default")
    }

    /// True when both `default` and at least one of `cheap`/`strong` are
    /// configured — the minimum viable setup for a working router.
    pub fn is_minimally_configured(&self) -> bool {
        self.slots.contains_key("default")
            && (self.slots.contains_key("cheap") || self.slots.contains_key("strong"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(provider: &str, model: &str) -> ModelChoice {
        ModelChoice::new(provider, model, 8192)
    }

    #[test]
    fn resolve_returns_configured_slot() {
        let mut t = PricingTable::new();
        t.insert("cheap", c("anthropic", "claude-haiku-4-5"));
        let resolved = t.resolve("cheap").unwrap();
        assert_eq!(resolved.model_id, "claude-haiku-4-5");
    }

    #[test]
    fn resolve_unknown_slot_returns_error() {
        let t = PricingTable::new();
        let err = t.resolve("ghost").unwrap_err();
        assert!(err.to_string().contains("ghost"));
    }

    #[test]
    fn resolve_or_default_falls_back() {
        let mut t = PricingTable::new();
        t.insert("default", c("anthropic", "claude-sonnet-4-7"));
        let hit = t.resolve_or_default("missing_slot").unwrap();
        assert_eq!(hit.model_id, "claude-sonnet-4-7");
    }

    #[test]
    fn resolve_or_default_errors_when_default_missing() {
        let t = PricingTable::new();
        assert!(t.resolve_or_default("anything").is_err());
    }

    #[test]
    fn is_minimally_configured_requires_default_plus_tier() {
        let mut t = PricingTable::new();
        assert!(!t.is_minimally_configured());
        t.insert("default", c("anthropic", "claude-sonnet-4-7"));
        assert!(!t.is_minimally_configured());
        t.insert("cheap", c("anthropic", "claude-haiku-4-5"));
        assert!(t.is_minimally_configured());
    }
}
