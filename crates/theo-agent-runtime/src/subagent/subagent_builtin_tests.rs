//! Sibling test body of `subagent/mod.rs` — split per-feature (T3.5 of code-hygiene-5x5).
//!
//! Test-only file; gates use the inner `cfg(test)` attribute below to
//! classify every line as test code.

#![cfg(test)]
#![allow(unused_imports)]

use super::*;
use super::subagent_test_helpers::{mcp_env_lock, CaptureListener};
use super::*;
use theo_domain::tool::ToolCategory;

#[test]
fn builtin_explorer_capability_is_read_only() {
    let spec = builtins::explorer();
    assert!(
        !spec.capability_set
            .can_use_tool("bash", ToolCategory::Execution)
    );
    assert!(
        !spec.capability_set
            .can_use_tool("edit", ToolCategory::FileOps)
    );
    assert!(
        !spec.capability_set
            .can_use_tool("write", ToolCategory::FileOps)
    );
}

#[test]
fn builtin_implementer_capability_is_unrestricted() {
    let spec = builtins::implementer();
    assert!(spec.capability_set.denied_tools.is_empty());
    assert_eq!(
        spec.capability_set.allowed_tools,
        theo_domain::capability::AllowedTools::All
    );
}

#[test]
fn builtin_verifier_cannot_edit_can_bash() {
    let spec = builtins::verifier();
    assert!(spec.capability_set.denied_tools.contains("edit"));
    assert!(spec.capability_set.denied_tools.contains("write"));
    assert!(!spec.capability_set.denied_tools.contains("bash"));
}

#[test]
fn builtin_reviewer_is_read_only() {
    let spec = builtins::reviewer();
    assert!(spec.capability_set.denied_tools.contains("edit"));
    assert!(spec.capability_set.denied_tools.contains("write"));
}

