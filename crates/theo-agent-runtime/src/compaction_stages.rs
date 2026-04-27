//! Multi-stage compaction — classifier + Prune stage.
//!
//! Port of opendev `OptimizationLevel`:
//! `referencias/opendev/crates/opendev-context/src/compaction/levels.rs:6-18`
//!
//! Thresholds: 0/70/80/85/90/99% → None/Warning/Mask/Prune/Aggressive/Compact.
//! Mask already lives in `compaction::compact_if_needed`.
//! Warning/Aggressive/Compact are future iterations.
//!
//! T11.1 — Compaction stages now wired into `compact_with_staging_if_enabled`.
//! The Compact branch additionally injects a structured `fallback_summary`
//! (deterministic, no LLM round-trip) so the model sees background context
//! instead of an abrupt truncation.

use crate::compaction::{CompactionContext, compact_with_policy, estimate_total_tokens};
use crate::compaction_summary::{SUMMARY_PREFIX, fallback_summary};
use crate::config::CompactionPolicy;
use crate::tool_pair_integrity::sanitize_tool_pairs;
use theo_infra_llm::types::{Message, Role};

/// Sentinel substituted for the content of a pruned tool message.
pub const PRUNED_SENTINEL: &str = "[pruned]";

/// Prefix of the Mask-stage sentinel that preserves tool_call_id for audit.
/// Format: `"[ref: tool result {id} — see history]"`.
///
/// Public API — consumed by audit/observability integrations that
/// inspect compacted transcripts; internal callers go through
/// [`mask_sentinel`].
#[allow(dead_code)]
pub const MASK_SENTINEL_PREFIX: &str = "[ref: tool result ";

/// Tool categories that must NEVER be masked/pruned (opendev
/// `PROTECTED_TOOL_TYPES`). These results carry irreducible signal.
///
/// Bug 2026-04-27 (dogfood): the list referenced `read_file`, a name
/// that has not existed in the production registry since at least the
/// snapshot-pin contract test (`default_registry_tool_id_snapshot_is_pinned`).
/// As a result file-read tool results were not protected from compaction
/// despite being one of the most expensive things to re-fetch. Now
/// also includes `read` (production ID) plus `lsp_definition` and
/// `lsp_references` whose source-graph anchors are equally expensive
/// to recompute.
pub const PROTECTED_TOOL_NAMES: &[&str] = &[
    "read",
    // 2026-04-27: was `graph_context` — name not registered. The
    // production tool exposing the structural code map is registered
    // as `codebase_context` (theo-tooling tool_manifest.rs:48).
    "codebase_context",
    // `skill` is a MetaTool injected by tool_bridge (not in
    // create_default_registry); `invoke_skill` / `present_plan` from
    // the original list never existed in production and were dropped.
    "skill",
    "lsp_definition",
    "lsp_references",
    // 2026-04-27: also protect plan-state tools — losing their results
    // forces the agent to rebuild plan reasoning from scratch which
    // wastes far more tokens than keeping the message intact.
    "plan_create",
    "plan_summary",
    "plan_next_task",
];

/// Build the canonical Mask sentinel for a tool result.
///
/// Public API — used by `theo-application` integrations (or by
/// downstream tooling building new mask sentinels for offline
/// rewrites). Currently unused inside `theo-agent-runtime` itself
/// (the compaction stages emit sentinels via inline `format!`).
#[allow(dead_code)]
pub fn mask_sentinel(tool_call_id: &str) -> String {
    format!("{MASK_SENTINEL_PREFIX}{tool_call_id} — see history]")
}

/// Return true when the tool message is protected from masking/pruning.
pub fn is_protected(name: Option<&str>) -> bool {
    matches!(name, Some(n) if PROTECTED_TOOL_NAMES.contains(&n))
}

/// Return true when `content` already carries a Mask sentinel (idempotence check).
///
/// Public API — consumed by external integrations that need to skip
/// re-masking. Internal compaction stages currently dedupe via
/// observation-mask checks; left available for round-trip tooling.
#[allow(dead_code)]
pub fn is_already_masked(content: Option<&str>) -> bool {
    matches!(content, Some(c) if c.starts_with(MASK_SENTINEL_PREFIX))
}

/// Legacy default — callers should prefer `CompactionPolicy::prune_keep_recent`.
const PRUNE_KEEP_RECENT_DEFAULT: usize = 3;

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
    tracing::warn!(
        stage = "warning",
        ratio = ratio,
        used = used,
        limit = limit,
        "context pressure"
    );
}

/// Prune stage: replace content of old tool results with `[pruned]`,
/// preserving the last `prune_keep_recent` (default 3) results integrally.
///
/// Convenience entry point that uses the default keep count; callers
/// with explicit policy should prefer [`apply_prune_with_policy`].
#[allow(dead_code)] // public API for symmetry with apply_prune_with_policy
pub fn apply_prune(messages: &mut Vec<Message>) {
    apply_prune_with_keep(messages, PRUNE_KEEP_RECENT_DEFAULT);
}

/// Prune stage with explicit keep count from policy.
pub fn apply_prune_with_policy(messages: &mut Vec<Message>, policy: &CompactionPolicy) {
    apply_prune_with_keep(messages, policy.prune_keep_recent);
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
/// Staged dispatcher — the single entry point the agent loop should use.
/// Uses [`CompactionPolicy::default()`].
#[allow(dead_code)] // public API for symmetry with compact_staged_with_policy
pub fn compact_staged(
    messages: &mut Vec<Message>,
    context_window_tokens: usize,
    ctx: Option<&CompactionContext>,
) -> OptimizationLevel {
    let policy = CompactionPolicy::default();
    compact_staged_with_policy(messages, context_window_tokens, ctx, &policy)
}

/// Staged dispatcher with explicit policy.
///
/// Checks pressure via `check_usage`, then invokes the appropriate stage:
/// - None    → no-op
/// - Warning → log only
/// - Mask    → `compact_with_policy` (existing truncation logic)
/// - Prune   → `apply_prune_with_policy`
/// - Aggressive → `apply_aggressive` (keep=1)
/// - Compact → `apply_aggressive` + Mask pass (LLM summary deferred)
///
/// Returns the level that was applied so callers can observe/metric it.
pub fn compact_staged_with_policy(
    messages: &mut Vec<Message>,
    context_window_tokens: usize,
    ctx: Option<&CompactionContext>,
    policy: &CompactionPolicy,
) -> OptimizationLevel {
    let level = check_usage(messages, context_window_tokens);
    match level {
        OptimizationLevel::None => {}
        OptimizationLevel::Warning => {
            apply_warning(estimate_total_tokens(messages), context_window_tokens);
            apply_observation_mask_with_policy(messages, policy);
        }
        OptimizationLevel::Mask => {
            compact_with_policy(messages, context_window_tokens, ctx, policy);
        }
        OptimizationLevel::Prune => {
            apply_prune_with_policy(messages, policy);
        }
        OptimizationLevel::Aggressive => {
            apply_aggressive(messages);
        }
        OptimizationLevel::Compact => {
            // T11.1 — At ≥99% pressure we run the deterministic
            // `fallback_summary` path before any destructive
            // operation. The summary captures the original transcript
            // (Active Task, Goal, Completed Actions, etc.) so the
            // model still has continuity after the aggressive pass
            // wipes the middle.
            apply_compact_with_summary(messages, context_window_tokens, ctx, policy);
        }
    }
    level
}

/// T11.1 — Apply the Compact stage:
///   1. Capture the message history into a `fallback_summary` string
///      (deterministic, side-effect free; works offline).
///   2. Aggressively prune tool observations (keep=1).
///   3. Run the Mask pass on what remains.
///   4. Inject the summary as a system-tagged background user
///      message immediately after the system prompt, so the model
///      treats it as background context (per `SUMMARY_PREFIX`).
///
/// Idempotent in the strict sense: if a summary is already injected
/// at the head, the new one replaces it instead of stacking.
fn apply_compact_with_summary(
    messages: &mut Vec<Message>,
    context_window_tokens: usize,
    ctx: Option<&CompactionContext>,
    policy: &CompactionPolicy,
) {
    // Snapshot BEFORE mutating — the summary must reflect the original
    // transcript, not the post-aggressive shadow.
    let summary_text = fallback_summary(messages);

    apply_aggressive(messages);
    compact_with_policy(messages, context_window_tokens, ctx, policy);

    // Drop any previous summary the policy_engine inserted, then
    // place ours right after the system prompt.
    drop_previous_compact_summaries(messages);
    insert_compact_summary_after_system(messages, summary_text);
}

/// Remove any existing Compact-stage summary message. We identify
/// summaries by the `SUMMARY_PREFIX` marker — the prefix is stable
/// public API of `compaction_summary`, used by the model to know
/// the message is background.
fn drop_previous_compact_summaries(messages: &mut Vec<Message>) {
    messages.retain(|m| {
        // Only user-role messages can be our injected summaries;
        // never drop tool/assistant/system content.
        if m.role != Role::User {
            return true;
        }
        match m.content.as_deref() {
            Some(c) => !c.starts_with(SUMMARY_PREFIX),
            None => true,
        }
    });
}

/// Insert the Compact summary immediately after the leading system
/// message(s). When no system prompt exists, prepend at index 0.
fn insert_compact_summary_after_system(messages: &mut Vec<Message>, summary: String) {
    let insert_at = messages
        .iter()
        .position(|m| m.role != Role::System)
        .unwrap_or(messages.len());
    let summary_msg = Message::user(&summary);
    messages.insert(insert_at, summary_msg);
}

// ---------------------------------------------------------------------------
// Observation Masking (Fase 1)
// ---------------------------------------------------------------------------

/// Prefix for observation mask headers.
const OBSERVATION_MASK_PREFIX: &str = "[observation masked: ";

/// Build the observation mask header for a tool result.
fn observation_mask_header(tool_name: Option<&str>, tool_call_id: &str) -> String {
    let name = tool_name.unwrap_or("unknown");
    format!("{OBSERVATION_MASK_PREFIX}{name} {tool_call_id}]")
}

/// Check if a message is already observation-masked.
fn is_observation_masked(content: Option<&str>) -> bool {
    matches!(content, Some(c) if c.starts_with(OBSERVATION_MASK_PREFIX))
}

/// Apply observation masking to tool results outside the recent window.
///
/// Replaces the content of older tool observations with a compact header
/// (`[observation masked: <tool_name> <tool_call_id>]`) while preserving
/// the last `window` observations intact. Protected tools (read_file,
/// graph_context, etc.) are never masked.
///
/// **NOT idempotent** by design — already-masked messages are skipped but
/// the window is counted from the current state. Call BEFORE `compact_if_needed`
/// in the staged pipeline (Warning level).
///
/// Ref: Complexity Trap paper — 84% of tokens are observations.
pub fn apply_observation_mask(messages: &mut [Message], window: usize) {
    // Collect indices of all Tool messages (observations).
    let tool_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == Role::Tool)
        .map(|(i, _)| i)
        .collect();

    if tool_indices.len() <= window {
        return; // Not enough observations to mask.
    }

    // The last `window` tool messages are preserved.
    let cutoff = tool_indices.len() - window;
    for &idx in &tool_indices[..cutoff] {
        let m = &messages[idx];

        // Skip already masked.
        if is_observation_masked(m.content.as_deref()) {
            continue;
        }
        // Skip already pruned.
        if m.content.as_deref() == Some(PRUNED_SENTINEL) {
            continue;
        }
        // Skip protected tools.
        if is_protected(m.name.as_deref()) {
            continue;
        }

        let header = observation_mask_header(
            messages[idx].name.as_deref(),
            messages[idx].tool_call_id.as_deref().unwrap_or("?"),
        );
        messages[idx].content = Some(header);
    }
}

/// Apply observation masking using policy's `observation_mask_window`.
pub fn apply_observation_mask_with_policy(
    messages: &mut [Message],
    policy: &CompactionPolicy,
) {
    apply_observation_mask(messages, policy.observation_mask_window);
}

#[cfg(test)]
mod tests {
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
}
