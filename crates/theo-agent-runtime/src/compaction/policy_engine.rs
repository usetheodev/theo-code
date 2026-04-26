//! Internal helpers for `compact_with_policy` — split out to keep the
//! top-level function readable as an orchestrator.
//!
//! Fase 4 (REMEDIATION_PLAN T4.6). Extracted from `compaction.rs`.
//! Behavior is byte-identical; these helpers mutate `messages` in place
//! or build intermediate state that the orchestrator then uses to
//! assemble the compaction summary.

use super::{truncate_utf8, CompactionContext, COMPACTED_PREFIX};
use theo_infra_llm::types::{Message, Role};

/// Running state assembled while walking the compactable prefix.
/// Owned by the orchestrator and threaded through the helper.
pub(super) struct CompactionState {
    pub tools_used: Vec<String>,
    pub files_mentioned: Vec<String>,
    pub compacted_turns: usize,
    /// True when at least one message was actually truncated. When this
    /// stays false the orchestrator should bail out without inserting a
    /// summary (nothing changed → idempotent no-op).
    pub compacted: bool,
}

impl CompactionState {
    pub(super) fn new() -> Self {
        Self {
            tools_used: Vec::new(),
            files_mentioned: Vec::new(),
            compacted_turns: 0,
            compacted: false,
        }
    }
}

/// Walk the messages from the tail backward and return the index that
/// separates compactable from preserved. Counts only non-system messages
/// toward the `preserve_tail` budget.
///
/// **Pair-atomicity (T3.4 / find_p4_007 / find_p4_009 / INV-001):**
/// after computing the naïve cut point, the boundary is **moved
/// forward** until it sits OUTSIDE any `assistant_with_tool_calls` →
/// `Role::Tool` pair. This prevents the historical bug where a tool
/// result could survive into the preserved tail while its matching
/// `tool_use` was about to be compacted (or vice versa) — leaving the
/// LLM with an orphan that providers reject. `sanitize_tool_pairs`
/// remains as a defensive backstop, but is now a no-op on the happy
/// path.
pub(super) fn find_boundary_idx(messages: &[Message], preserve_tail: usize) -> usize {
    let mut tail_count = 0usize;
    let mut boundary_idx = messages.len();
    for (i, m) in messages.iter().enumerate().rev() {
        if m.role != Role::System {
            tail_count += 1;
            if tail_count == preserve_tail {
                boundary_idx = i;
                break;
            }
        }
    }

    // Move the boundary forward while it would split a tool pair:
    //   - boundary points at a `Role::Tool` whose matching assistant
    //     tool_call is at boundary_idx-1 (i.e. lives BEFORE the
    //     preserved window). Advance past every contiguous `Role::Tool`
    //     so the entire result block is preserved with its assistant.
    //   - boundary points one position past an
    //     `assistant_with_tool_calls` whose matching `Role::Tool`
    //     results live AT or AFTER the boundary. Advance past those
    //     results so the assistant + its results stay together.
    while boundary_idx < messages.len()
        && (
            // Case A: cut would land in the middle of consecutive tool
            // results. Push forward until past the last contiguous Tool.
            messages[boundary_idx].role == Role::Tool
                || (boundary_idx > 0
                    && messages[boundary_idx - 1].role == Role::Assistant
                    && messages[boundary_idx - 1].tool_calls.is_some()
                    && messages[boundary_idx].role == Role::Tool)
        )
    {
        boundary_idx += 1;
    }

    boundary_idx
}

/// Walk `messages[..boundary_idx]` and compact everything compactable:
/// truncate tool results, truncate long assistant tool-call arguments,
/// and accumulate `tools_used` + `files_mentioned` as side-effects.
/// Already-compacted summaries are skipped; system messages are never
/// touched. Mutates `messages` in place; flips `state.compacted` when
/// anything was actually truncated.
pub(super) fn compact_older_messages(
    messages: &mut [Message],
    boundary_idx: usize,
    truncate_chars: usize,
    state: &mut CompactionState,
) {
    // Index-based loop needed: we read and mutate messages[i] across branches.
    #[allow(clippy::needless_range_loop)]
    for i in 0..boundary_idx {
        let m = &messages[i];

        if m.role == Role::System {
            continue;
        }

        // Skip already-compacted summaries (idempotence).
        if m.role == Role::User
            && let Some(ref c) = m.content
            && c.starts_with(COMPACTED_PREFIX)
        {
            continue;
        }

        if m.role == Role::Tool {
            compact_tool_message(messages, i, truncate_chars, state);
            continue;
        }

        if m.role == Role::Assistant {
            compact_assistant_tool_calls(messages, i, truncate_chars, state);
        }
    }
}

/// Compact a Tool-role message: extract tool name + any file mentions,
/// then UTF-8-safe-truncate the content when it exceeds ~4× the char cap.
fn compact_tool_message(
    messages: &mut [Message],
    i: usize,
    truncate_chars: usize,
    state: &mut CompactionState,
) {
    if let Some(ref name) = messages[i].name
        && !state.tools_used.contains(name)
    {
        state.tools_used.push(name.clone());
    }
    if let Some(ref content) = messages[i].content
        && content.len() > truncate_chars * 4
    {
        extract_file_mentions(content, &mut state.files_mentioned);
        messages[i].content = Some(truncate_utf8(content, truncate_chars));
        state.compacted = true;
    }
    state.compacted_turns += 1;
}

/// Compact an Assistant-role message's tool calls: record the call names
/// and truncate any oversized argument blobs in place.
fn compact_assistant_tool_calls(
    messages: &mut [Message],
    i: usize,
    truncate_chars: usize,
    state: &mut CompactionState,
) {
    if messages[i].tool_calls.is_none() {
        return;
    }
    // First pass: collect tool names (no mutation needed).
    if let Some(ref tcs) = messages[i].tool_calls {
        for tc in tcs {
            if !state.tools_used.contains(&tc.function.name) {
                state.tools_used.push(tc.function.name.clone());
            }
        }
    }
    // Second pass: truncate long arguments (mutable).
    if let Some(ref mut tcs) = messages[i].tool_calls {
        for tc in tcs.iter_mut() {
            if tc.function.arguments.len() > truncate_chars * 4 {
                tc.function.arguments = truncate_utf8(&tc.function.arguments, truncate_chars);
                state.compacted = true;
            }
        }
    }
    state.compacted_turns += 1;
}

/// Extract file-path-looking tokens from `content` (basic heuristic:
/// contains `/` or `.`, ends with a common source extension). Mutates
/// `files_mentioned` so the summary can surface which files the
/// truncated tool output touched.
fn extract_file_mentions(content: &str, files_mentioned: &mut Vec<String>) {
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
            let clean_str = clean.to_string();
            if !files_mentioned.contains(&clean_str) {
                files_mentioned.push(clean_str);
            }
        }
    }
}

/// Remove any previous `[COMPACTED]` summary from `messages` so that
/// re-running compaction replaces rather than accumulates summaries.
pub(super) fn remove_previous_summary(messages: &mut Vec<Message>) {
    messages.retain(|m| {
        !(m.role == Role::User
            && m.content
                .as_deref()
                .is_some_and(|c| c.starts_with(COMPACTED_PREFIX)))
    });
}

/// Build the final compaction summary string with optional progress
/// context (task objective / phase / targets / recent errors).
pub(super) fn build_summary(
    compacted_turns: usize,
    mut tools_used: Vec<String>,
    mut files_mentioned: Vec<String>,
    context: Option<&CompactionContext>,
) -> String {
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

    let progress_str = build_progress_str(context);

    format!(
        "{COMPACTED_PREFIX}Conversation history was compressed ({compacted_turns} older messages truncated).{progress_str}{tools_str}{files_str} Recent messages are preserved in full."
    )
}

/// Build the optional " Task: …. Phase: …. Targets: …. Errors: …"
/// fragment from `CompactionContext`. Returns an empty string when no
/// context is provided.
fn build_progress_str(context: Option<&CompactionContext>) -> String {
    let Some(ctx) = context else {
        return String::new();
    };
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
}

/// Insert the compaction summary after the last system message (or at
/// position 0 when no system messages exist). This keeps the system
/// preamble intact and the summary visible to the model as the first
/// user-role message.
pub(super) fn insert_summary_after_system(messages: &mut Vec<Message>, summary: String) {
    let insert_pos = messages
        .iter()
        .rposition(|m| m.role == Role::System)
        .map(|i| i + 1)
        .unwrap_or(0);
    messages.insert(insert_pos, Message::user(summary));
}

#[cfg(test)]
mod t34_pair_atomicity_tests {
    use super::*;
    use theo_infra_llm::types::{FunctionCall, ToolCall};

    fn user(s: &str) -> Message {
        Message::user(s)
    }

    fn assistant_with_tool_call(call_id: &str, name: &str) -> Message {
        Message::assistant_with_tool_calls(
            None,
            vec![ToolCall {
                id: call_id.into(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: name.into(),
                    arguments: "{}".into(),
                },
            }],
        )
    }

    fn tool_result(call_id: &str, name: &str, output: &str) -> Message {
        Message::tool_result(call_id, name, output)
    }

    /// T3.4 / find_p4_009 — when the naïve cut would land between an
    /// assistant tool_call and its matching tool_result, the boundary
    /// must be advanced past the entire result block so the pair stays
    /// together.
    #[test]
    fn t34_boundary_advances_past_tool_result_when_assistant_just_before() {
        let messages = vec![
            user("seed"),                                    // 0
            assistant_with_tool_call("c1", "read"),          // 1
            tool_result("c1", "read", "ok"),                 // 2 — would be split if boundary=2
            user("follow up"),                               // 3
            assistant_with_tool_call("c2", "write"),         // 4
            tool_result("c2", "write", "ok"),                // 5
        ];

        // preserve_tail=4 → naïve cut at index 2 (Tool result), which
        // would split the call_id="c1" pair across the boundary.
        let boundary = find_boundary_idx(&messages, 4);
        assert!(
            boundary >= 3,
            "boundary must move past the tool_result block; got {boundary}"
        );
        // Also: every tool_result that survives must have its
        // assistant tool_call surviving too.
        for (i, m) in messages[boundary..].iter().enumerate() {
            if m.role == Role::Tool {
                let absolute = boundary + i;
                assert!(
                    messages[..absolute]
                        .iter()
                        .any(|m| m.role == Role::Assistant && m.tool_calls.is_some()),
                    "surviving tool_result has no preserved assistant tool_call"
                );
            }
        }
    }

    /// T3.4 — when there are MULTIPLE consecutive tool_results after an
    /// assistant tool_call (parallel/batch), all results must move
    /// across together — splitting any of them is still an orphan.
    #[test]
    fn t34_boundary_advances_past_multiple_consecutive_tool_results() {
        let messages = vec![
            user("seed"),                                    // 0
            assistant_with_tool_call("c1", "batch"),         // 1
            tool_result("c1", "batch", "1/3"),               // 2
            tool_result("c1", "batch", "2/3"),               // 3
            tool_result("c1", "batch", "3/3"),               // 4
            user("done"),                                    // 5
        ];

        // preserve_tail=4 → naïve cut at index 2.
        let boundary = find_boundary_idx(&messages, 4);
        assert!(
            boundary >= 5,
            "boundary must skip ALL contiguous Role::Tool entries; got {boundary}"
        );
    }

    /// Sanity: when no pair is split, the boundary stays at the
    /// naïve position (no over-shooting that would defeat the
    /// preserve_tail budget).
    #[test]
    fn t34_boundary_unchanged_when_naive_cut_is_safe() {
        let messages = vec![
            user("a"),                          // 0
            user("b"),                          // 1
            user("c"),                          // 2
            user("d"),                          // 3
        ];
        let boundary = find_boundary_idx(&messages, 2);
        // Naïve cut: 2 user messages preserved → boundary at index 2.
        assert_eq!(boundary, 2);
    }
}
