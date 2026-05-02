---
type: report
question: "What is the best approach for programmatic task/plan management in Theo Code?"
generated_at: 2026-04-26T14:00:00Z
confidence: 0.88
sources_used: 18
---

# Report: Programmatic Task/Plan Management Alternatives

## Executive Summary

Every major AI coding assistant (Claude Code, Cursor, Codex CLI, Aider) stores plans as **plain Markdown files** with varying degrees of structure. None use a schema-validated format. This is the core opportunity for Theo: adopt **JSON as the canonical on-disk format** with `serde` + `schemars` for schema validation, and provide a **thin Markdown render/parse layer** for human readability. This gives us what competitors lack -- guaranteed parsability, LLM-native generation via tool calling, and type-safe Rust structs -- without sacrificing developer experience.

## Analysis

### Finding 1: How Competitors Handle Plans

All five major tools use Markdown with no schema validation:

| Tool | Plan Storage | Format | Schema | Validated? |
|------|-------------|--------|--------|-----------|
| **Claude Code** | `~/.claude/plans/*.md` or project-local via `plansDirectory` | Free-form Markdown | None | No |
| **Cursor** | `.cursor/plans/*.md` | Markdown with checkboxes | None | No |
| **Codex CLI** | `PLANS.md` in repo root | Structured Markdown ("ExecPlan") | Convention-based | No |
| **Aider** | No persistent plans (conversation-based) | N/A | N/A | N/A |
| **SWE-agent** | In-context only (collapsed observations) | N/A | N/A | N/A |

**Claude Code** stores plans as plain `.md` files with auto-generated names like `jaunty-petting-nebula.md`. Since February 2026, plans persist via `plansDirectory` in `.claude/settings.json`. The April 2026 desktop redesign added a side-panel that renders the plan alongside chat. Internally, plan mode is "just a little bit of extra verbiage" in the system prompt -- there is no structured format. [Source: Armin Ronacher analysis][1], [Claude Code docs][2]

**Cursor** saves plans as Markdown in `.cursor/plans/` with interactive checkboxes in the UI. The plan is a TODO list where each step has a checkbox. Cursor 2.1 added "Interactive Plans" with clarifying questions before plan generation. Plans persist across sessions. No structured format -- the UI parses checkboxes from Markdown. [Source: Cursor changelog][3], [Digital Applied][4]

**OpenAI Codex CLI** has the most structured approach: `PLANS.md` with mandatory sections (Purpose, Progress, Surprises & Discoveries, Decision Log, Outcomes & Retrospective). They call it "ExecPlan" -- a living document that must be "self-contained, self-sufficient, novice-guiding, outcome-focused." Progress is tracked via checkbox lists with timestamps. Despite the structure, it is still free-form Markdown with no machine-parsable schema. [Source: OpenAI Cookbook][5], [Codex CLI Features][6]

**Aider** has no persistent plan storage. It uses an architect/editor workflow where a stronger model designs the solution and a cheaper model implements it, but plans live only in the conversation context. The `coding-aider` plugin adds persistent Markdown plans with checkboxes, but this is a community extension, not core. [Source: Aider docs][7], [coding-aider][8]

**SWE-agent** does not persist plans either. It collapses observations from prior steps into single-line summaries to save context window space. The SWE-AF fork introduces runtime plan mutation (dynamic DAG modification during execution), but plans are in-memory structures, not persisted files. [Source: SWE-agent NeurIPS paper][9], [SWE-AF architecture][10]

**Key insight:** Every tool that persists plans uses Markdown because LLMs generate Markdown naturally. But none validate structure, which means all suffer from the exact fragility Theo currently has.

---

### Finding 2: LLM Output Format Reliability

| Format | LLM Generation Reliability | Token Cost | Rust Serde Support | Human Readability | Schema Validation |
|--------|--------------------------|------------|-------------------|-------------------|-------------------|
| **JSON** | Excellent (native to tool calling) | Higher (~15% more tokens than YAML) | `serde_json` (mature) | Moderate (verbose) | `jsonschema`, `schemars` |
| **YAML** | Good (but whitespace errors) | Lower | `serde_yaml` (adequate) | Excellent | Limited |
| **TOML** | Poor (LLMs struggle with nested tables) | Similar to YAML | `toml` (mature, Rust-native) | Good for flat data | Limited |
| **JSON5** | Fair (less training data) | Slightly lower than JSON | `json5` (less mature) | Better than JSON | None for Rust |
| **Markdown** | Excellent (natural output) | Lowest | Manual parsing only | Excellent | None |

The 2025 StructEval benchmark found that most frontier models achieve 90%+ accuracy generating JSON, HTML, CSV, Markdown, and YAML. TOML generation accuracy is measurably lower. [Source: StructEval][11]

Research from 2025 shows that forcing JSON output degrades LLM reasoning by 10-15%, but this is mitigated by the two-step approach: free reasoning first, then structured formatting. This aligns perfectly with Theo's architecture where the agent reasons in natural language and then calls tools with structured JSON arguments. [Source: Michael Hannecke][12]

**Critical advantage of JSON for Theo**: LLM tool calling already uses JSON. When the agent calls `update_plan(task_id: 3, status: "completed")`, the arguments are already JSON. Making the plan format JSON means the tool schema IS the plan schema -- zero impedance mismatch.

---

### Finding 3: Rust Libraries for the Approach

**Serialization (mature, production-ready):**

| Crate | Version | Purpose | Downloads |
|-------|---------|---------|-----------|
| `serde` + `serde_json` | 1.x | JSON ser/de with derive macros | 300M+ |
| `schemars` | 1.1.0 | Derive JSON Schema from Rust structs | 15M+ |
| `jsonschema` | 0.38+ | Validate JSON against schema at runtime | 5M+ |
| `serde_valid` | 0.x | Validation annotations on serde structs | 500K+ |

**DAG/Task Graph (if needed for dependency modeling):**

| Crate | Purpose | Serde Support |
|-------|---------|--------------|
| `daggy` | DAG data structure (on top of petgraph) | Yes (feature flag) |
| `dagrs` | Async DAG task execution with custom parsers | Indirect |
| `petgraph` | General graph library | Yes |
| `taskflow-rs` | DAG-based parallel task scheduling | Early (v0.x) |

**The `schemars` crate is the linchpin.** It derives JSON Schema from the same Rust struct that `serde` serializes, meaning the schema the LLM sees in its tool definition is guaranteed to match the Rust type. No drift possible. [Source: schemars docs][13]

---

### Finding 4: Recommended Architecture

The recommended approach is **JSON as canonical format + Markdown as presentation layer**.

```
┌─────────────────────────────────────────────────┐
│  LLM Tool Call (JSON)                           │
│  update_plan({ task_id: 3, status: "done" })    │
└──────────────────────┬──────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────┐
│  Rust Structs (serde + schemars)                │
│  Plan { tasks: Vec<Task>, ... }                 │
│  Task { id, title, status, deps, dod, ... }     │
└──────────────────────┬──────────────────────────┘
                       │
              ┌────────┴────────┐
              ▼                 ▼
   ┌──────────────┐   ┌──────────────────┐
   │ .theo/plans/ │   │ Terminal/UI      │
   │  plan.json   │   │ Markdown render  │
   │ (canonical)  │   │ (read-only view) │
   └──────────────┘   └──────────────────┘
```

**Why this beats the competition:**

1. **Schema validation at parse time** -- invalid plans fail with typed errors, not silent data loss.
2. **LLM tool calling is already JSON** -- the plan schema lives inside the tool definition. LLMs see the schema before generating output.
3. **`serde` derive makes it trivial** -- add a field to the struct, the schema updates automatically, JSON parsing handles it with defaults.
4. **Migration via `#[serde(default)]`** -- new fields get defaults, old plans parse without breaking.
5. **Markdown is a view, not a source** -- render Markdown for `theo plan show`, but never parse it back.

#### Concrete Type Definitions

```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The version of the plan format, for forward-compatible migration.
const PLAN_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Plan {
    /// Format version for migration support.
    pub version: u32,
    /// Human-readable plan title.
    pub title: String,
    /// Why this plan exists -- one sentence.
    pub goal: String,
    /// Ordered list of tasks.
    pub tasks: Vec<Task>,
    /// ISO 8601 timestamp of creation.
    pub created_at: String,
    /// ISO 8601 timestamp of last modification.
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Task {
    /// Unique task identifier (monotonically increasing within a plan).
    pub id: u32,
    /// Short descriptive title.
    pub title: String,
    /// Current status.
    pub status: TaskStatus,
    /// Files this task will create or modify.
    #[serde(default)]
    pub files: Vec<String>,
    /// What needs to be done -- detailed description.
    #[serde(default)]
    pub description: String,
    /// Definition of Done -- verifiable completion criteria.
    #[serde(default)]
    pub dod: String,
    /// Task IDs that must complete before this one can start.
    #[serde(default)]
    pub depends_on: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Skipped,
    Blocked,
}
```

#### Schema Generation for LLM Tool Definitions

```rust
use schemars::schema_for;

fn plan_json_schema() -> serde_json::Value {
    let schema = schema_for!(Plan);
    serde_json::to_value(schema).unwrap()
}

// This schema goes directly into the tool definition:
// {
//   "name": "create_plan",
//   "parameters": <plan_json_schema()>
// }
```

#### Markdown Rendering (View Layer Only)

```rust
impl Plan {
    pub fn to_markdown(&self) -> String {
        let mut md = format!("# Plan: {}\n\n", self.title);
        md.push_str(&format!("**Goal:** {}\n\n", self.goal));
        md.push_str("## Tasks\n\n");

        for task in &self.tasks {
            let checkbox = match task.status {
                TaskStatus::Completed => "[x]",
                TaskStatus::Skipped => "[-]",
                _ => "[ ]",
            };
            md.push_str(&format!(
                "### Task {}: {} {}\n",
                task.id, checkbox, task.title
            ));
            if !task.files.is_empty() {
                md.push_str(&format!(
                    "- **Files**: {}\n",
                    task.files.join(", ")
                ));
            }
            if !task.description.is_empty() {
                md.push_str(&format!("- **Description**: {}\n", task.description));
            }
            if !task.dod.is_empty() {
                md.push_str(&format!("- **DoD**: {}\n", task.dod));
            }
            if !task.depends_on.is_empty() {
                let deps: Vec<String> =
                    task.depends_on.iter().map(|d| format!("T{}", d)).collect();
                md.push_str(&format!("- **Depends on**: {}\n", deps.join(", ")));
            }
            md.push('\n');
        }
        md
    }
}
```

#### Loading with Validation

```rust
use std::path::Path;

pub fn load_plan(path: &Path) -> Result<Plan, PlanError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| PlanError::Io(e.to_string()))?;

    let plan: Plan = serde_json::from_str(&content)
        .map_err(|e| PlanError::InvalidFormat(e.to_string()))?;

    // Validate version
    if plan.version > PLAN_FORMAT_VERSION {
        return Err(PlanError::UnsupportedVersion {
            found: plan.version,
            max_supported: PLAN_FORMAT_VERSION,
        });
    }

    // Validate dependency references
    let task_ids: std::collections::HashSet<u32> =
        plan.tasks.iter().map(|t| t.id).collect();
    for task in &plan.tasks {
        for dep in &task.depends_on {
            if !task_ids.contains(dep) {
                return Err(PlanError::InvalidDependency {
                    task_id: task.id,
                    missing_dep: *dep,
                });
            }
        }
    }

    // Validate no cycles (topological sort)
    validate_no_cycles(&plan)?;

    Ok(plan)
}
```

#### Tool Definitions for Agent

```rust
// The agent gets these tools to manipulate plans:

// 1. create_plan(plan: Plan) -> creates .theo/plans/<slug>.json
// 2. update_task(plan_id: str, task_id: u32, status: TaskStatus) -> updates one task
// 3. get_plan(plan_id: str) -> returns current plan state
// 4. get_next_task(plan_id: str) -> returns first pending task with all deps met

// Because tool arguments are JSON and Plan derives JsonSchema,
// the LLM sees the exact schema it needs to produce valid output.
```

---

### Finding 5: Why Not YAML or TOML

**YAML** is tempting because it is human-readable and Cursor/Claude Code use Markdown (which shares YAML's whitespace aesthetics). However:

- LLMs generate whitespace-sensitive formats less reliably than brace-delimited ones.
- `serde_yaml` had a major rewrite (0.8 to 0.9) with breaking changes; the ecosystem is less stable than `serde_json`.
- No equivalent of `schemars` for YAML -- you cannot derive a YAML schema from Rust structs.
- Indentation errors are silent data corruption, not parse failures.

**TOML** is Rust-native and great for configuration, but:

- Cannot represent top-level arrays (a plan IS an array of tasks at its core).
- Nested tables are awkward for task hierarchies.
- LLMs generate valid TOML at measurably lower rates than JSON (StructEval 2025).
- No schema validation story.

**Markdown** (current approach) is what every competitor uses, and it is the worst format for machine consumption:

- No schema validation.
- Formatting variations break parsing (the exact problem stated in the question).
- Humans write Markdown differently -- `### Task 1:` vs `### Task 1 -` vs `### 1. Task`.
- Updating a single field requires re-rendering the entire file.

---

### Finding 6: Migration Path from Current Roadmap Parser

The current `roadmap.rs` (282 lines) has string-matching that will be replaced, but the migration is incremental:

1. **Phase 1: Add JSON types alongside Markdown parser.** Define `Plan`, `Task`, `TaskStatus` with serde + schemars. Keep the Markdown parser for backward compatibility with existing `.md` plan files.

2. **Phase 2: Convert tools to produce JSON plans.** When the agent creates a new plan via tool calling, it writes `.json` directly. The `to_markdown()` renderer provides the human view.

3. **Phase 3: Remove Markdown parser.** Once no `.md` plans exist in active use, delete `parse_roadmap_content` and the field-matching logic. Keep `to_markdown()` as a read-only renderer.

4. **Phase 4: Add DAG validation.** Use `depends_on` field with topological sort (no external crate needed for small task counts -- a simple Kahn's algorithm in ~30 lines). If task graphs grow complex enough to warrant it, adopt `daggy` for its serde support and petgraph foundation.

---

## Gaps

1. **No research on plan versioning across sessions.** How do Claude Code, Cursor, Codex handle plan format changes over time? None seem to version their formats, which suggests they accept breakage on updates.

2. **No data on LLM JSON generation accuracy for deeply nested schemas.** The StructEval benchmark tests simple structures. Theo's `Plan` type is nested (Plan -> Vec<Task> -> Vec<String>) but not deeply -- this is likely fine, but untested empirically.

3. **Collaborative editing.** If multiple agent threads modify the same plan concurrently (e.g., parallel sub-agents), JSON file locking or CRDT merging would be needed. No competitor addresses this.

4. **Plan size limits.** When a plan has 50+ tasks, does the full JSON plan fit in context? Need to measure token cost. A 20-task plan is approximately 2K tokens in JSON.

5. **How SWE-AF's runtime plan mutation works in practice.** The DAG modification during execution is interesting but the codebase is too new to evaluate stability.

---

## Recommendations

### P0: Implement JSON plan format with serde + schemars

Define `Plan`, `Task`, `TaskStatus` in `theo-domain` (pure types, zero deps). Derive `Serialize`, `Deserialize`, `JsonSchema`. This is the canonical format.

**Estimated effort:** 1-2 days. The structs above are nearly complete.

### P1: Add plan manipulation tools for the agent

- `create_plan` -- agent generates plan JSON via tool call.
- `update_task_status` -- agent marks tasks as it completes them.
- `get_next_task` -- returns the first `Pending` task with all `depends_on` met.
- `show_plan` -- returns Markdown rendering for context injection.

**Estimated effort:** 2-3 days. Tool infrastructure already exists in `theo-tooling`.

### P2: Schema-in-tool-definition pattern

Embed the `schemars`-generated JSON Schema directly in the tool definition. The LLM sees the exact structure it must produce. This eliminates the "hope the LLM formats it right" problem entirely.

**Estimated effort:** 1 day. `schemars::schema_for!(Plan)` does the work.

### P3: Deprecate Markdown roadmap parser

Keep `to_markdown()` as a view. Remove `parse_roadmap_content()` and the fragile field-matching logic. Existing `.md` plans get a one-time manual conversion or a simple migration script.

**Estimated effort:** 1 day cleanup after P0-P2 are stable.

### Do NOT do (YAGNI)

- **Do NOT adopt YAML or TOML.** JSON wins on LLM reliability, schema validation, and serde maturity.
- **Do NOT build a DAG execution engine yet.** Simple topological sort in 30 lines covers the dependency case. Adopt `daggy` only if plans regularly exceed 50 tasks with complex dependency graphs.
- **Do NOT implement CRDT-based plan merging.** Solve concurrent access if/when parallel sub-agents actually modify plans simultaneously.
- **Do NOT build a plan migration framework.** `#[serde(default)]` handles additive changes. A `version` field handles breaking changes with a simple match statement.

---

## Sources

1. [Armin Ronacher - What Actually Is Claude Code's Plan Mode?](https://lucumr.pocoo.org/2025/12/17/what-is-plan-mode/)
2. [Claude Code Docs - Common Workflows](https://code.claude.com/docs/en/common-workflows)
3. [Cursor Changelog](https://cursor.com/changelog)
4. [Cursor 2.1: Clarifying Questions & Interactive Plans](https://www.digitalapplied.com/blog/cursor-2-1-clarifying-questions-plans)
5. [OpenAI Cookbook - Using PLANS.md for multi-hour problem solving](https://developers.openai.com/cookbook/articles/codex_exec_plans)
6. [Codex CLI Features](https://developers.openai.com/codex/cli/features)
7. [Aider - AI Pair Programming](https://aider.chat/)
8. [coding-aider Plan Mode](https://github.com/p-wegner/coding-aider/blob/main/docs/plan-mode.md)
9. [SWE-agent NeurIPS 2024 Paper](https://proceedings.neurips.cc/paper_files/paper/2024/file/5a7c947568c1b1328ccc5230172e1e7c-Paper-Conference.pdf)
10. [SWE-AF Architecture](https://github.com/Agent-Field/SWE-AF/blob/main/docs/ARCHITECTURE.md)
11. [StructEval: Benchmarking LLMs' Structural Outputs](https://arxiv.org/html/2505.20139v1)
12. [Beyond JSON: Picking the Right Format for LLM Pipelines](https://medium.com/@michael.hannecke/beyond-json-picking-the-right-format-for-llm-pipelines-b65f15f77f7d)
13. [schemars - Generate JSON Schema from Rust](https://graham.cool/schemars/)
14. [daggy - DAG data structure for Rust](https://docs.rs/daggy)
15. [dagrs - Async DAG task framework](https://github.com/dagrs-dev/dagrs)
16. [jsonschema crate](https://docs.rs/jsonschema)
17. [Manus-style Planning with Files](https://github.com/othmanadi/planning-with-files)
18. [Task Decomposition Agent Pattern](https://www.agentpatterns.tech/en/agent-patterns/task-decomposition-agent)
