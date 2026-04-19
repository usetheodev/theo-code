//! Multi-stage compaction — classifier + Prune stage.
//!
//! Port of opendev `OptimizationLevel`:
//! `referencias/opendev/crates/opendev-context/src/compaction/levels.rs:6-18`
//!
//! Thresholds: 0/70/80/85/90/99% → None/Warning/Mask/Prune/Aggressive/Compact.
//! Mask already lives in `compaction::compact_if_needed`.
//! Warning/Aggressive/Compact are future iterations.

use crate::compaction::{CompactionContext, compact_if_needed_with_context, estimate_total_tokens};
use crate::sanitizer::sanitize_tool_pairs;
use theo_infra_llm::types::{Message, Role};

/// Sentinel substituted for the content of a pruned tool message.
pub const PRUNED_SENTINEL: &str = "[pruned]";

/// Prefix of the Mask-stage sentinel that preserves tool_call_id for audit.
/// Format: `"[ref: tool result {id} — see history]"`.
pub const MASK_SENTINEL_PREFIX: &str = "[ref: tool result ";

/// Tool categories that must NEVER be masked/pruned (opendev
/// `PROTECTED_TOOL_TYPES`). These results carry irreducible signal.
pub const PROTECTED_TOOL_NAMES: &[&str] =
    &["read_file", "graph_context", "skill", "invoke_skill", "present_plan"];

/// Build the canonical Mask sentinel for a tool result.
pub fn mask_sentinel(tool_call_id: &str) -> String {
    format!("{MASK_SENTINEL_PREFIX}{tool_call_id} — see history]")
}

/// Return true when the tool message is protected from masking/pruning.
pub fn is_protected(name: Option<&str>) -> bool {
    matches!(name, Some(n) if PROTECTED_TOOL_NAMES.contains(&n))
}

/// Return true when `content` already carries a Mask sentinel (idempotence check).
pub fn is_already_masked(content: Option<&str>) -> bool {
    matches!(content, Some(c) if c.starts_with(MASK_SENTINEL_PREFIX))
}

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

/// Warning stage: log (once) that context pressure is approaching threshold.
///
/// Non-mutating. Caller is expected to dedupe per-session via the
/// `warned_70` style flag (see opendev `ContextCompactor`). Here we just emit
/// a structured log line; dedupe lives in the caller to keep this pure.
pub fn apply_warning(used: usize, limit: usize) {
    let ratio = (used as f64 * 100.0 / limit as f64) as u32;
    eprintln!(
        "context_pressure: stage=warning ratio={ratio}% used={used} limit={limit}"
    );
}

/// Prune stage: replace content of old tool results with `[pruned]`,
/// preserving the last `PRUNE_KEEP_RECENT=3` results integrally.
pub fn apply_prune(messages: &mut Vec<Message>) {
    apply_prune_with_keep(messages, PRUNE_KEEP_RECENT);
}

/// Aggressive stage: same as Prune but keeps only 1 recent tool result.
pub fn apply_aggressive(messages: &mut Vec<Message>) {
    apply_prune_with_keep(messages, 1);
}

/// Internal: parametrized prune.
fn apply_prune_with_keep(messages: &mut Vec<Message>, keep: usize) {
    if messages.is_empty() {
        return;
    }

    let mut tool_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == Role::Tool)
        .map(|(i, _)| i)
        .collect();

    if tool_indices.len() <= keep {
        return;
    }

    let cutoff = tool_indices.len() - keep;
    tool_indices.truncate(cutoff);

    for idx in tool_indices {
        if messages[idx].content.as_deref() == Some(PRUNED_SENTINEL) {
            continue;
        }
        messages[idx].content = Some(PRUNED_SENTINEL.to_string());
    }

    sanitize_tool_pairs(messages);
}

/// Staged dispatcher — the single entry point the agent loop should use.
///
/// Checks pressure via `check_usage`, then invokes the appropriate stage:
/// - None    → no-op
/// - Warning → log only
/// - Mask    → `compact_if_needed_with_context` (existing truncation logic)
/// - Prune   → `apply_prune` (keep=3)
/// - Aggressive → `apply_aggressive` (keep=1)
/// - Compact → `apply_aggressive` + Mask pass (LLM summary deferred)
///
/// Returns the level that was applied so callers can observe/metric it.
pub fn compact_staged(
    messages: &mut Vec<Message>,
    context_window_tokens: usize,
    ctx: Option<&CompactionContext>,
) -> OptimizationLevel {
    let level = check_usage(messages, context_window_tokens);
    match level {
        OptimizationLevel::None => {}
        OptimizationLevel::Warning => {
            apply_warning(estimate_total_tokens(messages), context_window_tokens);
        }
        OptimizationLevel::Mask => {
            compact_if_needed_with_context(messages, context_window_tokens, ctx);
        }
        OptimizationLevel::Prune => {
            apply_prune(messages);
        }
        OptimizationLevel::Aggressive => {
            apply_aggressive(messages);
        }
        OptimizationLevel::Compact => {
            // LLM summarization is a future iteration. For now fall back to
            // aggressive pruning + masking on whatever survives.
            apply_aggressive(messages);
            compact_if_needed_with_context(messages, context_window_tokens, ctx);
        }
    }
    level
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
        assert!(is_protected(Some("read_file")));
        assert!(is_protected(Some("skill")));
        assert!(!is_protected(Some("bash")));
        assert!(!is_protected(None));
    }
}
