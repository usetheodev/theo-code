//! Multi-stage compaction — classifier + Prune stage.
//!
//! Port of opendev `OptimizationLevel`:
//! `referencias/opendev/crates/opendev-context/src/compaction/levels.rs:6-18`
//!
//! Thresholds: 0/70/80/85/90/99% → None/Warning/Mask/Prune/Aggressive/Compact.
//! Mask already lives in `compaction::compact_if_needed`.
//! Warning/Aggressive/Compact are future iterations.

use crate::compaction::estimate_total_tokens;
use crate::sanitizer::sanitize_tool_pairs;
use theo_infra_llm::types::{Message, Role};

/// Sentinel substituted for the content of a pruned tool message.
pub const PRUNED_SENTINEL: &str = "[pruned]";

/// How many recent tool results to preserve integrally during Prune.
const PRUNE_KEEP_RECENT: usize = 3;

/// Staged optimization level, determined by context-window occupancy.
///
/// Ordered from least to most destructive — useful for severity comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OptimizationLevel {
    None,
    Warning,
    Mask,
    Prune,
    Aggressive,
    Compact,
}

impl OptimizationLevel {
    /// Human-readable label for logs/metrics.
    pub fn label(self) -> &'static str {
        match self {
            OptimizationLevel::None => "none",
            OptimizationLevel::Warning => "warning",
            OptimizationLevel::Mask => "mask",
            OptimizationLevel::Prune => "prune",
            OptimizationLevel::Aggressive => "aggressive",
            OptimizationLevel::Compact => "compact",
        }
    }
}

/// Classify the current context pressure.
///
/// Thresholds follow opendev: 0 → None, 70% → Warning, 80% → Mask,
/// 85% → Prune, 90% → Aggressive, 99% → Compact.
pub fn check_usage(messages: &[Message], context_window_tokens: usize) -> OptimizationLevel {
    if context_window_tokens == 0 || messages.is_empty() {
        return OptimizationLevel::None;
    }
    let used = estimate_total_tokens(messages);
    let ratio = used as f64 / context_window_tokens as f64;
    if ratio >= 0.99 {
        OptimizationLevel::Compact
    } else if ratio >= 0.90 {
        OptimizationLevel::Aggressive
    } else if ratio >= 0.85 {
        OptimizationLevel::Prune
    } else if ratio >= 0.80 {
        OptimizationLevel::Mask
    } else if ratio >= 0.70 {
        OptimizationLevel::Warning
    } else {
        OptimizationLevel::None
    }
}

/// Prune stage: replace content of old tool results with `[pruned]`.
///
/// Differs from Mask: content is fully replaced (no character preservation).
/// `tool_call_id` kept so pairs remain intact. Most recent
/// `PRUNE_KEEP_RECENT` tool results are untouched. Idempotent.
/// Calls `sanitize_tool_pairs` for post-mutation integrity.
pub fn apply_prune(messages: &mut Vec<Message>) {
    if messages.is_empty() {
        return;
    }

    let mut tool_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == Role::Tool)
        .map(|(i, _)| i)
        .collect();

    if tool_indices.len() <= PRUNE_KEEP_RECENT {
        return;
    }

    let cutoff = tool_indices.len() - PRUNE_KEEP_RECENT;
    tool_indices.truncate(cutoff);

    for idx in tool_indices {
        if messages[idx].content.as_deref() == Some(PRUNED_SENTINEL) {
            continue;
        }
        messages[idx].content = Some(PRUNED_SENTINEL.to_string());
    }

    sanitize_tool_pairs(messages);
}

#[cfg(test)]
mod tests {
    use super::*;
    use theo_infra_llm::types::ToolCall;

    fn user_of(filler_chars: usize) -> Vec<Message> {
        vec![
            Message::system("sys"),
            Message::user(&"x".repeat(filler_chars)),
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
}
