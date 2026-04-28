//! Sibling test body of `run_engine/mod.rs` — split per-area (T3.2 of code-hygiene-5x5).
//!
//! Test-only file; gates use the inner `cfg(test)` attribute below to
//! classify every line as test code.

#![cfg(test)]
#![allow(unused_imports)]

use super::*;
use crate::event_bus::CapturingListener;
use theo_domain::session::SessionId;
use theo_domain::task::AgentType;

use crate::run_engine::test_helpers::TestSetup;

#[test]
fn engine_with_subagent_handoff_guardrails_stores_reference() {
    let setup = TestSetup::new();
    let chain = std::sync::Arc::new(
        crate::handoff_guardrail::GuardrailChain::with_default_builtins(),
    );
    let engine = setup
        .create_engine("test")
        .with_subagent_handoff_guardrails(chain);
    assert!(engine.subagent.handoff_guardrails.is_some());
}

#[test]
fn engine_with_subagent_mcp_discovery_stores_reference() {
    let setup = TestSetup::new();
    let cache = std::sync::Arc::new(theo_infra_mcp::DiscoveryCache::new());
    let engine = setup
        .create_engine("test")
        .with_subagent_mcp_discovery(cache);
    assert!(engine.subagent.mcp_discovery.is_some());
}

#[test]
fn engine_with_subagent_hooks_stores_reference() {
    let setup = TestSetup::new();
    let engine = setup
        .create_engine("test")
        .with_subagent_hooks(std::sync::Arc::new(
            crate::lifecycle_hooks::HookManager::new(),
        ));
    assert!(engine.subagent.hooks.is_some());
}

#[test]
fn is_mutating_tool_recognizes_known_writes() {
    assert!(AgentRunEngine::is_mutating_tool("edit"));
    assert!(AgentRunEngine::is_mutating_tool("write"));
    assert!(AgentRunEngine::is_mutating_tool("apply_patch"));
    assert!(AgentRunEngine::is_mutating_tool("bash"));
    assert!(!AgentRunEngine::is_mutating_tool("read"));
    assert!(!AgentRunEngine::is_mutating_tool("grep"));
    assert!(!AgentRunEngine::is_mutating_tool("glob"));
}

#[test]
fn maybe_checkpoint_returns_none_without_manager() {
    let setup = TestSetup::new();
    let engine = setup.create_engine("test");
    // No subagent_checkpoint attached → snapshot returns None even
    // for mutating tool
    assert!(engine.maybe_checkpoint_for_tool("edit", 1).is_none());
    assert!(engine.checkpoint_before_mutation("any").is_none());
}

#[test]
fn maybe_checkpoint_skips_non_mutating_tools() {
    let setup = TestSetup::new();
    let engine = setup.create_engine("test");
    // read is not mutating — even with manager attached this would
    // return None; with no manager, definitely None.
    assert!(engine.maybe_checkpoint_for_tool("read", 1).is_none());
    assert!(engine.maybe_checkpoint_for_tool("grep", 1).is_none());
}

#[test]
fn reset_turn_checkpoint_allows_new_snapshot() {
    let setup = TestSetup::new();
    let engine = setup.create_engine("test");
    // Mark snapshot as taken
    engine
        .tracking
        .checkpoint_taken_this_turn
        .store(true, std::sync::atomic::Ordering::Release);
    engine.reset_turn_checkpoint();
    assert!(
        !engine
            .tracking
            .checkpoint_taken_this_turn
            .load(std::sync::atomic::Ordering::Acquire)
    );
}

#[tokio::test]
async fn try_dispatch_mcp_tool_returns_none_for_non_mcp_name() {
    let setup = TestSetup::new();
    let engine = setup.create_engine("test");
    let call = theo_infra_llm::types::ToolCall {
        id: "1".into(),
        call_type: "function".into(),
        function: theo_infra_llm::types::FunctionCall {
            name: "read".into(),
            arguments: "{}".into(),
        },
    };
    assert!(engine.try_dispatch_mcp_tool(&call).await.is_none());
}

#[tokio::test]
async fn try_dispatch_mcp_tool_no_registry_returns_none() {
    let setup = TestSetup::new();
    let engine = setup.create_engine("test");
    let call = theo_infra_llm::types::ToolCall {
        id: "1".into(),
        call_type: "function".into(),
        function: theo_infra_llm::types::FunctionCall {
            name: "mcp:github:search".into(),
            arguments: "{}".into(),
        },
    };
    // No subagent_mcp attached → no dispatcher → None
    assert!(engine.try_dispatch_mcp_tool(&call).await.is_none());
}

#[tokio::test]
async fn try_dispatch_mcp_tool_unknown_server_returns_error_message() {
    let setup = TestSetup::new();
    let engine = setup
        .create_engine("test")
        .with_subagent_mcp(std::sync::Arc::new(theo_infra_mcp::McpRegistry::new()));
    let call = theo_infra_llm::types::ToolCall {
        id: "1".into(),
        call_type: "function".into(),
        function: theo_infra_llm::types::FunctionCall {
            name: "mcp:nonexistent:foo".into(),
            arguments: "{}".into(),
        },
    };
    let msg = engine.try_dispatch_mcp_tool(&call).await.unwrap();
    let content = msg.content.unwrap_or_default();
    assert!(content.contains("mcp dispatch failed"));
}

#[test]
fn engine_with_subagent_cancellation_stores_reference() {
    let setup = TestSetup::new();
    let engine = setup
        .create_engine("test")
        .with_subagent_cancellation(std::sync::Arc::new(
            crate::cancellation::CancellationTree::new(),
        ));
    assert!(engine.subagent.cancellation.is_some());
}

