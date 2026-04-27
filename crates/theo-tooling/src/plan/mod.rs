//! Plan tools — schema-validated planning surface for the LLM.
//!
//! Six tools (Meeting D3) backed by `theo_domain::plan::Plan` and a
//! canonical JSON file at `<project>/.theo/plans/plan.json`:
//!
//! | Tool | ID | Purpose |
//! |------|----|---------|
//! | `CreatePlanTool`     | `plan_create`        | Author a plan from JSON args. |
//! | `UpdateTaskTool`     | `plan_update_task`   | Change a task's status/outcome. |
//! | `AdvancePhaseTool`   | `plan_advance_phase` | Mark current phase complete, move to next. |
//! | `LogEntryTool`       | `plan_log`           | Append finding/error/decision to side files. |
//! | `GetPlanSummaryTool` | `plan_summary`       | Return `Plan::to_markdown()` view. |
//! | `GetNextTaskTool`    | `plan_next_task`     | Return next actionable task via toposort. |
//!
//! All filesystem writes go through atomic temp+rename. Every tool validates
//! the plan after mutating it — invalid plans never reach disk.
//!
//! `theo-tooling` cannot depend on `theo-agent-runtime`, so the IO helpers
//! here mirror (without sharing) the implementation in `plan_store.rs`.
//! See `docs/plans/sota-planning-system.md` Fase 3.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use theo_domain::clock::now_millis;
use theo_domain::error::ToolError;
use theo_domain::identifiers::{PhaseId, PlanTaskId};
use theo_domain::plan::{
    PLAN_FORMAT_VERSION, Phase, PhaseStatus, Plan, PlanDecision, PlanError, PlanTask,
    PlanTaskStatus,
};
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

// ---------------------------------------------------------------------------
// Disk helpers (private to this module — mirror of plan_store)
// ---------------------------------------------------------------------------

/// Returns `<project>/.theo/plans/plan.json`.
fn plan_path(project_dir: &Path) -> PathBuf {
    project_dir.join(".theo").join("plans").join("plan.json")
}

/// Returns `<project>/.theo/plans/findings.json`.
fn findings_path(project_dir: &Path) -> PathBuf {
    project_dir
        .join(".theo")
        .join("plans")
        .join("findings.json")
}

/// Returns `<project>/.theo/plans/progress.json`.
fn progress_path(project_dir: &Path) -> PathBuf {
    project_dir
        .join(".theo")
        .join("plans")
        .join("progress.json")
}

fn map_plan_err(prefix: &str, err: PlanError) -> ToolError {
    match err {
        PlanError::Io(e) => ToolError::Io(e),
        other => ToolError::Execution(format!("{prefix}: {other}")),
    }
}

fn read_plan(path: &Path) -> Result<Plan, ToolError> {
    let content = std::fs::read_to_string(path).map_err(ToolError::Io)?;
    let plan: Plan = serde_json::from_str(&content)
        .map_err(|e| ToolError::Execution(format!("invalid plan JSON: {e}")))?;
    if plan.version > PLAN_FORMAT_VERSION {
        return Err(ToolError::Execution(format!(
            "unsupported plan version: found {found}, max supported {max}",
            found = plan.version,
            max = PLAN_FORMAT_VERSION
        )));
    }
    plan.validate()
        .map_err(|e| ToolError::Execution(format!("plan validation failed: {e}")))?;
    Ok(plan)
}

fn write_plan(path: &Path, plan: &Plan) -> Result<(), ToolError> {
    plan.validate()
        .map_err(|e| ToolError::Execution(format!("plan validation failed: {e}")))?;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent).map_err(ToolError::Io)?;
    }
    let json = serde_json::to_string_pretty(plan)
        .map_err(|e| ToolError::Execution(format!("serialize plan: {e}")))?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, json.as_bytes()).map_err(ToolError::Io)?;
    std::fs::rename(&temp, path).map_err(ToolError::Io)?;
    Ok(())
}

fn require_string(args: &Value, key: &str) -> Result<String, ToolError> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| ToolError::InvalidArgs(format!("missing string `{key}`")))
}

fn optional_string(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn require_u32(args: &Value, key: &str) -> Result<u32, ToolError> {
    args.get(key)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| {
            ToolError::InvalidArgs(format!(
                "missing or non-u32 integer `{key}`"
            ))
        })
}

// ---------------------------------------------------------------------------
// Argument parsing helpers — JSON schema-shaped DTOs
// ---------------------------------------------------------------------------

/// Inline DTO shaped to match how the LLM is expected to author phases.
/// Decoupled from `theo_domain::plan::Phase` so tool args stay stable
/// across schema evolutions.
#[derive(Debug, Deserialize, Serialize)]
struct PhaseArg {
    pub id: u32,
    pub title: String,
    #[serde(default)]
    pub tasks: Vec<TaskArg>,
}

#[derive(Debug, Deserialize, Serialize)]
struct TaskArg {
    pub id: u32,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub dod: String,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<u32>,
    #[serde(default)]
    pub rationale: String,
}

fn build_plan_from_args(
    title: String,
    goal: String,
    phases: Vec<PhaseArg>,
) -> Result<Plan, ToolError> {
    if phases.is_empty() {
        return Err(ToolError::InvalidArgs(
            "plan must have at least one phase".into(),
        ));
    }
    let current_phase = PhaseId(phases[0].id);
    let domain_phases: Vec<Phase> = phases
        .into_iter()
        .map(|p| Phase {
            id: PhaseId(p.id),
            title: p.title,
            status: PhaseStatus::Pending,
            tasks: p
                .tasks
                .into_iter()
                .map(|t| PlanTask {
                    id: PlanTaskId(t.id),
                    title: t.title,
                    status: PlanTaskStatus::Pending,
                    files: t.files,
                    description: t.description,
                    dod: t.dod,
                    depends_on: t.depends_on.into_iter().map(PlanTaskId).collect(),
                    rationale: t.rationale,
                    outcome: None,
                    assignee: None,
                    failure_count: 0,
                })
                .collect(),
        })
        .collect();

    let now = now_millis();
    Ok(Plan {
        version: PLAN_FORMAT_VERSION,
        title,
        goal,
        current_phase,
        phases: domain_phases,
        decisions: vec![],
        created_at: now,
        updated_at: now,
        version_counter: 0,
    })
}

fn parse_status(s: &str) -> Result<PlanTaskStatus, ToolError> {
    match s {
        "pending" => Ok(PlanTaskStatus::Pending),
        "in_progress" => Ok(PlanTaskStatus::InProgress),
        "completed" => Ok(PlanTaskStatus::Completed),
        "skipped" => Ok(PlanTaskStatus::Skipped),
        "blocked" => Ok(PlanTaskStatus::Blocked),
        "failed" => Ok(PlanTaskStatus::Failed),
        other => Err(ToolError::InvalidArgs(format!(
            "invalid status `{other}`. Use one of: pending, in_progress, completed, skipped, blocked, failed"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Existing PlanExitTool (kept for backward compat — registered separately)
// ---------------------------------------------------------------------------

pub struct PlanExitTool;

impl Default for PlanExitTool {
    fn default() -> Self {
        Self::new()
    }
}

impl PlanExitTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for PlanExitTool {
    fn id(&self) -> &str {
        "plan_exit"
    }

    fn description(&self) -> &str {
        "Exit plan mode (experimental)"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new()
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Utility
    }

    async fn execute(
        &self,
        _args: Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput {
            title: "Plan mode exit".to_string(),
            output: "Switching to build agent...".to_string(),
            metadata: json!({}),
            attachments: None,
            llm_suffix: None,
        })
    }
}

// ---------------------------------------------------------------------------
// CreatePlanTool — `plan_create`
// ---------------------------------------------------------------------------

pub struct CreatePlanTool;
impl Default for CreatePlanTool {
    fn default() -> Self {
        Self
    }
}
impl CreatePlanTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for CreatePlanTool {
    fn id(&self) -> &str {
        "plan_create"
    }

    fn description(&self) -> &str {
        "Create a schema-validated plan and persist it to .theo/plans/plan.json. \
         Pass a title, goal, and an array of phases each containing tasks. \
         Tasks may declare `depends_on` (array of task IDs) so the runtime can \
         execute them in dependency order. Use this BEFORE starting work — the \
         pilot loop reads this file to drive iteration. Overwrites any existing \
         plan.json. Example: plan_create({title: 'Add auth', goal: 'JWT login', \
         phases: [{id: 1, title: 'Schema', tasks: [{id: 1, title: 'User model'}]}]})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "title".into(),
                    param_type: "string".into(),
                    description: "Plan title (required, non-empty)".into(),
                    required: true,
                },
                ToolParam {
                    name: "goal".into(),
                    param_type: "string".into(),
                    description: "What the plan achieves".into(),
                    required: true,
                },
                ToolParam {
                    name: "phases".into(),
                    param_type: "array".into(),
                    description:
                        "Array of phase objects: {id: u32, title: string, tasks: [...]}. \
                         Each task: {id: u32, title: string, description?, dod?, files?, depends_on?, rationale?}."
                            .into(),
                    required: true,
                },
            ],
            input_examples: vec![json!({
                "title": "Add JWT auth",
                "goal": "Users can log in via JWT bearer token",
                "phases": [{
                    "id": 1,
                    "title": "Schema",
                    "tasks": [{
                        "id": 1,
                        "title": "Add user model",
                        "files": ["src/models/user.rs"],
                        "dod": "User struct compiles + tests pass"
                    }]
                }, {
                    "id": 2,
                    "title": "Routes",
                    "tasks": [{
                        "id": 2,
                        "title": "POST /login",
                        "depends_on": [1],
                        "dod": "200 OK with token; 401 on bad creds"
                    }]
                }]
            })],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Orchestration
    }

    async fn execute(
        &self,
        args: Value,
        ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let title = require_string(&args, "title")?;
        let goal = require_string(&args, "goal")?;
        let phases_value = args
            .get("phases")
            .ok_or_else(|| ToolError::InvalidArgs("missing array `phases`".into()))?;
        let phases: Vec<PhaseArg> = serde_json::from_value(phases_value.clone())
            .map_err(|e| ToolError::InvalidArgs(format!("invalid `phases`: {e}")))?;

        let plan = build_plan_from_args(title.clone(), goal.clone(), phases)?;
        let path = plan_path(&ctx.project_dir);
        write_plan(&path, &plan)?;

        let task_count = plan.all_tasks().len();
        Ok(ToolOutput {
            title: format!("Plan created: {} ({} tasks)", plan.title, task_count),
            output: format!(
                "Plan saved to {}\nPhases: {}\nTasks: {}",
                path.display(),
                plan.phases.len(),
                task_count
            ),
            metadata: json!({
                "type": "plan_create",
                "path": path.display().to_string(),
                "phases": plan.phases.len(),
                "tasks": task_count,
            }),
            attachments: None,
            llm_suffix: None,
        })
    }
}

// ---------------------------------------------------------------------------
// UpdateTaskTool — `plan_update_task`
// ---------------------------------------------------------------------------

pub struct UpdateTaskTool;
impl Default for UpdateTaskTool {
    fn default() -> Self {
        Self
    }
}
impl UpdateTaskTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for UpdateTaskTool {
    fn id(&self) -> &str {
        "plan_update_task"
    }

    fn description(&self) -> &str {
        "Update the status (and optionally the outcome summary) of a single task in \
         the persisted plan. Pass `task_id` (u32) and `status` (one of: pending, \
         in_progress, completed, skipped, blocked, failed). Optional `outcome` is a \
         free-form summary of what happened. Persists atomically to plan.json. \
         Example: plan_update_task({task_id: 3, status: 'completed', outcome: 'Tests passed'})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "task_id".into(),
                    param_type: "integer".into(),
                    description: "Task ID (u32) within the current plan".into(),
                    required: true,
                },
                ToolParam {
                    name: "status".into(),
                    param_type: "string".into(),
                    description:
                        "New status: pending, in_progress, completed, skipped, blocked, failed"
                            .into(),
                    required: true,
                },
                ToolParam {
                    name: "outcome".into(),
                    param_type: "string".into(),
                    description: "Optional summary of what happened during the task".into(),
                    required: false,
                },
            ],
            input_examples: vec![json!({
                "task_id": 3,
                "status": "completed",
                "outcome": "All 12 unit tests green; coverage held at 87%."
            })],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Orchestration
    }

    async fn execute(
        &self,
        args: Value,
        ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let task_id = require_u32(&args, "task_id")?;
        let status_str = require_string(&args, "status")?;
        let status = parse_status(&status_str)?;
        let outcome = optional_string(&args, "outcome");

        let path = plan_path(&ctx.project_dir);
        let mut plan = read_plan(&path)?;

        let id = PlanTaskId(task_id);
        let task = plan
            .find_task_mut(id)
            .ok_or_else(|| ToolError::NotFound(format!("task {id} not found in plan")))?;
        task.status = status;
        if let Some(o) = outcome.clone() {
            task.outcome = Some(o);
        }
        plan.updated_at = now_millis();

        write_plan(&path, &plan)?;

        Ok(ToolOutput {
            title: format!("Task {} → {}", id, status_str),
            output: format!("Task {id} status set to {status_str}"),
            metadata: json!({
                "type": "plan_update_task",
                "task_id": task_id,
                "status": status_str,
                "outcome": outcome,
            }),
            attachments: None,
            llm_suffix: None,
        })
    }
}

// ---------------------------------------------------------------------------
// AdvancePhaseTool — `plan_advance_phase`
// ---------------------------------------------------------------------------

pub struct AdvancePhaseTool;
impl Default for AdvancePhaseTool {
    fn default() -> Self {
        Self
    }
}
impl AdvancePhaseTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for AdvancePhaseTool {
    fn id(&self) -> &str {
        "plan_advance_phase"
    }

    fn description(&self) -> &str {
        "Mark the current phase as Completed and move `current_phase` to the \
         next phase (by ascending phase ID). No-op when already at the last phase \
         (returns success with `last_phase: true`). Persists atomically to \
         plan.json. Use this AFTER all tasks in the current phase reach a \
         terminal status. Example: plan_advance_phase({})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new()
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Orchestration
    }

    async fn execute(
        &self,
        _args: Value,
        ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let path = plan_path(&ctx.project_dir);
        let mut plan = read_plan(&path)?;

        let current_id = plan.current_phase;
        // Sort phase IDs ascending; pick the next id strictly greater than current.
        let mut ids: Vec<PhaseId> = plan.phases.iter().map(|p| p.id).collect();
        ids.sort();
        let next_phase = ids.into_iter().find(|id| *id > current_id);

        if let Some(curr) = plan.phases.iter_mut().find(|p| p.id == current_id) {
            curr.status = PhaseStatus::Completed;
        }
        let last_phase = next_phase.is_none();
        let new_current = next_phase.unwrap_or(current_id);
        if let Some(next) = plan.phases.iter_mut().find(|p| p.id == new_current)
            && next.status == PhaseStatus::Pending
        {
            next.status = PhaseStatus::InProgress;
        }
        plan.current_phase = new_current;
        plan.updated_at = now_millis();
        write_plan(&path, &plan)?;

        Ok(ToolOutput {
            title: if last_phase {
                format!("Phase {current_id} completed (last phase)")
            } else {
                format!("Advanced from {current_id} to {new_current}")
            },
            output: format!(
                "Marked {current_id} as completed. Current phase: {new_current}{}",
                if last_phase { " (last)" } else { "" }
            ),
            metadata: json!({
                "type": "plan_advance_phase",
                "from": current_id.as_u32(),
                "to": new_current.as_u32(),
                "last_phase": last_phase,
            }),
            attachments: None,
            llm_suffix: None,
        })
    }
}

// ---------------------------------------------------------------------------
// LogEntryTool — `plan_log` (unified findings/error/decision log)
// ---------------------------------------------------------------------------

pub struct LogEntryTool;
impl Default for LogEntryTool {
    fn default() -> Self {
        Self
    }
}
impl LogEntryTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for LogEntryTool {
    fn id(&self) -> &str {
        "plan_log"
    }

    fn description(&self) -> &str {
        "Append a structured log entry to one of the side files in \
         .theo/plans/. `kind` selects the destination: 'finding' → \
         findings.json#research, 'resource' → findings.json#resources, \
         'requirement' → findings.json#requirements, 'error' → \
         progress.json#errors (with attempt counter), 'decision' → \
         plan.json#decisions. `content` carries the body; `rationale` is \
         optional context. Use after web/grep searches (Manus 2-action rule), \
         on failed attempts, and when a binding choice is made. Example: \
         plan_log({kind: 'finding', content: 'serde_json supports streaming', \
         rationale: 'Found in docs.rs/serde_json'})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "kind".into(),
                    param_type: "string".into(),
                    description:
                        "Entry type: finding, resource, requirement, error, decision"
                            .into(),
                    required: true,
                },
                ToolParam {
                    name: "content".into(),
                    param_type: "string".into(),
                    description: "Log entry body".into(),
                    required: true,
                },
                ToolParam {
                    name: "rationale".into(),
                    param_type: "string".into(),
                    description: "Optional explanation/context".into(),
                    required: false,
                },
                ToolParam {
                    name: "source".into(),
                    param_type: "string".into(),
                    description:
                        "Source attribution for findings/resources (e.g. URL, doc title)"
                            .into(),
                    required: false,
                },
            ],
            input_examples: vec![
                json!({
                    "kind": "finding",
                    "content": "FastAPI uses pydantic for validation",
                    "source": "https://fastapi.tiangolo.com/"
                }),
                json!({
                    "kind": "error",
                    "content": "cargo test failed: missing feature flag",
                    "rationale": "Need to enable `tokio/full` in Cargo.toml"
                }),
            ],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Orchestration
    }

    async fn execute(
        &self,
        args: Value,
        ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let kind = require_string(&args, "kind")?;
        let content = require_string(&args, "content")?;
        let rationale = optional_string(&args, "rationale");
        let source = optional_string(&args, "source");
        let now = now_millis();

        match kind.as_str() {
            "finding" => {
                let path = findings_path(&ctx.project_dir);
                append_finding(&path, &content, source.as_deref(), now)?;
            }
            "resource" => {
                let path = findings_path(&ctx.project_dir);
                let url = source
                    .ok_or_else(|| ToolError::InvalidArgs(
                        "kind=resource requires `source` (URL)".into(),
                    ))?;
                append_resource(&path, &content, &url)?;
            }
            "requirement" => {
                let path = findings_path(&ctx.project_dir);
                append_requirement(&path, &content)?;
            }
            "error" => {
                let path = progress_path(&ctx.project_dir);
                append_error_entry(
                    &path,
                    &content,
                    rationale.as_deref().unwrap_or(""),
                    now,
                )?;
            }
            "decision" => {
                let plan_p = plan_path(&ctx.project_dir);
                append_decision(
                    &plan_p,
                    &content,
                    rationale.as_deref().unwrap_or(""),
                    now,
                )?;
            }
            other => {
                return Err(ToolError::InvalidArgs(format!(
                    "invalid kind `{other}`. Use one of: finding, resource, requirement, error, decision"
                )));
            }
        }

        Ok(ToolOutput {
            title: format!("Logged {kind}"),
            output: format!("Recorded {kind}: {content}"),
            metadata: json!({
                "type": "plan_log",
                "kind": kind,
                "content": content,
            }),
            attachments: None,
            llm_suffix: None,
        })
    }
}

// ---- Side-file helpers (private) ---------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct FindingsFile {
    #[serde(default = "default_findings_version")]
    version: u32,
    #[serde(default)]
    requirements: Vec<String>,
    #[serde(default)]
    research: Vec<FindingEntry>,
    #[serde(default)]
    resources: Vec<ResourceEntry>,
}

fn default_findings_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FindingEntry {
    summary: String,
    source: String,
    timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ResourceEntry {
    title: String,
    url: String,
}

fn read_findings(path: &Path) -> Result<FindingsFile, ToolError> {
    if !path.exists() {
        return Ok(FindingsFile {
            version: default_findings_version(),
            ..Default::default()
        });
    }
    let content = std::fs::read_to_string(path).map_err(ToolError::Io)?;
    serde_json::from_str(&content)
        .map_err(|e| ToolError::Execution(format!("invalid findings.json: {e}")))
}

fn write_findings(path: &Path, findings: &FindingsFile) -> Result<(), ToolError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent).map_err(ToolError::Io)?;
    }
    let json = serde_json::to_string_pretty(findings)
        .map_err(|e| ToolError::Execution(format!("serialize findings: {e}")))?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, json.as_bytes()).map_err(ToolError::Io)?;
    std::fs::rename(&temp, path).map_err(ToolError::Io)?;
    Ok(())
}

fn append_finding(
    path: &Path,
    summary: &str,
    source: Option<&str>,
    timestamp: u64,
) -> Result<(), ToolError> {
    let mut findings = read_findings(path)?;
    findings.research.push(FindingEntry {
        summary: summary.to_owned(),
        source: source.unwrap_or("").to_owned(),
        timestamp,
    });
    write_findings(path, &findings)
}

fn append_resource(path: &Path, title: &str, url: &str) -> Result<(), ToolError> {
    let mut findings = read_findings(path)?;
    findings.resources.push(ResourceEntry {
        title: title.to_owned(),
        url: url.to_owned(),
    });
    write_findings(path, &findings)
}

fn append_requirement(path: &Path, requirement: &str) -> Result<(), ToolError> {
    let mut findings = read_findings(path)?;
    findings.requirements.push(requirement.to_owned());
    write_findings(path, &findings)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ProgressFile {
    #[serde(default = "default_progress_version")]
    version: u32,
    #[serde(default)]
    sessions: Vec<Value>,
    #[serde(default)]
    errors: Vec<ErrorEntry>,
    #[serde(default)]
    reboot_check: Value,
}

fn default_progress_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ErrorEntry {
    error: String,
    attempt: u32,
    resolution: String,
    timestamp: u64,
}

fn read_progress(path: &Path) -> Result<ProgressFile, ToolError> {
    if !path.exists() {
        return Ok(ProgressFile {
            version: default_progress_version(),
            ..Default::default()
        });
    }
    let content = std::fs::read_to_string(path).map_err(ToolError::Io)?;
    serde_json::from_str(&content)
        .map_err(|e| ToolError::Execution(format!("invalid progress.json: {e}")))
}

fn write_progress(path: &Path, progress: &ProgressFile) -> Result<(), ToolError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent).map_err(ToolError::Io)?;
    }
    let json = serde_json::to_string_pretty(progress)
        .map_err(|e| ToolError::Execution(format!("serialize progress: {e}")))?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, json.as_bytes()).map_err(ToolError::Io)?;
    std::fs::rename(&temp, path).map_err(ToolError::Io)?;
    Ok(())
}

fn append_error_entry(
    path: &Path,
    error: &str,
    resolution: &str,
    timestamp: u64,
) -> Result<(), ToolError> {
    let mut progress = read_progress(path)?;
    let attempt = progress
        .errors
        .iter()
        .filter(|e| e.error == error)
        .count()
        .saturating_add(1) as u32;
    progress.errors.push(ErrorEntry {
        error: error.to_owned(),
        attempt,
        resolution: resolution.to_owned(),
        timestamp,
    });
    write_progress(path, &progress)
}

fn append_decision(
    plan_p: &Path,
    decision: &str,
    rationale: &str,
    timestamp: u64,
) -> Result<(), ToolError> {
    let mut plan = read_plan(plan_p).map_err(|e| match e {
        ToolError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => ToolError::NotFound(
            "plan.json missing — call plan_create before plan_log(kind=decision)".into(),
        ),
        other => other,
    })?;
    plan.decisions.push(PlanDecision {
        decision: decision.to_owned(),
        rationale: rationale.to_owned(),
        timestamp,
    });
    plan.updated_at = now_millis();
    write_plan(plan_p, &plan).map_err(|e| match e {
        ToolError::Execution(msg) if msg.starts_with("plan validation failed") => {
            ToolError::Execution(msg)
        }
        other => other,
    })
}

// ---------------------------------------------------------------------------
// GetPlanSummaryTool — `plan_summary`
// ---------------------------------------------------------------------------

pub struct GetPlanSummaryTool;
impl Default for GetPlanSummaryTool {
    fn default() -> Self {
        Self
    }
}
impl GetPlanSummaryTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for GetPlanSummaryTool {
    fn id(&self) -> &str {
        "plan_summary"
    }

    fn description(&self) -> &str {
        "Return the persisted plan as a human-readable Markdown view (read-only). \
         Use this to inject the plan into a prompt when continuing work. \
         Returns an empty string + metadata.exists=false when no plan.json is \
         present. Example: plan_summary({})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new()
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Orchestration
    }

    async fn execute(
        &self,
        _args: Value,
        ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let path = plan_path(&ctx.project_dir);
        if !path.exists() {
            return Ok(ToolOutput {
                title: "No plan".into(),
                output: "No plan.json found. Call plan_create to author one.".into(),
                metadata: json!({"type": "plan_summary", "exists": false}),
                attachments: None,
                llm_suffix: None,
            });
        }
        let plan = read_plan(&path)?;
        let md = plan.to_markdown();
        let task_count = plan.all_tasks().len();
        Ok(ToolOutput {
            title: format!("Plan: {} ({} tasks)", plan.title, task_count),
            output: md,
            metadata: json!({
                "type": "plan_summary",
                "exists": true,
                "title": plan.title,
                "tasks": task_count,
                "current_phase": plan.current_phase.as_u32(),
            }),
            attachments: None,
            llm_suffix: None,
        })
    }
}

// ---------------------------------------------------------------------------
// GetNextTaskTool — `plan_next_task`
// ---------------------------------------------------------------------------

pub struct GetNextTaskTool;
impl Default for GetNextTaskTool {
    fn default() -> Self {
        Self
    }
}
impl GetNextTaskTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for GetNextTaskTool {
    fn id(&self) -> &str {
        "plan_next_task"
    }

    fn description(&self) -> &str {
        "Return the next actionable task from the persisted plan, respecting the \
         dependency DAG. Returns metadata.found=false when every task is in a \
         terminal state. Use this each iteration to know what to work on. \
         Example: plan_next_task({})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new()
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Orchestration
    }

    async fn execute(
        &self,
        _args: Value,
        ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let path = plan_path(&ctx.project_dir);
        if !path.exists() {
            return Ok(ToolOutput {
                title: "No plan".into(),
                output: "No plan.json found. Call plan_create to author one.".into(),
                metadata: json!({"type": "plan_next_task", "found": false}),
                attachments: None,
                llm_suffix: None,
            });
        }
        let plan = read_plan(&path)?;
        match plan.next_actionable_task() {
            Some(task) => {
                let prompt = plan.task_to_agent_prompt(task);
                Ok(ToolOutput {
                    title: format!("Next task: {} — {}", task.id, task.title),
                    output: prompt,
                    metadata: json!({
                        "type": "plan_next_task",
                        "found": true,
                        "task_id": task.id.as_u32(),
                        "title": task.title,
                        "depends_on": task
                            .depends_on
                            .iter()
                            .map(|d| d.as_u32())
                            .collect::<Vec<_>>(),
                    }),
                    attachments: None,
                    llm_suffix: None,
                })
            }
            None => Ok(ToolOutput {
                title: "No actionable task".into(),
                output: "All tasks are in terminal states (Completed/Skipped/Failed/Blocked) \
                         or blocked by failed dependencies. The plan is fully resolved or stuck."
                    .into(),
                metadata: json!({"type": "plan_next_task", "found": false}),
                attachments: None,
                llm_suffix: None,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// PlanFailureStatusTool — `plan_failure_status` (T6.1 part 4)
// ---------------------------------------------------------------------------

/// `plan_failure_status` — list tasks whose `failure_count` is >= threshold.
/// The agent reads this BEFORE deciding whether to call `plan_replan` with
/// a SkipTask / EditTask patch — turning the agent itself into the replan
/// advisor without needing a dedicated LLM round-trip.
pub struct PlanFailureStatusTool;

impl Default for PlanFailureStatusTool {
    fn default() -> Self {
        Self
    }
}

impl PlanFailureStatusTool {
    pub fn new() -> Self {
        Self
    }
}

const DEFAULT_FAILURE_THRESHOLD: u32 = 3;

#[async_trait]
impl Tool for PlanFailureStatusTool {
    fn id(&self) -> &str {
        "plan_failure_status"
    }

    fn description(&self) -> &str {
        "T6.1 — List tasks in the persisted plan whose failure_count meets or \
         exceeds `threshold` (default 3). Each entry carries id, title, \
         failure_count, status, and recent outcome — exactly the information \
         the agent needs to decide whether to call `plan_replan` with a \
         SkipTask, EditTask, or RemoveTask patch. The pilot's auto-replan \
         trigger logs threshold breaches; this tool lets the agent itself \
         observe the same state and act. \
         Example: plan_failure_status({}) or plan_failure_status({threshold: 5})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![ToolParam {
                name: "threshold".into(),
                param_type: "integer".into(),
                description:
                    "Minimum failure_count to include. Default 3 (matches the \
                     pilot's `replan_failure_threshold`). Pass 1 to see EVERY \
                     task that has failed at least once."
                        .into(),
                required: false,
            }],
            input_examples: vec![
                json!({}),
                json!({"threshold": 1}),
                json!({"threshold": 5}),
            ],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Orchestration
    }

    async fn execute(
        &self,
        args: Value,
        ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let threshold = args
            .get("threshold")
            .and_then(Value::as_u64)
            .map(|n| n as u32)
            .unwrap_or(DEFAULT_FAILURE_THRESHOLD);

        let path = plan_path(&ctx.project_dir);
        if !path.exists() {
            return Ok(ToolOutput {
                title: "plan_failure_status: no plan".into(),
                output: "No plan.json found at .theo/plans/plan.json. Call \
                         plan_create to author one."
                    .into(),
                metadata: json!({
                    "type": "plan_failure_status",
                    "threshold": threshold,
                    "stuck_count": 0,
                    "stuck_tasks": [],
                }),
                attachments: None,
                llm_suffix: None,
            });
        }

        let plan = read_plan(&path)?;
        let offender_ids = plan.tasks_exceeding_failure_threshold(threshold);
        let by_id: std::collections::HashMap<_, _> = plan
            .all_tasks()
            .into_iter()
            .map(|t| (t.id, t.clone()))
            .collect();

        let mut entries: Vec<Value> = Vec::with_capacity(offender_ids.len());
        for id in &offender_ids {
            if let Some(task) = by_id.get(id) {
                entries.push(json!({
                    "task_id": id.as_u32(),
                    "title": task.title,
                    "failure_count": task.failure_count,
                    "status": format!("{:?}", task.status),
                    "outcome": task.outcome.as_deref().unwrap_or(""),
                    "depends_on": task.depends_on.iter().map(|d| d.as_u32()).collect::<Vec<_>>(),
                }));
            }
        }

        if entries.is_empty() {
            return Ok(ToolOutput {
                title: format!(
                    "plan_failure_status: 0 task(s) ≥ {threshold} failures"
                ),
                output: format!(
                    "No tasks have reached the failure threshold ({threshold}). \
                     The plan is healthy. Check `plan_next_task` for the \
                     next actionable item."
                ),
                metadata: json!({
                    "type": "plan_failure_status",
                    "threshold": threshold,
                    "stuck_count": 0,
                    "stuck_tasks": [],
                }),
                attachments: None,
                llm_suffix: None,
            });
        }

        let mut output = format!(
            "plan_failure_status: {n} task(s) at or above threshold {threshold}\n\n",
            n = entries.len(),
        );
        for e in &entries {
            output.push_str(&format!(
                "  - id={id}  failures={fc}  status={st}\n    title: {title}\n",
                id = e["task_id"],
                fc = e["failure_count"],
                st = e["status"].as_str().unwrap_or("?"),
                title = e["title"].as_str().unwrap_or(""),
            ));
            let outcome = e["outcome"].as_str().unwrap_or("");
            if !outcome.is_empty() {
                let preview: String = outcome.chars().take(120).collect();
                let suffix = if outcome.chars().count() > 120 {
                    "…"
                } else {
                    ""
                };
                output.push_str(&format!("    last outcome: {preview}{suffix}\n"));
            }
        }
        output.push_str(
            "\nNext: call `plan_replan` with a SkipTask (unrecoverable), \
             EditTask (clarify dod/files), or RemoveTask patch for one of \
             the offenders.",
        );

        Ok(ToolOutput {
            title: format!(
                "plan_failure_status: {} stuck task(s)",
                entries.len()
            ),
            output,
            metadata: json!({
                "type": "plan_failure_status",
                "threshold": threshold,
                "stuck_count": entries.len(),
                "stuck_tasks": entries,
            }),
            attachments: None,
            llm_suffix: None,
        })
    }
}

// ---------------------------------------------------------------------------
// ReplanTool — `plan_replan` (T6.1)
// ---------------------------------------------------------------------------

pub struct ReplanTool;
impl Default for ReplanTool {
    fn default() -> Self {
        Self
    }
}
impl ReplanTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ReplanTool {
    fn id(&self) -> &str {
        "plan_replan"
    }

    fn description(&self) -> &str {
        "T6.1 — Apply a typed PlanPatch to the persisted plan to recover \
         from a stuck task. The patch must be one of: \
         {kind:'add_task', phase:u32, task:{...}, position?:'end'|'begin'|{after_task:u32}}, \
         {kind:'remove_task', id:u32}, \
         {kind:'edit_task', id:u32, edits:{...}}, \
         {kind:'reorder_deps', id:u32, new_deps:[u32]}, \
         {kind:'skip_task', id:u32, rationale:string}. \
         The plan is re-validated after the patch — invalid mutations \
         (cycle, orphan dep, etc.) are rejected and the plan is left \
         untouched. Use SkipTask when you cannot make progress on a \
         failed task. Example: \
         plan_replan({patch: {kind:'skip_task', id: 3, rationale: 'API deprecated'}})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![ToolParam {
                name: "patch".into(),
                param_type: "object".into(),
                description:
                    "PlanPatch JSON object with `kind` discriminator (add_task|remove_task|edit_task|reorder_deps|skip_task)."
                        .into(),
                required: true,
            }],
            input_examples: vec![
                json!({"patch": {"kind": "skip_task", "id": 3, "rationale": "Out of scope"}}),
                json!({
                    "patch": {
                        "kind": "edit_task",
                        "id": 1,
                        "edits": {"status": "blocked", "rationale": "External dep failed"}
                    }
                }),
            ],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Orchestration
    }

    async fn execute(
        &self,
        args: Value,
        ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let patch_value = args
            .get("patch")
            .ok_or_else(|| ToolError::InvalidArgs("missing object `patch`".into()))?;
        let patch: theo_domain::plan_patch::PlanPatch = serde_json::from_value(patch_value.clone())
            .map_err(|e| ToolError::InvalidArgs(format!("invalid `patch`: {e}")))?;

        let path = plan_path(&ctx.project_dir);
        let mut plan = read_plan(&path).map_err(|e| match e {
            ToolError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => {
                ToolError::NotFound(
                    "plan.json missing — call plan_create before plan_replan".into(),
                )
            }
            other => other,
        })?;

        plan.apply_patch(&patch).map_err(|e| match e {
            theo_domain::plan_patch::PatchError::TaskNotFound(id) => {
                ToolError::NotFound(format!("task {id} not found in plan"))
            }
            theo_domain::plan_patch::PatchError::PhaseNotFound(id) => {
                ToolError::NotFound(format!("phase {id} not found in plan"))
            }
            other => ToolError::Execution(format!("plan patch failed: {other}")),
        })?;
        plan.updated_at = now_millis();
        write_plan(&path, &plan)?;

        // Surface a short summary for both the user UI and the model.
        let kind_label = match &patch {
            theo_domain::plan_patch::PlanPatch::AddTask { .. } => "add_task",
            theo_domain::plan_patch::PlanPatch::RemoveTask { .. } => "remove_task",
            theo_domain::plan_patch::PlanPatch::EditTask { .. } => "edit_task",
            theo_domain::plan_patch::PlanPatch::ReorderDeps { .. } => "reorder_deps",
            theo_domain::plan_patch::PlanPatch::SkipTask { .. } => "skip_task",
            _ => "unknown",
        };

        Ok(ToolOutput {
            title: format!("Plan patched: {kind_label}"),
            output: format!(
                "Patch `{kind_label}` applied. Plan now has {tasks} task(s) across {phases} phase(s).",
                tasks = plan.all_tasks().len(),
                phases = plan.phases.len()
            ),
            metadata: json!({
                "type": "plan_replan",
                "kind": kind_label,
                "version_counter": plan.version_counter,
            }),
            attachments: None,
            llm_suffix: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Suppress unused warning for backward-compat helper.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn _silence_map_plan_err(err: PlanError) -> ToolError {
    map_plan_err("plan", err)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use theo_domain::session::{MessageId, SessionId};

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
}
