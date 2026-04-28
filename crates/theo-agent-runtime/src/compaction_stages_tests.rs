//! Sibling test body of `compaction_stages.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `compaction_stages.rs` via `#[path = "compaction_stages_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.


#![cfg(test)]

    use super::*;
    use theo_infra_llm::types::ToolCall;

    fn user_of(filler_chars: usize) -> Vec<Message> {
        vec![
            Message::system("sys"),
            Message::user("x".repeat(filler_chars)),
        ]
    }

    #[test]
    fn level_none_when_window_zero_or_below_warning() {
        assert_eq!(check_usage(&user_of(4000), 0), OptimizationLevel::None);
        assert_eq!(check_usage(&user_of(2000), 1000), OptimizationLevel::None);
    }

    #[test]
    fn level_thresholds_classified_correctly() {
        // (filler_chars, expected_level) — 1000 token window.
        let cases = [
            (3000, OptimizationLevel::Warning),    // 70%
            (3280, OptimizationLevel::Mask),       // 80%
            (3480, OptimizationLevel::Prune),      // 85%
            (3680, OptimizationLevel::Aggressive), // 90%
            (4000, OptimizationLevel::Compact),    // 99%+
        ];
        for (chars, expected) in cases {
            let actual = check_usage(&user_of(chars), 1000);
            assert_eq!(actual, expected, "filler={chars}");
        }
    }

    #[test]
    fn levels_ordered_by_severity() {
        assert!(OptimizationLevel::None < OptimizationLevel::Warning);
        assert!(OptimizationLevel::Mask < OptimizationLevel::Prune);
        assert!(OptimizationLevel::Aggressive < OptimizationLevel::Compact);
    }

    fn build_four_tool_turns() -> Vec<Message> {
        let mut msgs = Vec::new();
        for i in 1..=4 {
            let id = format!("c{i}");
            msgs.push(Message::assistant_with_tool_calls(
                None,
                vec![ToolCall::new(id.clone(), "read", "{}")],
            ));
            msgs.push(Message::tool_result(id, "read", format!("content{i}")));
        }
        msgs
    }

    #[test]
    fn prune_replaces_old_content_preserves_recent() {
        let mut msgs = build_four_tool_turns();
        apply_prune(&mut msgs);
        assert_eq!(msgs[1].content.as_deref(), Some(PRUNED_SENTINEL));
        assert_eq!(msgs[3].content.as_deref(), Some("content2"));
        assert_eq!(msgs[7].content.as_deref(), Some("content4"));
        assert_eq!(msgs[1].tool_call_id.as_deref(), Some("c1"));
    }

    #[test]
    fn prune_noop_when_under_keep_recent() {
        let mut msgs = vec![
            Message::tool_result("c1", "r", "a"),
            Message::tool_result("c2", "r", "b"),
        ];
        let snap = msgs.clone();
        apply_prune(&mut msgs);
        assert_eq!(msgs, snap);
    }

    #[test]
    fn prune_is_idempotent() {
        let mut msgs = build_four_tool_turns();
        apply_prune(&mut msgs);
        let snap = msgs.clone();
        apply_prune(&mut msgs);
        assert_eq!(msgs, snap);
    }

    #[test]
    fn aggressive_keeps_only_one_recent_tool_result() {
        let mut msgs = build_four_tool_turns();
        apply_aggressive(&mut msgs);
        assert_eq!(msgs[1].content.as_deref(), Some(PRUNED_SENTINEL));
        assert_eq!(msgs[3].content.as_deref(), Some(PRUNED_SENTINEL));
        assert_eq!(msgs[5].content.as_deref(), Some(PRUNED_SENTINEL));
        assert_eq!(msgs[7].content.as_deref(), Some("content4"));
    }

    #[test]
    fn compact_staged_returns_none_below_warning_threshold() {
        let mut msgs = user_of(100);
        let level = compact_staged(&mut msgs, 10_000, None);
        assert_eq!(level, OptimizationLevel::None);
    }

    #[test]
    fn compact_staged_returns_level_applied() {
        let mut msgs = user_of(3500);
        let level = compact_staged(&mut msgs, 1000, None);
        assert_eq!(level, OptimizationLevel::Prune);
    }

    #[test]
    fn compact_staged_applies_aggressive_at_95_percent() {
        let mut msgs = build_four_tool_turns();
        // Force at least Aggressive level by using tiny window.
        let level = compact_staged(&mut msgs, 20, None);
        assert!(level >= OptimizationLevel::Aggressive);
        // At least 3 tool results pruned (keeping 1).
        let pruned = msgs
            .iter()
            .filter(|m| m.content.as_deref() == Some(PRUNED_SENTINEL))
            .count();
        assert!(pruned >= 3, "expected >=3 pruned, got {pruned}");
    }

    #[test]
    fn mask_sentinel_format_is_canonical() {
        let s = mask_sentinel("call_42");
        assert!(s.starts_with(MASK_SENTINEL_PREFIX));
        assert!(s.contains("call_42"));
        assert!(s.ends_with("— see history]"));
    }

    #[test]
    fn is_already_masked_detects_sentinel() {
        let s = mask_sentinel("c1");
        assert!(is_already_masked(Some(&s)));
        assert!(!is_already_masked(Some("normal content")));
        assert!(!is_already_masked(None));
    }

    #[test]
    fn protected_names_covered() {
        assert!(is_protected(Some("read")));
        assert!(is_protected(Some("skill")));
        assert!(!is_protected(Some("bash")));
        assert!(!is_protected(None));
    }

    // -----------------------------------------------------------------------
    // Observation masking tests
    // -----------------------------------------------------------------------

    fn build_tool_turns(count: usize, tool_name: &str) -> Vec<Message> {
        let mut msgs = Vec::new();
        for i in 1..=count {
            let id = format!("c{i}");
            msgs.push(Message::assistant_with_tool_calls(
                None,
                vec![ToolCall::new(id.clone(), tool_name, "{}")],
            ));
            msgs.push(Message::tool_result(id, tool_name, format!("output{i}")));
        }
        msgs
    }

    #[test]
    fn observation_mask_preserves_last_m_observations() {
        // 8 tool turns, window=3 → first 5 masked, last 3 preserved.
        let mut msgs = build_tool_turns(8, "bash");
        apply_observation_mask(&mut msgs, 3);

        let tools: Vec<_> = msgs
            .iter()
            .filter(|m| m.role == Role::Tool)
            .collect();
        assert_eq!(tools.len(), 8);

        // First 5 should be masked.
        for t in &tools[..5] {
            assert!(
                t.content.as_deref().unwrap().starts_with(OBSERVATION_MASK_PREFIX),
                "Expected masked, got: {:?}",
                t.content
            );
        }
        // Last 3 should be preserved.
        assert_eq!(tools[5].content.as_deref(), Some("output6"));
        assert_eq!(tools[6].content.as_deref(), Some("output7"));
        assert_eq!(tools[7].content.as_deref(), Some("output8"));
    }

    #[test]
    fn observation_mask_replaces_old_observations_with_header() {
        let mut msgs = build_tool_turns(4, "bash");
        apply_observation_mask(&mut msgs, 2);

        let first_tool = msgs.iter().find(|m| m.role == Role::Tool).unwrap();
        let content = first_tool.content.as_deref().unwrap();
        assert!(content.starts_with("[observation masked: bash c1]"));
    }

    #[test]
    fn observation_mask_preserves_non_tool_messages() {
        let mut msgs = vec![
            Message::system("sys"),
            Message::user("hello"),
        ];
        msgs.extend(build_tool_turns(5, "bash"));
        msgs.push(Message::assistant("thinking..."));

        let len_before = msgs.len();
        apply_observation_mask(&mut msgs, 3);

        // Message count unchanged (masking replaces content, doesn't remove).
        assert_eq!(msgs.len(), len_before);
        // System and user messages untouched.
        assert_eq!(msgs[0].content.as_deref(), Some("sys"));
        assert_eq!(msgs[1].content.as_deref(), Some("hello"));
        // Last assistant message untouched.
        assert_eq!(
            msgs.last().unwrap().content.as_deref(),
            Some("thinking...")
        );
    }

    #[test]
    fn observation_mask_skips_protected_tools() {
        let mut msgs = Vec::new();
        // 3 read_file results (protected) + 3 bash results.
        for i in 1..=3 {
            let id = format!("rf{i}");
            msgs.push(Message::assistant_with_tool_calls(
                None,
                vec![ToolCall::new(id.clone(), "read", "{}")],
            ));
            msgs.push(Message::tool_result(&id, "read", format!("file_content{i}")));
        }
        for i in 1..=3 {
            let id = format!("b{i}");
            msgs.push(Message::assistant_with_tool_calls(
                None,
                vec![ToolCall::new(id.clone(), "bash", "{}")],
            ));
            msgs.push(Message::tool_result(&id, "bash", format!("bash_out{i}")));
        }

        // Window=1 → mask all but last 1 observation.
        apply_observation_mask(&mut msgs, 1);

        // read_file results should be preserved (protected).
        let read_file_tools: Vec<_> = msgs
            .iter()
            .filter(|m| m.role == Role::Tool && m.name.as_deref() == Some("read"))
            .collect();
        for t in &read_file_tools {
            assert!(
                !t.content.as_deref().unwrap().starts_with(OBSERVATION_MASK_PREFIX),
                "Protected tool should not be masked"
            );
        }

        // First 2 bash results masked, last 1 preserved.
        let bash_tools: Vec<_> = msgs
            .iter()
            .filter(|m| m.role == Role::Tool && m.name.as_deref() == Some("bash"))
            .collect();
        assert!(bash_tools[0].content.as_deref().unwrap().starts_with(OBSERVATION_MASK_PREFIX));
        assert!(bash_tools[1].content.as_deref().unwrap().starts_with(OBSERVATION_MASK_PREFIX));
        assert_eq!(bash_tools[2].content.as_deref(), Some("bash_out3"));
    }

    #[test]
    fn observation_mask_noop_when_under_window() {
        let mut msgs = build_tool_turns(3, "bash");
        let snap = msgs.clone();
        apply_observation_mask(&mut msgs, 5); // Window bigger than tool count.
        assert_eq!(msgs, snap);
    }

    #[test]
    fn observation_mask_skips_already_masked() {
        let mut msgs = build_tool_turns(5, "bash");
        apply_observation_mask(&mut msgs, 2);
        let snap = msgs.clone();
        apply_observation_mask(&mut msgs, 2);
        assert_eq!(msgs, snap, "Double masking should be idempotent");
    }

    #[test]
    fn observation_mask_with_policy_uses_window() {
        let mut msgs = build_tool_turns(6, "bash");
        let policy = CompactionPolicy {
            observation_mask_window: 2,
            ..Default::default()
        };
        apply_observation_mask_with_policy(&mut msgs, &policy);

        let masked_count = msgs
            .iter()
            .filter(|m| {
                m.role == Role::Tool
                    && m.content
                        .as_deref()
                        .is_some_and(|c| c.starts_with(OBSERVATION_MASK_PREFIX))
            })
            .count();
        assert_eq!(masked_count, 4, "6 tools - 2 window = 4 masked");
    }

    // ── T11.1: Compact stage end-to-end with summary injection ─────

    fn pressure_msgs() -> Vec<Message> {
        // Construct a transcript heavy enough to trigger Compact at
        // the 99% threshold. 4000 chars + tool turns.
        let mut msgs = vec![
            Message::system("you are theo"),
            Message::user("Fix the login redirect bug in auth.rs"),
        ];
        for i in 1..=6 {
            let id = format!("c{i}");
            msgs.push(Message::assistant_with_tool_calls(
                None,
                vec![ToolCall::new(id.clone(), "read", "{}")],
            ));
            msgs.push(Message::tool_result(
                id,
                "read",
                format!("file contents iteration {i} ").repeat(40),
            ));
        }
        msgs.push(Message::assistant("Found root cause on line 42"));
        msgs
    }

    #[test]
    fn t111_compact_branch_returns_compact_level() {
        let mut msgs = pressure_msgs();
        let policy = CompactionPolicy {
            staged_compaction: true,
            ..Default::default()
        };
        // Tiny window forces Compact threshold (99%).
        let level = compact_staged_with_policy(&mut msgs, 100, None, &policy);
        assert_eq!(level, OptimizationLevel::Compact);
    }

    #[test]
    fn t111_compact_injects_summary_marked_with_prefix() {
        let mut msgs = pressure_msgs();
        let policy = CompactionPolicy {
            staged_compaction: true,
            ..Default::default()
        };
        let _ = compact_staged_with_policy(&mut msgs, 100, None, &policy);
        // Exactly one message should carry SUMMARY_PREFIX, and it
        // must be the first non-system message (so the model sees
        // the background BEFORE any subsequent turn).
        let prefix_count = msgs
            .iter()
            .filter(|m| {
                m.content
                    .as_deref()
                    .is_some_and(|c| c.contains(SUMMARY_PREFIX))
            })
            .count();
        assert_eq!(prefix_count, 1, "expected exactly one summary message");
        let first_non_system = msgs.iter().find(|m| m.role != Role::System).unwrap();
        assert!(
            first_non_system
                .content
                .as_deref()
                .unwrap_or("")
                .contains(SUMMARY_PREFIX),
            "summary must be the FIRST non-system message"
        );
    }

    #[test]
    fn t111_compact_summary_includes_active_task_from_user() {
        let mut msgs = pressure_msgs();
        let policy = CompactionPolicy {
            staged_compaction: true,
            ..Default::default()
        };
        let _ = compact_staged_with_policy(&mut msgs, 100, None, &policy);
        let summary = msgs
            .iter()
            .find(|m| {
                m.content
                    .as_deref()
                    .is_some_and(|c| c.contains(SUMMARY_PREFIX))
            })
            .expect("summary present");
        let body = summary.content.as_deref().unwrap();
        // The user's original task must survive into the summary so
        // the model still knows what it's working on.
        assert!(
            body.contains("Fix the login redirect bug"),
            "summary must preserve the user's verbatim task"
        );
    }

    #[test]
    fn t111_compact_idempotent_does_not_stack_summaries() {
        let mut msgs = pressure_msgs();
        let policy = CompactionPolicy {
            staged_compaction: true,
            ..Default::default()
        };
        // Run Compact twice — second run must replace, not append.
        let _ = compact_staged_with_policy(&mut msgs, 100, None, &policy);
        let after_first = msgs.len();
        // Pad the transcript again to keep us at Compact pressure.
        msgs.push(Message::user("more pressure ".repeat(800)));
        let _ = compact_staged_with_policy(&mut msgs, 100, None, &policy);
        let prefix_count = msgs
            .iter()
            .filter(|m| {
                m.content
                    .as_deref()
                    .is_some_and(|c| c.contains(SUMMARY_PREFIX))
            })
            .count();
        assert_eq!(
            prefix_count, 1,
            "duplicate Compact runs must replace, not stack summaries"
        );
        // A second compaction may add or remove other messages, so
        // we just assert the summary count didn't bloat.
        let _ = after_first;
    }

    #[test]
    fn t111_compact_preserves_system_message_at_head() {
        let mut msgs = pressure_msgs();
        let policy = CompactionPolicy {
            staged_compaction: true,
            ..Default::default()
        };
        let _ = compact_staged_with_policy(&mut msgs, 100, None, &policy);
        // System message must remain at index 0 with original content.
        assert_eq!(msgs[0].role, Role::System);
        assert_eq!(msgs[0].content.as_deref(), Some("you are theo"));
    }

    #[test]
    fn t111_compact_no_op_when_pressure_below_compact_threshold() {
        let mut msgs = pressure_msgs();
        let original_len = msgs.len();
        let policy = CompactionPolicy {
            staged_compaction: true,
            ..Default::default()
        };
        // Huge window keeps us at None — Compact branch should NOT
        // run, and no summary should be injected.
        let level = compact_staged_with_policy(&mut msgs, 1_000_000, None, &policy);
        assert_eq!(level, OptimizationLevel::None);
        // No SUMMARY_PREFIX in any message.
        assert!(!msgs.iter().any(|m| m
            .content
            .as_deref()
            .is_some_and(|c| c.contains(SUMMARY_PREFIX))));
        // Message count is unchanged (None branch is a no-op).
        assert_eq!(msgs.len(), original_len);
    }

    #[test]
    fn t111_drop_previous_compact_summaries_only_drops_user_summaries() {
        let mut msgs = vec![
            Message::system("sys"),
            Message::user(format!("{SUMMARY_PREFIX}\n\n## Active Task\nold")),
            Message::user("real user message"),
            Message::assistant("ok"),
        ];
        drop_previous_compact_summaries(&mut msgs);
        // Only the SUMMARY_PREFIX-marked user message is removed.
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].role, Role::System);
        assert_eq!(msgs[1].content.as_deref(), Some("real user message"));
        assert_eq!(msgs[2].role, Role::Assistant);
    }

    #[test]
    fn t111_drop_previous_compact_summaries_never_touches_assistant_or_tool() {
        // Even if an assistant/tool message accidentally starts with
        // SUMMARY_PREFIX (improbable but possible), we never drop it
        // — the predicate requires Role::User.
        let mut msgs = vec![
            Message::system("sys"),
            Message::assistant(format!("{SUMMARY_PREFIX} not a real summary")),
            Message::user("normal user msg"),
        ];
        let len_before = msgs.len();
        drop_previous_compact_summaries(&mut msgs);
        assert_eq!(msgs.len(), len_before, "assistant must not be removed");
    }

    #[test]
    fn t111_insert_compact_summary_after_system_handles_no_system() {
        let mut msgs = vec![Message::user("first user msg")];
        insert_compact_summary_after_system(&mut msgs, "BG\nbody".to_string());
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].content.as_deref(), Some("BG\nbody"));
        assert_eq!(msgs[1].content.as_deref(), Some("first user msg"));
    }

    #[test]
    fn t111_insert_compact_summary_after_system_handles_multiple_systems() {
        // Two system messages at the head — summary goes at index 2.
        let mut msgs = vec![
            Message::system("sys1"),
            Message::system("sys2"),
            Message::user("u"),
        ];
        insert_compact_summary_after_system(&mut msgs, "S".to_string());
        assert_eq!(msgs[0].role, Role::System);
        assert_eq!(msgs[1].role, Role::System);
        assert_eq!(msgs[2].content.as_deref(), Some("S"));
        assert_eq!(msgs[3].content.as_deref(), Some("u"));
    }
