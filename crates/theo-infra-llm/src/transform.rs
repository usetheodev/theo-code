//! Cross-provider message transformation for session replay and model switching.
//!
//! When replaying a conversation history with a different LLM provider/model,
//! the messages may contain artifacts incompatible with the target:
//! - Tool call IDs that exceed length limits or contain invalid characters
//! - Orphaned tool calls (assistant requested a tool, but no result follows)
//! - Error/aborted assistant messages that should be stripped
//!
//! **Pi-mono ref:** `packages/ai/src/providers/transform-messages.ts`

use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};

use crate::types::{Message, Role};

/// Maximum length for a tool call ID (Anthropic's limit).
const MAX_TOOL_CALL_ID_LEN: usize = 64;

/// Characters allowed in tool call IDs (Anthropic constraint: `[a-zA-Z0-9_-]`).
fn is_valid_tool_call_id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// Normalize a tool call ID to be compatible with all providers.
///
/// If the ID exceeds MAX_TOOL_CALL_ID_LEN or contains invalid characters,
/// it is deterministically shortened using a hash-based prefix.
/// Uses two rounds of `DefaultHasher` (SipHash) to produce a 128-bit
/// fingerprint formatted as 32 hex chars, giving `"tc_" + 32 = 35` total chars.
pub fn normalize_tool_call_id(id: &str) -> String {
    let needs_normalization =
        id.len() > MAX_TOOL_CALL_ID_LEN || !id.chars().all(is_valid_tool_call_id_char);

    if !needs_normalization {
        return id.to_string();
    }

    // Deterministic 128-bit fingerprint from two independent SipHash passes.
    let mut h1 = DefaultHasher::new();
    id.hash(&mut h1);
    let hash1 = h1.finish();

    let mut h2 = DefaultHasher::new();
    hash1.hash(&mut h2);
    id.hash(&mut h2);
    let hash2 = h2.finish();

    // "tc_" + 16 hex + 16 hex = 35 chars, well within the 64-char limit.
    format!("tc_{:016x}{:016x}", hash1, hash2)
}

/// Transform messages for cross-provider compatibility.
///
/// Operations:
/// 1. Normalize tool call IDs (length/charset constraints)
/// 2. Patch orphaned tool calls (insert synthetic error results)
/// 3. Strip empty/error assistant messages (no content AND no tool calls)
pub fn transform_messages(messages: &[Message]) -> Vec<Message> {
    let mut result: Vec<Message> = Vec::with_capacity(messages.len());

    // Track pending tool calls: normalized_id → tool_name
    let mut pending_tool_calls: Vec<(String, String)> = Vec::new();

    for msg in messages {
        match msg.role {
            Role::Assistant => {
                // Strip empty error/aborted messages (no content and no tool calls)
                let has_content = msg
                    .content
                    .as_deref()
                    .is_some_and(|c| !c.is_empty());
                let has_tool_calls = msg
                    .tool_calls
                    .as_ref()
                    .is_some_and(|tc| !tc.is_empty());

                if !has_content && !has_tool_calls {
                    continue;
                }

                // Normalize tool call IDs
                let mut transformed = msg.clone();
                if let Some(ref mut tool_calls) = transformed.tool_calls {
                    for tc in tool_calls.iter_mut() {
                        let normalized = normalize_tool_call_id(&tc.id);
                        pending_tool_calls
                            .push((normalized.clone(), tc.function.name.clone()));
                        tc.id = normalized;
                    }
                }
                result.push(transformed);
            }
            Role::Tool => {
                // Normalize tool_call_id reference to match the normalized assistant IDs
                let mut transformed = msg.clone();
                if let Some(ref original_id) = msg.tool_call_id {
                    let normalized = normalize_tool_call_id(original_id);
                    transformed.tool_call_id = Some(normalized.clone());

                    // Remove from pending (this tool call now has a result)
                    pending_tool_calls.retain(|(nid, _)| nid != &normalized);
                }
                result.push(transformed);
            }
            Role::User => {
                // Before inserting a user message, patch any orphaned tool calls
                // from a previous assistant turn.
                flush_orphaned_tool_calls(&mut result, &mut pending_tool_calls);
                result.push(msg.clone());
            }
            Role::System => {
                result.push(msg.clone());
            }
        }
    }

    // Patch any remaining orphaned tool calls at the end of the history
    flush_orphaned_tool_calls(&mut result, &mut pending_tool_calls);

    result
}

/// Insert synthetic error results for orphaned tool calls so that
/// the API never sees an assistant message with unresolved tool_call IDs.
fn flush_orphaned_tool_calls(
    messages: &mut Vec<Message>,
    pending: &mut Vec<(String, String)>,
) {
    for (normalized_id, tool_name) in pending.drain(..) {
        messages.push(Message::tool_result(
            &normalized_id,
            &tool_name,
            "[Error: tool call result was not provided — session was interrupted or model was switched]",
        ));
    }
}

/// Detect orphaned tool call IDs in a message sequence (diagnostic utility).
///
/// Returns tool_call IDs that appear in assistant tool_calls but have no
/// matching Tool message with that tool_call_id.
pub fn find_orphaned_tool_call_ids(messages: &[Message]) -> HashSet<String> {
    let mut requested: HashSet<String> = HashSet::new();
    let mut responded: HashSet<String> = HashSet::new();

    for msg in messages {
        if let Some(ref tcs) = msg.tool_calls {
            for tc in tcs {
                requested.insert(tc.id.clone());
            }
        }
        if let Some(ref id) = msg.tool_call_id {
            responded.insert(id.clone());
        }
    }

    requested.difference(&responded).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ToolCall;

    // ── Tool call ID normalization ──────────────────────────────

    #[test]
    fn short_valid_id_unchanged() {
        assert_eq!(normalize_tool_call_id("call_abc123"), "call_abc123");
    }

    #[test]
    fn empty_id_gets_normalized() {
        // Empty string has no valid chars check issue, but length 0 < 64.
        // However it's a degenerate case; should still be deterministic.
        let result = normalize_tool_call_id("");
        // Empty string passes both checks (len=0 ≤ 64, no invalid chars),
        // so it should be returned as-is.
        assert_eq!(result, "");
    }

    #[test]
    fn long_id_gets_normalized() {
        let long_id = "a".repeat(100);
        let normalized = normalize_tool_call_id(&long_id);
        assert!(
            normalized.len() <= MAX_TOOL_CALL_ID_LEN,
            "Normalized ID '{}' is {} chars, expected ≤ {}",
            normalized,
            normalized.len(),
            MAX_TOOL_CALL_ID_LEN
        );
        assert!(normalized.starts_with("tc_"));
    }

    #[test]
    fn id_with_pipe_gets_normalized() {
        let id = "msg_Abc123|call_001";
        let normalized = normalize_tool_call_id(id);
        assert!(
            normalized.chars().all(is_valid_tool_call_id_char),
            "Normalized ID '{}' contains invalid chars",
            normalized
        );
        assert!(normalized.starts_with("tc_"));
    }

    #[test]
    fn id_with_slash_gets_normalized() {
        let id = "call/with/slashes";
        let normalized = normalize_tool_call_id(id);
        assert!(normalized.chars().all(is_valid_tool_call_id_char));
    }

    #[test]
    fn normalization_is_deterministic() {
        let id = "some-long-id-with-special|chars.that" ;
        let a = normalize_tool_call_id(id);
        let b = normalize_tool_call_id(id);
        assert_eq!(a, b, "Same input must produce same output");
    }

    #[test]
    fn different_ids_produce_different_normalized() {
        let a = normalize_tool_call_id("msg_A|call_001");
        let b = normalize_tool_call_id("msg_B|call_002");
        assert_ne!(a, b, "Different inputs should produce different outputs");
    }

    #[test]
    fn exact_64_char_valid_id_unchanged() {
        let id = "a".repeat(64);
        assert_eq!(normalize_tool_call_id(&id), id);
    }

    #[test]
    fn exactly_65_char_id_gets_normalized() {
        let id = "a".repeat(65);
        let normalized = normalize_tool_call_id(&id);
        assert!(normalized.len() <= MAX_TOOL_CALL_ID_LEN);
        assert!(normalized.starts_with("tc_"));
    }

    // ── Orphaned tool call patching ─────────────────────────────

    #[test]
    fn orphaned_tool_call_gets_synthetic_result() {
        let messages = vec![
            Message::system("sys"),
            Message::user("do something"),
            Message::assistant_with_tool_calls(
                None,
                vec![ToolCall::new("call_1", "read", r#"{"filePath":"a.rs"}"#)],
            ),
            // No tool result for call_1!
            Message::user("continue"),
        ];

        let transformed = transform_messages(&messages);

        let tool_results: Vec<_> = transformed
            .iter()
            .filter(|m| m.role == Role::Tool)
            .collect();
        assert_eq!(tool_results.len(), 1, "Expected one synthetic tool result");
        assert!(
            tool_results[0]
                .content
                .as_deref()
                .unwrap()
                .contains("interrupted"),
            "Synthetic result should mention interruption"
        );
    }

    #[test]
    fn orphaned_at_end_of_messages_gets_patched() {
        let messages = vec![
            Message::user("test"),
            Message::assistant_with_tool_calls(
                None,
                vec![ToolCall::new("call_x", "write", "{}")],
            ),
            // History ends without tool result
        ];

        let transformed = transform_messages(&messages);
        let tool_results: Vec<_> = transformed
            .iter()
            .filter(|m| m.role == Role::Tool)
            .collect();
        assert_eq!(tool_results.len(), 1);
    }

    #[test]
    fn multiple_orphaned_tool_calls_all_patched() {
        let messages = vec![
            Message::user("do things"),
            Message::assistant_with_tool_calls(
                None,
                vec![
                    ToolCall::new("c1", "read", "{}"),
                    ToolCall::new("c2", "write", "{}"),
                    ToolCall::new("c3", "bash", "{}"),
                ],
            ),
            Message::tool_result("c1", "read", "ok"),
            // c2 and c3 orphaned
            Message::user("next"),
        ];

        let transformed = transform_messages(&messages);
        let synthetic: Vec<_> = transformed
            .iter()
            .filter(|m| {
                m.role == Role::Tool
                    && m.content
                        .as_deref()
                        .is_some_and(|c| c.contains("interrupted"))
            })
            .collect();
        assert_eq!(synthetic.len(), 2, "Two orphaned calls should get patched");
    }

    #[test]
    fn complete_tool_call_not_patched() {
        let messages = vec![
            Message::user("read a file"),
            Message::assistant_with_tool_calls(
                None,
                vec![ToolCall::new("call_1", "read", r#"{"filePath":"a.rs"}"#)],
            ),
            Message::tool_result("call_1", "read", "file contents here"),
        ];

        let transformed = transform_messages(&messages);

        let tool_results: Vec<_> = transformed
            .iter()
            .filter(|m| m.role == Role::Tool)
            .collect();
        assert_eq!(tool_results.len(), 1, "Should have exactly one tool result");
        assert_eq!(
            tool_results[0].content.as_deref().unwrap(),
            "file contents here",
            "Original result should be preserved"
        );
    }

    // ── Empty/error assistant stripping ─────────────────────────

    #[test]
    fn empty_assistant_message_stripped() {
        let messages = vec![
            Message::user("hello"),
            Message {
                role: Role::Assistant,
                content: Some(String::new()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            Message::user("retry"),
        ];

        let transformed = transform_messages(&messages);
        let assistants: Vec<_> = transformed
            .iter()
            .filter(|m| m.role == Role::Assistant)
            .collect();
        assert!(
            assistants.is_empty(),
            "Empty assistant message should be stripped"
        );
    }

    #[test]
    fn none_content_no_tools_assistant_stripped() {
        let messages = vec![
            Message::user("hello"),
            Message {
                role: Role::Assistant,
                content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            Message::user("retry"),
        ];

        let transformed = transform_messages(&messages);
        let assistants: Vec<_> = transformed
            .iter()
            .filter(|m| m.role == Role::Assistant)
            .collect();
        assert!(assistants.is_empty());
    }

    #[test]
    fn assistant_with_content_preserved() {
        let messages = vec![
            Message::user("hello"),
            Message::assistant("I can help with that"),
        ];

        let transformed = transform_messages(&messages);
        let assistants: Vec<_> = transformed
            .iter()
            .filter(|m| m.role == Role::Assistant)
            .collect();
        assert_eq!(assistants.len(), 1);
    }

    #[test]
    fn assistant_with_tool_calls_but_no_content_preserved() {
        let messages = vec![
            Message::user("read file"),
            Message::assistant_with_tool_calls(
                None,
                vec![ToolCall::new("c1", "read", "{}")],
            ),
            Message::tool_result("c1", "read", "content"),
        ];

        let transformed = transform_messages(&messages);
        let assistants: Vec<_> = transformed
            .iter()
            .filter(|m| m.role == Role::Assistant)
            .collect();
        assert_eq!(
            assistants.len(),
            1,
            "Assistant with tool calls should be preserved even without content"
        );
    }

    // ── Tool call ID normalization within messages ──────────────

    #[test]
    fn tool_call_ids_normalized_consistently_in_assistant_and_result() {
        let long_id = "x".repeat(100);
        let messages = vec![
            Message::user("test"),
            Message::assistant_with_tool_calls(
                None,
                vec![ToolCall::new(&long_id, "read", r#"{"filePath":"a.rs"}"#)],
            ),
            Message::tool_result(&long_id, "read", "content"),
        ];

        let transformed = transform_messages(&messages);

        let assistant = transformed
            .iter()
            .find(|m| m.role == Role::Assistant)
            .unwrap();
        let tool_result = transformed.iter().find(|m| m.role == Role::Tool).unwrap();

        let assistant_tc_id = &assistant.tool_calls.as_ref().unwrap()[0].id;
        let result_tc_id = tool_result.tool_call_id.as_ref().unwrap();

        assert_eq!(
            assistant_tc_id, result_tc_id,
            "Assistant and tool result must reference the same normalized ID"
        );
        assert!(
            assistant_tc_id.len() <= MAX_TOOL_CALL_ID_LEN,
            "Normalized ID must fit within {MAX_TOOL_CALL_ID_LEN} chars"
        );
    }

    // ── System messages always preserved ────────────────────────

    #[test]
    fn system_messages_pass_through_unchanged() {
        let messages = vec![
            Message::system("You are a helpful assistant"),
            Message::user("hello"),
            Message::assistant("hi"),
        ];

        let transformed = transform_messages(&messages);
        let systems: Vec<_> = transformed
            .iter()
            .filter(|m| m.role == Role::System)
            .collect();
        assert_eq!(systems.len(), 1);
        assert_eq!(
            systems[0].content.as_deref().unwrap(),
            "You are a helpful assistant"
        );
    }

    // ── find_orphaned_tool_call_ids ─────────────────────────────

    #[test]
    fn find_orphaned_detects_missing_results() {
        let messages = vec![
            Message::assistant_with_tool_calls(
                None,
                vec![
                    ToolCall::new("call_1", "read", "{}"),
                    ToolCall::new("call_2", "write", "{}"),
                ],
            ),
            Message::tool_result("call_1", "read", "ok"),
            // call_2 has no result
        ];

        let orphaned = find_orphaned_tool_call_ids(&messages);
        assert!(orphaned.contains("call_2"));
        assert!(!orphaned.contains("call_1"));
    }

    #[test]
    fn find_orphaned_returns_empty_when_all_resolved() {
        let messages = vec![
            Message::assistant_with_tool_calls(
                None,
                vec![ToolCall::new("c1", "read", "{}")],
            ),
            Message::tool_result("c1", "read", "ok"),
        ];

        let orphaned = find_orphaned_tool_call_ids(&messages);
        assert!(orphaned.is_empty());
    }

    // ── Edge cases ──────────────────────────────────────────────

    #[test]
    fn empty_message_list_returns_empty() {
        let transformed = transform_messages(&[]);
        assert!(transformed.is_empty());
    }

    #[test]
    fn only_system_and_user_messages_pass_through() {
        let messages = vec![
            Message::system("sys"),
            Message::user("u1"),
            Message::user("u2"),
        ];

        let transformed = transform_messages(&messages);
        assert_eq!(transformed.len(), 3);
    }
}
