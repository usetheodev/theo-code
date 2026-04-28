//! Lifecycle hooks system — 22 Claude Agent SDK-aligned events.
//!
//! Distinct from `hooks.rs` (shell-script-based external hooks). This module
//! implements **declarative**, **deterministic** hooks with static responses
//! defined in YAML frontmatter (per-agent) or `AgentConfig` (global).
//!
//! Design decisions:
//! - Events match Claude SDK names (PreToolUse, PostToolUse, SubagentStart, ...).
//! - Responses are STATIC (Allow / Block / Replace / InjectContext) — not LLM calls.
//!   Justification: prevents loops and per-hook cost.
//! - Hooks per-agent override globals on conflict (most-specific-wins).
//! - Timeout default 60s (Claude SDK alignment). Timeout = response Allow
//!   (fail-open, deadlock-prevention).

use std::collections::BTreeMap;

use regex::Regex;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// HookEvent — 22 events aligned with Claude Agent SDK + Archon hooks.ts:10-32
// ---------------------------------------------------------------------------

/// Lifecycle event identifier. The 22 variants match the Claude Agent SDK
/// HookEvent type and Archon's `workflowHookEventSchema`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
    PostToolUseFailure,
    Notification,
    UserPromptSubmit,
    SessionStart,
    SessionEnd,
    Stop,
    SubagentStart,
    SubagentStop,
    PreCompact,
    PermissionRequest,
    Setup,
    TeammateIdle,
    TaskCompleted,
    Elicitation,
    ElicitationResult,
    ConfigChange,
    WorktreeCreate,
    WorktreeRemove,
    InstructionsLoaded,
    /// Fired right before a `delegate_task` spawn. Hook may
    /// `Block` the handoff (returns `HookResponse::Block`) — the
    /// guardrail chain runs first, the hook is the final gate.
    PreHandoff,
}

impl HookEvent {
    pub const ALL: [HookEvent; 22] = [
        HookEvent::PreToolUse,
        HookEvent::PostToolUse,
        HookEvent::PostToolUseFailure,
        HookEvent::Notification,
        HookEvent::UserPromptSubmit,
        HookEvent::SessionStart,
        HookEvent::SessionEnd,
        HookEvent::Stop,
        HookEvent::SubagentStart,
        HookEvent::SubagentStop,
        HookEvent::PreCompact,
        HookEvent::PermissionRequest,
        HookEvent::Setup,
        HookEvent::TeammateIdle,
        HookEvent::TaskCompleted,
        HookEvent::Elicitation,
        HookEvent::ElicitationResult,
        HookEvent::ConfigChange,
        HookEvent::WorktreeCreate,
        HookEvent::WorktreeRemove,
        HookEvent::InstructionsLoaded,
        HookEvent::PreHandoff,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            HookEvent::PreToolUse => "PreToolUse",
            HookEvent::PostToolUse => "PostToolUse",
            HookEvent::PostToolUseFailure => "PostToolUseFailure",
            HookEvent::Notification => "Notification",
            HookEvent::UserPromptSubmit => "UserPromptSubmit",
            HookEvent::SessionStart => "SessionStart",
            HookEvent::SessionEnd => "SessionEnd",
            HookEvent::Stop => "Stop",
            HookEvent::SubagentStart => "SubagentStart",
            HookEvent::SubagentStop => "SubagentStop",
            HookEvent::PreCompact => "PreCompact",
            HookEvent::PermissionRequest => "PermissionRequest",
            HookEvent::Setup => "Setup",
            HookEvent::TeammateIdle => "TeammateIdle",
            HookEvent::TaskCompleted => "TaskCompleted",
            HookEvent::Elicitation => "Elicitation",
            HookEvent::ElicitationResult => "ElicitationResult",
            HookEvent::ConfigChange => "ConfigChange",
            HookEvent::WorktreeCreate => "WorktreeCreate",
            HookEvent::WorktreeRemove => "WorktreeRemove",
            HookEvent::InstructionsLoaded => "InstructionsLoaded",
            HookEvent::PreHandoff => "PreHandoff",
        }
    }
}

// ---------------------------------------------------------------------------
// HookContext — runtime data passed to dispatch
// ---------------------------------------------------------------------------

/// Context passed to a hook dispatch. Used by matcher (regex) to decide if
/// the matcher fires.
#[derive(Debug, Clone, Default)]
pub struct HookContext {
    /// For PreToolUse / PostToolUse: tool name to match against `matcher` regex.
    pub tool_name: Option<String>,
    /// For PreToolUse: input arguments (used by HookResponse::Replace).
    pub tool_args: Option<serde_json::Value>,
    /// For PostToolUse: tool result.
    pub tool_result: Option<serde_json::Value>,
    /// For PreHandoff: target sub-agent name.
    pub target_agent: Option<String>,
    /// For PreHandoff: objective string the parent passed to delegate_task.
    pub target_objective: Option<String>,
}

// ---------------------------------------------------------------------------
// HookResponse — what the hook returns
// ---------------------------------------------------------------------------

/// Response returned by a hook matcher.
///
/// - `Allow` — continue normally (default if no matcher fires).
/// - `Block { reason }` — block the operation; the agent sees `reason` as error.
/// - `Replace { value }` — substitute the input/result.
/// - `InjectContext { content }` — prepend to message stream (Rippletide pattern).
///
/// **Security contract (T2.4 / FIND-P6-002 / D5):** `InjectContext.content`
/// is hook-controlled (project or user shell-script output) and MUST be
/// sanitized via [`HookResponse::inject_context_sanitized`] before being
/// concatenated into the LLM prompt. Direct access to the inner `content`
/// field bypasses the strip-injection-tokens step and re-opens find_p6_002.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[derive(Default)]
pub enum HookResponse {
    #[default]
    Allow,
    Block { reason: String },
    Replace { value: serde_json::Value },
    InjectContext { content: String },
}

impl HookResponse {
    /// Returns the `content` of an `InjectContext` response with all
    /// known LLM-injection tokens stripped and bounded to
    /// `max_bytes`. Returns `None` for any other variant.
    ///
    /// Callers integrating the result into the LLM message stream MUST
    /// use this helper instead of pattern-matching on the variant
    /// directly. T2.4 / FIND-P6-002.
    pub fn inject_context_sanitized(&self, max_bytes: usize) -> Option<String> {
        match self {
            HookResponse::InjectContext { content } => Some(
                theo_domain::prompt_sanitizer::fence_untrusted(
                    content,
                    "hook:inject_context",
                    max_bytes,
                ),
            ),
            _ => None,
        }
    }
}


// ---------------------------------------------------------------------------
// HookMatcher — single rule (matcher + response + timeout)
// ---------------------------------------------------------------------------

/// A single hook rule. The `matcher` is an optional regex that's tested
/// against `HookContext.tool_name` (for tool-related events) or skipped
/// (always matches) for events without a target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookMatcher {
    #[serde(default)]
    pub matcher: Option<String>,
    pub response: HookResponse,
    /// Timeout in seconds (default 60). Currently informational; full
    /// async timeout enforcement happens at HookManager::dispatch.
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u32,
}

fn default_timeout_secs() -> u32 {
    60
}

impl HookMatcher {
    /// Test whether this matcher fires for a given context.
    /// Returns `Err` if the regex is invalid.
    ///
    /// Match target precedence (first non-None wins):
    /// 1. `tool_name` — PreToolUse / PostToolUse
    /// 2. `target_agent` — PreHandoff
    /// 3. `target_objective` — PreHandoff fallback if target_agent absent
    pub fn matches(&self, ctx: &HookContext) -> Result<bool, regex::Error> {
        match &self.matcher {
            None => Ok(true), // No matcher = always fires
            Some(pattern) => {
                let re = Regex::new(pattern)?;
                if let Some(name) = &ctx.tool_name {
                    Ok(re.is_match(name))
                } else if let Some(agent) = &ctx.target_agent {
                    Ok(re.is_match(agent))
                } else if let Some(obj) = &ctx.target_objective {
                    Ok(re.is_match(obj))
                } else {
                    Ok(false)
                }
            }
        }
    }
}

/// Returned by [`HookManager::validate_regexes`] when a registered hook
/// matcher contains a syntactically invalid regex.
///
/// `event` and `index` pinpoint the offending matcher so the caller can
/// surface the problem to the user verbatim. T4.2 / find_p6_007.
#[derive(Debug, thiserror::Error)]
#[error("invalid hook regex for event {event} matcher #{index}: {pattern} ({source})")]
pub struct HookRegexError {
    pub event: String,
    pub index: usize,
    pub pattern: String,
    #[source]
    pub source: regex::Error,
}

// ---------------------------------------------------------------------------
// HookManager — registry of hooks per event
// ---------------------------------------------------------------------------

/// Map of event → list of matchers, evaluated in order.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HookManager {
    /// Hooks keyed by event name. BTreeMap for deterministic iteration.
    #[serde(flatten)]
    by_event: BTreeMap<String, Vec<HookMatcher>>,
}

impl HookManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a matcher under an event.
    pub fn add(&mut self, event: HookEvent, matcher: HookMatcher) {
        self.by_event
            .entry(event.as_str().to_string())
            .or_default()
            .push(matcher);
    }

    /// Pre-compile every matcher's regex and return the first compile
    /// error (along with which event + index it affects). Callers that
    /// load `HookManager` from user-supplied YAML/JSON SHOULD invoke
    /// this immediately after `serde` deserialization so a malformed
    /// regex fails loud at startup instead of being silently treated as
    /// "allow" at dispatch time (T4.2 / find_p6_007).
    pub fn validate_regexes(&self) -> Result<(), HookRegexError> {
        for (event, matchers) in &self.by_event {
            for (idx, m) in matchers.iter().enumerate() {
                if let Some(pattern) = &m.matcher
                    && let Err(e) = Regex::new(pattern)
                {
                    return Err(HookRegexError {
                        event: event.clone(),
                        index: idx,
                        pattern: pattern.clone(),
                        source: e,
                    });
                }
            }
        }
        Ok(())
    }

    /// Get all matchers registered for an event (deterministic order).
    pub fn get(&self, event: HookEvent) -> &[HookMatcher] {
        self.by_event
            .get(event.as_str())
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Evaluate all matchers for an event with the given context.
    /// Returns the FIRST non-Allow response (Block/Replace/InjectContext)
    /// or `HookResponse::Allow` if none match.
    ///
    /// Errors from regex compilation are converted to `Allow` (fail-open).
    pub fn dispatch(&self, event: HookEvent, ctx: &HookContext) -> HookResponse {
        for m in self.get(event) {
            match m.matches(ctx) {
                Ok(true) => match &m.response {
                    HookResponse::Allow => continue,
                    other => return other.clone(),
                },
                Ok(false) => continue,
                Err(e) => {
                    // T4.2 / find_p6_007 — Previously this arm
                    // silently fail-open. Now it emits a structured
                    // log so a broken regex is at least observable in
                    // tracing output. Callers SHOULD also invoke
                    // `HookManager::validate_regexes()` at load time
                    // to surface the failure even earlier.
                    tracing::error!(
                        event = ?event,
                        pattern = ?m.matcher,
                        error = %e,
                        "hook matcher regex compile failed; treating as no-match (fail-open)"
                    );
                    continue;
                }
            }
        }
        HookResponse::Allow
    }

    /// Merge another HookManager into this one. Per-agent hooks should be
    /// merged into globals so per-agent matches take precedence (added LAST
    /// so `dispatch` sees them first via append).
    ///
    /// Wait — `dispatch` iterates in insertion order, so to give priority
    /// to `other`, prepend its matchers. We do that here.
    pub fn merge_with_priority(&mut self, other: HookManager) {
        for (event, matchers) in other.by_event {
            let entry = self.by_event.entry(event).or_default();
            // Prepend other's matchers so they're evaluated first
            let mut merged = matchers;
            merged.extend(std::mem::take(entry));
            *entry = merged;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.by_event.is_empty()
    }

    pub fn event_count(&self) -> usize {
        self.by_event.len()
    }
}

#[cfg(test)]
#[path = "lifecycle_hooks_tests.rs"]
mod tests;
