//! T5.1 / T5.2 — Auto-test-generation tools.
//!
//! Tools that PRODUCE test files for the agent to compile and run via
//! `bash`. Generation is a pure, deterministic templating operation —
//! no subprocesses, no test execution. The agent decides when to invoke
//! these (system-prompt instruction + few-shot examples).
//!
//! Currently exposed:
//! - `gen_property_test` (T5.1) — proptest scaffolding for a Rust function.
//!
//! Mutation testing (T5.2) wraps `cargo-mutants` and is a subprocess
//! call; kept as a separate module pending that tool's installation in CI.
//!
//! See `docs/plans/sota-tier1-tier2-plan.md` §T5.1 + ADR D7.

pub mod property;

pub use property::GenPropertyTestTool;
