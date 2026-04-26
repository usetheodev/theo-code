//! Structured summary template for LLM-powered compaction.
//!
//! Used by `OptimizationLevel::Compact` to replace the middle of the
//! conversation with a compact structured summary, preserving the
//! "Active Task" (the user's verbatim request) as the load-bearing field.
//!
//! Also exposes `fallback_summary` that produces a non-LLM summary when
//! the provider is unavailable.
//!
//! References:
//! - `referencias/hermes-agent/agent/context_compressor.py:586-644` (template)
//! - `referencias/opendev/crates/opendev-context/src/compaction/compactor/summary.rs:130-191`
//!   (fallback without LLM)
//!
//! Reserved-for-future-use Compact-stage summary builder; not yet wired.
#![allow(dead_code)]

use theo_infra_llm::types::{Message, Role};

/// Injected before any compacted summary so the model treats it as
/// background, not a new instruction.
pub const SUMMARY_PREFIX: &str =
    "Background reference only. Respond only to messages AFTER this summary.";

/// Prompt template sent to the auxiliary LLM that produces the summary.
pub const SUMMARY_TEMPLATE: &str = "\
Produce a concise structured summary with EXACTLY these sections:

## Active Task
The user's most recent request, quoted verbatim. THIS IS THE SINGLE \
MOST IMPORTANT FIELD — if unclear, extract from the latest user message.

## Goal
One sentence on the end state being pursued.

## Completed Actions
Bulleted list. Format: `N. ACTION target — outcome [tool: name]`.
Max 20 items, most recent first.

## Active State
Current in-progress work: open files, running processes, pending edits.

## Blocked
Anything the agent is stuck on (errors, missing info).

## Remaining Work
Concrete next steps, ordered.

Rules:
- No speculation. Use only information already present in the transcript.
- Prefer file paths and exact names over paraphrase.
- If a section is empty, write 'none'.
";

/// Produce a non-LLM fallback summary from message history alone.
///
/// Extracts:
/// - Goal: the first user message (≤300 chars).
/// - Completed actions: the `max_actions` most recent tool invocations
///   (trimmed to `action_chars` each).
/// - Last state: the last assistant message (≤300 chars).
///
/// This is deterministic and side-effect-free — safe to use in tests
/// and offline contexts.
pub fn fallback_summary(messages: &[Message]) -> String {
    let goal = messages
        .iter()
        .find(|m| m.role == Role::User)
        .and_then(|m| m.content.as_deref())
        .map(|s| truncate_chars(s, 300))
        .unwrap_or_else(|| "none".into());

    let max_actions = 20;
    let action_chars = 120;
    let mut actions: Vec<String> = Vec::new();
    for m in messages.iter() {
        if actions.len() >= max_actions {
            break;
        }
        if m.role == Role::Tool {
            let name = m.name.as_deref().unwrap_or("tool");
            let content = m.content.as_deref().unwrap_or("");
            let body = truncate_chars(content, action_chars);
            actions.push(format!("- [tool: {name}] {body}"));
        }
    }

    let last_state = messages
        .iter()
        .rev()
        .find(|m| m.role == Role::Assistant)
        .and_then(|m| m.content.as_deref())
        .map(|s| truncate_chars(s, 300))
        .unwrap_or_else(|| "none".into());

    let actions_str = if actions.is_empty() {
        "none".to_string()
    } else {
        actions.join("\n")
    };

    format!(
        "{SUMMARY_PREFIX}\n\n## Active Task\n{goal}\n\n## Goal\n{goal}\n\n## Completed Actions\n{actions_str}\n\n## Active State\n{last_state}\n\n## Blocked\nnone\n\n## Remaining Work\nnone\n"
    )
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use theo_infra_llm::types::ToolCall;

    fn sample_transcript() -> Vec<Message> {
        vec![
            Message::system("sys"),
            Message::user("Fix the login redirect bug in auth.rs"),
            Message::assistant_with_tool_calls(
                None,
                vec![ToolCall::new("c1", "read", r#"{"path":"auth.rs"}"#)],
            ),
            Message::tool_result("c1", "read", "fn login() { redirect(); }"),
            Message::assistant("found the issue on line 42"),
        ]
    }

    #[test]
    fn summary_prefix_marks_as_background() {
        assert!(SUMMARY_PREFIX.to_lowercase().contains("background"));
    }

    #[test]
    fn template_has_all_required_sections() {
        for header in [
            "## Active Task",
            "## Goal",
            "## Completed Actions",
            "## Active State",
            "## Blocked",
            "## Remaining Work",
        ] {
            assert!(SUMMARY_TEMPLATE.contains(header), "missing section: {header}");
        }
    }

    #[test]
    fn fallback_extracts_goal_from_first_user_message() {
        let s = fallback_summary(&sample_transcript());
        assert!(s.contains("Fix the login redirect bug"));
    }

    #[test]
    fn fallback_includes_tool_calls_with_names() {
        let s = fallback_summary(&sample_transcript());
        assert!(s.contains("[tool: read]"));
    }

    #[test]
    fn fallback_extracts_last_assistant_state() {
        let s = fallback_summary(&sample_transcript());
        assert!(s.contains("found the issue on line 42"));
    }

    #[test]
    fn fallback_starts_with_summary_prefix() {
        let s = fallback_summary(&sample_transcript());
        assert!(s.starts_with(SUMMARY_PREFIX));
    }

    #[test]
    fn fallback_handles_empty_transcript() {
        let s = fallback_summary(&[]);
        assert!(s.contains("## Active Task\nnone"));
        assert!(s.contains("## Completed Actions\nnone"));
    }

    #[test]
    fn fallback_caps_tool_content() {
        let msgs = vec![Message::tool_result("c1", "read", "x".repeat(500))];
        let s = fallback_summary(&msgs);
        // 120 char cap plus ellipsis
        assert!(!s.contains(&"x".repeat(200)));
    }

    #[test]
    fn fallback_caps_actions_at_20() {
        let mut msgs: Vec<Message> = Vec::new();
        for i in 0..30 {
            msgs.push(Message::tool_result(format!("c{i}"), "read", "ok"));
        }
        let s = fallback_summary(&msgs);
        let bullet_count = s.matches("- [tool: read]").count();
        assert_eq!(bullet_count, 20);
    }
}
