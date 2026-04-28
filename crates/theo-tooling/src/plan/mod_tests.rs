//! Sibling test body of `mod.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `mod.rs` via `#[path = "mod_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.

    #![allow(unused_imports)]

    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    use theo_domain::clock::now_millis;
    use theo_domain::error::ToolError;
    use theo_domain::identifiers::{PhaseId, PlanTaskId};
    use theo_domain::plan::{
        Phase, PhaseStatus, Plan, PlanDecision, PlanError, PlanTask, PlanTaskStatus,
    };
    use theo_domain::session::{MessageId, SessionId};
    use theo_domain::tool::{
        PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput,
    };

    use crate::plan::shared::{
        findings_path, plan_path, progress_path, read_plan, write_plan,
    };
    use crate::plan::side_files::{
        FindingsFile, ProgressFile, append_decision, append_error_entry, append_finding,
        append_requirement, append_resource,
    };

    fn make_ctx(project_dir: PathBuf) -> ToolContext {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        ToolContext {
            session_id: SessionId::new("ses_test"),
            message_id: MessageId::new(""),
            call_id: "call_test".into(),
            agent: "build".into(),
            abort: rx,
            project_dir,
            graph_context: None,
            stdout_tx: None,
        }
    }

    fn sample_phase_args() -> Value {
        json!([
            {
                "id": 1,
                "title": "Setup",
                "tasks": [
                    {"id": 1, "title": "Create struct", "dod": "compiles"}
                ]
            },
            {
                "id": 2,
                "title": "Tests",
                "tasks": [
                    {"id": 2, "title": "Add unit test", "depends_on": [1]}
                ]
            }
        ])
    }

    // ---- RED 18 ----
    #[tokio::test]
    async fn test_tool_plan_create_writes_valid_json() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        let tool = CreatePlanTool::new();
        let result = tool
            .execute(
                json!({
                    "title": "Demo",
                    "goal": "Demonstrate",
                    "phases": sample_phase_args(),
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();
        assert!(result.output.contains("plan.json"));

        let path = plan_path(dir.path());
        assert!(path.exists());
        let plan = read_plan(&path).unwrap();
        assert_eq!(plan.title, "Demo");
        assert_eq!(plan.phases.len(), 2);
        assert_eq!(plan.all_tasks().len(), 2);
        plan.validate().unwrap();
    }

    #[tokio::test]
    async fn test_tool_plan_create_rejects_empty_phases() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        let tool = CreatePlanTool::new();
        let err = tool
            .execute(
                json!({
                    "title": "Demo",
                    "goal": "Demonstrate",
                    "phases": [],
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn test_tool_plan_create_rejects_invalid_dependency() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        let tool = CreatePlanTool::new();
        let err = tool
            .execute(
                json!({
                    "title": "Demo",
                    "goal": "Demonstrate",
                    "phases": [{
                        "id": 1,
                        "title": "P1",
                        "tasks": [{"id": 1, "title": "T1", "depends_on": [99]}]
                    }],
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::Execution(_)));
    }

    // ---- RED 19 ----
    #[tokio::test]
    async fn test_tool_plan_update_task_changes_status() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        // First create a plan.
        CreatePlanTool::new()
            .execute(
                json!({
                    "title": "Demo",
                    "goal": "Demonstrate",
                    "phases": sample_phase_args(),
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        // Now update task 1 → completed.
        UpdateTaskTool::new()
            .execute(
                json!({"task_id": 1, "status": "completed", "outcome": "Done"}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let plan = read_plan(&plan_path(dir.path())).unwrap();
        let task = plan.find_task(PlanTaskId(1)).unwrap();
        assert_eq!(task.status, PlanTaskStatus::Completed);
        assert_eq!(task.outcome.as_deref(), Some("Done"));
    }

    #[tokio::test]
    async fn test_tool_plan_update_task_unknown_id_returns_not_found() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        CreatePlanTool::new()
            .execute(
                json!({
                    "title": "Demo",
                    "goal": "Demonstrate",
                    "phases": sample_phase_args(),
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let err = UpdateTaskTool::new()
            .execute(
                json!({"task_id": 99, "status": "completed"}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_tool_plan_update_task_invalid_status() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        CreatePlanTool::new()
            .execute(
                json!({
                    "title": "Demo",
                    "goal": "Demonstrate",
                    "phases": sample_phase_args(),
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let err = UpdateTaskTool::new()
            .execute(
                json!({"task_id": 1, "status": "wrong_value"}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    // ---- RED 20 ----
    #[tokio::test]
    async fn test_tool_plan_next_task_follows_deps() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        CreatePlanTool::new()
            .execute(
                json!({
                    "title": "Demo",
                    "goal": "Demonstrate",
                    "phases": sample_phase_args(),
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let result = GetNextTaskTool::new()
            .execute(json!({}), &ctx, &mut perms)
            .await
            .unwrap();
        assert_eq!(result.metadata["task_id"], 1);

        UpdateTaskTool::new()
            .execute(
                json!({"task_id": 1, "status": "completed"}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let result = GetNextTaskTool::new()
            .execute(json!({}), &ctx, &mut perms)
            .await
            .unwrap();
        assert_eq!(result.metadata["task_id"], 2);
    }

    #[tokio::test]
    async fn test_tool_plan_next_task_returns_none_when_all_done() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        CreatePlanTool::new()
            .execute(
                json!({
                    "title": "Demo",
                    "goal": "Demonstrate",
                    "phases": sample_phase_args(),
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        for id in [1u32, 2u32] {
            UpdateTaskTool::new()
                .execute(
                    json!({"task_id": id, "status": "completed"}),
                    &ctx,
                    &mut perms,
                )
                .await
                .unwrap();
        }
        let result = GetNextTaskTool::new()
            .execute(json!({}), &ctx, &mut perms)
            .await
            .unwrap();
        assert_eq!(result.metadata["found"], false);
    }

    #[tokio::test]
    async fn test_tool_plan_next_task_when_no_plan_returns_not_found_meta() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        let result = GetNextTaskTool::new()
            .execute(json!({}), &ctx, &mut perms)
            .await
            .unwrap();
        assert_eq!(result.metadata["found"], false);
    }

    // ---- RED 21 ----
    #[tokio::test]
    async fn test_tool_plan_summary_returns_markdown() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        CreatePlanTool::new()
            .execute(
                json!({
                    "title": "Demo",
                    "goal": "Demonstrate",
                    "phases": sample_phase_args(),
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let result = GetPlanSummaryTool::new()
            .execute(json!({}), &ctx, &mut perms)
            .await
            .unwrap();
        assert_eq!(result.metadata["exists"], true);
        assert!(result.output.contains("# Demo"));
        assert!(result.output.contains("Phase 1"));
    }

    #[tokio::test]
    async fn test_tool_plan_summary_when_no_plan() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        let result = GetPlanSummaryTool::new()
            .execute(json!({}), &ctx, &mut perms)
            .await
            .unwrap();
        assert_eq!(result.metadata["exists"], false);
    }

    // ---- AdvancePhase ----
    #[tokio::test]
    async fn test_tool_advance_phase_progresses() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        CreatePlanTool::new()
            .execute(
                json!({
                    "title": "Demo",
                    "goal": "Demonstrate",
                    "phases": sample_phase_args(),
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let result = AdvancePhaseTool::new()
            .execute(json!({}), &ctx, &mut perms)
            .await
            .unwrap();
        assert_eq!(result.metadata["from"], 1);
        assert_eq!(result.metadata["to"], 2);
        assert_eq!(result.metadata["last_phase"], false);

        let plan = read_plan(&plan_path(dir.path())).unwrap();
        assert_eq!(plan.current_phase, PhaseId(2));
        assert_eq!(plan.phases[0].status, PhaseStatus::Completed);
        assert_eq!(plan.phases[1].status, PhaseStatus::InProgress);
    }

    #[tokio::test]
    async fn test_tool_advance_phase_at_last_phase_is_idempotent_terminal() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        CreatePlanTool::new()
            .execute(
                json!({
                    "title": "Demo",
                    "goal": "Demonstrate",
                    "phases": sample_phase_args(),
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        // Advance once → at last phase.
        AdvancePhaseTool::new()
            .execute(json!({}), &ctx, &mut perms)
            .await
            .unwrap();
        // Second advance is a no-op terminal.
        let result = AdvancePhaseTool::new()
            .execute(json!({}), &ctx, &mut perms)
            .await
            .unwrap();
        assert_eq!(result.metadata["last_phase"], true);
    }

    // ---- LogEntryTool ----
    #[tokio::test]
    async fn test_tool_log_finding_writes_findings_json() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        LogEntryTool::new()
            .execute(
                json!({
                    "kind": "finding",
                    "content": "X uses Y",
                    "source": "https://x.example",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let path = findings_path(dir.path());
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("X uses Y"));
        assert!(content.contains("x.example"));
    }

    #[tokio::test]
    async fn test_tool_log_resource_requires_source() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        let err = LogEntryTool::new()
            .execute(
                json!({"kind": "resource", "content": "ADR-016"}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn test_tool_log_error_increments_attempt() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        for _ in 0..3 {
            LogEntryTool::new()
                .execute(
                    json!({
                        "kind": "error",
                        "content": "compile fail",
                        "rationale": "missing import",
                    }),
                    &ctx,
                    &mut perms,
                )
                .await
                .unwrap();
        }
        let path = progress_path(dir.path());
        let content = std::fs::read_to_string(&path).unwrap();
        let progress: ProgressFile = serde_json::from_str(&content).unwrap();
        assert_eq!(progress.errors.len(), 3);
        assert_eq!(progress.errors[0].attempt, 1);
        assert_eq!(progress.errors[1].attempt, 2);
        assert_eq!(progress.errors[2].attempt, 3);
    }

    #[tokio::test]
    async fn test_tool_log_decision_requires_existing_plan() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        let err = LogEntryTool::new()
            .execute(
                json!({"kind": "decision", "content": "use serde"}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_tool_log_decision_appends_to_plan() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        CreatePlanTool::new()
            .execute(
                json!({
                    "title": "Demo",
                    "goal": "Demonstrate",
                    "phases": sample_phase_args(),
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        LogEntryTool::new()
            .execute(
                json!({
                    "kind": "decision",
                    "content": "Use sqlite",
                    "rationale": "simpler than postgres for now",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let plan = read_plan(&plan_path(dir.path())).unwrap();
        assert_eq!(plan.decisions.len(), 1);
        assert_eq!(plan.decisions[0].decision, "Use sqlite");
    }

    #[tokio::test]
    async fn test_tool_log_invalid_kind() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        let err = LogEntryTool::new()
            .execute(
                json!({"kind": "bogus", "content": "x"}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    // ---- Schema validation ----
    #[test]
    fn all_plan_tools_have_valid_schemas() {
        let tools: Vec<Box<dyn Tool>> = vec![
            Box::new(CreatePlanTool::new()),
            Box::new(UpdateTaskTool::new()),
            Box::new(AdvancePhaseTool::new()),
            Box::new(LogEntryTool::new()),
            Box::new(GetPlanSummaryTool::new()),
            Box::new(GetNextTaskTool::new()),
            Box::new(ReplanTool::new()),
        ];
        for t in &tools {
            t.schema().validate().unwrap_or_else(|e| {
                panic!("tool `{}` has invalid schema: {}", t.id(), e)
            });
            assert_eq!(t.category(), ToolCategory::Orchestration);
        }
    }

    #[test]
    fn plan_tool_ids_are_correct() {
        assert_eq!(CreatePlanTool::new().id(), "plan_create");
        assert_eq!(UpdateTaskTool::new().id(), "plan_update_task");
        assert_eq!(AdvancePhaseTool::new().id(), "plan_advance_phase");
        assert_eq!(LogEntryTool::new().id(), "plan_log");
        assert_eq!(GetPlanSummaryTool::new().id(), "plan_summary");
        assert_eq!(GetNextTaskTool::new().id(), "plan_next_task");
        assert_eq!(ReplanTool::new().id(), "plan_replan");
    }

    // ----- T6.1 ReplanTool -----

    #[tokio::test]
    async fn t61_replan_tool_skip_task_persists_changes() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        // Seed a plan with one Pending task.
        CreatePlanTool::new()
            .execute(
                json!({
                    "title": "T61",
                    "goal": "test replan",
                    "phases": sample_phase_args(),
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        // Skip task 1.
        let result = ReplanTool::new()
            .execute(
                json!({"patch": {"kind": "skip_task", "id": 1, "rationale": "Out of scope"}}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();
        assert_eq!(result.metadata["kind"], "skip_task");

        // Re-read plan and confirm task 1 is now Skipped with the rationale.
        let plan = read_plan(&plan_path(dir.path())).unwrap();
        let t = plan.find_task(PlanTaskId(1)).unwrap();
        assert_eq!(t.status, PlanTaskStatus::Skipped);
        assert_eq!(t.outcome.as_deref(), Some("Out of scope"));
    }

    #[tokio::test]
    async fn t61_replan_tool_unknown_task_returns_not_found() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        CreatePlanTool::new()
            .execute(
                json!({
                    "title": "T61",
                    "goal": "test replan",
                    "phases": sample_phase_args(),
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let err = ReplanTool::new()
            .execute(
                json!({"patch": {"kind": "skip_task", "id": 99, "rationale": "x"}}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[tokio::test]
    async fn t61_replan_tool_invalid_patch_shape_returns_invalid_args() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        CreatePlanTool::new()
            .execute(
                json!({
                    "title": "T61",
                    "goal": "test replan",
                    "phases": sample_phase_args(),
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let err = ReplanTool::new()
            .execute(
                json!({"patch": {"kind": "skip_task"}}), // missing id + rationale
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn t61_replan_tool_missing_plan_returns_not_found() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        let err = ReplanTool::new()
            .execute(
                json!({"patch": {"kind": "skip_task", "id": 1, "rationale": "x"}}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[tokio::test]
    async fn t61_replan_tool_cycle_introducing_patch_rolls_back() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        // Plan: t1 → t2 (t2 depends on t1).
        CreatePlanTool::new()
            .execute(
                json!({
                    "title": "T61",
                    "goal": "test rollback",
                    "phases": sample_phase_args(),
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        // ReorderDeps to make t1 depend on t2 → cycle 1↔2.
        let err = ReplanTool::new()
            .execute(
                json!({
                    "patch": {
                        "kind": "reorder_deps",
                        "id": 1,
                        "new_deps": [2]
                    }
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::Execution(_)));

        // Disk state unchanged: t1 still has empty deps.
        let plan = read_plan(&plan_path(dir.path())).unwrap();
        let t1 = plan.find_task(PlanTaskId(1)).unwrap();
        assert!(t1.depends_on.is_empty());
    }

    #[tokio::test]
    async fn t61_replan_tool_records_increment_in_metadata() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        CreatePlanTool::new()
            .execute(
                json!({
                    "title": "T61",
                    "goal": "metadata check",
                    "phases": sample_phase_args(),
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let result = ReplanTool::new()
            .execute(
                json!({"patch": {"kind": "skip_task", "id": 1, "rationale": "skip"}}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();
        // version_counter is part of the metadata so callers can correlate
        // log lines with the saved plan.
        assert!(result.metadata.get("version_counter").is_some());
    }

    // ── T6.1 part 4 — plan_failure_status ─────────────────────────

    async fn create_plan_with_failures(
        dir: &std::path::Path,
        ctx: &ToolContext,
    ) -> Plan {
        let mut perms = PermissionCollector::new();
        // Build a 2-phase plan with 2 tasks.
        let _ = CreatePlanTool::new()
            .execute(
                json!({
                    "title": "Demo",
                    "goal": "Demo",
                    "phases": sample_phase_args(),
                }),
                ctx,
                &mut perms,
            )
            .await
            .unwrap();
        // Read it back, bump failure_counts directly, write it back.
        let path = plan_path(dir);
        let mut plan = read_plan(&path).unwrap();
        // Task 1: 4 failures (above default threshold 3).
        for _ in 0..4 {
            plan.record_failure(theo_domain::identifiers::PlanTaskId(1));
        }
        // Task 2: 1 failure (below threshold).
        plan.record_failure(theo_domain::identifiers::PlanTaskId(2));
        write_plan(&path, &plan).unwrap();
        plan
    }

    #[tokio::test]
    async fn t61_plan_failure_status_id_and_category() {
        let t = PlanFailureStatusTool::new();
        assert_eq!(t.id(), "plan_failure_status");
        assert_eq!(t.category(), ToolCategory::Orchestration);
    }

    #[tokio::test]
    async fn t61_plan_failure_status_schema_validates() {
        let t = PlanFailureStatusTool::new();
        let schema = t.schema();
        schema.validate().unwrap();
        let threshold = schema
            .params
            .iter()
            .find(|p| p.name == "threshold")
            .unwrap();
        assert!(!threshold.required, "threshold must be optional");
    }

    #[tokio::test]
    async fn t61_plan_failure_status_no_plan_returns_zero_stuck_tasks() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();
        let result = PlanFailureStatusTool::new()
            .execute(json!({}), &ctx, &mut perms)
            .await
            .unwrap();
        assert_eq!(result.metadata["stuck_count"], 0);
        assert!(result.title.contains("no plan"));
    }

    #[tokio::test]
    async fn t61_plan_failure_status_default_threshold_is_3() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let _ = create_plan_with_failures(dir.path(), &ctx).await;
        let mut perms = PermissionCollector::new();
        let result = PlanFailureStatusTool::new()
            .execute(json!({}), &ctx, &mut perms)
            .await
            .unwrap();
        // Task 1 (4 failures) is at-or-above threshold 3 → listed.
        // Task 2 (1 failure) is below → omitted.
        assert_eq!(result.metadata["threshold"], 3);
        assert_eq!(result.metadata["stuck_count"], 1);
        let stuck = result.metadata["stuck_tasks"].as_array().unwrap();
        assert_eq!(stuck[0]["task_id"], 1);
        assert_eq!(stuck[0]["failure_count"], 4);
    }

    #[tokio::test]
    async fn t61_plan_failure_status_threshold_1_lists_every_failed_task() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let _ = create_plan_with_failures(dir.path(), &ctx).await;
        let mut perms = PermissionCollector::new();
        let result = PlanFailureStatusTool::new()
            .execute(json!({"threshold": 1}), &ctx, &mut perms)
            .await
            .unwrap();
        // Both task 1 (4) and task 2 (1) reach >= 1 failure.
        assert_eq!(result.metadata["stuck_count"], 2);
    }

    #[tokio::test]
    async fn t61_plan_failure_status_high_threshold_returns_empty() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let _ = create_plan_with_failures(dir.path(), &ctx).await;
        let mut perms = PermissionCollector::new();
        let result = PlanFailureStatusTool::new()
            .execute(json!({"threshold": 99}), &ctx, &mut perms)
            .await
            .unwrap();
        assert_eq!(result.metadata["stuck_count"], 0);
        // The "healthy plan" message points the agent at plan_next_task.
        assert!(result.output.contains("plan_next_task"));
    }

    #[tokio::test]
    async fn t61_plan_failure_status_output_includes_actionable_next_step() {
        // Output must mention plan_replan + the available patch
        // shapes so the agent can self-replan without prompting.
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let _ = create_plan_with_failures(dir.path(), &ctx).await;
        let mut perms = PermissionCollector::new();
        let result = PlanFailureStatusTool::new()
            .execute(json!({}), &ctx, &mut perms)
            .await
            .unwrap();
        assert!(result.output.contains("plan_replan"));
        assert!(result.output.contains("SkipTask"));
        assert!(result.output.contains("EditTask"));
    }

    #[tokio::test]
    async fn t61_plan_failure_status_includes_task_outcome_when_present() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let _ = create_plan_with_failures(dir.path(), &ctx).await;
        // Inject an outcome on task 1 so the rendered output should
        // surface it.
        let path = plan_path(dir.path());
        let mut plan = read_plan(&path).unwrap();
        if let Some(task) = plan.find_task_mut(theo_domain::identifiers::PlanTaskId(1)) {
            task.outcome = Some("compilation failed: undefined symbol foo".into());
        }
        write_plan(&path, &plan).unwrap();
        let mut perms = PermissionCollector::new();
        let result = PlanFailureStatusTool::new()
            .execute(json!({}), &ctx, &mut perms)
            .await
            .unwrap();
        assert!(result.output.contains("last outcome"));
        assert!(result.output.contains("compilation failed"));
    }
