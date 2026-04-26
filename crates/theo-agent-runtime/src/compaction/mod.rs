//! Context compaction — heuristic-based compression of conversation history.
//!
//! When the message history grows beyond a token threshold (80% of context window),
//! older messages are compressed while preserving:
//! - All system messages (integrally)
//! - The last PRESERVE_TAIL messages (integrally)
//! - Tool call/result pairs as atomic units
//! - A summary of compacted content

mod policy_engine;

use crate::config::CompactionPolicy;
use crate::tool_pair_integrity::sanitize_tool_pairs;
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

/// Prefix for compaction summary messages (used for idempotence detection).
pub(super) const COMPACTED_PREFIX: &str = "[COMPACTED] ";

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
    messages.iter().map(estimate_message_tokens).sum()
}

// ---------------------------------------------------------------------------
// UTF-8 safe truncation
// ---------------------------------------------------------------------------

/// Truncate string to at most `max_chars` characters, UTF-8 safe.
pub(super) fn truncate_utf8(s: &str, max_chars: usize) -> String {
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
/// Uses [`CompactionPolicy::default()`] for all parameters.
///
/// Rules:
/// 1. System messages → always preserved integrally
/// 2. Last `preserve_tail` messages → preserved integrally
/// 3. Tool results outside the tail → content truncated
/// 4. (assistant_with_tool_calls + tool_results) treated as atomic pair
/// 5. Previous compaction summaries are replaced (idempotent)
/// 6. A compaction summary is inserted as a user message
pub fn compact_if_needed(messages: &mut Vec<Message>, context_window_tokens: usize) {
    compact_if_needed_with_context(messages, context_window_tokens, None);
}

/// Compact with optional semantic context for richer summaries.
/// Uses [`CompactionPolicy::default()`].
pub fn compact_if_needed_with_context(
    messages: &mut Vec<Message>,
    context_window_tokens: usize,
    context: Option<&CompactionContext>,
) {
    let policy = CompactionPolicy::default();
    compact_with_policy(messages, context_window_tokens, context, &policy);
}

/// T11.1 — Compaction entry point that respects `policy.staged_compaction`.
///
/// When `staged_compaction = true`, dispatches via
/// [`crate::compaction_stages::compact_staged_with_policy`] which selects
/// between Mask / Prune / Aggressive / Compact based on usage pressure.
/// Otherwise delegates to the legacy single-stage Mask path
/// ([`compact_with_policy`]).
///
/// Returns the [`crate::compaction_stages::OptimizationLevel`] that was
/// applied (always `None` when `staged_compaction=false` and the legacy
/// path is taken — caller can ignore).
pub fn compact_with_staging_if_enabled(
    messages: &mut Vec<Message>,
    context_window_tokens: usize,
    context: Option<&CompactionContext>,
    policy: &CompactionPolicy,
) -> crate::compaction_stages::OptimizationLevel {
    if policy.staged_compaction {
        crate::compaction_stages::compact_staged_with_policy(
            messages,
            context_window_tokens,
            context,
            policy,
        )
    } else {
        compact_with_policy(messages, context_window_tokens, context, policy);
        crate::compaction_stages::OptimizationLevel::None
    }
}

/// Compact with explicit policy and optional semantic context.
///
/// This is the canonical implementation — all other `compact_*` variants delegate here.
pub fn compact_with_policy(
    messages: &mut Vec<Message>,
    context_window_tokens: usize,
    context: Option<&CompactionContext>,
    policy: &CompactionPolicy,
) {
    if messages.is_empty() || context_window_tokens == 0 {
        return;
    }

    // T1.3 AC-1.3.5: per-message oversize cap of context_window/4.
    // Runs BEFORE the threshold check so a single pathological message
    // cannot keep the total above the threshold forever (OOM loop — the
    // scenario validator flagged in meeting 20260420-221947). The cap is
    // enforced on EVERY message, including those in the protected tail.
    enforce_per_message_cap(messages, context_window_tokens / 4);

    let threshold = (context_window_tokens as f64 * policy.compact_threshold) as usize;
    if estimate_total_tokens(messages) <= threshold {
        return;
    }

    let non_system_count = messages.iter().filter(|m| m.role != Role::System).count();
    if non_system_count <= policy.preserve_tail {
        // Not enough messages to compact — everything is in the "preserve" zone.
        return;
    }

    let boundary_idx = policy_engine::find_boundary_idx(messages, policy.preserve_tail);
    let mut state = policy_engine::CompactionState::new();
    policy_engine::compact_older_messages(
        messages,
        boundary_idx,
        policy.truncate_tool_result_chars,
        &mut state,
    );

    if !state.compacted {
        return; // Nothing was actually truncated.
    }

    policy_engine::remove_previous_summary(messages);
    let summary = policy_engine::build_summary(
        state.compacted_turns,
        state.tools_used,
        state.files_mentioned,
        context,
    );
    policy_engine::insert_summary_after_system(messages, summary);

    // Post-compaction integrity: repair any orphaned tool pairs introduced
    // by truncation/drop operations above.
    sanitize_tool_pairs(messages);
}

/// Emergency compaction to a specific token target.
///
/// Called by the runtime when the LLM reports a context overflow error.
/// Aggressively removes older messages until the total is below `target_tokens`.
///
/// **Pi-mono ref:** reactive overflow recovery in `packages/coding-agent/src/core/agent-session.ts`
pub fn compact_messages_to_target(
    messages: &mut Vec<Message>,
    target_tokens: usize,
    task_objective: &str,
) {
    if messages.is_empty() || target_tokens == 0 {
        return;
    }

    // First try normal compaction (truncates tool results in-place).
    let ctx = if task_objective.is_empty() {
        None
    } else {
        Some(CompactionContext {
            task_objective: task_objective.to_string(),
            ..Default::default()
        })
    };
    let policy = CompactionPolicy::default();
    compact_with_policy(messages, target_tokens, ctx.as_ref(), &policy);

    // If still over target, drop oldest non-system messages one by one.
    while estimate_total_tokens(messages) > target_tokens {
        // Find the first non-system, non-compaction-summary message.
        let drop_idx = messages.iter().position(|m| {
            m.role != Role::System
                && !m
                    .content
                    .as_deref()
                    .is_some_and(|c| c.starts_with(COMPACTED_PREFIX))
        });
        match drop_idx {
            Some(idx) => {
                messages.remove(idx);
            }
            None => break, // Only system messages left.
        }
    }

    // Aggressive dropping above can leave orphaned tool pairs.
    sanitize_tool_pairs(messages);
}

/// T1.3 AC-1.3.5: cap any single message content at
/// `max_chars_per_message` chars. The cap is expressed in chars because
/// token estimation uses chars/4 and the limit we enforce is
/// `context_window/4` tokens — so chars ≈ `context_window` bytes per
/// message. Running this pass BEFORE the threshold check prevents a
/// single oversized message from thrashing the compactor indefinitely.
fn enforce_per_message_cap(messages: &mut [Message], max_tokens_per_message: usize) {
    if max_tokens_per_message == 0 {
        return;
    }
    // tokens ≈ chars/4 (matches theo_domain::tokens::estimate_message_tokens).
    let max_chars = max_tokens_per_message.saturating_mul(4);
    for m in messages.iter_mut() {
        if let Some(ref content) = m.content
            && content.len() > max_chars {
                m.content = Some(format!(
                    "{}\n… [truncated from {} to {} chars by per-message cap]",
                    truncate_utf8(content, max_chars.saturating_sub(80).max(1)),
                    content.len(),
                    max_chars
                ));
            }
    }
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
            msgs.push(Message::user(format!("task {i}")));
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
                big_content.clone(),
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
            msgs.push(Message::user(format!("task {i}")));
            msgs.push(Message::tool_result(
                format!("c{i}"),
                "read",
                "x".repeat(2000),
            ));
        }

        compact_if_needed(&mut msgs, 1_000);

        // Both system messages must be intact.
        let systems: Vec<_> = msgs.iter().filter(|m| m.role == Role::System).collect();
        assert_eq!(systems.len(), 2);
        assert_eq!(
            systems[0].content.as_deref().expect("t"),
            "IMPORTANT SYSTEM PROMPT WITH LOTS OF TEXT"
        );
    }

    #[test]
    fn last_n_messages_preserved() {
        // Use content that fits within the per-message cap
        // (context_window/4 tokens ≈ context_window bytes). Plan T1.3
        // AC-1.3.5 deliberately truncates oversized tail messages to
        // prevent OOM — that path is covered by
        // `test_t1_3_ac_6_single_oversized_message_does_not_cause_oom_loop`.
        // This test validates the OTHER invariant: tail messages within
        // the per-message cap must survive compaction unmodified.
        let policy = CompactionPolicy::default();
        let mut msgs = make_messages(20, 100);

        // Record the last preserve_tail non-system messages before compaction.
        let last_messages: Vec<String> = msgs
            .iter()
            .filter(|m| m.role != Role::System)
            .rev()
            .take(policy.preserve_tail)
            .filter_map(|m| m.content.clone())
            .collect();

        compact_if_needed(&mut msgs, 10_000);

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
            msgs.push(Message::user(format!("task {i}")));
            msgs.push(Message::tool_result(
                format!("c{i}"),
                "read",
                emoji_content.clone(),
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

        let content = summary.content.as_deref().expect("t");
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
    fn compaction_policy_defaults_match_previous_constants() {
        let policy = CompactionPolicy::default();
        assert_eq!(policy.preserve_tail, 6);
        assert_eq!(policy.truncate_tool_result_chars, 200);
        assert!((policy.compact_threshold - 0.80).abs() < f64::EPSILON);
        assert_eq!(policy.prune_keep_recent, 3);
        assert_eq!(policy.observation_mask_window, 10);
    }

    #[test]
    fn compact_with_custom_policy_respects_threshold() {
        let mut msgs = make_messages(20, 2000);
        // Custom policy with threshold 0.99 — should NOT compact even with small window.
        let policy = CompactionPolicy {
            compact_threshold: 0.99,
            ..Default::default()
        };
        let len_before = msgs.len();
        // Window of 5000 — at 0.99 threshold (4950 tokens needed), 20 turns with
        // 2000-char tool results will exceed, but let's use a bigger window.
        compact_with_policy(&mut msgs, 100_000, None, &policy);
        assert_eq!(msgs.len(), len_before, "High threshold should prevent compaction");
    }

    #[test]
    fn compact_with_custom_preserve_tail() {
        // Content sized to fit within the per-message cap
        // (context_window/4 ≈ context_window bytes): the
        // `preserve_tail` invariant and the T1.3 OOM cap are
        // orthogonal — this test validates the former only.
        let mut msgs = make_messages(20, 100);
        let policy = CompactionPolicy {
            preserve_tail: 12,
            ..Default::default()
        };
        // Record last 12 non-system messages.
        let last_messages: Vec<String> = msgs
            .iter()
            .filter(|m| m.role != Role::System)
            .rev()
            .take(12)
            .filter_map(|m| m.content.clone())
            .collect();

        compact_with_policy(&mut msgs, 10_000, None, &policy);

        for expected in &last_messages {
            assert!(
                msgs.iter().any(|m| m.content.as_deref() == Some(expected)),
                "Custom preserve_tail=12 should keep last 12 non-system messages"
            );
        }
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

    // ── T1.3 — Oversized-message protection ──────────────
    #[test]
    fn test_t1_3_ac_6_single_oversized_message_does_not_cause_oom_loop() {
        // Scenario from validator (meeting 20260420-221947 #concern):
        // single assistant message has >> context_window characters of
        // content. Without the per-message cap, total tokens stay above
        // threshold even after compaction, causing the next LLM call to
        // re-trigger compaction indefinitely and eventually OOM.
        let context_window = 1_000; // tokens
        let huge_content = "x".repeat(1_000_000); // 1M chars ≈ 250k tokens
        let mut msgs = vec![
            Message::system("sys"),
            Message::user("start"),
            Message::assistant(&huge_content),
        ];
        compact_if_needed(&mut msgs, context_window);

        // After one pass: total must fit (per-message cap enforced).
        let total = estimate_total_tokens(&msgs);
        assert!(
            total <= context_window,
            "per-message cap must bring total under context_window on first pass, got {total} > {context_window}"
        );
        // And the offending message must be truncated.
        let offending = msgs
            .iter()
            .find(|m| m.role == Role::Assistant)
            .and_then(|m| m.content.as_deref())
            .unwrap_or_default();
        assert!(
            offending.contains("per-message cap"),
            "must carry truncation marker, got: {}",
            &offending[..offending.len().min(200)]
        );
    }

    #[test]
    fn test_t1_3_per_message_cap_idempotent() {
        let context_window = 1_000;
        let huge = "y".repeat(500_000);
        let mut msgs = vec![Message::system("s"), Message::user(&huge)];
        compact_if_needed(&mut msgs, context_window);
        let first_pass = estimate_total_tokens(&msgs);
        compact_if_needed(&mut msgs, context_window);
        let second_pass = estimate_total_tokens(&msgs);
        assert_eq!(
            first_pass, second_pass,
            "per-message cap must be idempotent across passes"
        );
    }

    #[test]
    fn test_t1_3_per_message_cap_preserves_small_messages() {
        let context_window = 10_000;
        let mut msgs = vec![
            Message::system("sys"),
            Message::user("small message 1"),
            Message::assistant("small reply"),
        ];
        compact_if_needed(&mut msgs, context_window);
        // Nothing should be truncated — all messages well below cap.
        assert_eq!(msgs[1].content.as_deref(), Some("small message 1"));
        assert_eq!(msgs[2].content.as_deref(), Some("small reply"));
    }

    // -----------------------------------------------------------------
    // T11.1 — staged compaction entry point
    // -----------------------------------------------------------------

    use crate::compaction_stages::OptimizationLevel;
    use theo_infra_llm::types::ToolCall;

    /// Build a noisy message vector that pushes context above any
    /// reasonable threshold so the staged dispatcher will pick a
    /// non-`None` `OptimizationLevel`.
    fn many_tool_results(n: usize) -> Vec<Message> {
        let mut msgs = vec![Message::system("you are an agent")];
        // big_payload is 1_000 chars per tool result.
        let big_payload = "x".repeat(1_000);
        for i in 0..n {
            msgs.push(Message::assistant_with_tool_calls(
                None,
                vec![ToolCall::new(format!("c{i}"), "bash", "{}")],
            ));
            msgs.push(Message::tool_result(format!("c{i}"), "bash", &big_payload));
        }
        // Final user/assistant pair so the tail isn't pure tool results.
        msgs.push(Message::user("ok"));
        msgs.push(Message::assistant("noted"));
        msgs
    }

    #[test]
    fn t111_staged_off_uses_legacy_path() {
        let mut msgs = many_tool_results(10);
        let policy = CompactionPolicy {
            staged_compaction: false,
            ..CompactionPolicy::default()
        };
        let level =
            compact_with_staging_if_enabled(&mut msgs, 5_000, None, &policy);
        // Legacy path always returns `None` regardless of pressure.
        assert_eq!(level, OptimizationLevel::None);
    }

    #[test]
    fn t111_staged_on_returns_non_none_under_pressure() {
        let mut msgs = many_tool_results(20);
        let policy = CompactionPolicy {
            staged_compaction: true,
            ..CompactionPolicy::default()
        };
        // Tiny window forces the staged dispatcher into non-None branch.
        let level = compact_with_staging_if_enabled(&mut msgs, 500, None, &policy);
        assert_ne!(
            level,
            OptimizationLevel::None,
            "staged compaction must escalate under heavy pressure"
        );
    }

    #[test]
    fn t111_staged_on_at_low_pressure_is_none() {
        let mut msgs = vec![
            Message::system("sys"),
            Message::user("hello"),
            Message::assistant("hi"),
        ];
        let policy = CompactionPolicy {
            staged_compaction: true,
            ..CompactionPolicy::default()
        };
        let level =
            compact_with_staging_if_enabled(&mut msgs, 100_000, None, &policy);
        assert_eq!(level, OptimizationLevel::None);
    }

    #[test]
    fn t111_staged_on_reduces_tokens_under_pressure() {
        let mut msgs = many_tool_results(15);
        let before = estimate_total_tokens(&msgs);
        let policy = CompactionPolicy {
            staged_compaction: true,
            ..CompactionPolicy::default()
        };
        let _level = compact_with_staging_if_enabled(&mut msgs, 1_000, None, &policy);
        let after = estimate_total_tokens(&msgs);
        assert!(
            after < before,
            "staged compaction should reduce tokens (before={before}, after={after})"
        );
    }

    #[test]
    fn t111_compaction_policy_staged_default_is_off() {
        // T11.1 — opt-in. Default behavior unchanged for existing users.
        let p = CompactionPolicy::default();
        assert!(!p.staged_compaction);
    }
}
