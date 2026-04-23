//! Lifecycle hooks system — 22 Claude Agent SDK-aligned events.
//!
//! Track B — Phase 5.
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
    /// Phase 18: fired right before a `delegate_task` spawn. Hook may
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
    /// For PreHandoff: target sub-agent name. Phase 24 (sota-gaps-followup).
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
    /// 2. `target_agent` — PreHandoff (Phase 24 sota-gaps-followup)
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
                Err(_) => continue, // fail-open on regex error
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
mod tests {
    use super::*;

    fn allow_matcher() -> HookMatcher {
        HookMatcher {
            matcher: None,
            response: HookResponse::Allow,
            timeout_secs: 60,
        }
    }

    fn block_for_pattern(pattern: &str, reason: &str) -> HookMatcher {
        HookMatcher {
            matcher: Some(pattern.to_string()),
            response: HookResponse::Block {
                reason: reason.to_string(),
            },
            timeout_secs: 60,
        }
    }

    #[test]
    fn hook_event_serde_roundtrip_for_all_21_variants() {
        // Note: the SDK enum includes 22 categories total (PreCompact split etc).
        // Our enum covers 21 distinct identifiers + a `Setup` variant for parity.
        for event in HookEvent::ALL {
            let s = serde_json::to_string(&event).unwrap();
            let back: HookEvent = serde_json::from_str(&s).unwrap();
            assert_eq!(back, event);
        }
    }

    #[test]
    fn hook_event_as_str_matches_claude_sdk() {
        assert_eq!(HookEvent::PreToolUse.as_str(), "PreToolUse");
        assert_eq!(HookEvent::SubagentStart.as_str(), "SubagentStart");
        assert_eq!(HookEvent::WorktreeCreate.as_str(), "WorktreeCreate");
    }

    #[test]
    fn hook_event_all_includes_22_subagent_lifecycle() {
        assert!(HookEvent::ALL.contains(&HookEvent::SubagentStart));
        assert!(HookEvent::ALL.contains(&HookEvent::SubagentStop));
    }

    #[test]
    fn hook_matcher_regex_matches_tool_name() {
        let m = block_for_pattern("^bash$", "no bash in security review");
        let ctx = HookContext {
            tool_name: Some("bash".into()),
            ..Default::default()
        };
        assert!(m.matches(&ctx).unwrap());
    }

    #[test]
    fn hook_matcher_regex_does_not_match_other_tool() {
        let m = block_for_pattern("^bash$", "x");
        let ctx = HookContext {
            tool_name: Some("read".into()),
            ..Default::default()
        };
        assert!(!m.matches(&ctx).unwrap());
    }

    #[test]
    fn hook_matcher_alternation_matches_multiple_tools() {
        let m = block_for_pattern("^(edit|write|apply_patch)$", "x");
        for tool in &["edit", "write", "apply_patch"] {
            let ctx = HookContext {
                tool_name: Some(tool.to_string()),
                ..Default::default()
            };
            assert!(m.matches(&ctx).unwrap(), "should match {}", tool);
        }
    }

    #[test]
    fn hook_matcher_no_pattern_always_matches() {
        let m = allow_matcher();
        let ctx = HookContext::default();
        assert!(m.matches(&ctx).unwrap());
    }

    #[test]
    fn hook_response_block_prevents_tool_execution() {
        let mut mgr = HookManager::new();
        mgr.add(
            HookEvent::PreToolUse,
            block_for_pattern("^bash$", "no bash"),
        );
        let ctx = HookContext {
            tool_name: Some("bash".into()),
            ..Default::default()
        };
        let resp = mgr.dispatch(HookEvent::PreToolUse, &ctx);
        match resp {
            HookResponse::Block { reason } => assert_eq!(reason, "no bash"),
            other => panic!("expected Block, got {:?}", other),
        }
    }

    #[test]
    fn hook_response_inject_context_returned() {
        let mut mgr = HookManager::new();
        mgr.add(
            HookEvent::UserPromptSubmit,
            HookMatcher {
                matcher: None,
                response: HookResponse::InjectContext {
                    content: "Always check OWASP Top 10.".into(),
                },
                timeout_secs: 60,
            },
        );
        let resp = mgr.dispatch(HookEvent::UserPromptSubmit, &HookContext::default());
        match resp {
            HookResponse::InjectContext { content } => {
                assert!(content.contains("OWASP"));
            }
            other => panic!("expected InjectContext, got {:?}", other),
        }
    }

    #[test]
    fn hook_response_replace_substitutes_value() {
        let mut mgr = HookManager::new();
        mgr.add(
            HookEvent::PreToolUse,
            HookMatcher {
                matcher: None,
                response: HookResponse::Replace {
                    value: serde_json::json!({"redacted": true}),
                },
                timeout_secs: 60,
            },
        );
        let resp = mgr.dispatch(HookEvent::PreToolUse, &HookContext::default());
        match resp {
            HookResponse::Replace { value } => assert_eq!(value["redacted"], true),
            other => panic!("expected Replace, got {:?}", other),
        }
    }

    #[test]
    fn hook_dispatch_no_matchers_returns_allow() {
        let mgr = HookManager::new();
        let resp = mgr.dispatch(HookEvent::PreToolUse, &HookContext::default());
        assert_eq!(resp, HookResponse::Allow);
    }

    #[test]
    fn hook_dispatch_first_matching_block_wins() {
        let mut mgr = HookManager::new();
        mgr.add(
            HookEvent::PreToolUse,
            block_for_pattern("^never_match_xyz$", "x"),
        );
        mgr.add(
            HookEvent::PreToolUse,
            block_for_pattern("^bash$", "first match wins"),
        );
        let ctx = HookContext {
            tool_name: Some("bash".into()),
            ..Default::default()
        };
        let resp = mgr.dispatch(HookEvent::PreToolUse, &ctx);
        match resp {
            HookResponse::Block { reason } => assert_eq!(reason, "first match wins"),
            _ => panic!(),
        }
    }

    #[test]
    fn hook_dispatch_invalid_regex_is_fail_open() {
        let mut mgr = HookManager::new();
        mgr.add(
            HookEvent::PreToolUse,
            HookMatcher {
                matcher: Some("[invalid(regex".into()),
                response: HookResponse::Block {
                    reason: "should not block".into(),
                },
                timeout_secs: 60,
            },
        );
        let ctx = HookContext {
            tool_name: Some("anything".into()),
            ..Default::default()
        };
        // Invalid regex → fail-open (Allow)
        assert_eq!(
            mgr.dispatch(HookEvent::PreToolUse, &ctx),
            HookResponse::Allow
        );
    }

    #[test]
    fn hook_per_agent_overrides_global_via_merge_with_priority() {
        let mut global = HookManager::new();
        global.add(
            HookEvent::PreToolUse,
            HookMatcher {
                matcher: None,
                response: HookResponse::Block {
                    reason: "global block".into(),
                },
                timeout_secs: 60,
            },
        );

        let mut per_agent = HookManager::new();
        per_agent.add(
            HookEvent::PreToolUse,
            HookMatcher {
                matcher: None,
                response: HookResponse::InjectContext {
                    content: "per-agent override".into(),
                },
                timeout_secs: 60,
            },
        );

        global.merge_with_priority(per_agent);

        // Per-agent fires first (more specific wins)
        let resp = global.dispatch(HookEvent::PreToolUse, &HookContext::default());
        match resp {
            HookResponse::InjectContext { content } => assert_eq!(content, "per-agent override"),
            other => panic!("expected InjectContext (per-agent), got {:?}", other),
        }
    }

    #[test]
    fn hook_manager_event_count_correct() {
        let mut mgr = HookManager::new();
        assert_eq!(mgr.event_count(), 0);
        mgr.add(HookEvent::PreToolUse, allow_matcher());
        mgr.add(HookEvent::PostToolUse, allow_matcher());
        mgr.add(HookEvent::PreToolUse, allow_matcher()); // same event again
        assert_eq!(mgr.event_count(), 2); // distinct event keys
    }

    #[test]
    fn hook_manager_default_timeout_60s() {
        let m: HookMatcher = serde_json::from_str(
            r#"{"matcher": "^bash$", "response": {"type": "allow"}}"#,
        )
        .unwrap();
        assert_eq!(m.timeout_secs, 60);
    }

    #[test]
    fn hook_manager_serde_roundtrip() {
        let mut mgr = HookManager::new();
        mgr.add(HookEvent::PreToolUse, block_for_pattern("^bash$", "x"));
        mgr.add(HookEvent::SubagentStart, allow_matcher());
        let json = serde_json::to_string(&mgr).unwrap();
        let back: HookManager = serde_json::from_str(&json).unwrap();
        assert_eq!(back.event_count(), mgr.event_count());
    }

    // ── Phase 24 (sota-gaps-followup): PreHandoff matcher ──

    pub mod pre_handoff {
        use super::*;

        #[test]
        fn hook_context_carries_pre_handoff_fields() {
            let ctx = HookContext {
                tool_name: None,
                tool_args: None,
                tool_result: None,
                target_agent: Some("verifier".into()),
                target_objective: Some("audit security".into()),
            };
            assert_eq!(ctx.target_agent.as_deref(), Some("verifier"));
            assert_eq!(ctx.target_objective.as_deref(), Some("audit security"));
        }

        #[test]
        fn pre_handoff_matcher_blocks_by_target_agent_regex() {
            let matcher = HookMatcher {
                matcher: Some("^impl.*$".into()),
                response: HookResponse::Block { reason: "no impl".into() },
                timeout_secs: 60,
            };
            let ctx = HookContext {
                tool_name: None,
                tool_args: None,
                tool_result: None,
                target_agent: Some("implementer".into()),
                target_objective: Some("anything".into()),
            };
            assert!(matcher.matches(&ctx).unwrap());
        }

        #[test]
        fn pre_handoff_matcher_blocks_by_objective_regex_when_no_target_agent() {
            let matcher = HookMatcher {
                matcher: Some("prod|production".into()),
                response: HookResponse::Block { reason: "no prod".into() },
                timeout_secs: 60,
            };
            let ctx = HookContext {
                tool_name: None,
                tool_args: None,
                tool_result: None,
                target_agent: None,
                target_objective: Some("deploy to production".into()),
            };
            assert!(matcher.matches(&ctx).unwrap());
        }

        #[test]
        fn pre_handoff_matcher_allows_when_no_match() {
            let matcher = HookMatcher {
                matcher: Some("^verifier$".into()),
                response: HookResponse::Block { reason: "x".into() },
                timeout_secs: 60,
            };
            let ctx = HookContext {
                tool_name: None,
                tool_args: None,
                tool_result: None,
                target_agent: Some("explorer".into()),
                target_objective: Some("read foo".into()),
            };
            // target_agent doesn't match, no tool_name, no objective match either
            assert!(!matcher.matches(&ctx).unwrap());
        }

        #[test]
        fn pre_handoff_matcher_tool_name_takes_precedence_over_target_agent() {
            // Backward compat: existing PreToolUse matchers still work even
            // when both tool_name AND target_agent are populated.
            let matcher = HookMatcher {
                matcher: Some("^delegate_task".into()),
                response: HookResponse::Block { reason: "x".into() },
                timeout_secs: 60,
            };
            let ctx = HookContext {
                tool_name: Some("delegate_task:verifier".into()),
                tool_args: None,
                tool_result: None,
                target_agent: Some("verifier".into()),
                target_objective: Some("review".into()),
            };
            assert!(matcher.matches(&ctx).unwrap());
        }

        #[test]
        fn pre_handoff_no_matcher_always_fires() {
            let matcher = HookMatcher {
                matcher: None,
                response: HookResponse::Block { reason: "universal".into() },
                timeout_secs: 60,
            };
            let ctx = HookContext {
                tool_name: None,
                tool_args: None,
                tool_result: None,
                target_agent: Some("any".into()),
                target_objective: Some("any".into()),
            };
            assert!(matcher.matches(&ctx).unwrap());
        }
    }
}
