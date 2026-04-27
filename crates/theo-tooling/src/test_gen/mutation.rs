//! T5.2 — `gen_mutation_test` tool: invokes `cargo-mutants` and surfaces
//! surviving mutations to the agent.
//!
//! `cargo-mutants` is the only canonical Rust mutation-testing tool. It
//! generates source-level mutations (e.g. `+` → `-`, `>` → `>=`, deleting
//! `?`) and runs the test suite against each one; mutations that the
//! tests still PASS for are "survivors" — they reveal under-tested code
//! paths.
//!
//! This tool:
//! 1. Spawns `cargo mutants --json` (or reads an existing
//!    `mutants.out/outcomes.json` file) under the project root.
//! 2. Parses outcomes and filters to survivors.
//! 3. Returns a structured list with file/line/mutation-text for each.
//!
//! The agent decides whether to write tests that kill the survivors,
//! call `gen_property_test`, or accept the survival as intentional.
//!
//! Reasoning around external tooling:
//! - `cargo-mutants` is NOT installed by this crate. The tool fails with
//!   a clear `Execution` error if the binary is missing — matches the
//!   existing pattern of `cargo-audit`/`cargo-tarpaulin` in this repo.
//! - Tests below cover the JSON parser end-to-end against real
//!   `outcomes.json` shapes from cargo-mutants 24.x. They do NOT
//!   actually invoke the binary; that's an integration test concern.
//!
//! See `docs/plans/sota-tier1-tier2-plan.md` §T5.2.

use std::path::Path;
use std::process::Command;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

/// One surviving mutation (test suite passed despite the source change).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MutationSurvivor {
    /// Source file the mutation was applied to.
    pub file: String,
    /// 1-based line number.
    pub line: u32,
    /// Short human-readable description (e.g. `replace + with -`).
    pub mutation: String,
    /// Optional function name when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
}

/// Parsed `outcomes.json` summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationReport {
    pub total: usize,
    pub caught: usize,
    pub missed: usize,
    pub survivors: Vec<MutationSurvivor>,
}

/// Errors specific to this tool.
#[derive(Debug, thiserror::Error)]
pub enum MutationError {
    #[error("cargo-mutants binary not found: install with `cargo install cargo-mutants`")]
    BinaryMissing,
    #[error("cargo-mutants failed (exit {code:?}): {stderr}")]
    Subprocess { code: Option<i32>, stderr: String },
    #[error("invalid outcomes.json: {0}")]
    InvalidOutcomes(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<MutationError> for ToolError {
    fn from(e: MutationError) -> Self {
        match e {
            MutationError::BinaryMissing => ToolError::NotFound(e.to_string()),
            MutationError::InvalidOutcomes(msg) => {
                ToolError::Execution(format!("invalid outcomes: {msg}"))
            }
            MutationError::Io(io) => ToolError::Io(io),
            MutationError::Subprocess { .. } => ToolError::Execution(e.to_string()),
        }
    }
}

/// Tool that runs cargo-mutants and reports surviving mutations.
pub struct GenMutationTestTool;

impl Default for GenMutationTestTool {
    fn default() -> Self {
        Self
    }
}

impl GenMutationTestTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for GenMutationTestTool {
    fn id(&self) -> &str {
        "gen_mutation_test"
    }

    fn description(&self) -> &str {
        "T5.2 — Run `cargo-mutants` and report surviving mutations \
         (source changes the test suite did NOT catch). Survivors signal \
         under-tested code paths the agent should harden, typically by \
         adding a unit test or `gen_property_test`. Pass `path` (project \
         subdir to mutate, default `.`) and optional `existing_outcomes` \
         (path to an outcomes.json from a previous run — skips the \
         expensive subprocess and just reparses). Requires \
         `cargo install cargo-mutants` on PATH; when missing, the call \
         returns a typed BinaryMissing error — fall back to \
         `gen_property_test` to expand coverage with proptest \
         scaffolding instead of mutation testing. Example: \
         gen_mutation_test({path: 'crates/theo-domain'})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "path".into(),
                    param_type: "string".into(),
                    description:
                        "Project-relative path under which cargo-mutants runs. Default `.`."
                            .into(),
                    required: false,
                },
                ToolParam {
                    name: "existing_outcomes".into(),
                    param_type: "string".into(),
                    description:
                        "Optional path to an existing outcomes.json — when present, parse it instead of running the subprocess."
                            .into(),
                    required: false,
                },
            ],
            input_examples: vec![
                json!({"path": "crates/theo-domain"}),
                json!({"existing_outcomes": "mutants.out/outcomes.json"}),
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
        let path = args
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or(".")
            .to_owned();
        let existing = args
            .get("existing_outcomes")
            .and_then(Value::as_str)
            .map(str::to_owned);

        // T14.1 — surface progress around cargo-mutants. The
        // subprocess can run for many minutes on a large crate; a
        // single static "tool started" indicator is misleading.
        // Two checkpoints: spawning, then "still running" once the
        // outcomes parser has the raw output (the real wall-clock
        // gap; the parser itself is fast).
        let report = if let Some(rel) = existing {
            crate::partial::emit_progress(
                ctx,
                "gen_mutation_test",
                format!("Reading outcomes from `{rel}`…"),
            );
            let abs = ctx.project_dir.join(&rel);
            let raw = std::fs::read_to_string(&abs).map_err(ToolError::Io)?;
            parse_outcomes(&raw).map_err(ToolError::from)?
        } else {
            crate::partial::emit_progress(
                ctx,
                "gen_mutation_test",
                format!(
                    "Running cargo-mutants in `{path}` (this can take \
                     many minutes — set `existing_outcomes` to a cached \
                     outcomes.json to skip)"
                ),
            );
            let r = run_cargo_mutants(&ctx.project_dir, &path)
                .map_err(ToolError::from)?;
            crate::partial::emit_progress(
                ctx,
                "gen_mutation_test",
                format!(
                    "cargo-mutants finished: {} total, {} caught, {} survivors",
                    r.total, r.caught, r.survivors.len()
                ),
            );
            r
        };

        let summary = format!(
            "cargo-mutants: total={total} caught={caught} missed={missed} (kill rate={kill_rate:.1}%)\n\n{listing}",
            total = report.total,
            caught = report.caught,
            missed = report.missed,
            kill_rate = if report.total == 0 {
                100.0
            } else {
                100.0 * report.caught as f64 / report.total as f64
            },
            listing = render_survivor_list(&report.survivors),
        );

        Ok(ToolOutput {
            title: format!(
                "{caught}/{total} mutations caught ({n} survivors)",
                caught = report.caught,
                total = report.total,
                n = report.survivors.len()
            ),
            output: summary,
            metadata: json!({
                "type": "gen_mutation_test",
                "total": report.total,
                "caught": report.caught,
                "missed": report.missed,
                "survivors": report.survivors,
            }),
            attachments: None,
            llm_suffix: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Pure helpers (testable without cargo-mutants installed)
// ---------------------------------------------------------------------------

/// Run `cargo mutants --json --in-place=false` under `cwd/path` and parse
/// the resulting `outcomes.json`. Returns `BinaryMissing` when the binary
/// is not on PATH.
pub fn run_cargo_mutants(cwd: &Path, path: &str) -> Result<MutationReport, MutationError> {
    let target = cwd.join(path);
    let output = Command::new("cargo")
        .args([
            "mutants",
            "--json",
            "--no-shuffle",
            "--in-place=false",
        ])
        .current_dir(&target)
        .output();
    let output = match output {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(MutationError::BinaryMissing);
        }
        Err(e) => return Err(MutationError::Io(e)),
    };
    if !output.status.success() && output.status.code() != Some(1) {
        // cargo-mutants exits 1 when survivors exist — that's NOT a failure.
        return Err(MutationError::Subprocess {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }
    let outcomes_path = target.join("mutants.out").join("outcomes.json");
    let raw = std::fs::read_to_string(&outcomes_path).map_err(MutationError::Io)?;
    parse_outcomes(&raw)
}

/// Parse `outcomes.json` from cargo-mutants 24+.
///
/// The schema has evolved across releases. We keep the parser permissive:
/// only the fields we actually need are extracted (file, line, source-text
/// mutation, function), and unknown fields are ignored.
pub fn parse_outcomes(raw: &str) -> Result<MutationReport, MutationError> {
    let v: Value = serde_json::from_str(raw)
        .map_err(|e| MutationError::InvalidOutcomes(e.to_string()))?;
    let outcomes = v
        .get("outcomes")
        .and_then(Value::as_array)
        .ok_or_else(|| MutationError::InvalidOutcomes("missing `outcomes` array".into()))?;

    let mut total = 0usize;
    let mut caught = 0usize;
    let mut missed = 0usize;
    let mut survivors = Vec::new();

    for outcome in outcomes {
        total += 1;
        let summary = outcome
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_lowercase();
        let is_survivor = matches!(summary.as_str(), "missed" | "survived");
        if is_survivor {
            missed += 1;
            // Mutant location lives at scenario.mutant.{source_file{path,line},function?,replacement}
            let mutant = outcome
                .get("scenario")
                .and_then(|s| s.get("mutant"))
                .or_else(|| outcome.get("mutant"));
            let file = mutant
                .and_then(|m| m.get("source_file"))
                .and_then(|f| f.get("path"))
                .and_then(Value::as_str)
                .or_else(|| {
                    // Older shape: mutant.file
                    mutant.and_then(|m| m.get("file")).and_then(Value::as_str)
                })
                .unwrap_or("<unknown>")
                .to_owned();
            let line = mutant
                .and_then(|m| m.get("source_file"))
                .and_then(|f| f.get("line"))
                .and_then(Value::as_u64)
                .or_else(|| {
                    mutant.and_then(|m| m.get("line")).and_then(Value::as_u64)
                })
                .unwrap_or(0) as u32;
            let mutation = mutant
                .and_then(|m| m.get("description"))
                .and_then(Value::as_str)
                .or_else(|| {
                    mutant.and_then(|m| m.get("replacement")).and_then(Value::as_str)
                })
                .unwrap_or("<unknown mutation>")
                .to_owned();
            let function = mutant
                .and_then(|m| m.get("function"))
                .and_then(|f| f.get("name").or(Some(f)))
                .and_then(Value::as_str)
                .map(str::to_owned);

            survivors.push(MutationSurvivor {
                file,
                line,
                mutation,
                function,
            });
        } else {
            // Anything else ("caught", "unviable", "timeout", "success") counts as caught
            // for the kill-rate metric; "unviable" is technically neutral but we treat it
            // as caught so unviable code doesn't drag the kill rate down spuriously.
            caught += 1;
        }
    }

    Ok(MutationReport {
        total,
        caught,
        missed,
        survivors,
    })
}

/// Render survivors as a markdown list for the tool output.
fn render_survivor_list(survivors: &[MutationSurvivor]) -> String {
    if survivors.is_empty() {
        return "All mutations caught — test suite is killing every survivor.\n".to_string();
    }
    let mut s = String::from("Survivors:\n");
    for sv in survivors {
        let fn_part = sv
            .function
            .as_deref()
            .map(|f| format!(" in `{f}`"))
            .unwrap_or_default();
        s.push_str(&format!(
            "- {file}:{line}{fn_part} — {mutation}\n",
            file = sv.file,
            line = sv.line,
            mutation = sv.mutation,
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcomes_json(outcomes: Value) -> String {
        serde_json::to_string(&json!({"outcomes": outcomes})).unwrap()
    }

    #[test]
    fn t52_parser_counts_zero_outcomes_as_no_survivors() {
        let r = parse_outcomes(&outcomes_json(json!([]))).unwrap();
        assert_eq!(r.total, 0);
        assert_eq!(r.caught, 0);
        assert_eq!(r.missed, 0);
        assert!(r.survivors.is_empty());
    }

    #[test]
    fn t52_parser_classifies_caught_outcome() {
        let r = parse_outcomes(&outcomes_json(json!([
            {"summary": "caught"},
            {"summary": "Caught"}, // case-insensitive
        ])))
        .unwrap();
        assert_eq!(r.total, 2);
        assert_eq!(r.caught, 2);
        assert_eq!(r.missed, 0);
        assert!(r.survivors.is_empty());
    }

    #[test]
    fn t52_parser_classifies_missed_as_survivor() {
        let r = parse_outcomes(&outcomes_json(json!([
            {
                "summary": "missed",
                "scenario": {
                    "mutant": {
                        "source_file": {"path": "src/foo.rs", "line": 42},
                        "description": "replace + with -"
                    }
                }
            }
        ])))
        .unwrap();
        assert_eq!(r.total, 1);
        assert_eq!(r.missed, 1);
        assert_eq!(r.caught, 0);
        assert_eq!(r.survivors.len(), 1);
        assert_eq!(r.survivors[0].file, "src/foo.rs");
        assert_eq!(r.survivors[0].line, 42);
        assert!(r.survivors[0].mutation.contains("replace +"));
    }

    #[test]
    fn t52_parser_treats_survived_as_synonym_for_missed() {
        let r = parse_outcomes(&outcomes_json(json!([
            {
                "summary": "survived",
                "scenario": {
                    "mutant": {
                        "source_file": {"path": "x.rs", "line": 1},
                        "description": "delete ?"
                    }
                }
            }
        ])))
        .unwrap();
        assert_eq!(r.missed, 1);
        assert_eq!(r.survivors.len(), 1);
    }

    #[test]
    fn t52_parser_handles_legacy_flat_mutant_shape() {
        // Older cargo-mutants shape: mutant.file / mutant.line / mutant.replacement.
        let r = parse_outcomes(&outcomes_json(json!([
            {
                "summary": "missed",
                "mutant": {
                    "file": "src/legacy.rs",
                    "line": 7,
                    "replacement": "replace > with >="
                }
            }
        ])))
        .unwrap();
        assert_eq!(r.survivors.len(), 1);
        assert_eq!(r.survivors[0].file, "src/legacy.rs");
        assert_eq!(r.survivors[0].line, 7);
    }

    #[test]
    fn t52_parser_ignores_unknown_summaries_as_caught() {
        // Anything not in {missed, survived} counts toward caught — keeps
        // the kill rate stable across cargo-mutants version drift.
        let r = parse_outcomes(&outcomes_json(json!([
            {"summary": "unviable"},
            {"summary": "timeout"},
            {"summary": "success"},
            {"summary": ""},
        ])))
        .unwrap();
        assert_eq!(r.total, 4);
        assert_eq!(r.caught, 4);
        assert_eq!(r.missed, 0);
    }

    #[test]
    fn t52_parser_returns_error_for_missing_outcomes_key() {
        let raw = r#"{"version": 1}"#;
        let err = parse_outcomes(raw).unwrap_err();
        assert!(matches!(err, MutationError::InvalidOutcomes(_)));
    }

    #[test]
    fn t52_parser_returns_error_for_invalid_json() {
        let err = parse_outcomes("not json").unwrap_err();
        assert!(matches!(err, MutationError::InvalidOutcomes(_)));
    }

    #[test]
    fn t52_render_empty_says_all_caught() {
        let s = render_survivor_list(&[]);
        assert!(s.contains("All mutations caught"));
    }

    #[test]
    fn t52_render_survivor_list_includes_file_line_mutation() {
        let sv = vec![MutationSurvivor {
            file: "src/x.rs".into(),
            line: 10,
            mutation: "replace + with -".into(),
            function: Some("calculate".into()),
        }];
        let s = render_survivor_list(&sv);
        assert!(s.contains("src/x.rs:10"));
        assert!(s.contains("calculate"));
        assert!(s.contains("replace +"));
    }

    #[test]
    fn t52_mutation_error_to_tool_error_maps_correctly() {
        let te: ToolError = MutationError::BinaryMissing.into();
        assert!(matches!(te, ToolError::NotFound(_)));

        let te: ToolError = MutationError::InvalidOutcomes("x".into()).into();
        assert!(matches!(te, ToolError::Execution(_)));

        let te: ToolError = MutationError::Subprocess {
            code: Some(2),
            stderr: "boom".into(),
        }
        .into();
        assert!(matches!(te, ToolError::Execution(_)));
    }

    #[test]
    fn t52_tool_id_and_category() {
        let t = GenMutationTestTool::new();
        assert_eq!(t.id(), "gen_mutation_test");
        assert_eq!(t.category(), ToolCategory::Orchestration);
    }

    #[test]
    fn t52_tool_schema_validates() {
        GenMutationTestTool::new().schema().validate().unwrap();
    }

    #[test]
    fn t52_mutation_survivor_serde_roundtrip() {
        let sv = MutationSurvivor {
            file: "src/x.rs".into(),
            line: 12,
            mutation: "replace + with *".into(),
            function: Some("multiply".into()),
        };
        let json = serde_json::to_string(&sv).unwrap();
        let back: MutationSurvivor = serde_json::from_str(&json).unwrap();
        assert_eq!(back, sv);
    }

    #[tokio::test]
    async fn t52_tool_uses_existing_outcomes_when_provided() {
        use tempfile::tempdir;
        use theo_domain::session::{MessageId, SessionId};

        let dir = tempdir().unwrap();
        // Seed an existing outcomes.json under the project root.
        let outcomes_relpath = "mutants.out/outcomes.json";
        let abs = dir.path().join(outcomes_relpath);
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        std::fs::write(
            &abs,
            outcomes_json(json!([
                {"summary": "caught"},
                {"summary": "missed", "scenario": {"mutant": {
                    "source_file": {"path": "src/a.rs", "line": 1},
                    "description": "tweak"
                }}},
            ])),
        )
        .unwrap();

        let (_tx, rx) = tokio::sync::watch::channel(false);
        let ctx = ToolContext {
            session_id: SessionId::new("ses_test"),
            message_id: MessageId::new(""),
            call_id: "call_test".into(),
            agent: "build".into(),
            abort: rx,
            project_dir: dir.path().to_path_buf(),
            graph_context: None,
            stdout_tx: None,
        };
        let mut perms = PermissionCollector::new();

        let result = GenMutationTestTool::new()
            .execute(
                json!({"existing_outcomes": outcomes_relpath}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();
        assert_eq!(result.metadata["total"], 2);
        assert_eq!(result.metadata["caught"], 1);
        assert_eq!(result.metadata["missed"], 1);
    }
}
