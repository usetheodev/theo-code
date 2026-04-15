//! Dual-message abstraction: AgentMessage wraps both LLM-compatible messages
//! and UI-only messages (compaction summaries, branch summaries, custom).
//!
//! The agent loop works with `Vec<AgentMessage>` internally.
//! `convert_to_llm()` filters and transforms at the LLM call boundary.
//!
//! **Pi-mono ref:**
//! - `packages/agent/src/types.ts:236-246` (AgentMessage type)
//! - `packages/coding-agent/src/core/messages.ts:1-195` (convertToLlm)
//! - `packages/agent/src/agent-loop.ts:247-258` (convertToLlm at boundary)

use theo_infra_llm::types::Message;

/// A message in the agent's conversation history.
///
/// Unlike raw `Message` (which is strictly LLM-compatible), `AgentMessage`
/// can represent UI-only entries that are filtered out before LLM calls.
#[derive(Debug, Clone)]
pub enum AgentMessage {
    /// Standard LLM message (system, user, assistant, tool).
    Llm(Message),

    /// Summary inserted after context compaction.
    /// The agent sees this as context; the LLM sees it as a user message.
    CompactionSummary {
        summary: String,
        tokens_before: usize,
    },

    /// Summary of an abandoned branch (for context after branching).
    BranchSummary {
        summary: String,
        from_id: String,
    },

    /// Custom extension message.
    /// `display`: if true, shown in UI; always filtered from LLM context.
    Custom {
        custom_type: String,
        content: String,
        display: bool,
    },
}

/// Convert a sequence of `AgentMessage` into LLM-compatible `Message` vec.
///
/// - `Llm(msg)` → passed through as-is
/// - `CompactionSummary` → converted to a user message with prefix
/// - `BranchSummary` → converted to a user message with `<summary>` tags
/// - `Custom` → filtered out (never sent to LLM)
pub fn convert_to_llm(messages: &[AgentMessage]) -> Vec<Message> {
    let mut result = Vec::with_capacity(messages.len());

    for msg in messages {
        match msg {
            AgentMessage::Llm(m) => {
                result.push(m.clone());
            }
            AgentMessage::CompactionSummary { summary, .. } => {
                result.push(Message::user(format!("[COMPACTED] {summary}")));
            }
            AgentMessage::BranchSummary { summary, .. } => {
                result.push(Message::user(format!(
                    "<branch_summary>{summary}</branch_summary>"
                )));
            }
            AgentMessage::Custom { .. } => {
                // UI-only — never sent to LLM
            }
        }
    }

    result
}

/// Convenience: wrap a `Message` into `AgentMessage::Llm`.
impl From<Message> for AgentMessage {
    fn from(msg: Message) -> Self {
        AgentMessage::Llm(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theo_infra_llm::types::Role;

    #[test]
    fn convert_llm_messages_pass_through() {
        let messages = vec![
            AgentMessage::Llm(Message::system("system prompt")),
            AgentMessage::Llm(Message::user("hello")),
            AgentMessage::Llm(Message::assistant("hi")),
        ];

        let llm = convert_to_llm(&messages);
        assert_eq!(llm.len(), 3);
        assert_eq!(llm[0].role, Role::System);
        assert_eq!(llm[1].role, Role::User);
        assert_eq!(llm[2].role, Role::Assistant);
    }

    #[test]
    fn convert_compaction_summary_becomes_user_message() {
        let messages = vec![AgentMessage::CompactionSummary {
            summary: "10 older messages compressed".into(),
            tokens_before: 50000,
        }];

        let llm = convert_to_llm(&messages);
        assert_eq!(llm.len(), 1);
        assert_eq!(llm[0].role, Role::User);
        assert!(llm[0]
            .content
            .as_deref()
            .unwrap()
            .starts_with("[COMPACTED]"));
    }

    #[test]
    fn convert_branch_summary_becomes_user_message_with_tags() {
        let messages = vec![AgentMessage::BranchSummary {
            summary: "Tried approach A, failed on import error".into(),
            from_id: "entry_42".into(),
        }];

        let llm = convert_to_llm(&messages);
        assert_eq!(llm.len(), 1);
        assert_eq!(llm[0].role, Role::User);
        let content = llm[0].content.as_deref().unwrap();
        assert!(content.contains("<branch_summary>"));
        assert!(content.contains("approach A"));
    }

    #[test]
    fn convert_custom_messages_filtered_out() {
        let messages = vec![
            AgentMessage::Llm(Message::user("real")),
            AgentMessage::Custom {
                custom_type: "notification".into(),
                content: "Build succeeded".into(),
                display: true,
            },
            AgentMessage::Llm(Message::assistant("response")),
        ];

        let llm = convert_to_llm(&messages);
        assert_eq!(llm.len(), 2, "Custom messages should be filtered out");
        assert_eq!(llm[0].content.as_deref().unwrap(), "real");
        assert_eq!(llm[1].content.as_deref().unwrap(), "response");
    }

    #[test]
    fn convert_empty_returns_empty() {
        assert!(convert_to_llm(&[]).is_empty());
    }

    #[test]
    fn from_message_creates_llm_variant() {
        let msg = Message::user("test");
        let agent_msg: AgentMessage = msg.into();
        match agent_msg {
            AgentMessage::Llm(m) => assert_eq!(m.content.as_deref().unwrap(), "test"),
            _ => panic!("Expected Llm variant"),
        }
    }

    #[test]
    fn mixed_messages_preserve_order() {
        let messages = vec![
            AgentMessage::Llm(Message::system("sys")),
            AgentMessage::CompactionSummary {
                summary: "compacted".into(),
                tokens_before: 100,
            },
            AgentMessage::Custom {
                custom_type: "ui".into(),
                content: "ignored".into(),
                display: false,
            },
            AgentMessage::BranchSummary {
                summary: "branch info".into(),
                from_id: "e1".into(),
            },
            AgentMessage::Llm(Message::user("query")),
        ];

        let llm = convert_to_llm(&messages);
        // sys + compaction + branch + query = 4 (custom filtered)
        assert_eq!(llm.len(), 4);
        assert_eq!(llm[0].role, Role::System);
        assert_eq!(llm[1].role, Role::User); // compaction
        assert_eq!(llm[2].role, Role::User); // branch summary
        assert_eq!(llm[3].role, Role::User); // query
    }
}
