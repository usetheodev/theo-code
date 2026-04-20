//! Smart-model routing plumbing for theo-code.
//!
//! This module hosts the infrastructure pieces (benchmark harness, pricing,
//! future rule-based classifier). The `ModelRouter` trait itself lives in
//! `theo-domain::routing` so every consumer can depend on the trait without
//! pulling in this crate's implementation.
//!
//! Plan: `outputs/smart-model-routing-plan.md`. R0 ships the metrics harness.

pub mod metrics;
