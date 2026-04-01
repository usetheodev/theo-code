//! Sandbox subsystem for secure command execution.
//!
//! Provides kernel-level filesystem isolation via landlock (Linux 5.13+),
//! command validation, and denied path enforcement.

pub mod command_validator;
pub mod denied_paths;
pub mod env_sanitizer;
pub mod executor;
pub mod network;
pub mod probe;
pub mod rlimits;
