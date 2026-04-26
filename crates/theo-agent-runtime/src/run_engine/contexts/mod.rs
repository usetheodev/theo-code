//! Engine context bundles — first slice of the AgentRunEngine
//! god-object split (T3.1, see `docs/plans/T3.1-god-object-split-roadmap.md`).
//!
//! Each context holds a coherent slice of the previously-flat
//! `AgentRunEngine` field set. Submodules and call sites import the
//! one context they need rather than reaching across the whole
//! engine surface.

pub mod llm;
pub mod observability;
pub mod runtime;
pub mod subagent;
pub mod tracking;

pub use llm::LlmContext;
pub use observability::ObservabilityContext;
pub use runtime::RuntimeContext;
pub use subagent::SubagentContext;
pub use tracking::TrackingContext;
