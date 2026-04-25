//! Tool pair integrity — post-compaction structural correctness.
//!
//! **This module is NOT a secret/PII scrubber.** Despite the legacy name
//! `sanitizer.rs` (renamed to `tool_pair_integrity.rs` in T1.2 of the
//! agent-runtime remediation plan, see ADR/finding FIND-P6-008), this
//! module's only responsibility is repairing **orphaned `tool_use` /
//! `tool_result` pairs** in a message list after compaction.
//!
//! For PII / API-key redaction see `secret_scrubber.rs` (added in T4.5).
//!
//! After compaction (masking, pruning, summarization) the message list
//! can end up with:
//!
//! 1. **Orphaned tool results** — a `Role::Tool` message whose matching
//!    assistant `tool_calls[].id` was dropped. Providers reject this
//!    with "No tool call found for call_id".
//! 2. **Orphaned tool calls** — an assistant `tool_calls[i]` whose
//!    matching `Role::Tool` result was dropped. Providers reject this
//!    with "Tool call has no matching result".
//!
//! This module provides `sanitize_tool_pairs` that MUST be called after
//! any compaction that mutates the message list. Idempotent when pairs
//! are valid. After T3.4 the compaction algorithm preserves pairs at
//! design level — `sanitize_tool_pairs` becomes a defensive backstop.
//!
//! Reference pattern: `referencias/hermes-agent/agent/context_compressor.py:778-836`

use std::collections::HashSet;
use theo_infra_llm::types::{Message, Role};

/// Placeholder content injected for an orphaned tool call whose result was elided.
const ELIDED_RESULT: &str = "[result elided by compaction]";

/// Ensure every `Role::Tool` message matches an existing assistant tool_call,
/// and every assistant tool_call has a matching `Role::Tool` result.
///
/// Strategy:
/// 1. Collect `surviving_call_ids` from all `Role::Assistant` tool_calls.
/// 2. Collect `result_call_ids` from all `Role::Tool` messages.
/// 3. Remove orphaned results (tool messages with `tool_call_id` not in surviving).
/// 4. For each surviving call_id without a result, append a stub result immediately
///    after the assistant message containing that call.
///
/// Idempotent: running twice on a sanitized list is a no-op.
pub fn sanitize_tool_pairs(messages: &mut Vec<Message>) {
    let surviving: HashSet<String> = messages
        .iter()
        .filter(|m| m.role == Role::Assistant)
        .filter_map(|m| m.tool_calls.as_ref())
        .flat_map(|tcs| tcs.iter().map(|tc| tc.id.clone()))
        .collect();

    messages.retain(|m| {
        if m.role != Role::Tool {
            return true;
        }
        match &m.tool_call_id {
            Some(id) => surviving.contains(id),
            None => false,
        }
    });

    let answered: HashSet<String> = messages
        .iter()
        .filter(|m| m.role == Role::Tool)
        .filter_map(|m| m.tool_call_id.clone())
        .collect();

    let mut insertions: Vec<(usize, Message)> = Vec::new();
    for (idx, msg) in messages.iter().enumerate() {
        if msg.role != Role::Assistant {
            continue;
        }
        let Some(tool_calls) = &msg.tool_calls else {
            continue;
        };
        for tc in tool_calls {
            if !answered.contains(&tc.id) {
                insertions.push((
                    idx + 1,
                    Message::tool_result(tc.id.clone(), tc.function.name.clone(), ELIDED_RESULT),
                ));
            }
        }
    }

    for (offset, (insert_at, stub)) in insertions.into_iter().enumerate() {
        messages.insert(insert_at + offset, stub);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theo_infra_llm::types::ToolCall;

    fn assistant_with_call(id: &str, name: &str) -> Message {
        Message::assistant_with_tool_calls(
            None,
            vec![ToolCall::new(id, name, r#"{"q":1}"#)],
        )
    }

    #[test]
    fn removes_orphaned_tool_result_without_matching_call() {
        let mut msgs = vec![
            Message::user("hi"),
            Message::tool_result("stale-id", "read", "content"),
        ];
        sanitize_tool_pairs(&mut msgs);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, Role::User);
    }

    #[test]
    fn injects_stub_for_tool_call_without_result() {
        let mut msgs = vec![
            Message::user("hi"),
            assistant_with_call("c1", "read"),
        ];
        sanitize_tool_pairs(&mut msgs);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[2].role, Role::Tool);
        assert_eq!(msgs[2].tool_call_id.as_deref(), Some("c1"));
        assert_eq!(msgs[2].content.as_deref(), Some(ELIDED_RESULT));
    }

    #[test]
    fn idempotent_when_pairs_already_valid() {
        let mut msgs = vec![
            assistant_with_call("c1", "read"),
            Message::tool_result("c1", "read", "ok"),
        ];
        let snapshot = msgs.clone();
        sanitize_tool_pairs(&mut msgs);
        assert_eq!(msgs, snapshot);
    }

    #[test]
    fn running_twice_is_a_noop() {
        let mut msgs = vec![
            Message::user("hi"),
            assistant_with_call("c1", "read"),
            Message::tool_result("stale", "x", "drop me"),
        ];
        sanitize_tool_pairs(&mut msgs);
        let after_first = msgs.clone();
        sanitize_tool_pairs(&mut msgs);
        assert_eq!(msgs, after_first);
    }

    #[test]
    fn handles_multiple_calls_in_single_assistant_message() {
        let msg = Message::assistant_with_tool_calls(
            None,
            vec![
                ToolCall::new("c1", "read", "{}"),
                ToolCall::new("c2", "write", "{}"),
            ],
        );
        let mut msgs = vec![msg, Message::tool_result("c1", "read", "ok")];
        sanitize_tool_pairs(&mut msgs);
        assert_eq!(msgs.len(), 3);
        let stub_ids: Vec<&str> = msgs
            .iter()
            .filter(|m| m.role == Role::Tool)
            .filter_map(|m| m.tool_call_id.as_deref())
            .collect();
        assert!(stub_ids.contains(&"c1"));
        assert!(stub_ids.contains(&"c2"));
    }

    #[test]
    fn preserves_system_and_user_messages_untouched() {
        let mut msgs = vec![
            Message::system("sys"),
            Message::user("u"),
            Message::assistant("ok"),
        ];
        let snapshot = msgs.clone();
        sanitize_tool_pairs(&mut msgs);
        assert_eq!(msgs, snapshot);
    }

    #[test]
    fn tool_message_without_tool_call_id_is_dropped() {
        let mut msgs = vec![
            Message::user("u"),
            Message {
                role: Role::Tool,
                content: Some("dangling".to_string()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
        ];
        sanitize_tool_pairs(&mut msgs);
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn empty_vec_is_noop() {
        let mut msgs: Vec<Message> = vec![];
        sanitize_tool_pairs(&mut msgs);
        assert!(msgs.is_empty());
    }
}
