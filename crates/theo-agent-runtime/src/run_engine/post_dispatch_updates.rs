//! Post-dispatch state-update helpers — keep working set, context
//! metrics, and the context-loop edit/read log in sync after each
//! tool call returns.
//!
//! Split out of `run_engine/main_loop.rs` (REMEDIATION_PLAN T4.* —
//! production-LOC trim toward the per-file 500-line target). Methods
//! stay `pub(super)` on `impl AgentRunEngine`, so `main_loop.rs`
//! callers are unchanged.

use super::AgentRunEngine;

impl AgentRunEngine {
    /// Post-dispatch working set + context metrics update. Classifies
    /// the tool call by name (read/edit/write/apply_patch vs grep/glob/
    /// codebase_context) and feeds the usefulness pipeline + action log.
    pub(super) fn update_working_set_post_tool(
        &mut self,
        call: &theo_infra_llm::types::ToolCall,
        name: &str,
        iteration: usize,
    ) {
        match name {
            "read" | "edit" | "write" | "apply_patch" => {
                if let Ok(args) = call.parse_arguments()
                    && let Some(path) = args
                        .get("filePath")
                        .or(args.get("file_path"))
                        .and_then(|p| p.as_str())
                {
                    self.obs.working_set.touch_file(path);
                    self.obs.context_metrics.record_artifact_fetch(path, iteration);
                    // Feed usefulness pipeline — which files the agent
                    // actually references vs just scans.
                    self.obs.context_metrics.record_tool_reference(path);
                }
            }
            "grep" | "glob" | "codebase_context" => {
                if let Ok(args) = call.parse_arguments() {
                    let query = args
                        .get("pattern")
                        .or(args.get("query"))
                        .and_then(|p| p.as_str())
                        .unwrap_or("");
                    self.obs.context_metrics
                        .record_action(&format!("{}: {}", name, query), iteration);
                    if let Some(path) = args.get("path").and_then(|p| p.as_str()) {
                        self.obs.context_metrics.record_tool_reference(path);
                    }
                }
            }
            _ => {}
        }
        self.obs.working_set
            .record_event(format!("tool:{}:iter{}", name, iteration), 20);
    }

    /// Post-dispatch context-loop state update. Records reads, search
    /// actions, and edit attempts — the last branch extracts the
    /// edited file path from `filePath` or from a `+++ b/<file>` line
    /// inside `patchText` (apply_patch case).
    pub(super) fn update_context_loop_post_tool(
        &mut self,
        call: &theo_infra_llm::types::ToolCall,
        name: &str,
        success: bool,
        output: &str,
    ) {
        match name {
            "read" => {
                if let Ok(args) = call.parse_arguments()
                    && let Some(path) = args.get("filePath").and_then(|p| p.as_str())
                {
                    self.rt.context_loop_state.record_read(path);
                }
            }
            "grep" | "glob" => self.rt.context_loop_state.record_search(),
            "edit" | "write" | "apply_patch" => {
                let file = call
                    .parse_arguments()
                    .ok()
                    .and_then(|args| {
                        args.get("filePath")
                            .or(args.get("file_path"))
                            .and_then(|p| p.as_str())
                            .map(String::from)
                            .or_else(|| {
                                args.get("patchText").and_then(|p| p.as_str()).and_then(
                                    |patch| {
                                        patch
                                            .lines()
                                            .find(|l| l.starts_with("+++ "))
                                            .and_then(|l| {
                                                l.strip_prefix("+++ b/")
                                                    .or(l.strip_prefix("+++ "))
                                            })
                                            .filter(|f| *f != "/dev/null")
                                            .map(String::from)
                                    },
                                )
                            })
                    })
                    .unwrap_or_default();
                self.rt.context_loop_state.record_edit_attempt(
                    &file,
                    success,
                    if success { None } else { Some(output.to_string()) },
                );
            }
            _ => {}
        }
    }
}
