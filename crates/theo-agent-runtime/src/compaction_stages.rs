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
#[path = "compaction_stages_tests.rs"]
mod tests;
