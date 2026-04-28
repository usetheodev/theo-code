//! Shared test fixtures for plan_*_tests.rs sibling files (T3.4 split).
#![cfg(test)]
#![allow(unused_imports)]

use super::*;
use super::*;

pub(super) fn task(id: u32, status: PlanTaskStatus, deps: Vec<u32>) -> PlanTask {
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

pub(super) fn phase(id: u32, status: PhaseStatus, tasks: Vec<PlanTask>) -> Phase {
    Phase {
        id: PhaseId(id),
        title: format!("Phase {}", id),
        status,
        tasks,
    }
}

pub(super) fn make_plan(phases: Vec<Phase>) -> Plan {
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

pub(super) fn plan_with_two_tasks() -> Plan {
    make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![
            task(1, PlanTaskStatus::Pending, vec![]),
            task(2, PlanTaskStatus::Pending, vec![1]),
        ],
    )])
}

