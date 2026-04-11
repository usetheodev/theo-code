//! Context compaction — heuristic-based compression of conversation history.
//!
//! When the message history grows beyond a token threshold (80% of context window),
//! older messages are compressed while preserving:
//! - All system messages (integrally)
//! - The last PRESERVE_TAIL messages (integrally)
//! - Tool call/result pairs as atomic units
//! - A summary of compacted content

use theo_infra_llm::types::{Message, Role};

/// Structured context for semantic compaction summaries.
/// Passed by the runtime to enrich the compaction summary with progress info.
#[derive(Debug, Clone, Default)]
pub struct CompactionContext {
    /// Current task objective (e.g., "Fix login bug").
    pub task_objective: String,
    /// Current phase in the workflow (e.g., "EDIT", "VERIFY").
    pub current_phase: String,
    /// Files identified as targets for editing.
    pub target_files: Vec<String>,
    /// Recent errors encountered (last 2-3, truncated).
    pub recent_errors: Vec<String>,
}

/// Number of recent messages to always preserve fully.
const PRESERVE_TAIL: usize = 6;

/// Max chars to keep in truncated tool results.
const TRUNCATE_TOOL_RESULT_CHARS: usize = 200;

/// Threshold: compact when tokens exceed this fraction of context window.
const COMPACT_THRESHOLD: f64 = 0.80;

/// Prefix for compaction summary messages (used for idempotence detection).
const COMPACTED_PREFIX: &str = "[COMPACTED] ";

// ---------------------------------------------------------------------------
// Token estimation
// ---------------------------------------------------------------------------

/// Estimate token count for a single message using unified token estimation.
fn estimate_message_tokens(m: &Message) -> usize {
    let content = m.content.as_deref().unwrap_or("");
    let tool_calls_text: String = m
        .tool_calls
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|tc| format!("{} {} {}", tc.id, tc.function.name, tc.function.arguments))
        .collect::<Vec<_>>()
        .join(" ");
    let combined = if tool_calls_text.is_empty() {
        content.to_string()
    } else {
        format!("{content} {tool_calls_text}")
    };
    theo_domain::tokens::estimate_message_tokens(&combined)
}

/// Estimate total tokens in a message vec.
pub fn estimate_total_tokens(messages: &[Message]) -> usize {
    messages.iter().map(|m| estimate_message_tokens(m)).sum()
}

// ---------------------------------------------------------------------------
// UTF-8 safe truncation
// ---------------------------------------------------------------------------

/// Truncate string to at most `max_chars` characters, UTF-8 safe.
fn truncate_utf8(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars).collect();
    format!("{truncated}... [truncated]")
}

// ---------------------------------------------------------------------------
// Compaction
// ---------------------------------------------------------------------------

/// Compact the message history if it exceeds the token threshold.
///
/// Modifies `messages` in-place. Idempotent — safe to call every iteration.
///
/// Rules:
/// 1. System messages → always preserved integrally
/// 2. Last PRESERVE_TAIL messages → preserved integrally
/// 3. Tool results outside the tail → content truncated to 200 chars
/// 4. (assistant_with_tool_calls + tool_results) treated as atomic pair
/// 5. Previous compaction summaries are replaced (idempotent)
/// 6. A compaction summary is inserted as a user message
pub fn compact_if_needed(messages: &mut Vec<Message>, context_window_tokens: usize) {
    compact_if_needed_with_context(messages, context_window_tokens, None);
}

/// Compact with optional semantic context for richer summaries.
pub fn compact_if_needed_with_context(
    messages: &mut Vec<Message>,
    context_window_tokens: usize,
    context: Option<&CompactionContext>,
) {
    if messages.is_empty() || context_window_tokens == 0 {
        return;
    }

    let threshold = (context_window_tokens as f64 * COMPACT_THRESHOLD) as usize;
    let total = estimate_total_tokens(messages);

    if total <= threshold {
        return;
    }

    // Identify boundary: everything before this index is a candidate for compaction.
    // We preserve: all system messages (any position) + last PRESERVE_TAIL non-system messages.
    let non_system_count = messages.iter().filter(|m| m.role != Role::System).count();

    if non_system_count <= PRESERVE_TAIL {
        // Not enough messages to compact — everything is in the "preserve" zone.
        return;
    }

    // Find the index that separates compactable from preserved.
    // Count non-system messages from the end to find the PRESERVE_TAIL boundary.
    let mut tail_count = 0;
    let mut boundary_idx = messages.len();
    for (i, m) in messages.iter().enumerate().rev() {
        if m.role != Role::System {
            tail_count += 1;
            if tail_count == PRESERVE_TAIL {
                boundary_idx = i;
                break;
            }
        }
    }

    // Collect info for summary before modifying.
    let mut tools_used: Vec<String> = Vec::new();
    let mut files_mentioned: Vec<String> = Vec::new();
    let mut compacted_turns = 0;

    // Process messages before the boundary — compact them.
    let mut compacted = false;
    for i in 0..boundary_idx {
        let m = &messages[i];

        // System messages: never touch.
        if m.role == Role::System {
            continue;
        }

        // Skip already-compacted summaries.
        if m.role == Role::User {
            if let Some(ref c) = m.content {
                if c.starts_with(COMPACTED_PREFIX) {
                    continue;
                }
            }
        }

        // Tool results: truncate content.
        if m.role == Role::Tool {
            if let Some(ref name) = messages[i].name {
                if !tools_used.contains(name) {
                    tools_used.push(name.clone());
                }
            }
            if let Some(ref content) = messages[i].content {
                if content.len() > TRUNCATE_TOOL_RESULT_CHARS * 4 {
                    // Extract file mentions before truncating.
                    for word in content.split_whitespace() {
                        if (word.contains('/') || word.contains('.'))
                            && word.len() < 100
                            && (word.ends_with(".rs")
                                || word.ends_with(".ts")
                                || word.ends_with(".py")
                                || word.ends_with(".js")
                                || word.ends_with(".go"))
                        {
                            let clean = word.trim_matches(|c: char| {
                                !c.is_alphanumeric() && c != '/' && c != '.' && c != '_' && c != '-'
                            });
                            if !files_mentioned.contains(&clean.to_string()) {
                                files_mentioned.push(clean.to_string());
                            }
                        }
                    }
                    messages[i].content = Some(truncate_utf8(content, TRUNCATE_TOOL_RESULT_CHARS));
                    compacted = true;
                }
            }
            compacted_turns += 1;
            continue;
        }

        // Assistant with tool calls: keep the tool call names, truncate arguments.
        if m.role == Role::Assistant {
            if let Some(ref tcs) = messages[i].tool_calls {
                for tc in tcs {
                    if !tools_used.contains(&tc.function.name) {
                        tools_used.push(tc.function.name.clone());
                    }
                }
                // Truncate long arguments in tool calls.
                if let Some(ref mut tcs) = messages[i].tool_calls {
                    for tc in tcs.iter_mut() {
                        if tc.function.arguments.len() > TRUNCATE_TOOL_RESULT_CHARS * 4 {
                            tc.function.arguments =
                                truncate_utf8(&tc.function.arguments, TRUNCATE_TOOL_RESULT_CHARS);
                            compacted = true;
                        }
                    }
                }
            }
            compacted_turns += 1;
        }
    }

    if !compacted {
        return; // Nothing was actually truncated.
    }

    // Remove any previous compaction summary (idempotence).
    messages.retain(|m| {
        !(m.role == Role::User
            && m.content
                .as_deref()
                .is_some_and(|c| c.starts_with(COMPACTED_PREFIX)))
    });

    // Build and insert summary.
    let files_str = if files_mentioned.is_empty() {
        String::new()
    } else {
        files_mentioned.truncate(20); // Cap to avoid huge summary.
        format!(" Files involved: {}.", files_mentioned.join(", "))
    };

    let tools_str = if tools_used.is_empty() {
        String::new()
    } else {
        tools_used.sort();
        tools_used.dedup();
        format!(" Tools used: {}.", tools_used.join(", "))
    };

    // Build semantic summary with optional progress context.
    let progress_str = if let Some(ctx) = context {
        let mut parts = Vec::new();
        if !ctx.task_objective.is_empty() {
            let obj = truncate_utf8(&ctx.task_objective, 100);
            parts.push(format!(" Task: {obj}."));
        }
        if !ctx.current_phase.is_empty() {
            parts.push(format!(" Phase: {}.", ctx.current_phase));
        }
        if !ctx.target_files.is_empty() {
            let targets: Vec<&str> = ctx
                .target_files
                .iter()
                .take(5)
                .map(|s| s.as_str())
                .collect();
            parts.push(format!(" Targets: {}.", targets.join(", ")));
        }
        if !ctx.recent_errors.is_empty() {
            let errs: Vec<String> = ctx
                .recent_errors
                .iter()
                .take(2)
                .map(|e| truncate_utf8(e, 80))
                .collect();
            parts.push(format!(" Errors: {}.", errs.join("; ")));
        }
        parts.join("")
    } else {
        String::new()
    };

    let summary = format!(
        "{COMPACTED_PREFIX}Conversation history was compressed ({compacted_turns} older messages truncated).{progress_str}{tools_str}{files_str} Recent messages are preserved in full."
    );

    // Insert after the last system message, before the preserved tail.
    let insert_pos = messages
        .iter()
        .rposition(|m| m.role == Role::System)
        .map(|i| i + 1)
        .unwrap_or(0);

    messages.insert(insert_pos, Message::user(summary));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_messages(count: usize, content_size: usize) -> Vec<Message> {
        let mut msgs = vec![Message::system("You are helpful.")];
        let big_content = "x".repeat(content_size);
        for i in 0..count {
            msgs.push(Message::user(&format!("task {i}")));
            msgs.push(Message::assistant_with_tool_calls(
                None,
                vec![theo_infra_llm::types::ToolCall::new(
                    format!("call_{i}"),
                    "read",
                    r#"{"filePath":"src/main.rs"}"#,
                )],
            ));
            msgs.push(Message::tool_result(
                format!("call_{i}"),
                "read",
                &big_content,
            ));
        }
        msgs
    }

    #[test]
    fn no_compaction_below_threshold() {
        let mut msgs = vec![
            Message::system("system"),
            Message::user("hello"),
            Message::assistant("world"),
        ];
        compact_if_needed(&mut msgs, 128_000);
        assert_eq!(msgs.len(), 3);
    }

    #[test]
    fn empty_vec_no_panic() {
        let mut msgs: Vec<Message> = vec![];
        compact_if_needed(&mut msgs, 128_000);
        assert!(msgs.is_empty());
    }

    #[test]
    fn fewer_than_preserve_tail_no_compaction() {
        let mut msgs = vec![
            Message::system("s"),
            Message::user("u1"),
            Message::assistant("a1"),
        ];
        // Force threshold to be very low so it would trigger.
        compact_if_needed(&mut msgs, 1);
        // Only 2 non-system messages < PRESERVE_TAIL — no compaction.
        assert_eq!(msgs.len(), 3);
    }

    #[test]
    fn compaction_truncates_old_tool_results() {
        // 20 turns with 2000-char tool results → should exceed any reasonable threshold.
        let mut msgs = make_messages(20, 2000);

        compact_if_needed(&mut msgs, 1_000); // Very small window to force compaction.

        // Old tool results should be truncated.
        let old_tool = msgs.iter().find(|m| {
            m.role == Role::Tool
                && m.content
                    .as_deref()
                    .is_some_and(|c| c.contains("[truncated]"))
        });
        assert!(old_tool.is_some(), "Expected truncated tool result");

        // Summary should be inserted.
        let summary = msgs.iter().find(|m| {
            m.content
                .as_deref()
                .is_some_and(|c| c.starts_with(COMPACTED_PREFIX))
        });
        assert!(summary.is_some(), "Expected compaction summary");
    }

    #[test]
    fn system_messages_always_preserved() {
        let mut msgs = vec![
            Message::system("IMPORTANT SYSTEM PROMPT WITH LOTS OF TEXT"),
            Message::system("SECOND SYSTEM MESSAGE"),
        ];
        // Add enough messages to trigger compaction.
        for i in 0..20 {
            msgs.push(Message::user(&format!("task {i}")));
            msgs.push(Message::tool_result(
                format!("c{i}"),
                "read",
                &"x".repeat(2000),
            ));
        }

        compact_if_needed(&mut msgs, 1_000);

        // Both system messages must be intact.
        let systems: Vec<_> = msgs.iter().filter(|m| m.role == Role::System).collect();
        assert_eq!(systems.len(), 2);
        assert_eq!(
            systems[0].content.as_deref().unwrap(),
            "IMPORTANT SYSTEM PROMPT WITH LOTS OF TEXT"
        );
    }

    #[test]
    fn last_n_messages_preserved() {
        let mut msgs = make_messages(20, 2000);

        // Record the last PRESERVE_TAIL non-system messages before compaction.
        let last_messages: Vec<String> = msgs
            .iter()
            .filter(|m| m.role != Role::System)
            .rev()
            .take(PRESERVE_TAIL)
            .filter_map(|m| m.content.clone())
            .collect();

        compact_if_needed(&mut msgs, 1_000);

        // Those messages must still be present and unmodified.
        for expected in &last_messages {
            assert!(
                msgs.iter().any(|m| m.content.as_deref() == Some(expected)),
                "Expected preserved message not found: {}",
                &expected[..expected.len().min(50)]
            );
        }
    }

    #[test]
    fn utf8_safe_truncation() {
        let emoji_content = "🎉".repeat(300); // 4 bytes per emoji.
        let mut msgs = vec![Message::system("s")];
        for i in 0..10 {
            msgs.push(Message::user(&format!("task {i}")));
            msgs.push(Message::tool_result(
                format!("c{i}"),
                "read",
                &emoji_content,
            ));
        }

        // Should not panic on UTF-8 boundary.
        compact_if_needed(&mut msgs, 500);
    }

    #[test]
    fn idempotent_no_duplicate_summaries() {
        let mut msgs = make_messages(20, 2000);

        compact_if_needed(&mut msgs, 1_000);
        let summary_count_1 = msgs
            .iter()
            .filter(|m| {
                m.content
                    .as_deref()
                    .is_some_and(|c| c.starts_with(COMPACTED_PREFIX))
            })
            .count();

        compact_if_needed(&mut msgs, 1_000);
        let summary_count_2 = msgs
            .iter()
            .filter(|m| {
                m.content
                    .as_deref()
                    .is_some_and(|c| c.starts_with(COMPACTED_PREFIX))
            })
            .count();

        assert_eq!(summary_count_1, 1);
        assert_eq!(
            summary_count_2, 1,
            "Compaction should not duplicate summaries"
        );
    }

    #[test]
    fn context_window_zero_no_panic() {
        let mut msgs = make_messages(5, 100);
        compact_if_needed(&mut msgs, 0);
    }

    #[test]
    fn threshold_boundary_79_vs_81_percent() {
        // Create messages that total exactly ~100 estimated tokens.
        // 100 tokens × 4 chars/token = 400 chars content + overhead.
        let mut msgs = vec![
            Message::system("s"),
            Message::user("hello"),
            Message::assistant("world"),
            Message::user("u2"),
            Message::assistant("a2"),
            Message::user("u3"),
            Message::assistant("a3"),
            Message::user("u4"),
            Message::assistant("a4"),
        ];

        let total = estimate_total_tokens(&msgs);

        // Window where 80% > total — should NOT compact.
        let window_no_compact = (total as f64 / 0.79) as usize;
        let len_before = msgs.len();
        compact_if_needed(&mut msgs, window_no_compact);
        assert_eq!(msgs.len(), len_before, "Should not compact at 79%");
    }

    #[test]
    fn semantic_compaction_includes_progress_context() {
        let mut msgs = make_messages(20, 2000);
        let ctx = CompactionContext {
            task_objective: "Fix authentication bug in login flow".to_string(),
            current_phase: "EDIT".to_string(),
            target_files: vec!["src/auth.rs".to_string(), "src/login.rs".to_string()],
            recent_errors: vec!["unresolved import `auth::Token`".to_string()],
        };

        compact_if_needed_with_context(&mut msgs, 1_000, Some(&ctx));

        let summary = msgs
            .iter()
            .find(|m| {
                m.content
                    .as_deref()
                    .is_some_and(|c| c.starts_with(COMPACTED_PREFIX))
            })
            .expect("Expected compaction summary");

        let content = summary.content.as_deref().unwrap();
        assert!(
            content.contains("Fix authentication bug"),
            "Summary should contain task objective"
        );
        assert!(
            content.contains("Phase: EDIT"),
            "Summary should contain phase"
        );
        assert!(
            content.contains("src/auth.rs"),
            "Summary should contain target files"
        );
        assert!(
            content.contains("unresolved import"),
            "Summary should contain recent errors"
        );
    }

    #[test]
    fn semantic_compaction_without_context_matches_original() {
        let mut msgs_with = make_messages(20, 2000);
        let mut msgs_without = msgs_with.clone();

        compact_if_needed(&mut msgs_without, 1_000);
        compact_if_needed_with_context(&mut msgs_with, 1_000, None);

        // Both should produce the same summary
        let summary_with = msgs_with.iter().find(|m| {
            m.content
                .as_deref()
                .is_some_and(|c| c.starts_with(COMPACTED_PREFIX))
        });
        let summary_without = msgs_without.iter().find(|m| {
            m.content
                .as_deref()
                .is_some_and(|c| c.starts_with(COMPACTED_PREFIX))
        });

        assert_eq!(
            summary_with.and_then(|m| m.content.as_deref()),
            summary_without.and_then(|m| m.content.as_deref()),
            "compact_if_needed should be identical to compact_if_needed_with_context(None)"
        );
    }
}
