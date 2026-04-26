//! Smart-model routing plumbing for theo-code.
//!
//! This module hosts the infrastructure pieces (benchmark harness, pricing,
//! rule-based classifier). The `ModelRouter` trait itself lives in
//! `theo-domain::routing` so every consumer can depend on the trait without
//! pulling in this crate's implementation.
//!
//! Plan: `outputs/smart-model-routing-plan.md`.

pub mod auto;
pub mod cascade;
pub mod complexity;
pub mod config;
pub mod keywords;
pub mod metrics;
pub mod pricing;
pub mod rules;

pub use auto::{tier_to_slot, AutomaticModelRouter};
pub use cascade::{CascadeState, CascadeStep, MAX_FALLBACK_HOPS, exhausted_error};
pub use complexity::{ComplexityClassifier, ComplexitySignals, TaskType};
pub use config::{RoutingConfig, SlotConfig, env_disables_routing};
pub use pricing::{PricingError, PricingTable};
pub use rules::RuleBasedRouter;
