//! Contract test — observability sensors / loop detector / compaction
//! protection MUST reference only tool IDs that exist in the production
//! registry.
//!
//! Bug 2026-04-27 (dogfood): five production lists in
//! `theo-agent-runtime` referenced `edit_file` / `write_file` /
//! `read_file` — names that haven't existed in the registry since at
//! least the snapshot-pin contract test shipped. The mismatch silently
//! disabled `WeakVerification` / `ConversationHistoryLoss`, fired
//! `PrematureTermination` as a false positive on every successful run,
//! turned every `edit→read` workflow into a loop-detector warning, and
//! left `read` results unprotected from compaction.
//!
//! This test is the structural guarantee that the same class of bug
//! cannot land again. It builds the actual production registry and
//! asserts every tool name string referenced by:
//!   * `compaction_stages::PROTECTED_TOOL_NAMES`
//!   * `loop_detector::EXPECTED_SEQUENCES`
//!   * the hardcoded edit-tool IDs inside `failure_sensors::is_edit_tool`
//!     (tested indirectly by exercising the public sensors)
//!
//! is either:
//!   (a) a registered tool in `create_default_registry_with_project(".")`,
//!       OR
//!   (b) explicitly allowlisted in [`TOOL_NAME_ALLOWLIST`] (e.g.
//!       legacy aliases, MCP-discovered names that aren't in the
//!       default registry, or pseudo-event types like `bash` that the
//!       sensor matches on but the registry exposes under a different
//!       category).
//!
//! When the registry IS renamed in the future, both the snapshot pin
//! AND this test will fail in lockstep — the operator has to update
//! the sensor lists in the same commit, no silent regressions
//! possible.

use std::collections::HashSet;

/// Names that are referenced by observability lists but are intentionally
/// not in the default registry NOR in the manifest meta-tools. Each
/// entry needs a one-line justification.
const TOOL_NAME_ALLOWLIST: &[&str] = &[
    // (none currently — every observability reference must be either a
    // default-registry tool or a manifest meta-tool)
];

/// Union of:
///   * tool IDs registered by `create_default_registry_with_project`
///   * tool IDs declared in `theo_tooling::tool_manifest::TOOL_MANIFEST`
///     (meta-tools, experimental, internal — every tool the agent
///     might ever invoke)
fn known_tool_ids() -> HashSet<String> {
    let registry =
        theo_tooling::registry::create_default_registry_with_project(std::path::Path::new("."));
    let mut ids: HashSet<String> = registry.ids().into_iter().collect();
    for entry in theo_tooling::tool_manifest::TOOL_MANIFEST {
        ids.insert(entry.id.to_string());
    }
    ids
}

fn assert_known(name: &str, registered: &HashSet<String>, source: &str) {
    if registered.contains(name) || TOOL_NAME_ALLOWLIST.contains(&name) {
        return;
    }
    panic!(
        "{source} references tool name {name:?} but it is neither in the production \
         registry nor in TOOL_NAME_ALLOWLIST. Production tool IDs are pinned by \
         `default_registry_tool_id_snapshot_is_pinned` (theo-tooling). Either:\n\
         - rename the sensor's reference to a real production ID, OR\n\
         - register the new tool in the default registry, OR\n\
         - add it to TOOL_NAME_ALLOWLIST with explicit justification."
    );
}

/// PROTECTED_TOOL_NAMES (compaction_stages) — every entry must be a
/// real registered tool so that compaction's "do not mask" guarantee
/// actually fires.
#[test]
fn production_registry_recognises_compaction_protected_tool_names() {
    let registered = known_tool_ids();
    for name in theo_agent_runtime::compaction_stages::PROTECTED_TOOL_NAMES {
        assert_known(name, &registered, "compaction_stages::PROTECTED_TOOL_NAMES");
    }
}

/// EXPECTED_SEQUENCES (loop_detector) — every name in every (a, b) pair
/// must be a real registered tool so the whitelist actually matches.
#[test]
fn production_registry_recognises_loop_detector_expected_sequences() {
    let registered = known_tool_ids();
    for (a, b) in theo_agent_runtime::observability::loop_detector::EXPECTED_SEQUENCES {
        assert_known(a, &registered, "loop_detector::EXPECTED_SEQUENCES (left)");
        assert_known(b, &registered, "loop_detector::EXPECTED_SEQUENCES (right)");
    }
}

/// `failure_sensors::is_edit_tool` is a private predicate. Exercise it
/// indirectly by feeding the production edit-tool IDs through
/// `detect_premature_termination`: those IDs MUST count as edits, so
/// the sensor must NOT fire when one of them is the only ToolCallCompleted.
///
/// If a future change drops one of these IDs from the predicate's match
/// arm, the sensor will treat that tool's calls as zero progress and
/// `PrematureTermination` will fire even though the tool actually did
/// real work — exactly the dogfood-2026-04-27 false-positive class.
#[test]
fn production_edit_tool_ids_count_as_progress_for_premature_termination() {
    use theo_agent_runtime::observability::failure_sensors::detect_premature_termination;
    use theo_agent_runtime::observability::projection::{ProjectedStep, StepOutcome};
    use theo_domain::event::EventKind;

    let registered = known_tool_ids();
    let edit_ids = ["edit", "write", "apply_patch"];

    for id in edit_ids {
        // The IDs must exist in the actual registry — guarantees the
        // predicate is testing real production tools.
        assert!(
            registered.contains(id),
            "production registry MUST register {id:?} (failure_sensors::is_edit_tool depends on it)"
        );

        let steps = vec![
            ProjectedStep {
                sequence: 0,
                event_type: "RunInitialized".into(),
                event_kind: Some(EventKind::Tooling),
                timestamp: 0,
                entity_id: "e0".into(),
                payload_summary: String::new(),
                duration_ms: None,
                tool_name: None,
                outcome: None,
            },
            ProjectedStep {
                sequence: 1,
                event_type: "LlmCallStart".into(),
                event_kind: Some(EventKind::Tooling),
                timestamp: 1,
                entity_id: "e1".into(),
                payload_summary: String::new(),
                duration_ms: None,
                tool_name: None,
                outcome: None,
            },
            ProjectedStep {
                sequence: 2,
                event_type: "LlmCallStart".into(),
                event_kind: Some(EventKind::Tooling),
                timestamp: 2,
                entity_id: "e2".into(),
                payload_summary: String::new(),
                duration_ms: None,
                tool_name: None,
                outcome: None,
            },
            ProjectedStep {
                sequence: 3,
                event_type: "ToolCallCompleted".into(),
                event_kind: Some(EventKind::Tooling),
                timestamp: 3,
                entity_id: "e3".into(),
                payload_summary: String::new(),
                duration_ms: None,
                tool_name: Some(id.into()),
                outcome: Some(StepOutcome::Success),
            },
            ProjectedStep {
                sequence: 4,
                event_type: "RunStateChanged".into(),
                event_kind: Some(EventKind::Tooling),
                timestamp: 4,
                entity_id: "e4".into(),
                payload_summary: "converged".into(),
                duration_ms: None,
                tool_name: None,
                outcome: None,
            },
        ];
        assert!(
            !detect_premature_termination(&steps),
            "PrematureTermination must accept {id:?} as real progress (regression of dogfood F-B1)"
        );
    }
}
