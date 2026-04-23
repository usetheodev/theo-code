//! Phase 23 (sota-gaps-followup): load project-level guardrails from
//! `.theo/handoff_guardrails.toml` into a `GuardrailChain`.
//!
//! The chain is built by:
//! 1. Seeding with `GuardrailChain::with_default_builtins()` (tier 1).
//! 2. Appending each entry from the TOML file (tier 2 — project).
//!
//! Tier 3 (PreHandoff hook) is dispatched separately by `evaluate_handoff`
//! in the runtime (see Phase 24).
//!
//! Loose contract: missing file → return chain with builtins only.
//! Malformed TOML → return chain with builtins only + log a warning.

use std::path::Path;
use std::sync::Arc;

use theo_agent_runtime::handoff_guardrail::{
    parse_guardrails_toml, DeclarativeGuardrail, GuardrailChain,
};

const PROJECT_GUARDRAILS_PATH: &str = ".theo/handoff_guardrails.toml";

/// Build a `GuardrailChain` seeded with built-in defaults plus any entries
/// declared in `<project_dir>/.theo/handoff_guardrails.toml`.
///
/// Returns the chain — never errors. Operator visibility into a malformed
/// config comes via the `eprintln!` warning (loose by design: a typo in
/// a guardrail file should NOT break the agent).
pub fn load_project_guardrails(project_dir: &Path) -> GuardrailChain {
    let mut chain = GuardrailChain::with_default_builtins();
    let path = project_dir.join(PROJECT_GUARDRAILS_PATH);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return chain, // file absent → builtins only
    };
    match parse_guardrails_toml(&content) {
        Ok(specs) => {
            for spec in specs {
                chain.add(Arc::new(DeclarativeGuardrail::from_spec(spec)));
            }
        }
        Err(e) => {
            eprintln!(
                "[theo] WARNING: malformed {} — using builtins only: {}",
                path.display(),
                e
            );
        }
    }
    chain
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fixture_with_toml(content: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        let theo = dir.path().join(".theo");
        std::fs::create_dir_all(&theo).unwrap();
        std::fs::write(theo.join("handoff_guardrails.toml"), content).unwrap();
        dir
    }

    #[test]
    fn load_project_guardrails_empty_when_file_absent() {
        let dir = TempDir::new().unwrap();
        let chain = load_project_guardrails(dir.path());
        // Builtins only (2 entries: read_only_must_not_mutate +
        // objective_must_not_be_empty).
        assert_eq!(chain.len(), 2);
    }

    #[test]
    fn load_project_guardrails_parses_2_entries() {
        let dir = fixture_with_toml(
            r#"
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
            "#,
        );
        let chain = load_project_guardrails(dir.path());
        // 2 builtins + 2 declarative
        assert_eq!(chain.len(), 4);
        let ids = chain.ids();
        assert!(ids.contains(&"no-prod".to_string()));
        assert!(ids.contains(&"redirect-verifier".to_string()));
    }

    #[test]
    fn load_project_guardrails_returns_builtins_only_for_malformed_toml() {
        let dir = fixture_with_toml("not valid [toml");
        let chain = load_project_guardrails(dir.path());
        // Loose contract: builtins still present, declarative ignored.
        assert_eq!(chain.len(), 2);
    }

    #[test]
    fn load_project_guardrails_chain_evaluates_declarative_entries() {
        use theo_agent_runtime::handoff_guardrail::HandoffContext;
        use theo_agent_runtime::subagent::builtins;

        let dir = fixture_with_toml(
            r#"
            [[guardrail]]
            id = "block-implementer"
            matcher.target_agent = "implementer"
            decision.kind = "block"
            decision.reason = "test"
            "#,
        );
        let chain = load_project_guardrails(dir.path());
        let target = builtins::implementer();
        let ctx = HandoffContext {
            source_agent: "main",
            target_agent: &target.name,
            target_spec: &target,
            objective: "anything",
            source_capabilities: None,
        };
        let (id, _) = chain.first_block(&ctx).expect("must block");
        assert_eq!(id, "block-implementer");
    }

    #[test]
    fn load_project_guardrails_empty_toml_returns_builtins_only() {
        let dir = fixture_with_toml("");
        let chain = load_project_guardrails(dir.path());
        assert_eq!(chain.len(), 2);
    }
}
