//! `DeclarativeGuardrail` — TOML/YAML-driven `HandoffGuardrail` impl.
//!
//! Phase 23 (sota-gaps-followup): closes gap #2 (PreHandoff hook sem
//! YAML loader). Schema (`.theo/handoff_guardrails.toml`):
//!
//! ```toml
//! [[guardrail]]
//! id = "no-implementer-touches-prod"
//! matcher.target_agent = "implementer"        # exact match (optional)
//! matcher.objective_pattern = "production|prod"  # regex (optional)
//! decision.kind = "block"                     # allow|block|redirect|rewrite|warn
//! decision.reason = "production changes require human review"
//!
//! [[guardrail]]
//! id = "verifier-cannot-mutate"
//! matcher.target_agent = "verifier"
//! matcher.objective_pattern = "implement|write|edit"
//! decision.kind = "redirect"
//! decision.new_agent_name = "implementer"
//! ```
//!
//! Both matchers must be present together OR omitted individually:
//! - target_agent only → matches when target name == value
//! - objective_pattern only → matches when objective ~= pattern
//! - both → AND-combined
//! - neither → always matches (universal guardrail)

use serde::{Deserialize, Serialize};

use super::{GuardrailDecision, HandoffContext, HandoffGuardrail};

/// Match criteria for a declarative guardrail. All present fields must
/// match; absent fields are wildcards.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeclarativeMatcher {
    /// Exact-match against `HandoffContext::target_agent`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_agent: Option<String>,
    /// Regex against `HandoffContext::objective` (case-insensitive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub objective_pattern: Option<String>,
}

impl DeclarativeMatcher {
    /// True when every present field matches; absent fields are wildcards.
    pub fn matches(&self, ctx: &HandoffContext<'_>) -> bool {
        if let Some(name) = &self.target_agent
            && ctx.target_agent != name
        {
            return false;
        }
        if let Some(pat) = &self.objective_pattern {
            let lowered = ctx.objective.to_lowercase();
            let regex = match regex::Regex::new(&pat.to_lowercase()) {
                Ok(r) => r,
                Err(_) => return false, // invalid regex → never matches (fail-soft)
            };
            if !regex.is_match(&lowered) {
                return false;
            }
        }
        true
    }
}

/// Decision serialized in TOML/YAML.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeclarativeDecision {
    Allow,
    Block { reason: String },
    Warn { message: String },
    Redirect { new_agent_name: String },
    RewriteObjective { new_objective: String },
}

impl From<DeclarativeDecision> for GuardrailDecision {
    fn from(d: DeclarativeDecision) -> Self {
        match d {
            DeclarativeDecision::Allow => GuardrailDecision::Allow,
            DeclarativeDecision::Block { reason } => GuardrailDecision::Block { reason },
            DeclarativeDecision::Warn { message } => GuardrailDecision::Warn { message },
            DeclarativeDecision::Redirect { new_agent_name } => {
                GuardrailDecision::Redirect { new_agent_name }
            }
            DeclarativeDecision::RewriteObjective { new_objective } => {
                GuardrailDecision::RewriteObjective { new_objective }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeclarativeGuardrailSpec {
    pub id: String,
    #[serde(default)]
    pub matcher: DeclarativeMatcher,
    pub decision: DeclarativeDecision,
}

/// `HandoffGuardrail` impl backed by a TOML spec.
#[derive(Debug, Clone)]
pub struct DeclarativeGuardrail {
    spec: DeclarativeGuardrailSpec,
}

impl DeclarativeGuardrail {
    pub fn from_spec(spec: DeclarativeGuardrailSpec) -> Self {
        Self { spec }
    }

    pub fn id(&self) -> &str {
        &self.spec.id
    }
}

impl HandoffGuardrail for DeclarativeGuardrail {
    fn id(&self) -> &str {
        &self.spec.id
    }

    fn evaluate(&self, ctx: &HandoffContext<'_>) -> GuardrailDecision {
        if self.spec.matcher.matches(ctx) {
            self.spec.decision.clone().into()
        } else {
            GuardrailDecision::Allow
        }
    }
}

/// File schema for `.theo/handoff_guardrails.toml`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DeclarativeGuardrailsFile {
    #[serde(default)]
    pub guardrail: Vec<DeclarativeGuardrailSpec>,
}

/// Parse the TOML content. Empty file → empty vec.
pub fn parse_guardrails_toml(
    content: &str,
) -> Result<Vec<DeclarativeGuardrailSpec>, toml::de::Error> {
    let f: DeclarativeGuardrailsFile = toml::from_str(content)?;
    Ok(f.guardrail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subagent::builtins;
    use std::sync::Arc;

    fn ctx<'a>(target: &'a theo_domain::agent_spec::AgentSpec, objective: &'a str)
        -> HandoffContext<'a>
    {
        HandoffContext {
            source_agent: "main",
            target_agent: &target.name,
            target_spec: target,
            objective,
            source_capabilities: None,
        }
    }

    // ── DeclarativeDecision serde ──

    #[test]
    fn declarative_guardrail_block_decision_serializes_correctly() {
        let d = DeclarativeDecision::Block { reason: "x".into() };
        let s = toml::to_string(&d).unwrap();
        assert!(s.contains("kind = \"block\""), "got: {}", s);
        assert!(s.contains("x"));
    }

    #[test]
    fn declarative_guardrail_redirect_decision_serializes_correctly() {
        let d = DeclarativeDecision::Redirect {
            new_agent_name: "implementer".into(),
        };
        let s = toml::to_string(&d).unwrap();
        assert!(s.contains("kind = \"redirect\""));
        assert!(s.contains("implementer"));
    }

    #[test]
    fn declarative_decision_serde_roundtrip_all_variants() {
        for d in [
            DeclarativeDecision::Allow,
            DeclarativeDecision::Block { reason: "r".into() },
            DeclarativeDecision::Warn { message: "m".into() },
            DeclarativeDecision::Redirect { new_agent_name: "a".into() },
            DeclarativeDecision::RewriteObjective { new_objective: "o".into() },
        ] {
            let s = toml::to_string(&d).unwrap();
            let _back: DeclarativeDecision = toml::from_str(&s)
                .unwrap_or_else(|e| panic!("roundtrip failed for {:?}: {}", d, e));
        }
    }

    // ── matcher behavior ──

    #[test]
    fn declarative_guardrail_matches_target_agent_exact() {
        let target = builtins::implementer();
        let g = DeclarativeGuardrail::from_spec(DeclarativeGuardrailSpec {
            id: "test".into(),
            matcher: DeclarativeMatcher {
                target_agent: Some("implementer".into()),
                objective_pattern: None,
            },
            decision: DeclarativeDecision::Block { reason: "r".into() },
        });
        let d = g.evaluate(&ctx(&target, "do anything"));
        assert!(d.is_block());
    }

    #[test]
    fn declarative_guardrail_matches_objective_via_regex() {
        let target = builtins::implementer();
        let g = DeclarativeGuardrail::from_spec(DeclarativeGuardrailSpec {
            id: "test".into(),
            matcher: DeclarativeMatcher {
                target_agent: None,
                objective_pattern: Some("production|prod".into()),
            },
            decision: DeclarativeDecision::Block { reason: "no prod".into() },
        });
        assert!(g.evaluate(&ctx(&target, "deploy to PRODUCTION")).is_block());
        assert!(g.evaluate(&ctx(&target, "fix prod outage")).is_block());
        assert!(g.evaluate(&ctx(&target, "develop new feature")).is_allow());
    }

    #[test]
    fn declarative_guardrail_skips_when_matcher_misses() {
        let target = builtins::implementer();
        let g = DeclarativeGuardrail::from_spec(DeclarativeGuardrailSpec {
            id: "test".into(),
            matcher: DeclarativeMatcher {
                target_agent: Some("verifier".into()), // mismatch
                objective_pattern: None,
            },
            decision: DeclarativeDecision::Block { reason: "x".into() },
        });
        assert!(g.evaluate(&ctx(&target, "do x")).is_allow());
    }

    #[test]
    fn declarative_guardrail_combines_target_and_objective_with_and() {
        let target = builtins::implementer();
        let g = DeclarativeGuardrail::from_spec(DeclarativeGuardrailSpec {
            id: "test".into(),
            matcher: DeclarativeMatcher {
                target_agent: Some("implementer".into()),
                objective_pattern: Some("delete".into()),
            },
            decision: DeclarativeDecision::Block { reason: "no delete".into() },
        });
        // Both match → block
        assert!(g.evaluate(&ctx(&target, "delete the table")).is_block());
        // Target matches but objective doesn't → allow
        assert!(g.evaluate(&ctx(&target, "create the table")).is_allow());
    }

    #[test]
    fn declarative_guardrail_with_no_matcher_always_matches() {
        let target = builtins::implementer();
        let g = DeclarativeGuardrail::from_spec(DeclarativeGuardrailSpec {
            id: "universal".into(),
            matcher: DeclarativeMatcher::default(),
            decision: DeclarativeDecision::Warn { message: "audit".into() },
        });
        assert!(g.evaluate(&ctx(&target, "anything")).is_warn());
    }

    #[test]
    fn declarative_guardrail_invalid_regex_never_matches() {
        let target = builtins::implementer();
        let g = DeclarativeGuardrail::from_spec(DeclarativeGuardrailSpec {
            id: "broken".into(),
            matcher: DeclarativeMatcher {
                target_agent: None,
                objective_pattern: Some("[unclosed".into()),
            },
            decision: DeclarativeDecision::Block { reason: "x".into() },
        });
        // Invalid regex → fail-soft → allow
        assert!(g.evaluate(&ctx(&target, "anything")).is_allow());
    }

    // ── parse_guardrails_toml ──

    #[test]
    fn parse_guardrails_toml_empty_file_returns_empty_vec() {
        let v = parse_guardrails_toml("").unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn parse_guardrails_toml_parses_two_entries() {
        let toml = r#"
            [[guardrail]]
            id = "no-prod"
            matcher.target_agent = "implementer"
            matcher.objective_pattern = "prod"
            decision.kind = "block"
            decision.reason = "review needed"

            [[guardrail]]
            id = "redirect-verifier"
            matcher.target_agent = "verifier"
            matcher.objective_pattern = "implement"
            decision.kind = "redirect"
            decision.new_agent_name = "implementer"
        "#;
        let v = parse_guardrails_toml(toml).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].id, "no-prod");
        assert_eq!(v[1].id, "redirect-verifier");
    }

    #[test]
    fn parse_guardrails_toml_returns_err_for_malformed_toml() {
        let res = parse_guardrails_toml("not [valid toml");
        assert!(res.is_err());
    }

    #[test]
    fn declarative_guardrail_can_be_added_to_chain() {
        use super::super::GuardrailChain;
        let mut chain = GuardrailChain::new();
        chain.add(Arc::new(DeclarativeGuardrail::from_spec(
            DeclarativeGuardrailSpec {
                id: "block-implementer-on-everything".into(),
                matcher: DeclarativeMatcher {
                    target_agent: Some("implementer".into()),
                    objective_pattern: None,
                },
                decision: DeclarativeDecision::Block { reason: "x".into() },
            },
        )));
        let target = builtins::implementer();
        let (id, _) = chain.first_block(&ctx(&target, "do x")).unwrap();
        assert_eq!(id, "block-implementer-on-everything");
    }
}
