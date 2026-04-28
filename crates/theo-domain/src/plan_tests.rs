//! Sibling test body of `plan.rs` (T5.1 of god-files-2026-07-23-plan.md).


#![cfg(test)]

#![allow(unused_imports)]

use super::*;

    use super::*;

    fn task(id: u32, status: PlanTaskStatus, deps: Vec<u32>) -> PlanTask {
        PlanTask {
            id: PlanTaskId(id),
            title: format!("Task {}", id),
            status,
            files: vec![],
            description: String::new(),
            dod: String::new(),
            depends_on: deps.into_iter().map(PlanTaskId).collect(),
            rationale: String::new(),
            outcome: None,
            assignee: None,
            failure_count: 0,
        }
    }

    fn phase(id: u32, status: PhaseStatus, tasks: Vec<PlanTask>) -> Phase {
        Phase {
            id: PhaseId(id),
            title: format!("Phase {}", id),
            status,
            tasks,
        }
    }

    fn make_plan(phases: Vec<Phase>) -> Plan {
        Plan {
            version: PLAN_FORMAT_VERSION,
            title: "Sample Plan".to_string(),
            goal: "Demonstrate planning".to_string(),
            current_phase: phases.first().map(|p| p.id).unwrap_or(PhaseId(1)),
            phases,
            decisions: vec![],
            created_at: 100,
            updated_at: 100,
            version_counter: 0,
        }
    }

    // ----- RED 1 -----
    #[test]
    fn test_plan_task_id_serde_roundtrip() {
        let id = PlanTaskId(42);
        let json = serde_json::to_string(&id).unwrap();
        let back: PlanTaskId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    // ----- RED 2 -----
    #[test]
    fn test_plan_serde_roundtrip() {
        let plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![
                task(1, PlanTaskStatus::Completed, vec![]),
                task(2, PlanTaskStatus::Pending, vec![1]),
            ],
        )]);
        let json = serde_json::to_string_pretty(&plan).unwrap();
        let back: Plan = serde_json::from_str(&json).unwrap();
        assert_eq!(plan, back);
    }

    // ----- RED 3 -----
    #[test]
    fn test_plan_validate_rejects_duplicate_task_ids() {
        let plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![
                task(1, PlanTaskStatus::Pending, vec![]),
                task(1, PlanTaskStatus::Pending, vec![]),
            ],
        )]);
        let err = plan.validate().unwrap_err();
        assert_eq!(err, PlanValidationError::DuplicateTaskId(PlanTaskId(1)));
    }

    // ----- RED 4 -----
    #[test]
    fn test_plan_validate_rejects_orphan_dependency() {
        let plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![task(1, PlanTaskStatus::Pending, vec![99])],
        )]);
        let err = plan.validate().unwrap_err();
        assert_eq!(
            err,
            PlanValidationError::InvalidDependency {
                task_id: PlanTaskId(1),
                missing_dep: PlanTaskId(99),
            }
        );
    }

    // ----- RED 5 -----
    #[test]
    fn test_plan_validate_rejects_cycle() {
        let plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![
                task(1, PlanTaskStatus::Pending, vec![3]),
                task(2, PlanTaskStatus::Pending, vec![1]),
                task(3, PlanTaskStatus::Pending, vec![2]),
            ],
        )]);
        let err = plan.validate().unwrap_err();
        assert_eq!(err, PlanValidationError::CycleDetected);
    }

    // ----- RED 6 -----
    #[test]
    fn test_plan_topological_order_respects_deps() {
        let plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![
                task(1, PlanTaskStatus::Pending, vec![]),
                task(2, PlanTaskStatus::Pending, vec![]),
                task(3, PlanTaskStatus::Pending, vec![1, 2]),
            ],
        )]);
        let order = plan.topological_order().unwrap();
        let pos = |id: u32| order.iter().position(|t| *t == PlanTaskId(id)).unwrap();
        assert!(pos(1) < pos(3));
        assert!(pos(2) < pos(3));
    }

    // ----- RED 7 -----
    #[test]
    fn test_plan_next_actionable_task_with_deps() {
        let plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![
                task(1, PlanTaskStatus::Pending, vec![]),
                task(2, PlanTaskStatus::Pending, vec![1]),
            ],
        )]);
        let next = plan.next_actionable_task().unwrap();
        assert_eq!(next.id, PlanTaskId(1));

        let plan2 = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![
                task(1, PlanTaskStatus::Completed, vec![]),
                task(2, PlanTaskStatus::Pending, vec![1]),
            ],
        )]);
        let next2 = plan2.next_actionable_task().unwrap();
        assert_eq!(next2.id, PlanTaskId(2));
    }

    // ----- RED 8 -----
    #[test]
    fn test_plan_next_actionable_task_all_done() {
        let plan = make_plan(vec![phase(
            1,
            PhaseStatus::Completed,
            vec![
                task(1, PlanTaskStatus::Completed, vec![]),
                task(2, PlanTaskStatus::Completed, vec![1]),
            ],
        )]);
        assert!(plan.next_actionable_task().is_none());
    }

    // ----- RED 9 -----
    #[test]
    fn test_plan_to_markdown_renders_phases_and_tasks() {
        let plan = make_plan(vec![
            phase(
                1,
                PhaseStatus::Completed,
                vec![task(1, PlanTaskStatus::Completed, vec![])],
            ),
            phase(
                2,
                PhaseStatus::InProgress,
                vec![
                    task(2, PlanTaskStatus::InProgress, vec![1]),
                    task(3, PlanTaskStatus::Pending, vec![2]),
                ],
            ),
        ]);
        let md = plan.to_markdown();
        assert!(md.contains("# Sample Plan"));
        assert!(md.contains("Phase 1"));
        assert!(md.contains("Phase 2"));
        assert!(md.contains("[x]"));
        assert!(md.contains("[>]"));
        assert!(md.contains("[ ]"));
        assert!(md.contains("T1"));
        assert!(md.contains("T2"));
        assert!(md.contains("T3"));
    }

    // ----- RED 10 -----
    #[test]
    fn test_plan_schema_evolution_missing_optional_field() {
        // JSON without `outcome`, `decisions`, `description` etc.
        let json = r#"{
            "version": 1,
            "title": "Compat",
            "goal": "test",
            "current_phase": 1,
            "phases": [{
                "id": 1,
                "title": "Phase 1",
                "status": "in_progress",
                "tasks": [{
                    "id": 1,
                    "title": "Task 1",
                    "status": "pending"
                }]
            }],
            "created_at": 0,
            "updated_at": 0
        }"#;
        let plan: Plan = serde_json::from_str(json).unwrap();
        let task = &plan.phases[0].tasks[0];
        assert!(task.outcome.is_none());
        assert!(task.depends_on.is_empty());
        assert!(task.files.is_empty());
        assert!(plan.decisions.is_empty());
    }

    // ----- RED 11 -----
    #[test]
    fn test_plan_task_status_serde_all_variants() {
        for variant in [
            PlanTaskStatus::Pending,
            PlanTaskStatus::InProgress,
            PlanTaskStatus::Completed,
            PlanTaskStatus::Skipped,
            PlanTaskStatus::Blocked,
            PlanTaskStatus::Failed,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let back: PlanTaskStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, back);
        }
    }

    #[test]
    fn test_plan_task_status_serde_uses_snake_case() {
        let json = serde_json::to_string(&PlanTaskStatus::InProgress).unwrap();
        assert_eq!(json, "\"in_progress\"");
    }

    // ----- RED 12 -----
    #[test]
    fn test_plan_validate_accepts_valid_plan() {
        let plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![
                task(1, PlanTaskStatus::Pending, vec![]),
                task(2, PlanTaskStatus::Pending, vec![1]),
            ],
        )]);
        plan.validate().unwrap();
    }

    // ----- additional sanity checks -----

    #[test]
    fn test_plan_validate_rejects_self_dependency() {
        let plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![task(1, PlanTaskStatus::Pending, vec![1])],
        )]);
        let err = plan.validate().unwrap_err();
        assert_eq!(err, PlanValidationError::SelfDependency(PlanTaskId(1)));
    }

    #[test]
    fn test_plan_validate_rejects_duplicate_phase_ids() {
        let plan = make_plan(vec![
            phase(1, PhaseStatus::InProgress, vec![task(1, PlanTaskStatus::Pending, vec![])]),
            phase(1, PhaseStatus::Pending, vec![task(2, PlanTaskStatus::Pending, vec![])]),
        ]);
        let err = plan.validate().unwrap_err();
        assert_eq!(err, PlanValidationError::DuplicatePhaseId(PhaseId(1)));
    }

    #[test]
    fn test_plan_validate_rejects_invalid_phase_ref() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![task(1, PlanTaskStatus::Pending, vec![])],
        )]);
        plan.current_phase = PhaseId(99);
        let err = plan.validate().unwrap_err();
        assert_eq!(err, PlanValidationError::InvalidPhaseRef(PhaseId(99)));
    }

    #[test]
    fn test_plan_validate_rejects_empty_plan() {
        let plan = make_plan(vec![]);
        let err = plan.validate().unwrap_err();
        assert_eq!(err, PlanValidationError::EmptyPlan);
    }

    #[test]
    fn test_plan_validate_rejects_empty_title() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![task(1, PlanTaskStatus::Pending, vec![])],
        )]);
        plan.title = String::new();
        let err = plan.validate().unwrap_err();
        assert_eq!(err, PlanValidationError::EmptyTitle);
    }

    #[test]
    fn test_topological_order_is_deterministic_with_ties() {
        // Two ready tasks at every step — order must be by ID ascending.
        let plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![
                task(1, PlanTaskStatus::Pending, vec![]),
                task(2, PlanTaskStatus::Pending, vec![]),
                task(3, PlanTaskStatus::Pending, vec![]),
            ],
        )]);
        let order = plan.topological_order().unwrap();
        assert_eq!(order, vec![PlanTaskId(1), PlanTaskId(2), PlanTaskId(3)]);
    }

    #[test]
    fn test_next_actionable_task_skipped_dep_is_satisfied() {
        let plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![
                task(1, PlanTaskStatus::Skipped, vec![]),
                task(2, PlanTaskStatus::Pending, vec![1]),
            ],
        )]);
        let next = plan.next_actionable_task().unwrap();
        assert_eq!(next.id, PlanTaskId(2));
    }

    #[test]
    fn test_next_actionable_task_failed_dep_blocks_downstream() {
        let plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![
                task(1, PlanTaskStatus::Failed, vec![]),
                task(2, PlanTaskStatus::Pending, vec![1]),
            ],
        )]);
        // T2 cannot run because T1 failed (and T1 is not actionable either,
        // it's terminal). Result: None.
        assert!(plan.next_actionable_task().is_none());
    }

    #[test]
    fn test_find_task_round_trip() {
        let plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![task(7, PlanTaskStatus::Pending, vec![])],
        )]);
        assert!(plan.find_task(PlanTaskId(7)).is_some());
        assert!(plan.find_task(PlanTaskId(99)).is_none());
    }

    #[test]
    fn test_task_to_agent_prompt_contains_metadata() {
        let plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![PlanTask {
                id: PlanTaskId(1),
                title: "Implement X".into(),
                status: PlanTaskStatus::Pending,
                files: vec!["src/main.rs".into()],
                description: "Add struct".into(),
                dod: "Tests pass".into(),
                depends_on: vec![],
                rationale: "Because".into(),
                outcome: None,
                assignee: None,
                failure_count: 0,
            }],
        )]);
        let task = &plan.phases[0].tasks[0];
        let prompt = plan.task_to_agent_prompt(task);
        assert!(prompt.contains("T1: Implement X"));
        assert!(prompt.contains("src/main.rs"));
        assert!(prompt.contains("Tests pass"));
        assert!(prompt.contains("Because"));
        assert!(prompt.contains("Demonstrate planning"));
    }

    #[test]
    fn plan_status_terminal_helpers_are_consistent() {
        assert!(PlanTaskStatus::Completed.is_terminal());
        assert!(PlanTaskStatus::Failed.is_terminal());
        assert!(PlanTaskStatus::Skipped.is_terminal());
        assert!(!PlanTaskStatus::Pending.is_terminal());
        assert!(!PlanTaskStatus::InProgress.is_terminal());
        assert!(!PlanTaskStatus::Blocked.is_terminal());

        assert!(PlanTaskStatus::Completed.satisfies_dependency());
        assert!(PlanTaskStatus::Skipped.satisfies_dependency());
        assert!(!PlanTaskStatus::Failed.satisfies_dependency());
        assert!(!PlanTaskStatus::Pending.satisfies_dependency());
    }

    // ---------------------------------------------------------------------
    // T6.1 — PlanPatch + apply_patch
    // ---------------------------------------------------------------------

    use crate::plan_patch::{InsertPosition, PatchError, PlanPatch, TaskEdits};

    #[test]
    fn t61_apply_patch_skip_task_marks_skipped_with_outcome() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![task(1, PlanTaskStatus::Pending, vec![])],
        )]);
        plan.apply_patch(&PlanPatch::SkipTask {
            id: PlanTaskId(1),
            rationale: "Out of scope".into(),
        })
        .unwrap();
        let t = plan.find_task(PlanTaskId(1)).unwrap();
        assert_eq!(t.status, PlanTaskStatus::Skipped);
        assert_eq!(t.outcome.as_deref(), Some("Out of scope"));
    }

    #[test]
    fn t61_apply_patch_skip_unknown_id_returns_not_found() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![task(1, PlanTaskStatus::Pending, vec![])],
        )]);
        let err = plan
            .apply_patch(&PlanPatch::SkipTask {
                id: PlanTaskId(99),
                rationale: "x".into(),
            })
            .unwrap_err();
        assert_eq!(err, PatchError::TaskNotFound(PlanTaskId(99)));
    }

    #[test]
    fn t61_apply_patch_remove_task_with_dependents_rejected() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![
                task(1, PlanTaskStatus::Pending, vec![]),
                task(2, PlanTaskStatus::Pending, vec![1]),
            ],
        )]);
        let err = plan
            .apply_patch(&PlanPatch::RemoveTask { id: PlanTaskId(1) })
            .unwrap_err();
        assert_eq!(err, PatchError::RemoveWouldOrphan(PlanTaskId(1)));
        // Plan unchanged on error (atomicity).
        assert_eq!(plan.all_tasks().len(), 2);
    }

    #[test]
    fn t61_apply_patch_remove_leaf_task_succeeds() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![
                task(1, PlanTaskStatus::Pending, vec![]),
                task(2, PlanTaskStatus::Pending, vec![1]),
            ],
        )]);
        plan.apply_patch(&PlanPatch::RemoveTask { id: PlanTaskId(2) })
            .unwrap();
        assert_eq!(plan.all_tasks().len(), 1);
        assert!(plan.find_task(PlanTaskId(2)).is_none());
    }

    #[test]
    fn t61_apply_patch_add_task_at_end_preserves_validity() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![task(1, PlanTaskStatus::Pending, vec![])],
        )]);
        let new_task = task(2, PlanTaskStatus::Pending, vec![1]);
        plan.apply_patch(&PlanPatch::AddTask {
            phase: PhaseId(1),
            task: new_task,
            position: InsertPosition::End,
        })
        .unwrap();
        assert_eq!(plan.phases[0].tasks.len(), 2);
        assert_eq!(plan.phases[0].tasks[1].id, PlanTaskId(2));
    }

    #[test]
    fn t61_apply_patch_add_task_with_invalid_dep_rolls_back() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![task(1, PlanTaskStatus::Pending, vec![])],
        )]);
        let bad_task = task(2, PlanTaskStatus::Pending, vec![99]); // dep doesn't exist
        let err = plan
            .apply_patch(&PlanPatch::AddTask {
                phase: PhaseId(1),
                task: bad_task,
                position: InsertPosition::End,
            })
            .unwrap_err();
        match err {
            PatchError::Validation(PlanValidationError::InvalidDependency { .. }) => {}
            other => panic!("expected InvalidDependency: {other:?}"),
        }
        // Plan unchanged: only original task survives.
        assert_eq!(plan.all_tasks().len(), 1);
    }

    #[test]
    fn t61_apply_patch_add_task_unknown_phase_returns_phase_not_found() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![task(1, PlanTaskStatus::Pending, vec![])],
        )]);
        let err = plan
            .apply_patch(&PlanPatch::AddTask {
                phase: PhaseId(99),
                task: task(2, PlanTaskStatus::Pending, vec![]),
                position: InsertPosition::End,
            })
            .unwrap_err();
        assert_eq!(err, PatchError::PhaseNotFound(PhaseId(99)));
    }

    #[test]
    fn t61_apply_patch_add_task_after_anchor_inserts_correctly() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![
                task(1, PlanTaskStatus::Pending, vec![]),
                task(3, PlanTaskStatus::Pending, vec![1]),
            ],
        )]);
        plan.apply_patch(&PlanPatch::AddTask {
            phase: PhaseId(1),
            task: task(2, PlanTaskStatus::Pending, vec![1]),
            position: InsertPosition::AfterTask { id: PlanTaskId(1) },
        })
        .unwrap();
        let ids: Vec<u32> = plan.phases[0].tasks.iter().map(|t| t.id.0).collect();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[test]
    fn t61_apply_patch_edit_task_changes_only_specified_fields() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![task(1, PlanTaskStatus::Pending, vec![])],
        )]);
        plan.apply_patch(&PlanPatch::EditTask {
            id: PlanTaskId(1),
            edits: TaskEdits {
                title: Some("Renamed".into()),
                status: Some(PlanTaskStatus::Blocked),
                ..Default::default()
            },
        })
        .unwrap();
        let t = plan.find_task(PlanTaskId(1)).unwrap();
        assert_eq!(t.title, "Renamed");
        assert_eq!(t.status, PlanTaskStatus::Blocked);
        // Untouched fields preserved.
        assert!(t.dod.is_empty());
    }

    #[test]
    fn t61_apply_patch_edit_empty_returns_empty_error() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![task(1, PlanTaskStatus::Pending, vec![])],
        )]);
        let err = plan
            .apply_patch(&PlanPatch::EditTask {
                id: PlanTaskId(1),
                edits: TaskEdits::default(),
            })
            .unwrap_err();
        assert_eq!(err, PatchError::Empty);
    }

    #[test]
    fn t61_apply_patch_reorder_deps_introducing_cycle_rolls_back() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![
                task(1, PlanTaskStatus::Pending, vec![]),
                task(2, PlanTaskStatus::Pending, vec![1]),
            ],
        )]);
        let err = plan
            .apply_patch(&PlanPatch::ReorderDeps {
                id: PlanTaskId(1),
                new_deps: vec![PlanTaskId(2)], // creates cycle 1→2→1
            })
            .unwrap_err();
        assert_eq!(err, PatchError::Validation(PlanValidationError::CycleDetected));
        // Plan unchanged.
        let t1 = plan.find_task(PlanTaskId(1)).unwrap();
        assert!(t1.depends_on.is_empty());
    }

    // ---------------------------------------------------------------------
    // T7.1 — Multi-agent claim/release + version_counter
    // ---------------------------------------------------------------------

    #[test]
    fn t71_claim_succeeds_when_unclaimed() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![task(1, PlanTaskStatus::Pending, vec![])],
        )]);
        let r = plan.claim_task(PlanTaskId(1), "agent-A");
        assert_eq!(r, ClaimResult::Claimed);
        assert_eq!(
            plan.find_task(PlanTaskId(1)).unwrap().assignee.as_deref(),
            Some("agent-A")
        );
        assert!(plan.version_counter > 0);
    }

    #[test]
    fn t71_claim_already_held_returns_already_claimed() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![task(1, PlanTaskStatus::Pending, vec![])],
        )]);
        plan.claim_task(PlanTaskId(1), "agent-A");
        let r = plan.claim_task(PlanTaskId(1), "agent-B");
        match r {
            ClaimResult::AlreadyClaimed { by } => assert_eq!(by, "agent-A"),
            other => panic!("expected AlreadyClaimed, got {other:?}"),
        }
    }

    #[test]
    fn t71_claim_self_is_idempotent() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![task(1, PlanTaskStatus::Pending, vec![])],
        )]);
        let _ = plan.claim_task(PlanTaskId(1), "agent-A");
        let v_after_first = plan.version_counter;
        let r = plan.claim_task(PlanTaskId(1), "agent-A");
        assert_eq!(r, ClaimResult::Claimed);
        // Second claim by same agent does NOT bump counter (no mutation).
        assert_eq!(plan.version_counter, v_after_first);
    }

    #[test]
    fn t71_claim_unknown_id_returns_not_found() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![task(1, PlanTaskStatus::Pending, vec![])],
        )]);
        let r = plan.claim_task(PlanTaskId(99), "agent-A");
        assert_eq!(r, ClaimResult::NotFound);
    }

    #[test]
    fn t71_claim_terminal_task_returns_terminal() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![task(1, PlanTaskStatus::Completed, vec![])],
        )]);
        let r = plan.claim_task(PlanTaskId(1), "agent-A");
        assert_eq!(r, ClaimResult::Terminal);
    }

    #[test]
    fn t71_release_clears_assignee_when_owner_matches() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![task(1, PlanTaskStatus::Pending, vec![])],
        )]);
        plan.claim_task(PlanTaskId(1), "agent-A");
        let v_before_release = plan.version_counter;
        assert!(plan.release_task(PlanTaskId(1), "agent-A"));
        assert!(plan.find_task(PlanTaskId(1)).unwrap().assignee.is_none());
        assert!(plan.version_counter > v_before_release);
    }

    #[test]
    fn t71_release_by_different_agent_is_noop() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![task(1, PlanTaskStatus::Pending, vec![])],
        )]);
        plan.claim_task(PlanTaskId(1), "agent-A");
        let v_before = plan.version_counter;
        assert!(!plan.release_task(PlanTaskId(1), "agent-B"));
        assert_eq!(
            plan.find_task(PlanTaskId(1)).unwrap().assignee.as_deref(),
            Some("agent-A")
        );
        assert_eq!(plan.version_counter, v_before);
    }

    #[test]
    fn t71_release_unknown_or_unclaimed_is_noop() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![task(1, PlanTaskStatus::Pending, vec![])],
        )]);
        assert!(!plan.release_task(PlanTaskId(99), "agent-A"));
        assert!(!plan.release_task(PlanTaskId(1), "agent-A"));
    }

    #[test]
    fn t71_next_unclaimed_actionable_skips_assigned_tasks() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![
                task(1, PlanTaskStatus::Pending, vec![]),
                task(2, PlanTaskStatus::Pending, vec![]),
            ],
        )]);
        plan.claim_task(PlanTaskId(1), "agent-A");
        let next = plan.next_unclaimed_actionable_task().unwrap();
        assert_eq!(next.id, PlanTaskId(2));
    }

    #[test]
    fn t71_next_unclaimed_returns_none_when_all_claimed() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![
                task(1, PlanTaskStatus::Pending, vec![]),
                task(2, PlanTaskStatus::Pending, vec![1]),
            ],
        )]);
        plan.claim_task(PlanTaskId(1), "a");
        // T2 has unsatisfied dep (T1 is in_progress not completed) → None
        assert!(plan.next_unclaimed_actionable_task().is_none());
    }

    #[test]
    fn t71_version_counter_serde_roundtrip() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![task(1, PlanTaskStatus::Pending, vec![])],
        )]);
        plan.version_counter = 42;
        let json = serde_json::to_string(&plan).unwrap();
        let back: Plan = serde_json::from_str(&json).unwrap();
        assert_eq!(back.version_counter, 42);
    }

    #[test]
    fn t71_legacy_plan_without_version_counter_loads_with_default() {
        let json = r#"{
            "version": 1,
            "title": "Legacy",
            "goal": "test",
            "current_phase": 1,
            "phases": [{
                "id": 1,
                "title": "P",
                "status": "in_progress",
                "tasks": [{"id": 1, "title": "T", "status": "pending"}]
            }],
            "created_at": 0,
            "updated_at": 0
        }"#;
        let plan: Plan = serde_json::from_str(json).unwrap();
        assert_eq!(plan.version_counter, 0);
        assert!(plan.phases[0].tasks[0].assignee.is_none());
    }

    /// Backward-compat regression guard for the `sota-tier1-tier2-plan`
    /// global DoD: a plan.json written before T6.1 (no `failure_count` on
    /// PlanTask) and before T7.1 (no `assignee` on PlanTask, no
    /// `version_counter` on Plan) MUST load under the current schema and
    /// every new field MUST default deterministically. Locks the
    /// `#[serde(default)]` contract on the bumped types.
    #[test]
    fn pre_t6_t7_legacy_plan_json_loads_with_all_new_fields_at_default() {
        // Wire shape from a hypothetical theo build at HEAD~ before T6.1
        // and T7.1. Only canonical pre-bump fields included.
        let json = r#"{
            "version": 1,
            "title": "pre-bump",
            "goal": "regression guard",
            "current_phase": 1,
            "phases": [
                {
                    "id": 1,
                    "title": "Phase 1",
                    "status": "in_progress",
                    "tasks": [
                        {"id": 1, "title": "Task 1", "status": "pending"},
                        {"id": 2, "title": "Task 2", "status": "completed",
                         "depends_on": [1]}
                    ]
                }
            ],
            "created_at": 1700000000,
            "updated_at": 1700000000
        }"#;
        let plan: Plan = serde_json::from_str(json).expect(
            "pre-T6/T7 plan JSON must remain loadable under the current schema",
        );
        // T7.1 — version_counter defaults to 0 on legacy plans.
        assert_eq!(plan.version_counter, 0);
        // T6.1 + T7.1 — every task gets defaulted new fields.
        for task in plan.phases.iter().flat_map(|p| p.tasks.iter()) {
            assert!(
                task.assignee.is_none(),
                "T7.1 assignee must default to None on legacy plans"
            );
            assert_eq!(
                task.failure_count, 0,
                "T6.1 failure_count must default to 0 on legacy plans"
            );
        }
        // Plan must still validate end-to-end.
        plan.validate().expect("legacy plan must still pass validate()");
    }

    #[test]
    fn t71_claim_result_is_owned_predicate() {
        assert!(ClaimResult::Claimed.is_owned());
        assert!(!ClaimResult::NotFound.is_owned());
        assert!(!ClaimResult::Terminal.is_owned());
        assert!(!ClaimResult::AlreadyClaimed { by: "x".into() }.is_owned());
    }

    #[test]
    fn t61_apply_patch_atomicity_rejected_patch_leaves_plan_intact() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![
                task(1, PlanTaskStatus::Pending, vec![]),
                task(2, PlanTaskStatus::Pending, vec![1]),
            ],
        )]);
        let snapshot_before = plan.clone();
        let _ = plan.apply_patch(&PlanPatch::ReorderDeps {
            id: PlanTaskId(1),
            new_deps: vec![PlanTaskId(2)],
        });
        assert_eq!(plan, snapshot_before);
    }

    // ── T6.1 — failure_count + auto-replan trigger helpers ────────

    fn plan_with_two_tasks() -> Plan {
        make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![
                task(1, PlanTaskStatus::Pending, vec![]),
                task(2, PlanTaskStatus::Pending, vec![1]),
            ],
        )])
    }

    #[test]
    fn t61_record_failure_increments_count_and_returns_new_value() {
        let mut plan = plan_with_two_tasks();
        assert_eq!(plan.record_failure(PlanTaskId(1)), Some(1));
        assert_eq!(plan.record_failure(PlanTaskId(1)), Some(2));
        assert_eq!(plan.record_failure(PlanTaskId(1)), Some(3));
        let task = plan
            .all_tasks()
            .into_iter()
            .find(|t| t.id == PlanTaskId(1))
            .unwrap();
        assert_eq!(task.failure_count, 3);
    }

    #[test]
    fn t61_record_failure_returns_none_for_unknown_task() {
        let mut plan = plan_with_two_tasks();
        assert!(plan.record_failure(PlanTaskId(999)).is_none());
    }

    #[test]
    fn t61_record_failure_bumps_version_counter() {
        let mut plan = plan_with_two_tasks();
        let v_before = plan.version_counter;
        plan.record_failure(PlanTaskId(1));
        assert_eq!(
            plan.version_counter,
            v_before + 1,
            "record_failure must bump version_counter for CAS-aware persisters"
        );
    }

    #[test]
    fn t61_record_failure_does_not_change_task_status() {
        // Some failures should keep the task pending for retry
        // (network glitch); others should mark Failed (logic bug).
        // Decoupling lets the caller choose.
        let mut plan = plan_with_two_tasks();
        plan.record_failure(PlanTaskId(1));
        let task = plan
            .all_tasks()
            .into_iter()
            .find(|t| t.id == PlanTaskId(1))
            .unwrap();
        assert_eq!(
            task.status,
            PlanTaskStatus::Pending,
            "record_failure must not auto-mark Failed"
        );
    }

    #[test]
    fn t61_reset_failure_count_zeroes_and_returns_true_when_present() {
        let mut plan = plan_with_two_tasks();
        plan.record_failure(PlanTaskId(1));
        plan.record_failure(PlanTaskId(1));
        assert!(plan.reset_failure_count(PlanTaskId(1)));
        let task = plan
            .all_tasks()
            .into_iter()
            .find(|t| t.id == PlanTaskId(1))
            .unwrap();
        assert_eq!(task.failure_count, 0);
    }

    #[test]
    fn t61_reset_failure_count_returns_false_for_unknown_task() {
        let mut plan = plan_with_two_tasks();
        assert!(!plan.reset_failure_count(PlanTaskId(999)));
    }

    #[test]
    fn t61_reset_failure_count_bumps_version_only_when_count_changed() {
        // Idempotency-like behaviour: resetting an already-zero count
        // should not bump the version (saves churn for CAS persisters
        // that re-write whenever version changes).
        let mut plan = plan_with_two_tasks();
        let v_before = plan.version_counter;
        // Task starts at failure_count=0; reset is a no-op.
        plan.reset_failure_count(PlanTaskId(1));
        assert_eq!(
            plan.version_counter, v_before,
            "no-op reset must not bump version_counter"
        );
        // Now record a failure and reset — version should bump twice.
        plan.record_failure(PlanTaskId(1)); // +1
        plan.reset_failure_count(PlanTaskId(1)); // +1
        assert_eq!(plan.version_counter, v_before + 2);
    }

    #[test]
    fn t61_tasks_exceeding_threshold_lists_only_offenders_in_order() {
        let mut plan = make_plan(vec![phase(
            1,
            PhaseStatus::InProgress,
            vec![
                task(1, PlanTaskStatus::Pending, vec![]),
                task(2, PlanTaskStatus::Pending, vec![]),
                task(3, PlanTaskStatus::Pending, vec![]),
            ],
        )]);
        for _ in 0..3 {
            plan.record_failure(PlanTaskId(1));
        }
        // Task 2: 2 failures (below threshold 3)
        plan.record_failure(PlanTaskId(2));
        plan.record_failure(PlanTaskId(2));
        for _ in 0..5 {
            plan.record_failure(PlanTaskId(3));
        }
        let offenders = plan.tasks_exceeding_failure_threshold(3);
        assert_eq!(offenders, vec![PlanTaskId(1), PlanTaskId(3)]);
    }

    #[test]
    fn t61_tasks_exceeding_threshold_zero_returns_empty_not_everyone() {
        // Threshold 0 would technically match every task. Returning
        // empty here is a safety guard so a misconfigured threshold
        // doesn't trigger replan on every fresh task.
        let plan = plan_with_two_tasks();
        assert!(plan.tasks_exceeding_failure_threshold(0).is_empty());
    }

    #[test]
    fn t61_tasks_exceeding_threshold_high_value_returns_empty() {
        let mut plan = plan_with_two_tasks();
        plan.record_failure(PlanTaskId(1));
        // Threshold higher than any task's count → no offenders.
        assert!(plan.tasks_exceeding_failure_threshold(99).is_empty());
    }

    #[test]
    fn t61_failure_count_round_trips_through_serde() {
        // Critical for plan.json persistence — the field must
        // serialize/deserialize with the rest of PlanTask so
        // failure history survives across agent runs.
        let mut plan = plan_with_two_tasks();
        plan.record_failure(PlanTaskId(1));
        plan.record_failure(PlanTaskId(1));
        let json = serde_json::to_string(&plan).unwrap();
        let back: Plan = serde_json::from_str(&json).unwrap();
        let task = back
            .all_tasks()
            .into_iter()
            .find(|t| t.id == PlanTaskId(1))
            .unwrap();
        assert_eq!(task.failure_count, 2);
    }

    #[test]
    fn t61_failure_count_omitted_in_legacy_json_defaults_to_zero() {
        // Backwards-compat: a plan.json written BEFORE T6.1 has no
        // failure_count field. Loading it must succeed and default
        // to 0 for every task. Otherwise upgrades would break every
        // agent's persisted plan.
        let legacy = r#"{
            "version": 1,
            "title": "old plan",
            "goal": "",
            "current_phase": 1,
            "phases": [
                {
                    "id": 1,
                    "title": "p1",
                    "status": "in_progress",
                    "tasks": [
                        {
                            "id": 1,
                            "title": "t1",
                            "status": "pending"
                        }
                    ]
                }
            ],
            "decisions": [],
            "created_at": 0,
            "updated_at": 0
        }"#;
        let plan: Plan = serde_json::from_str(legacy).expect("legacy plan must load");
        let task = &plan.phases[0].tasks[0];
        assert_eq!(task.failure_count, 0, "missing field must default to 0");
    }
