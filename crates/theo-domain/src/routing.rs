//! Model-routing trait surface (plan phase R1).
//!
//! Lives in `theo-domain` so every downstream crate can depend on the
//! `ModelRouter` trait without pulling infra-specific code. Concrete
//! implementations (rule-based, learned, cascade) live in
//! `theo-infra-llm::routing` and wire into `theo-agent-runtime`.
//!
//! Design notes are in `outputs/smart-model-routing.md` §4.1.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

/// Logical role of a model invocation.
///
/// The router uses the phase to pick a slot (compact / vision / subagent)
/// before falling back to keyword-based tier selection for `Normal`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum RoutingPhase {
    /// Default agent turn — the bulk of traffic.
    Normal,
    /// Session compaction (summary + mask stages). Typically a cheap tier.
    Compaction,
    /// Vision / multimodal call.
    Vision,
    /// Sub-agent delegation. Carries a role id so the registry maps each
    /// role to its configured slot.
    Subagent {
        role: SubAgentRoleId,
    },
    /// Self-critique / reviewer pass (typically the strongest tier).
    SelfCritique,
    /// Meta-classifier call (a cheap model deciding for another router).
    Classifier,
}

impl RoutingPhase {
    /// Convenience constructor so call sites read naturally.
    pub fn subagent(role: SubAgentRoleId) -> Self {
        Self::Subagent { role }
    }
}

/// Stable string identifier for a sub-agent role (e.g. `"explorer"`).
/// The runtime maps its enum variants to these ids; the domain stays
/// dependency-free. `Cow<'static, str>` lets the canonical constants
/// stay zero-alloc while still supporting serde round-trips.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SubAgentRoleId(pub Cow<'static, str>);

impl SubAgentRoleId {
    pub const EXPLORER: Self = Self(Cow::Borrowed("explorer"));
    pub const IMPLEMENTER: Self = Self(Cow::Borrowed("implementer"));
    pub const VERIFIER: Self = Self(Cow::Borrowed("verifier"));
    pub const REVIEWER: Self = Self(Cow::Borrowed("reviewer"));

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Hints that classify a failed LLM call so the router can pick a
/// different model on retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RoutingFailureHint {
    /// Response or prompt overflowed the current model's context window.
    ContextOverflow,
    /// HTTP 429 or provider-reported throttling.
    RateLimit,
    /// HTTP 5xx, network timeout, or other retryable infra failure.
    Transient,
    /// Budget / quota exceeded — must not silently downgrade quality.
    BudgetExhausted,
}

/// All inputs the router needs to choose a model for the next call.
///
/// Built fresh on every turn. The lifetime parameter lets the caller pass
/// borrowed prompt slices without cloning.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RoutingContext<'a> {
    pub phase: RoutingPhase,
    pub latest_user_message: Option<&'a str>,
    pub conversation_tokens: u64,
    pub iteration: usize,
    pub requires_vision: bool,
    pub requires_tool_use: bool,
    pub previous_failure: Option<RoutingFailureHint>,
}

impl<'a> RoutingContext<'a> {
    /// Construct a default context for the given phase. All optional
    /// fields start at their zero-value.
    pub fn new(phase: RoutingPhase) -> Self {
        Self {
            phase,
            latest_user_message: None,
            conversation_tokens: 0,
            iteration: 0,
            requires_vision: false,
            requires_tool_use: false,
            previous_failure: None,
        }
    }
}

/// A concrete provider + model + tuning choice emitted by the router.
///
/// `routing_reason` is a `&'static str` so log lines cost nothing extra
/// at the call site (no allocation per turn).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelChoice {
    pub provider_id: String,
    pub model_id: String,
    pub max_output_tokens: u32,
    pub reasoning_effort: Option<String>,
    pub routing_reason: &'static str,
}

impl ModelChoice {
    /// Build a `ModelChoice` for the "default" path with the given provider
    /// + model.
    ///
    /// Consumers tweak other fields through the struct fields directly.
    pub fn new(
        provider_id: impl Into<String>,
        model_id: impl Into<String>,
        max_output_tokens: u32,
    ) -> Self {
        Self {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            max_output_tokens,
            reasoning_effort: None,
            routing_reason: "default",
        }
    }
}

/// The central trait every router implementation satisfies.
///
/// The trait is `Send + Sync` so a single `Arc<dyn ModelRouter>` can be
/// shared across the async runtime.
pub trait ModelRouter: Send + Sync {
    /// Pick a model for the given context.
    fn route(&self, ctx: &RoutingContext<'_>) -> ModelChoice;

    /// Pick the next choice to try after `previous` failed with `hint`.
    /// Return `None` to surface the failure without retrying.
    fn fallback(
        &self,
        previous: &ModelChoice,
        hint: RoutingFailureHint,
    ) -> Option<ModelChoice>;
}

/// Behaviour-preserving router: always returns its injected default and
/// never suggests a fallback. Used when routing is disabled (the
/// `AgentConfig.router = None` case) and as a test stand-in.
#[derive(Debug, Clone)]
pub struct NullRouter {
    default: ModelChoice,
}

impl NullRouter {
    pub fn new(default: ModelChoice) -> Self {
        Self { default }
    }
}

impl ModelRouter for NullRouter {
    fn route(&self, _ctx: &RoutingContext<'_>) -> ModelChoice {
        self.default.clone()
    }

    fn fallback(
        &self,
        _previous: &ModelChoice,
        _hint: RoutingFailureHint,
    ) -> Option<ModelChoice> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_choice() -> ModelChoice {
        ModelChoice::new("anthropic", "claude-sonnet-4-7", 8192)
    }

    // ── R1-AC-1 ─────────────────────────────────────────────────

    #[test]
    fn test_null_router_returns_default_for_every_phase() {
        let router = NullRouter::new(default_choice());
        let phases = [
            RoutingPhase::Normal,
            RoutingPhase::Compaction,
            RoutingPhase::Vision,
            RoutingPhase::subagent(SubAgentRoleId::EXPLORER),
            RoutingPhase::SelfCritique,
            RoutingPhase::Classifier,
        ];
        for phase in phases {
            let ctx = RoutingContext::new(phase.clone());
            let choice = router.route(&ctx);
            assert_eq!(
                choice,
                default_choice(),
                "NullRouter must return default for phase {phase:?}"
            );
        }
    }

    // ── R1-AC-2 ─────────────────────────────────────────────────

    #[test]
    fn test_null_router_fallback_returns_none() {
        let router = NullRouter::new(default_choice());
        let hints = [
            RoutingFailureHint::ContextOverflow,
            RoutingFailureHint::RateLimit,
            RoutingFailureHint::Transient,
            RoutingFailureHint::BudgetExhausted,
        ];
        for hint in hints {
            assert!(
                router.fallback(&default_choice(), hint).is_none(),
                "NullRouter must never suggest a fallback (hint {hint:?})"
            );
        }
    }

    // ── R1-AC-3 ─────────────────────────────────────────────────

    #[test]
    fn test_routing_phase_serializes_round_trips() {
        let phases = [
            RoutingPhase::Normal,
            RoutingPhase::Compaction,
            RoutingPhase::Vision,
            RoutingPhase::subagent(SubAgentRoleId::IMPLEMENTER),
            RoutingPhase::SelfCritique,
            RoutingPhase::Classifier,
        ];
        for phase in phases {
            let json = serde_json::to_string(&phase).unwrap();
            let back: RoutingPhase = serde_json::from_str(&json).unwrap();
            assert_eq!(back, phase, "round-trip must preserve variant + data");
        }
    }

    // ── R1-AC-4 ─────────────────────────────────────────────────

    #[test]
    fn test_model_choice_equality_is_structural() {
        let a = ModelChoice::new("anthropic", "claude-sonnet-4-7", 8192);
        let b = ModelChoice::new("anthropic", "claude-sonnet-4-7", 8192);
        assert_eq!(a, b);
        let c = a.clone();
        assert_eq!(a, c);
        let different = ModelChoice::new("openai", "gpt-5", 8192);
        assert_ne!(a, different);
    }

    // ── R1-AC-5 ─────────────────────────────────────────────────
    // Trait object safety — compile-only check. If the trait stops being
    // object-safe, this test fails to compile.

    #[test]
    fn test_model_router_is_object_safe() {
        let _: Box<dyn ModelRouter> = Box::new(NullRouter::new(default_choice()));
    }

    // ── R1-AC-6 ─────────────────────────────────────────────────

    #[test]
    fn test_routing_context_builder_sets_defaults() {
        let ctx = RoutingContext::new(RoutingPhase::Normal);
        assert_eq!(ctx.phase, RoutingPhase::Normal);
        assert!(ctx.latest_user_message.is_none());
        assert_eq!(ctx.conversation_tokens, 0);
        assert_eq!(ctx.iteration, 0);
        assert!(!ctx.requires_vision);
        assert!(!ctx.requires_tool_use);
        assert!(ctx.previous_failure.is_none());
    }

    // ── Bonus: SubAgentRoleId canonical constants ───────────────

    #[test]
    fn sub_agent_role_id_constants_are_canonical() {
        assert_eq!(SubAgentRoleId::EXPLORER.as_str(), "explorer");
        assert_eq!(SubAgentRoleId::IMPLEMENTER.as_str(), "implementer");
        assert_eq!(SubAgentRoleId::VERIFIER.as_str(), "verifier");
        assert_eq!(SubAgentRoleId::REVIEWER.as_str(), "reviewer");
    }
}
