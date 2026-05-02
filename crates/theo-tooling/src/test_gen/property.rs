//! T5.1 — `gen_property_test` tool: generates a proptest scaffold.
//!
//! The tool is a deterministic template engine. It does NOT execute the
//! generated tests — the agent runs them via `bash` after generation.
//! This keeps the tool stateless, sandboxable, and trivially testable
//! without `proptest` itself being available at tool-execution time.
//!
//! Generated file shape (single test, single function):
//!
//! ```ignore
//! use proptest::prelude::*;
//! use crate::path::to::module::function_name;
//!
//! proptest! {
//!     #[test]
//!     fn property_function_name(arg0 in any::<u32>(), arg1 in any::<f64>()) {
//!         let _ = function_name(arg0, arg1);
//!     }
//! }
//! ```

use async_trait::async_trait;
use serde_json::{Value, json};

use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

/// Tool that generates a proptest file from a function signature.
pub struct GenPropertyTestTool;

impl Default for GenPropertyTestTool {
    fn default() -> Self {
        Self
    }
}

impl GenPropertyTestTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for GenPropertyTestTool {
    fn id(&self) -> &str {
        "gen_property_test"
    }

    fn description(&self) -> &str {
        "T5.1 — Generate a `proptest` scaffold for a Rust function. Pass \
         `function_path` (Rust module path used in `use`), `function_name`, \
         `strategies` (array of proptest strategy expressions, one per \
         arg, e.g. ['any::<u32>()', 'any::<f64>()']), and `output_path` \
         (where to write the file). The function is invoked with each \
         generated tuple; assertions are left blank for the agent to \
         fill in. Use BEFORE adding logic-heavy code to a public function. \
         Example: \
         gen_property_test({function_path: 'crate::tax::calculate_tax', \
         function_name: 'calculate_tax', strategies: ['any::<f64>()'], \
         output_path: 'tests/calculate_tax_property.rs'})"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "function_path".into(),
                    param_type: "string".into(),
                    description:
                        "Rust module path used in `use` (e.g. `crate::tax::calculate_tax`)."
                            .into(),
                    required: true,
                },
                ToolParam {
                    name: "function_name".into(),
                    param_type: "string".into(),
                    description: "Bare function name (used in the test name and call expression)."
                        .into(),
                    required: true,
                },
                ToolParam {
                    name: "strategies".into(),
                    param_type: "array".into(),
                    description:
                        "Array of proptest strategy expressions, one per argument."
                            .into(),
                    required: true,
                },
                ToolParam {
                    name: "output_path".into(),
                    param_type: "string".into(),
                    description:
                        "Project-relative path where the generated `.rs` file is written."
                            .into(),
                    required: true,
                },
            ],
            input_examples: vec![json!({
                "function_path": "crate::tax::calculate_tax",
                "function_name": "calculate_tax",
                "strategies": ["any::<f64>()", "any::<u32>()"],
                "output_path": "tests/calculate_tax_property.rs"
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
        let function_path = require_string(&args, "function_path")?;
        let function_name = require_string(&args, "function_name")?;
        let output_path = require_string(&args, "output_path")?;
        let strategies_v = args.get("strategies").ok_or_else(|| {
            ToolError::InvalidArgs("missing array `strategies`".into())
        })?;
        let strategies: Vec<String> = serde_json::from_value(strategies_v.clone())
            .map_err(|e| ToolError::InvalidArgs(format!("invalid `strategies`: {e}")))?;

        // Validate inputs cheaply before doing IO.
        validate_function_name(&function_name)?;
        validate_function_path(&function_path)?;
        validate_strategies(&strategies)?;

        let body = render_property_test(&function_path, &function_name, &strategies);

        // Write under the project dir; absolute paths land where the
        // user asked. Path-traversal and external-dir permissioning are
        // out of scope for this tool — the agent is expected to write
        // under the project root.
        let target = ctx.project_dir.join(&output_path);
        if let Some(parent) = target.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent).map_err(ToolError::Io)?;
        }
        std::fs::write(&target, body.as_bytes()).map_err(ToolError::Io)?;

        Ok(ToolOutput {
            title: format!("Generated proptest: {function_name}"),
            output: format!(
                "Wrote {} bytes to {}\n\
                 Run with: cargo test --test {}",
                body.len(),
                target.display(),
                file_stem_for_test_arg(&output_path),
            ),
            metadata: json!({
                "type": "gen_property_test",
                "function_name": function_name,
                "output_path": output_path,
                "n_strategies": strategies.len(),
            }),
            attachments: None,
            llm_suffix: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Pure helpers (testable without filesystem / async)
// ---------------------------------------------------------------------------

fn require_string(args: &Value, key: &str) -> Result<String, ToolError> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| ToolError::InvalidArgs(format!("missing string `{key}`")))
}

/// Render the proptest scaffold given a function path, name, and strategies.
/// Pure function — no IO. Exposed for tests and callers that want the
/// template without writing it to disk.
pub fn render_property_test(
    function_path: &str,
    function_name: &str,
    strategies: &[String],
) -> String {
    let mut params = String::new();
    let mut args = String::new();
    for (i, strategy) in strategies.iter().enumerate() {
        if i > 0 {
            params.push_str(", ");
            args.push_str(", ");
        }
        params.push_str(&format!("arg{i} in {strategy}"));
        args.push_str(&format!("arg{i}"));
    }

    let header = "// Generated by theo `gen_property_test` (T5.1).\n\
                  // Edit the assertions inside the proptest! block; do not \n\
                  // remove the strategy declarations without regenerating.\n";

    if strategies.is_empty() {
        // Zero-arg property tests still compile — they just exercise the
        // function once with no input variation. Useful for purity checks.
        format!(
            "{header}\nuse proptest::prelude::*;\nuse {function_path};\n\n\
             proptest! {{\n    #[test]\n    fn property_{function_name}() {{\n        \
             let _ = {function_name}();\n    }}\n}}\n"
        )
    } else {
        format!(
            "{header}\nuse proptest::prelude::*;\nuse {function_path};\n\n\
             proptest! {{\n    #[test]\n    fn property_{function_name}({params}) {{\n        \
             let _ = {function_name}({args});\n    }}\n}}\n"
        )
    }
}

/// Strict-but-permissive Rust identifier check. Rejects empty strings,
/// names with whitespace or special chars; accepts ASCII alphanumeric +
/// underscore, must not start with a digit. Matches the cargo-test arg
/// conventions enough that generated files stay loadable.
fn validate_function_name(name: &str) -> Result<(), ToolError> {
    if name.is_empty() {
        return Err(ToolError::InvalidArgs("function_name is empty".into()));
    }
    let first = name.chars().next().unwrap();
    if !first.is_ascii_alphabetic() && first != '_' {
        return Err(ToolError::InvalidArgs(
            "function_name must start with letter or underscore".into(),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(ToolError::InvalidArgs(
            "function_name may only contain ASCII alphanumeric or underscore".into(),
        ));
    }
    Ok(())
}

/// Validate that `function_path` looks like `seg1::seg2::...::name` —
/// each segment a valid identifier; at least one segment.
fn validate_function_path(path: &str) -> Result<(), ToolError> {
    if path.is_empty() {
        return Err(ToolError::InvalidArgs("function_path is empty".into()));
    }
    for seg in path.split("::") {
        if seg.is_empty() {
            return Err(ToolError::InvalidArgs(
                "function_path has empty segment (consecutive `::`)".into(),
            ));
        }
        // Each segment must itself be a valid identifier OR `crate`/`super`/`self`.
        if !is_valid_path_segment(seg) {
            return Err(ToolError::InvalidArgs(format!(
                "function_path segment `{seg}` is not a valid Rust identifier"
            )));
        }
    }
    Ok(())
}

fn is_valid_path_segment(seg: &str) -> bool {
    if seg.is_empty() {
        return false;
    }
    let first = match seg.chars().next() {
        Some(c) => c,
        None => return false,
    };
    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }
    seg.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Strategies must each be non-empty. We deliberately do NOT parse them
/// as Rust expressions — the LLM is responsible for valid proptest
/// strategies and the generated file's compilation is the source of
/// truth.
fn validate_strategies(strategies: &[String]) -> Result<(), ToolError> {
    for (i, s) in strategies.iter().enumerate() {
        if s.trim().is_empty() {
            return Err(ToolError::InvalidArgs(format!(
                "strategies[{i}] is empty"
            )));
        }
    }
    Ok(())
}

/// Strip directory + extension from `tests/calculate_tax_property.rs`
/// so we can show `cargo test --test calculate_tax_property`.
fn file_stem_for_test_arg(output_path: &str) -> String {
    std::path::Path::new(output_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("integration")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use theo_domain::session::{MessageId, SessionId};

    fn make_ctx(project_dir: std::path::PathBuf) -> ToolContext {
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

    #[test]
    fn t51_render_with_two_strategies_emits_use_and_proptest_block() {
        let body = render_property_test(
            "crate::math::sum",
            "sum",
            &["any::<i32>()".into(), "any::<i32>()".into()],
        );
        assert!(body.contains("use proptest::prelude::*;"));
        assert!(body.contains("use crate::math::sum;"));
        assert!(body.contains("proptest!"));
        assert!(body.contains("fn property_sum(arg0 in any::<i32>(), arg1 in any::<i32>())"));
        assert!(body.contains("let _ = sum(arg0, arg1);"));
    }

    #[test]
    fn t51_render_zero_args_compiles_form() {
        let body = render_property_test("crate::pure::ping", "ping", &[]);
        assert!(body.contains("fn property_ping()"));
        assert!(body.contains("let _ = ping();"));
    }

    #[test]
    fn t51_render_includes_doc_header_for_audit() {
        let body = render_property_test("crate::x", "x", &["any::<u8>()".into()]);
        assert!(body.contains("Generated by theo"));
    }

    #[test]
    fn t51_validate_function_name_rejects_invalid() {
        assert!(validate_function_name("").is_err());
        assert!(validate_function_name("1bad").is_err());
        assert!(validate_function_name("has space").is_err());
        assert!(validate_function_name("good_name").is_ok());
        assert!(validate_function_name("_underscore").is_ok());
    }

    #[test]
    fn t51_validate_function_path_rejects_consecutive_separators() {
        assert!(validate_function_path("a::::b").is_err());
        assert!(validate_function_path("a::b::c").is_ok());
        assert!(validate_function_path("crate::module::fn").is_ok());
    }

    #[test]
    fn t51_validate_strategies_rejects_empty() {
        assert!(validate_strategies(&["any::<u8>()".into(), "".into()]).is_err());
        assert!(validate_strategies(&["any::<u8>()".into()]).is_ok());
    }

    #[test]
    fn t51_file_stem_strips_dir_and_ext() {
        assert_eq!(file_stem_for_test_arg("tests/foo.rs"), "foo");
        assert_eq!(file_stem_for_test_arg("foo.rs"), "foo");
        assert_eq!(file_stem_for_test_arg("path/to/bar.rs"), "bar");
    }

    #[tokio::test]
    async fn t51_tool_writes_file_to_project_dir() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();
        let tool = GenPropertyTestTool::new();
        let result = tool
            .execute(
                json!({
                    "function_path": "crate::x::add",
                    "function_name": "add",
                    "strategies": ["any::<u32>()", "any::<u32>()"],
                    "output_path": "tests/add_property.rs"
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();
        assert_eq!(result.metadata["function_name"], "add");

        let written = dir.path().join("tests").join("add_property.rs");
        assert!(written.exists());
        let content = std::fs::read_to_string(&written).unwrap();
        assert!(content.contains("fn property_add"));
        assert!(content.contains("use crate::x::add;"));
    }

    #[tokio::test]
    async fn t51_tool_creates_nested_parent_dir() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        GenPropertyTestTool::new()
            .execute(
                json!({
                    "function_path": "crate::x",
                    "function_name": "x",
                    "strategies": [],
                    "output_path": "deeply/nested/path/x_property.rs"
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();
        assert!(
            dir.path()
                .join("deeply/nested/path/x_property.rs")
                .exists()
        );
    }

    #[tokio::test]
    async fn t51_tool_invalid_function_name_returns_error() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        let err = GenPropertyTestTool::new()
            .execute(
                json!({
                    "function_path": "crate::x",
                    "function_name": "1invalid",
                    "strategies": [],
                    "output_path": "x.rs"
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn t51_tool_invalid_strategies_shape_returns_error() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();

        let err = GenPropertyTestTool::new()
            .execute(
                json!({
                    "function_path": "crate::x",
                    "function_name": "x",
                    "strategies": "not an array",
                    "output_path": "x.rs"
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[test]
    fn t51_tool_id_and_category() {
        let t = GenPropertyTestTool::new();
        assert_eq!(t.id(), "gen_property_test");
        assert_eq!(t.category(), ToolCategory::Orchestration);
    }

    #[test]
    fn t51_tool_schema_validates() {
        GenPropertyTestTool::new().schema().validate().unwrap();
    }
}
