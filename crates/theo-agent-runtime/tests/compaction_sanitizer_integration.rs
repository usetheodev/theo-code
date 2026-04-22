//! Integration tests: compaction + sanitizer invariants.
//!
//! Validates that post-compaction the message list never contains:
//! - Tool results without a matching assistant tool_call (orphan results)
//! - Assistant tool_calls without a matching tool result (orphan calls)
//!
//! These invariants must hold after both `compact_if_needed` and
//! `compact_messages_to_target`, otherwise the next LLM request will be
//! rejected with "No tool call found for call_id".

use std::collections::HashSet;
use theo_agent_runtime::compaction::{compact_if_needed, compact_messages_to_target};
use theo_infra_llm::types::{Message, Role, ToolCall};

fn collect_assistant_call_ids(messages: &[Message]) -> HashSet<String> {
    messages
        .iter()
        .filter(|m| m.role == Role::Assistant)
        .filter_map(|m| m.tool_calls.as_ref())
        .flat_map(|tcs| tcs.iter().map(|tc| tc.id.clone()))
        .collect()
}

fn collect_result_call_ids(messages: &[Message]) -> HashSet<String> {
    messages
        .iter()
        .filter(|m| m.role == Role::Tool)
        .filter_map(|m| m.tool_call_id.clone())
        .collect()
}

fn assert_tool_pairs_intact(messages: &[Message]) {
    let calls = collect_assistant_call_ids(messages);
    let results = collect_result_call_ids(messages);
    assert_eq!(
        calls, results,
        "tool pair mismatch: calls={:?} results={:?}",
        calls, results
    );
}

fn build_heavy_session(turns: usize, payload_chars: usize) -> Vec<Message> {
    let mut msgs = vec![Message::system("you are helpful")];
    let big = "x".repeat(payload_chars);
    for i in 0..turns {
        msgs.push(Message::user(format!("turn {i}")));
        msgs.push(Message::assistant_with_tool_calls(
            None,
            vec![ToolCall::new(
                format!("call_{i}"),
                "read",
                r#"{"path":"f.rs"}"#,
            )],
        ));
        msgs.push(Message::tool_result(
            format!("call_{i}"),
            "read",
            &big,
        ));
    }
    msgs
}

#[test]
fn compact_if_needed_preserves_tool_pair_integrity() {
    let mut msgs = build_heavy_session(20, 2000);
    compact_if_needed(&mut msgs, 1_000);
    assert_tool_pairs_intact(&msgs);
}

#[test]
fn compact_messages_to_target_preserves_tool_pair_integrity() {
    let mut msgs = build_heavy_session(20, 2000);
    compact_messages_to_target(&mut msgs, 500, "Fix login bug");
    assert_tool_pairs_intact(&msgs);
}

#[test]
fn compaction_then_sanitizer_is_idempotent() {
    let mut msgs = build_heavy_session(20, 2000);
    compact_if_needed(&mut msgs, 1_000);
    let snapshot = msgs.clone();
    compact_if_needed(&mut msgs, 1_000);
    assert_eq!(msgs, snapshot);
}

#[test]
fn aggressive_drop_never_leaves_orphan_results() {
    let mut msgs = build_heavy_session(30, 500);
    // Very aggressive target forces drops in compact_messages_to_target.
    compact_messages_to_target(&mut msgs, 50, "task");
    assert_tool_pairs_intact(&msgs);
    for msg in &msgs {
        if msg.role == Role::Tool {
            assert!(
                msg.tool_call_id.is_some(),
                "tool message missing tool_call_id after compaction"
            );
        }
    }
}
