//! T13.1 — Agent-callable DAP tool family.
//!
//! Wraps `DapSessionManager` so the agent can drive native debug
//! adapters (`lldb-vscode`, `debugpy`, `dlv dap`, `vscode-js-debug`,
//! `java-debug-server`) for live debugging sessions.
//!
//! Sessions are caller-keyed: the agent picks a `session_id`
//! (e.g. `"rust-bin-foo"`) and uses it across every debug_* call to
//! refer back to the same adapter process. `debug_launch` opens the
//! session; `debug_terminate` closes it (the adapter dies via
//! `kill_on_drop`).
//!
//! All tools share one `Arc<DapSessionManager>` so a future agent
//! workflow that spawns multiple debug sessions concurrently can do
//! so without fighting for state.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

use crate::dap::client::DapClient;
use crate::dap::protocol::DapResponse;
use crate::dap::session_manager::{DapSessionError, DapSessionManager};

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn parse_session_id(args: &Value) -> Result<String, ToolError> {
    let id = args
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidArgs("missing string `session_id`".into()))?
        .trim()
        .to_string();
    if id.is_empty() {
        return Err(ToolError::InvalidArgs("`session_id` is empty".into()));
    }
    Ok(id)
}

fn map_session_error(err: DapSessionError) -> ToolError {
    match err {
        DapSessionError::NoAdapterForLanguage { language } => ToolError::Execution(format!(
            "no DAP adapter installed for language `{language}`. Install one \
             (e.g. lldb-vscode for rust/c/cpp, debugpy for python, dlv for go, \
             js-debug-adapter for javascript/typescript) or fall back to print \
             debugging."
        )),
        DapSessionError::SessionAlreadyExists { id } => ToolError::InvalidArgs(format!(
            "debug session id `{id}` is already active. Pick a different \
             session_id, or call `debug_terminate({{session_id: \"{id}\"}})` \
             first."
        )),
        DapSessionError::InitializeFailed(msg) => ToolError::Execution(format!(
            "DAP `initialize` failed: {msg}"
        )),
        DapSessionError::LaunchFailed(msg) => ToolError::Execution(format!(
            "DAP `launch` failed: {msg}"
        )),
        DapSessionError::AttachFailed(msg) => ToolError::Execution(format!(
            "DAP `attach` failed: {msg}"
        )),
        DapSessionError::Client(e) => ToolError::Execution(format!("DAP client error: {e}")),
    }
}

fn require_session(
    manager: &DapSessionManager,
    session_id: &str,
) -> impl std::future::Future<
    Output = Result<Arc<DapClient<tokio::process::ChildStdin>>, ToolError>,
> + Send {
    let id = session_id.to_string();
    let manager = manager.clone();
    async move {
        manager.session(&id).await.ok_or_else(|| {
            ToolError::Execution(format!(
                "no active debug session with id `{id}`. Call `debug_launch` \
                 first to open one."
            ))
        })
    }
}

fn check_response(resp: &DapResponse, command: &str) -> Result<(), ToolError> {
    if !resp.success {
        let msg = resp
            .message
            .as_deref()
            .unwrap_or("(no message)");
        return Err(ToolError::Execution(format!(
            "DAP `{command}` failed: {msg}"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// `debug_status`
// ---------------------------------------------------------------------------

/// `debug_status` — report which DAP adapters are discoverable on
/// PATH and which active sessions exist. Lets the agent decide
/// whether to debug at all OR fall back to print-debugging /
/// reading test output.
pub struct DebugStatusTool {
    manager: Arc<DapSessionManager>,
}

impl DebugStatusTool {
    pub fn new(manager: Arc<DapSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for DebugStatusTool {
    fn id(&self) -> &str {
        "debug_status"
    }

    fn description(&self) -> &str {
        "T13.1 — Report which DAP adapters are discoverable on PATH \
         (lldb-vscode for rust/c/cpp; debugpy for python; dlv for go; \
         js-debug-adapter for js/ts; java-debug-server for java) AND list \
         the currently active debug sessions (id + language). Use BEFORE \
         `debug_launch` to know whether the language has an installed \
         adapter; when none, fall back to print-debugging or reading test \
         output. \
         Example: debug_status({})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![],
            input_examples: vec![json!({})],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    async fn execute(
        &self,
        _args: Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let mut languages = self.manager.supported_languages().await;
        languages.sort();

        let active = self.manager.active_sessions().await;
        let mut active_sorted = active.clone();
        active_sorted.sort();

        let lang_count = languages.len();
        let active_count = active_sorted.len();
        let metadata = json!({
            "type": "debug_status",
            "supported_language_count": lang_count,
            "supported_languages": languages,
            "active_session_count": active_count,
            "active_session_ids": active_sorted,
        });

        let mut output = String::new();
        if languages.is_empty() {
            output.push_str(
                "No DAP adapters discovered on PATH. Install one for the \
                 languages you debug (lldb-vscode for rust/c/cpp, debugpy \
                 for python, dlv for go, js-debug-adapter for js/ts) or \
                 fall back to print-debugging.\n",
            );
        } else {
            output.push_str(&format!(
                "{lang_count} language(s) routable to an installed DAP \
                 adapter: {}.\n",
                languages.join(", ")
            ));
        }
        if active_sorted.is_empty() {
            output.push_str(
                "No active debug sessions. Open one with `debug_launch`.",
            );
        } else {
            output.push_str(&format!(
                "{active_count} active debug session(s): {}. Use the id \
                 with debug_set_breakpoint / debug_continue / debug_step / \
                 debug_eval / debug_terminate.",
                active_sorted.join(", "),
            ));
        }

        Ok(ToolOutput::new(
            format!(
                "debug_status: {lang_count} language(s) routable, \
                 {active_count} active session(s)"
            ),
            output,
        )
        .with_metadata(metadata))
    }
}

// ---------------------------------------------------------------------------
// `debug_launch`
// ---------------------------------------------------------------------------

/// `debug_launch` — start a new debug session, spawning the adapter
/// for `language` and issuing the DAP `launch` request.
pub struct DebugLaunchTool {
    manager: Arc<DapSessionManager>,
}

impl DebugLaunchTool {
    pub fn new(manager: Arc<DapSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for DebugLaunchTool {
    fn id(&self) -> &str {
        "debug_launch"
    }

    fn description(&self) -> &str {
        "T13.1 — Start a new debug session via DAP. Spawns the right adapter \
         for the language (lldb-vscode for rust/c/cpp, debugpy for python, dlv \
         for go, js-debug-adapter for js/ts). Pass a unique `session_id` you'll \
         reuse across debug_* calls (e.g. \"my-bin-foo\"). The `program` field \
         is the binary or script to debug; `cwd`, `args`, and `env` are \
         optional. After launch, set breakpoints with `debug_set_breakpoint`, \
         then resume with `debug_continue`. Always pair with `debug_terminate` \
         to free the adapter. \
         Example: debug_launch({session_id: \"a\", language: \"rust\", program: \"target/debug/myapp\", args: [\"--flag\"]})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "session_id".into(),
                    param_type: "string".into(),
                    description:
                        "Caller-chosen identifier for this session. Reuse the \
                         same id across set_breakpoint/continue/terminate."
                            .into(),
                    required: true,
                },
                ToolParam {
                    name: "language".into(),
                    param_type: "string".into(),
                    description:
                        "Language of the debuggee — drives adapter selection \
                         (rust, c, cpp, python, go, javascript, typescript, java)."
                            .into(),
                    required: true,
                },
                ToolParam {
                    name: "program".into(),
                    param_type: "string".into(),
                    description:
                        "Absolute path to the program / script to debug."
                            .into(),
                    required: true,
                },
                ToolParam {
                    name: "args".into(),
                    param_type: "array".into(),
                    description:
                        "Optional CLI arguments passed to the debuggee."
                            .into(),
                    required: false,
                },
                ToolParam {
                    name: "cwd".into(),
                    param_type: "string".into(),
                    description:
                        "Optional working directory for the debuggee."
                            .into(),
                    required: false,
                },
                ToolParam {
                    name: "env".into(),
                    param_type: "object".into(),
                    description:
                        "Optional env vars merged with the inherited env."
                            .into(),
                    required: false,
                },
                ToolParam {
                    name: "stop_on_entry".into(),
                    param_type: "boolean".into(),
                    description:
                        "When true, the adapter pauses at the first \
                         instruction so the agent can set breakpoints \
                         BEFORE execution starts. Default: true."
                            .into(),
                    required: false,
                },
            ],
            input_examples: vec![json!({
                "session_id": "a",
                "language": "rust",
                "program": "/abs/path/target/debug/myapp",
                "args": ["--flag"],
                "stop_on_entry": true,
            })],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    async fn execute(
        &self,
        args: Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let session_id = parse_session_id(&args)?;
        let language = args
            .get("language")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArgs("missing string `language`".into()))?
            .to_string();
        let program = args
            .get("program")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArgs("missing string `program`".into()))?
            .to_string();
        if program.trim().is_empty() {
            return Err(ToolError::InvalidArgs("`program` is empty".into()));
        }
        let stop_on_entry = args
            .get("stop_on_entry")
            .and_then(Value::as_bool)
            .unwrap_or(true);

        let mut launch_args = json!({
            "program": program,
            "stopOnEntry": stop_on_entry,
        });
        if let Some(prog_args) = args.get("args").cloned() {
            launch_args["args"] = prog_args;
        }
        if let Some(cwd) = args.get("cwd").cloned() {
            launch_args["cwd"] = cwd;
        }
        if let Some(env) = args.get("env").cloned() {
            launch_args["env"] = env;
        }

        let _client = self
            .manager
            .launch(&session_id, &language, launch_args)
            .await
            .map_err(map_session_error)?;

        Ok(ToolOutput::new(
            format!("debug_launch: session `{session_id}` ready"),
            format!(
                "Debug session `{session_id}` (language={language}, program={program}) is ready.\n\
                 Set breakpoints with debug_set_breakpoint, then resume with debug_continue.\n\
                 Always call debug_terminate({{session_id: \"{session_id}\"}}) when done."
            ),
        )
        .with_metadata(json!({
            "type": "debug_launch",
            "session_id": session_id,
            "language": language,
            "program": program,
            "stop_on_entry": stop_on_entry,
        })))
    }
}

// ---------------------------------------------------------------------------
// `debug_set_breakpoint`
// ---------------------------------------------------------------------------

/// `debug_set_breakpoint` — set line breakpoints in a file.
pub struct DebugSetBreakpointTool {
    manager: Arc<DapSessionManager>,
}

impl DebugSetBreakpointTool {
    pub fn new(manager: Arc<DapSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for DebugSetBreakpointTool {
    fn id(&self) -> &str {
        "debug_set_breakpoint"
    }

    fn description(&self) -> &str {
        "T13.1 — Set line breakpoints in `file_path` for the active debug \
         session. Pass the FULL list of lines you want active in the file — DAP \
         setBreakpoints REPLACES every breakpoint in that source. To clear, \
         pass an empty `lines` array. The adapter returns a per-breakpoint \
         `verified` flag indicating whether the line is debuggable. Pair with \
         `debug_continue` after setting. \
         Example: debug_set_breakpoint({session_id: \"a\", file_path: \"/abs/src/main.rs\", lines: [10, 25, 42]})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "session_id".into(),
                    param_type: "string".into(),
                    description: "ID of the active debug session.".into(),
                    required: true,
                },
                ToolParam {
                    name: "file_path".into(),
                    param_type: "string".into(),
                    description:
                        "Absolute path to the source file containing the \
                         breakpoints."
                            .into(),
                    required: true,
                },
                ToolParam {
                    name: "lines".into(),
                    param_type: "array".into(),
                    description:
                        "Full list of 1-based line numbers to break on in this \
                         file. DAP setBreakpoints REPLACES the previous set, so \
                         pass every line you want active. Empty array clears \
                         all breakpoints in the file."
                            .into(),
                    required: true,
                },
            ],
            input_examples: vec![json!({
                "session_id": "a",
                "file_path": "/abs/src/main.rs",
                "lines": [10, 25, 42],
            })],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    async fn execute(
        &self,
        args: Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let session_id = parse_session_id(&args)?;
        let file_path = args
            .get("file_path")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArgs("missing string `file_path`".into()))?
            .to_string();
        let raw_lines = args
            .get("lines")
            .and_then(Value::as_array)
            .ok_or_else(|| ToolError::InvalidArgs("missing array `lines`".into()))?;
        let lines: Vec<u64> = raw_lines
            .iter()
            .map(|v| {
                v.as_u64().ok_or_else(|| {
                    ToolError::InvalidArgs(
                        "`lines` must be an array of positive integers".into(),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        for &line in &lines {
            if line == 0 {
                return Err(ToolError::InvalidArgs(
                    "DAP line numbers are 1-based; 0 is not a valid breakpoint line".into(),
                ));
            }
        }

        let client = require_session(&self.manager, &session_id).await?;
        let breakpoints: Vec<Value> =
            lines.iter().map(|l| json!({ "line": l })).collect();
        let params = json!({
            "source": {"path": file_path},
            "breakpoints": breakpoints,
            "lines": lines, // legacy DAP field — some adapters require it
        });
        let resp = client
            .request("setBreakpoints", Some(params))
            .await
            .map_err(|e| ToolError::Execution(format!("setBreakpoints failed: {e}")))?;
        check_response(&resp, "setBreakpoints")?;
        Ok(format_set_breakpoint_output(&resp, &session_id, &file_path, &lines))
    }
}

fn format_set_breakpoint_output(
    resp: &DapResponse,
    session_id: &str,
    file_path: &str,
    requested: &[u64],
) -> ToolOutput {
    let body_breakpoints = resp
        .body
        .as_ref()
        .and_then(|b| b.get("breakpoints"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let summary: Vec<Value> = body_breakpoints
        .iter()
        .enumerate()
        .map(|(i, bp)| {
            let verified = bp.get("verified").and_then(Value::as_bool).unwrap_or(false);
            let line = bp.get("line").and_then(Value::as_u64);
            let message = bp.get("message").and_then(Value::as_str);
            json!({
                "index": i,
                "requested_line": requested.get(i).copied(),
                "actual_line": line,
                "verified": verified,
                "message": message,
            })
        })
        .collect();
    let verified_count = body_breakpoints
        .iter()
        .filter(|bp| bp.get("verified").and_then(Value::as_bool).unwrap_or(false))
        .count();
    let unverified_count = body_breakpoints.len().saturating_sub(verified_count);

    let mut out = format!(
        "debug_set_breakpoint: {} requested, {} verified, {} unverified in `{file_path}` (session=`{session_id}`)\n\n",
        requested.len(),
        verified_count,
        unverified_count,
    );
    for s in &summary {
        out.push_str(&format!(
            "  - line {req:?} → actual {act:?}  verified={ver}  msg={msg:?}\n",
            req = s["requested_line"],
            act = s["actual_line"],
            ver = s["verified"],
            msg = s["message"],
        ));
    }

    ToolOutput::new(
        format!(
            "debug_set_breakpoint: {} verified / {} requested",
            verified_count,
            requested.len()
        ),
        out,
    )
    .with_metadata(json!({
        "type": "debug_set_breakpoint",
        "session_id": session_id,
        "file_path": file_path,
        "requested": requested,
        "verified_count": verified_count,
        "unverified_count": unverified_count,
        "breakpoints": summary,
    }))
}

// ---------------------------------------------------------------------------
// `debug_continue`
// ---------------------------------------------------------------------------

/// `debug_continue` — resume the debuggee after a breakpoint hit.
pub struct DebugContinueTool {
    manager: Arc<DapSessionManager>,
}

impl DebugContinueTool {
    pub fn new(manager: Arc<DapSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for DebugContinueTool {
    fn id(&self) -> &str {
        "debug_continue"
    }

    fn description(&self) -> &str {
        "T13.1 — Resume execution of the debuggee. Pass `thread_id` to resume \
         a specific thread, or omit it to resume all threads (default). The \
         response indicates whether all threads were resumed. Typically called \
         after `debug_set_breakpoint` to start running, or after inspecting \
         state at a breakpoint hit. \
         Example: debug_continue({session_id: \"a\"})  // resume all threads."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "session_id".into(),
                    param_type: "string".into(),
                    description: "ID of the active debug session.".into(),
                    required: true,
                },
                ToolParam {
                    name: "thread_id".into(),
                    param_type: "integer".into(),
                    description:
                        "Optional thread to resume. When omitted, all threads \
                         are resumed."
                            .into(),
                    required: false,
                },
            ],
            input_examples: vec![json!({"session_id": "a"})],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    async fn execute(
        &self,
        args: Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let session_id = parse_session_id(&args)?;
        let thread_id = args.get("thread_id").and_then(Value::as_u64);

        let client = require_session(&self.manager, &session_id).await?;
        let mut params = json!({});
        if let Some(t) = thread_id {
            params["threadId"] = json!(t);
        } else {
            // Many adapters require threadId — pass 0 with `singleThread=false`
            // when omitted, per DAP spec.
            params["threadId"] = json!(0);
            params["singleThread"] = json!(false);
        }
        let resp = client
            .request("continue", Some(params))
            .await
            .map_err(|e| ToolError::Execution(format!("continue failed: {e}")))?;
        check_response(&resp, "continue")?;
        let all_resumed = resp
            .body
            .as_ref()
            .and_then(|b| b.get("allThreadsContinued"))
            .and_then(Value::as_bool)
            .unwrap_or(thread_id.is_none());
        Ok(ToolOutput::new(
            format!(
                "debug_continue: session `{session_id}` resumed{}",
                if all_resumed { " (all threads)" } else { "" }
            ),
            format!(
                "Debugger resumed.{} Watch for `stopped` events on the \
                 next breakpoint hit; capture state with future debug_eval / \
                 debug_stack_trace tools.",
                if all_resumed { " All threads continued." } else { " Single thread continued." }
            ),
        )
        .with_metadata(json!({
            "type": "debug_continue",
            "session_id": session_id,
            "thread_id": thread_id,
            "all_threads_continued": all_resumed,
        })))
    }
}

// ---------------------------------------------------------------------------
// `debug_step`
// ---------------------------------------------------------------------------

/// `debug_step` — single-step the debuggee. Three step kinds:
///   - `over` (DAP `next`):    step the current line, skipping calls.
///   - `in`   (DAP `stepIn`):  step into the next function call.
///   - `out`  (DAP `stepOut`): run until the current frame returns.
pub struct DebugStepTool {
    manager: Arc<DapSessionManager>,
}

impl DebugStepTool {
    pub fn new(manager: Arc<DapSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for DebugStepTool {
    fn id(&self) -> &str {
        "debug_step"
    }

    fn description(&self) -> &str {
        "T13.1 — Single-step the debuggee. `kind` selects the step style: \
         \"over\" (DAP `next`, skip calls), \"in\" (DAP `stepIn`, descend into \
         the next call), \"out\" (DAP `stepOut`, run until current frame \
         returns). `thread_id` IS REQUIRED by DAP for stepping (unlike \
         continue) — get it from the most recent `stopped` event or DAP \
         `threads` request. Watch for the next `stopped` event after stepping. \
         Example: debug_step({session_id: \"a\", kind: \"over\", thread_id: 1})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "session_id".into(),
                    param_type: "string".into(),
                    description: "ID of the active debug session.".into(),
                    required: true,
                },
                ToolParam {
                    name: "kind".into(),
                    param_type: "string".into(),
                    description:
                        "One of: `over` (step skipping function calls), \
                         `in` (step into the next call), `out` (run until \
                         current frame returns)."
                            .into(),
                    required: true,
                },
                ToolParam {
                    name: "thread_id".into(),
                    param_type: "integer".into(),
                    description:
                        "Thread to step. Required by DAP — get it from the \
                         most recent `stopped` event."
                            .into(),
                    required: true,
                },
            ],
            input_examples: vec![
                json!({"session_id": "a", "kind": "over", "thread_id": 1}),
                json!({"session_id": "a", "kind": "in", "thread_id": 1}),
                json!({"session_id": "a", "kind": "out", "thread_id": 1}),
            ],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    async fn execute(
        &self,
        args: Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let session_id = parse_session_id(&args)?;
        let kind = args
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArgs("missing string `kind`".into()))?;
        let dap_command = match kind {
            "over" => "next",
            "in" => "stepIn",
            "out" => "stepOut",
            other => {
                return Err(ToolError::InvalidArgs(format!(
                    "`kind` must be one of `over`, `in`, `out` (got `{other}`)"
                )));
            }
        };
        let thread_id = args
            .get("thread_id")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                ToolError::InvalidArgs(
                    "missing integer `thread_id` — DAP step requires it (get it from the most recent stopped event)"
                        .into(),
                )
            })?;

        let client = require_session(&self.manager, &session_id).await?;
        let resp = client
            .request(dap_command, Some(json!({"threadId": thread_id})))
            .await
            .map_err(|e| ToolError::Execution(format!("{dap_command} failed: {e}")))?;
        check_response(&resp, dap_command)?;
        Ok(ToolOutput::new(
            format!(
                "debug_step: session `{session_id}` stepped {kind} (thread {thread_id})"
            ),
            format!(
                "Step {kind} executed via DAP `{dap_command}`. Watch for the \
                 next `stopped` event to inspect new state."
            ),
        )
        .with_metadata(json!({
            "type": "debug_step",
            "session_id": session_id,
            "kind": kind,
            "dap_command": dap_command,
            "thread_id": thread_id,
        })))
    }
}

// ---------------------------------------------------------------------------
// `debug_eval`
// ---------------------------------------------------------------------------

/// `debug_eval` — evaluate an expression in the debuggee's context.
pub struct DebugEvalTool {
    manager: Arc<DapSessionManager>,
}

impl DebugEvalTool {
    pub fn new(manager: Arc<DapSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for DebugEvalTool {
    fn id(&self) -> &str {
        "debug_eval"
    }

    fn description(&self) -> &str {
        "T13.1 — Evaluate `expression` in the debuggee's current context. \
         Pass `frame_id` to evaluate inside a specific stack frame (get it \
         from a future debug_stack_trace tool); omit to evaluate in the \
         global / top frame the adapter chooses. `context` selects the eval \
         mode: \"watch\" (default — read-only inspection), \"repl\" (allows \
         side effects in some adapters), \"hover\" (terse). Returns the \
         result string, type, and a variablesReference for drill-down. \
         Example: debug_eval({session_id: \"a\", expression: \"my_var.field\"})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "session_id".into(),
                    param_type: "string".into(),
                    description: "ID of the active debug session.".into(),
                    required: true,
                },
                ToolParam {
                    name: "expression".into(),
                    param_type: "string".into(),
                    description:
                        "Expression to evaluate (language-specific syntax — \
                         the adapter parses it with the debuggee's parser)."
                            .into(),
                    required: true,
                },
                ToolParam {
                    name: "frame_id".into(),
                    param_type: "integer".into(),
                    description:
                        "Optional stack frame to evaluate in. Get from \
                         debug_stack_trace; omit for the adapter's default."
                            .into(),
                    required: false,
                },
                ToolParam {
                    name: "context".into(),
                    param_type: "string".into(),
                    description:
                        "DAP eval context: `watch` (default, read-only), \
                         `repl` (may have side effects), `hover` (terse)."
                            .into(),
                    required: false,
                },
            ],
            input_examples: vec![
                json!({"session_id": "a", "expression": "my_var"}),
                json!({"session_id": "a", "expression": "x + y", "frame_id": 7}),
            ],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    async fn execute(
        &self,
        args: Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let session_id = parse_session_id(&args)?;
        let expression = args
            .get("expression")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArgs("missing string `expression`".into()))?
            .to_string();
        if expression.trim().is_empty() {
            return Err(ToolError::InvalidArgs("`expression` is empty".into()));
        }
        let frame_id = args.get("frame_id").and_then(Value::as_u64);
        let context = args
            .get("context")
            .and_then(Value::as_str)
            .unwrap_or("watch")
            .to_string();
        if !["watch", "repl", "hover"].contains(&context.as_str()) {
            return Err(ToolError::InvalidArgs(format!(
                "`context` must be `watch`, `repl`, or `hover` (got `{context}`)"
            )));
        }

        let client = require_session(&self.manager, &session_id).await?;
        let mut params = json!({
            "expression": expression,
            "context": context,
        });
        if let Some(f) = frame_id {
            params["frameId"] = json!(f);
        }
        let resp = client
            .request("evaluate", Some(params))
            .await
            .map_err(|e| ToolError::Execution(format!("evaluate failed: {e}")))?;
        check_response(&resp, "evaluate")?;
        Ok(format_eval_output(&resp, &session_id, &expression, &context, frame_id))
    }
}

fn format_eval_output(
    resp: &DapResponse,
    session_id: &str,
    expression: &str,
    context: &str,
    frame_id: Option<u64>,
) -> ToolOutput {
    let body = resp.body.as_ref();
    let result = body
        .and_then(|b| b.get("result"))
        .and_then(Value::as_str)
        .unwrap_or("(no result)")
        .to_string();
    let ty = body
        .and_then(|b| b.get("type"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let variables_reference = body
        .and_then(|b| b.get("variablesReference"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let memory_reference = body
        .and_then(|b| b.get("memoryReference"))
        .and_then(Value::as_str)
        .map(str::to_string);

    let preview = result.lines().next().unwrap_or("");
    let title = format!("debug_eval: {preview}");
    let mut output = format!(
        "expression: {expression}\nresult: {result}"
    );
    if let Some(ref t) = ty {
        output.push_str(&format!("\ntype: {t}"));
    }
    if variables_reference > 0 {
        output.push_str(&format!(
            "\nvariablesReference: {variables_reference} (drill down with future debug_variables)"
        ));
    }
    if let Some(ref m) = memory_reference {
        output.push_str(&format!("\nmemoryReference: {m}"));
    }

    ToolOutput::new(title, output).with_metadata(json!({
        "type": "debug_eval",
        "session_id": session_id,
        "expression": expression,
        "context": context,
        "frame_id": frame_id,
        "result": result,
        "value_type": ty,
        "variables_reference": variables_reference,
        "memory_reference": memory_reference,
    }))
}

// ---------------------------------------------------------------------------
// `debug_stack_trace`
// ---------------------------------------------------------------------------

/// `debug_stack_trace` — fetch the call stack for a thread.
pub struct DebugStackTraceTool {
    manager: Arc<DapSessionManager>,
}

impl DebugStackTraceTool {
    pub fn new(manager: Arc<DapSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for DebugStackTraceTool {
    fn id(&self) -> &str {
        "debug_stack_trace"
    }

    fn description(&self) -> &str {
        "T13.1 — Fetch the call stack for `thread_id` (DAP `stackTrace`). \
         Use after a `stopped` event to see where execution paused. \
         `start_frame` (default 0) skips that many top frames; `levels` \
         (default 20) caps the slice. Each frame carries an `id` you can pass \
         to `debug_eval(frame_id)` or future `debug_scopes` to inspect locals \
         at that level. \
         Example: debug_stack_trace({session_id: \"a\", thread_id: 1})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "session_id".into(),
                    param_type: "string".into(),
                    description: "ID of the active debug session.".into(),
                    required: true,
                },
                ToolParam {
                    name: "thread_id".into(),
                    param_type: "integer".into(),
                    description:
                        "Thread to fetch the stack for. Get it from a \
                         `stopped` event or future debug_threads tool."
                            .into(),
                    required: true,
                },
                ToolParam {
                    name: "start_frame".into(),
                    param_type: "integer".into(),
                    description: "Skip this many top frames. Default 0.".into(),
                    required: false,
                },
                ToolParam {
                    name: "levels".into(),
                    param_type: "integer".into(),
                    description:
                        "Cap on frames returned. Default 20 — increase to see \
                         deeper stacks. 0 = adapter default (often unlimited)."
                            .into(),
                    required: false,
                },
            ],
            input_examples: vec![
                json!({"session_id": "a", "thread_id": 1}),
                json!({"session_id": "a", "thread_id": 1, "start_frame": 5, "levels": 10}),
            ],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    async fn execute(
        &self,
        args: Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let session_id = parse_session_id(&args)?;
        let thread_id = args
            .get("thread_id")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                ToolError::InvalidArgs("missing integer `thread_id`".into())
            })?;
        let start_frame = args
            .get("start_frame")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let levels = args.get("levels").and_then(Value::as_u64).unwrap_or(20);

        let client = require_session(&self.manager, &session_id).await?;
        let mut params = json!({
            "threadId": thread_id,
            "startFrame": start_frame,
        });
        if levels > 0 {
            params["levels"] = json!(levels);
        }
        let resp = client
            .request("stackTrace", Some(params))
            .await
            .map_err(|e| ToolError::Execution(format!("stackTrace failed: {e}")))?;
        check_response(&resp, "stackTrace")?;
        Ok(format_stack_trace_output(
            &resp,
            &session_id,
            thread_id,
            start_frame,
            levels,
        ))
    }
}

fn format_stack_trace_output(
    resp: &DapResponse,
    session_id: &str,
    thread_id: u64,
    start_frame: u64,
    levels: u64,
) -> ToolOutput {
    let body = resp.body.as_ref();
    let frames = body
        .and_then(|b| b.get("stackFrames"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let total_frames = body
        .and_then(|b| b.get("totalFrames"))
        .and_then(Value::as_u64);

    let summary: Vec<Value> = frames
        .iter()
        .map(|f| {
            json!({
                "id": f.get("id").and_then(Value::as_u64),
                "name": f.get("name").and_then(Value::as_str),
                "source_path": f
                    .get("source")
                    .and_then(|s| s.get("path"))
                    .and_then(Value::as_str),
                "line": f.get("line").and_then(Value::as_u64),
                "column": f.get("column").and_then(Value::as_u64),
            })
        })
        .collect();

    let mut out = format!(
        "debug_stack_trace: {} frame(s){} (session=`{session_id}`, thread={thread_id})\n\n",
        frames.len(),
        match total_frames {
            Some(n) => format!(" of {n} total"),
            None => String::new(),
        }
    );
    for (i, f) in summary.iter().enumerate() {
        let frame_num = start_frame + i as u64;
        out.push_str(&format!(
            "  #{frame_num}  id={id}  {name}  {path}:{line}:{col}\n",
            id = f["id"],
            name = f["name"].as_str().unwrap_or("(no name)"),
            path = f["source_path"].as_str().unwrap_or("(no source)"),
            line = f["line"],
            col = f["column"],
        ));
    }
    if frames.is_empty() {
        out.push_str("(no frames returned — thread may not be in a stopped state)\n");
    }

    ToolOutput::new(
        format!(
            "debug_stack_trace: {} frame(s) (thread {thread_id})",
            frames.len()
        ),
        out,
    )
    .with_metadata(json!({
        "type": "debug_stack_trace",
        "session_id": session_id,
        "thread_id": thread_id,
        "start_frame": start_frame,
        "levels": levels,
        "total_frames": total_frames,
        "frame_count": frames.len(),
        "frames": summary,
    }))
}

// ---------------------------------------------------------------------------
// `debug_variables`
// ---------------------------------------------------------------------------

/// `debug_variables` — drill into a variables container.
pub struct DebugVariablesTool {
    manager: Arc<DapSessionManager>,
}

impl DebugVariablesTool {
    pub fn new(manager: Arc<DapSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for DebugVariablesTool {
    fn id(&self) -> &str {
        "debug_variables"
    }

    fn description(&self) -> &str {
        "T13.1 — List children of a `variables_reference` (DAP `variables`). \
         The reference comes from `debug_eval` (when the result is structured: \
         struct, vec, hashmap), or from a future `debug_scopes` tool. \
         Returns each child's name, value preview, type, and its OWN \
         variables_reference (so the agent can recursively descend). Optional \
         `start` + `count` paginate large containers (e.g. a Vec<T> with \
         100k items). \
         Example: debug_variables({session_id: \"a\", variables_reference: 7})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "session_id".into(),
                    param_type: "string".into(),
                    description: "ID of the active debug session.".into(),
                    required: true,
                },
                ToolParam {
                    name: "variables_reference".into(),
                    param_type: "integer".into(),
                    description:
                        "Container reference from a previous `debug_eval` or \
                         `debug_scopes` response. MUST be > 0; references \
                         marked as 0 in the parent are non-drillable scalars."
                            .into(),
                    required: true,
                },
                ToolParam {
                    name: "start".into(),
                    param_type: "integer".into(),
                    description:
                        "Index to start at when paginating. Default 0."
                            .into(),
                    required: false,
                },
                ToolParam {
                    name: "count".into(),
                    param_type: "integer".into(),
                    description:
                        "Max children to return. Default 100. 0 = adapter \
                         default (often unlimited — risky for huge containers)."
                            .into(),
                    required: false,
                },
                ToolParam {
                    name: "filter".into(),
                    param_type: "string".into(),
                    description:
                        "DAP child filter: `indexed` (array entries) or \
                         `named` (struct fields). Omit to get both."
                            .into(),
                    required: false,
                },
            ],
            input_examples: vec![
                json!({"session_id": "a", "variables_reference": 7}),
                json!({"session_id": "a", "variables_reference": 7, "start": 0, "count": 50}),
            ],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    async fn execute(
        &self,
        args: Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let session_id = parse_session_id(&args)?;
        let var_ref = args
            .get("variables_reference")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                ToolError::InvalidArgs(
                    "missing integer `variables_reference`".into(),
                )
            })?;
        if var_ref == 0 {
            return Err(ToolError::InvalidArgs(
                "`variables_reference` must be > 0; reference == 0 marks \
                 a scalar value with no children to drill into"
                    .into(),
            ));
        }
        let start = args.get("start").and_then(Value::as_u64).unwrap_or(0);
        let count = args.get("count").and_then(Value::as_u64).unwrap_or(100);
        let filter = args.get("filter").and_then(Value::as_str).map(str::to_string);
        if let Some(ref f) = filter
            && !["indexed", "named"].contains(&f.as_str())
        {
            return Err(ToolError::InvalidArgs(format!(
                "`filter` must be `indexed` or `named` (got `{f}`)"
            )));
        }

        let client = require_session(&self.manager, &session_id).await?;
        let mut params = json!({
            "variablesReference": var_ref,
            "start": start,
        });
        if count > 0 {
            params["count"] = json!(count);
        }
        if let Some(ref f) = filter {
            params["filter"] = json!(f);
        }
        let resp = client
            .request("variables", Some(params))
            .await
            .map_err(|e| ToolError::Execution(format!("variables failed: {e}")))?;
        check_response(&resp, "variables")?;
        Ok(format_variables_output(
            &resp,
            &session_id,
            var_ref,
            start,
            count,
            filter.as_deref(),
        ))
    }
}

fn format_variables_output(
    resp: &DapResponse,
    session_id: &str,
    variables_reference: u64,
    start: u64,
    count: u64,
    filter: Option<&str>,
) -> ToolOutput {
    let vars = resp
        .body
        .as_ref()
        .and_then(|b| b.get("variables"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let summary: Vec<Value> = vars
        .iter()
        .map(|v| {
            json!({
                "name": v.get("name").and_then(Value::as_str),
                "value": v.get("value").and_then(Value::as_str),
                "type": v.get("type").and_then(Value::as_str),
                "variables_reference": v
                    .get("variablesReference")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                "evaluate_name": v.get("evaluateName").and_then(Value::as_str),
            })
        })
        .collect();

    let drillable_count = summary
        .iter()
        .filter(|s| s["variables_reference"].as_u64().unwrap_or(0) > 0)
        .count();

    let mut out = format!(
        "debug_variables: {} child(ren), {} drillable (session=`{session_id}`, ref={variables_reference})\n\n",
        summary.len(),
        drillable_count,
    );
    for s in &summary {
        let drill_hint = if s["variables_reference"].as_u64().unwrap_or(0) > 0 {
            format!("  [drill: {}]", s["variables_reference"])
        } else {
            String::new()
        };
        out.push_str(&format!(
            "  {name} = {value}{ty}{drill}\n",
            name = s["name"].as_str().unwrap_or("(unnamed)"),
            value = s["value"].as_str().unwrap_or("?"),
            ty = match s["type"].as_str() {
                Some(t) => format!("  : {t}"),
                None => String::new(),
            },
            drill = drill_hint,
        ));
    }

    ToolOutput::new(
        format!(
            "debug_variables: {} child(ren), {} drillable",
            summary.len(),
            drillable_count
        ),
        out,
    )
    .with_metadata(json!({
        "type": "debug_variables",
        "session_id": session_id,
        "variables_reference": variables_reference,
        "start": start,
        "count": count,
        "filter": filter,
        "child_count": summary.len(),
        "drillable_count": drillable_count,
        "variables": summary,
    }))
}

// ---------------------------------------------------------------------------
// `debug_scopes`
// ---------------------------------------------------------------------------

/// `debug_scopes` — list variable scopes (locals, globals, etc.) for a frame.
pub struct DebugScopesTool {
    manager: Arc<DapSessionManager>,
}

impl DebugScopesTool {
    pub fn new(manager: Arc<DapSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for DebugScopesTool {
    fn id(&self) -> &str {
        "debug_scopes"
    }

    fn description(&self) -> &str {
        "T13.1 — List variable scopes for a stack frame (DAP `scopes`). \
         Each scope carries its OWN variablesReference, which `debug_variables` \
         can drill. Typical scopes: \"Locals\", \"Globals\", \"Arguments\". \
         Use this AFTER `debug_stack_trace` (which gave you frame_id) and \
         BEFORE `debug_variables` (which descends into a scope). \
         Example: debug_scopes({session_id: \"a\", frame_id: 1000})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "session_id".into(),
                    param_type: "string".into(),
                    description: "ID of the active debug session.".into(),
                    required: true,
                },
                ToolParam {
                    name: "frame_id".into(),
                    param_type: "integer".into(),
                    description:
                        "Frame to fetch scopes for. Get from debug_stack_trace."
                            .into(),
                    required: true,
                },
            ],
            input_examples: vec![json!({"session_id": "a", "frame_id": 1000})],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    async fn execute(
        &self,
        args: Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let session_id = parse_session_id(&args)?;
        let frame_id = args
            .get("frame_id")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                ToolError::InvalidArgs("missing integer `frame_id`".into())
            })?;

        let client = require_session(&self.manager, &session_id).await?;
        let resp = client
            .request("scopes", Some(json!({"frameId": frame_id})))
            .await
            .map_err(|e| ToolError::Execution(format!("scopes failed: {e}")))?;
        check_response(&resp, "scopes")?;
        Ok(format_scopes_output(&resp, &session_id, frame_id))
    }
}

fn format_scopes_output(
    resp: &DapResponse,
    session_id: &str,
    frame_id: u64,
) -> ToolOutput {
    let scopes = resp
        .body
        .as_ref()
        .and_then(|b| b.get("scopes"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let summary: Vec<Value> = scopes
        .iter()
        .map(|s| {
            json!({
                "name": s.get("name").and_then(Value::as_str),
                "presentation_hint": s.get("presentationHint").and_then(Value::as_str),
                "variables_reference": s
                    .get("variablesReference")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                "expensive": s.get("expensive").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .collect();

    let mut out = format!(
        "debug_scopes: {} scope(s) for frame {frame_id} (session=`{session_id}`)\n\n",
        scopes.len(),
    );
    for s in &summary {
        let drill = if s["variables_reference"].as_u64().unwrap_or(0) > 0 {
            format!("  [drill: {}]", s["variables_reference"])
        } else {
            String::new()
        };
        let expensive = if s["expensive"].as_bool().unwrap_or(false) {
            "  (EXPENSIVE — fetches over network)"
        } else {
            ""
        };
        out.push_str(&format!(
            "  {name}{hint}{drill}{expensive}\n",
            name = s["name"].as_str().unwrap_or("(unnamed)"),
            hint = match s["presentation_hint"].as_str() {
                Some(h) => format!("  ({h})"),
                None => String::new(),
            },
            drill = drill,
            expensive = expensive,
        ));
    }
    if scopes.is_empty() {
        out.push_str(
            "(no scopes returned — frame may be invalid or thread not stopped)\n",
        );
    }

    ToolOutput::new(
        format!("debug_scopes: {} scope(s) for frame {frame_id}", scopes.len()),
        out,
    )
    .with_metadata(json!({
        "type": "debug_scopes",
        "session_id": session_id,
        "frame_id": frame_id,
        "scope_count": scopes.len(),
        "scopes": summary,
    }))
}

// ---------------------------------------------------------------------------
// `debug_threads`
// ---------------------------------------------------------------------------

/// `debug_threads` — list every thread in the debuggee.
pub struct DebugThreadsTool {
    manager: Arc<DapSessionManager>,
}

impl DebugThreadsTool {
    pub fn new(manager: Arc<DapSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for DebugThreadsTool {
    fn id(&self) -> &str {
        "debug_threads"
    }

    fn description(&self) -> &str {
        "T13.1 — List every thread in the debuggee (DAP `threads`). Useful \
         when a `stopped` event didn't carry thread_id, or when you need \
         to step through multi-threaded code and pick which thread to step. \
         Each thread has {id, name}. \
         Example: debug_threads({session_id: \"a\"})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![ToolParam {
                name: "session_id".into(),
                param_type: "string".into(),
                description: "ID of the active debug session.".into(),
                required: true,
            }],
            input_examples: vec![json!({"session_id": "a"})],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    async fn execute(
        &self,
        args: Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let session_id = parse_session_id(&args)?;
        let client = require_session(&self.manager, &session_id).await?;
        let resp = client
            .request("threads", None)
            .await
            .map_err(|e| ToolError::Execution(format!("threads failed: {e}")))?;
        check_response(&resp, "threads")?;
        Ok(format_threads_output(&resp, &session_id))
    }
}

fn format_threads_output(resp: &DapResponse, session_id: &str) -> ToolOutput {
    let threads = resp
        .body
        .as_ref()
        .and_then(|b| b.get("threads"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let summary: Vec<Value> = threads
        .iter()
        .map(|t| {
            json!({
                "id": t.get("id").and_then(Value::as_u64),
                "name": t.get("name").and_then(Value::as_str),
            })
        })
        .collect();

    let mut out = format!(
        "debug_threads: {} thread(s) (session=`{session_id}`)\n\n",
        threads.len(),
    );
    for t in &summary {
        out.push_str(&format!(
            "  id={id}  {name}\n",
            id = t["id"],
            name = t["name"].as_str().unwrap_or("(no name)"),
        ));
    }
    if threads.is_empty() {
        out.push_str(
            "(no threads — adapter may have terminated; call debug_terminate \
             and re-launch)\n",
        );
    }

    ToolOutput::new(
        format!("debug_threads: {} thread(s)", threads.len()),
        out,
    )
    .with_metadata(json!({
        "type": "debug_threads",
        "session_id": session_id,
        "thread_count": threads.len(),
        "threads": summary,
    }))
}

// ---------------------------------------------------------------------------
// `debug_terminate`
// ---------------------------------------------------------------------------

/// `debug_terminate` — close a debug session and free the adapter.
pub struct DebugTerminateTool {
    manager: Arc<DapSessionManager>,
}

impl DebugTerminateTool {
    pub fn new(manager: Arc<DapSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for DebugTerminateTool {
    fn id(&self) -> &str {
        "debug_terminate"
    }

    fn description(&self) -> &str {
        "T13.1 — End a debug session. Drops the cached DapClient, killing the \
         adapter via kill_on_drop. Idempotent: terminating an unknown session_id \
         returns success with `was_active=false`. Always pair with \
         `debug_launch` to avoid leaking adapter processes between agent runs. \
         Example: debug_terminate({session_id: \"a\"})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![ToolParam {
                name: "session_id".into(),
                param_type: "string".into(),
                description:
                    "ID of the session to terminate. Idempotent — unknown \
                     ids return success."
                        .into(),
                required: true,
            }],
            input_examples: vec![json!({"session_id": "a"})],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    async fn execute(
        &self,
        args: Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let session_id = parse_session_id(&args)?;
        let was_active = self.manager.terminate(&session_id).await;
        let title = if was_active {
            format!("debug_terminate: session `{session_id}` terminated")
        } else {
            format!(
                "debug_terminate: no active session `{session_id}` (no-op)"
            )
        };
        Ok(ToolOutput::new(
            title,
            if was_active {
                format!(
                    "Debug session `{session_id}` ended. The adapter process \
                     was killed via kill_on_drop. Safe to re-launch with the \
                     same id."
                )
            } else {
                format!(
                    "No session `{session_id}` was active — nothing to do. \
                     terminate() is idempotent."
                )
            },
        )
        .with_metadata(json!({
            "type": "debug_terminate",
            "session_id": session_id,
            "was_active": was_active,
        })))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// Suppress an unused-imports warning when only some tools end up
// referenced by tests on a given platform.
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
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

    fn empty_manager() -> Arc<DapSessionManager> {
        Arc::new(DapSessionManager::from_catalogue(HashMap::new()))
    }

    // ── debug_status ──────────────────────────────────────────────

    #[test]
    fn t131tool_status_id_and_category() {
        let t = DebugStatusTool::new(empty_manager());
        assert_eq!(t.id(), "debug_status");
        assert_eq!(t.category(), ToolCategory::Search);
    }

    #[test]
    fn t131tool_status_schema_validates_with_no_args() {
        let t = DebugStatusTool::new(empty_manager());
        let schema = t.schema();
        schema.validate().unwrap();
        assert!(schema.params.is_empty());
    }

    #[tokio::test]
    async fn t131tool_status_empty_catalogue_returns_zero_languages() {
        let t = DebugStatusTool::new(empty_manager());
        let mut perms = PermissionCollector::new();
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let out = t.execute(json!({}), &ctx, &mut perms).await.unwrap();
        assert_eq!(out.metadata["supported_language_count"], 0);
        assert_eq!(out.metadata["active_session_count"], 0);
        assert!(
            out.output.contains("No DAP adapters"),
            "empty catalogue must surface the install/print-debug fallback"
        );
        assert!(out.output.contains("print-debugging"));
        assert!(out.output.contains("No active debug sessions"));
    }

    #[tokio::test]
    async fn t131tool_status_lists_languages_and_no_active_sessions() {
        use std::collections::HashMap;
        use std::path::PathBuf;
        let mut catalogue: HashMap<&'static str, crate::dap::DiscoveredAdapter> =
            HashMap::new();
        catalogue.insert(
            "rust",
            crate::dap::DiscoveredAdapter {
                name: "lldb-vscode",
                command: PathBuf::from("/usr/bin/lldb-vscode"),
                args: vec![],
                languages: &["rust"],
                file_extensions: &["rs"],
            },
        );
        catalogue.insert(
            "python",
            crate::dap::DiscoveredAdapter {
                name: "debugpy",
                command: PathBuf::from("/usr/bin/debugpy-adapter"),
                args: vec![],
                languages: &["python"],
                file_extensions: &["py"],
            },
        );
        let manager = Arc::new(DapSessionManager::from_catalogue(catalogue));
        let t = DebugStatusTool::new(manager);
        let mut perms = PermissionCollector::new();
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let out = t.execute(json!({}), &ctx, &mut perms).await.unwrap();

        assert_eq!(out.metadata["supported_language_count"], 2);
        let langs = out.metadata["supported_languages"].as_array().unwrap();
        // Alphabetical: python before rust.
        assert_eq!(langs[0], "python");
        assert_eq!(langs[1], "rust");
        // No sessions yet.
        assert_eq!(out.metadata["active_session_count"], 0);
        // Output mentions debug_launch as the next step.
        assert!(out.output.contains("debug_launch"));
    }

    // ── debug_launch ──────────────────────────────────────────────

    #[test]
    fn t131tool_launch_id_and_category() {
        let t = DebugLaunchTool::new(empty_manager());
        assert_eq!(t.id(), "debug_launch");
        assert_eq!(t.category(), ToolCategory::Search);
    }

    #[test]
    fn t131tool_launch_schema_validates() {
        let t = DebugLaunchTool::new(empty_manager());
        t.schema().validate().unwrap();
    }

    #[test]
    fn t131tool_launch_schema_lists_required_fields() {
        let t = DebugLaunchTool::new(empty_manager());
        let names: Vec<_> = t.schema().params.into_iter().collect();
        let required: Vec<&str> = names
            .iter()
            .filter(|p| p.required)
            .map(|p| p.name.as_str())
            .collect();
        for r in ["session_id", "language", "program"] {
            assert!(required.contains(&r), "{r} should be required");
        }
        let optional: Vec<&str> = names
            .iter()
            .filter(|p| !p.required)
            .map(|p| p.name.as_str())
            .collect();
        for o in ["args", "cwd", "env", "stop_on_entry"] {
            assert!(optional.contains(&o), "{o} should be optional");
        }
    }

    #[tokio::test]
    async fn t131tool_launch_missing_session_id_returns_invalid_args() {
        let t = DebugLaunchTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"language": "rust", "program": "/bin/x"}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn t131tool_launch_empty_session_id_returns_invalid_args() {
        let t = DebugLaunchTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"session_id": "  ", "language": "rust", "program": "/bin/x"}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::InvalidArgs(msg) => assert!(msg.contains("`session_id` is empty")),
            other => panic!("expected InvalidArgs, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t131tool_launch_missing_program_returns_invalid_args() {
        let t = DebugLaunchTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"session_id": "a", "language": "rust"}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn t131tool_launch_unknown_language_returns_actionable_execution_error() {
        let t = DebugLaunchTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({
                    "session_id": "a",
                    "language": "haskell",
                    "program": "/bin/myprog"
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::Execution(msg) => {
                assert!(msg.contains("no DAP adapter installed"));
                assert!(msg.contains("`haskell`"));
                // Mentions at least one alternative for context.
                assert!(
                    msg.contains("lldb-vscode")
                        || msg.contains("debugpy")
                        || msg.contains("dlv")
                        || msg.contains("print debugging")
                );
            }
            other => panic!("expected Execution error, got {other:?}"),
        }
    }

    // ── debug_set_breakpoint ──────────────────────────────────────

    #[test]
    fn t131tool_breakpoint_id_and_category() {
        let t = DebugSetBreakpointTool::new(empty_manager());
        assert_eq!(t.id(), "debug_set_breakpoint");
        assert_eq!(t.category(), ToolCategory::Search);
    }

    #[test]
    fn t131tool_breakpoint_schema_validates() {
        let t = DebugSetBreakpointTool::new(empty_manager());
        t.schema().validate().unwrap();
    }

    #[tokio::test]
    async fn t131tool_breakpoint_missing_lines_returns_invalid_args() {
        let t = DebugSetBreakpointTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"session_id": "a", "file_path": "/x.rs"}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn t131tool_breakpoint_zero_line_returns_invalid_args() {
        // DAP line numbers are 1-based; 0 is a common bug.
        let t = DebugSetBreakpointTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"session_id": "a", "file_path": "/x.rs", "lines": [10, 0, 25]}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::InvalidArgs(msg) => assert!(msg.contains("1-based")),
            other => panic!("expected InvalidArgs, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t131tool_breakpoint_non_integer_line_returns_invalid_args() {
        let t = DebugSetBreakpointTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"session_id": "a", "file_path": "/x.rs", "lines": [10, "twenty", 25]}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::InvalidArgs(msg) => assert!(msg.contains("positive integers")),
            other => panic!("expected InvalidArgs, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t131tool_breakpoint_unknown_session_returns_actionable_error() {
        let t = DebugSetBreakpointTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"session_id": "ghost", "file_path": "/x.rs", "lines": [10]}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::Execution(msg) => {
                assert!(msg.contains("no active debug session"));
                assert!(msg.contains("`ghost`"));
                assert!(msg.contains("debug_launch"));
            }
            other => panic!("expected Execution error, got {other:?}"),
        }
    }

    #[test]
    fn t131tool_format_set_breakpoint_groups_verified_unverified() {
        let resp = DapResponse {
            seq: 1,
            message_type: "response".into(),
            request_seq: 1,
            command: "setBreakpoints".into(),
            success: true,
            message: None,
            body: Some(json!({
                "breakpoints": [
                    {"verified": true, "line": 10},
                    {"verified": false, "line": 25, "message": "no executable code at line"},
                    {"verified": true, "line": 42},
                ]
            })),
        };
        let out = format_set_breakpoint_output(
            &resp,
            "a",
            "/abs/x.rs",
            &[10, 25, 42],
        );
        assert_eq!(out.metadata["verified_count"], 2);
        assert_eq!(out.metadata["unverified_count"], 1);
        assert_eq!(out.metadata["session_id"], "a");
        assert!(out.output.contains("/abs/x.rs"));
    }

    #[test]
    fn t131tool_format_set_breakpoint_handles_empty_body() {
        let resp = DapResponse {
            seq: 1,
            message_type: "response".into(),
            request_seq: 1,
            command: "setBreakpoints".into(),
            success: true,
            message: None,
            body: None,
        };
        let out = format_set_breakpoint_output(&resp, "a", "/x.rs", &[]);
        assert_eq!(out.metadata["verified_count"], 0);
        assert_eq!(out.metadata["unverified_count"], 0);
    }

    // ── debug_continue ────────────────────────────────────────────

    #[test]
    fn t131tool_continue_id_and_category() {
        let t = DebugContinueTool::new(empty_manager());
        assert_eq!(t.id(), "debug_continue");
        assert_eq!(t.category(), ToolCategory::Search);
    }

    #[test]
    fn t131tool_continue_schema_validates() {
        let t = DebugContinueTool::new(empty_manager());
        t.schema().validate().unwrap();
    }

    #[tokio::test]
    async fn t131tool_continue_missing_session_id_returns_invalid_args() {
        let t = DebugContinueTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t.execute(json!({}), &ctx, &mut perms).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn t131tool_continue_unknown_session_returns_actionable_error() {
        let t = DebugContinueTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(json!({"session_id": "ghost"}), &ctx, &mut perms)
            .await
            .unwrap_err();
        match err {
            ToolError::Execution(msg) => assert!(msg.contains("no active debug session")),
            other => panic!("expected Execution error, got {other:?}"),
        }
    }

    // ── debug_terminate ───────────────────────────────────────────

    #[test]
    fn t131tool_terminate_id_and_category() {
        let t = DebugTerminateTool::new(empty_manager());
        assert_eq!(t.id(), "debug_terminate");
        assert_eq!(t.category(), ToolCategory::Search);
    }

    #[test]
    fn t131tool_terminate_schema_validates() {
        let t = DebugTerminateTool::new(empty_manager());
        t.schema().validate().unwrap();
    }

    #[tokio::test]
    async fn t131tool_terminate_unknown_session_returns_was_active_false() {
        // Idempotency invariant: terminating an unknown session is a
        // no-op success, NOT an error. The agent might call this in
        // a cleanup routine without knowing the state.
        let t = DebugTerminateTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let out = t
            .execute(json!({"session_id": "ghost"}), &ctx, &mut perms)
            .await
            .unwrap();
        assert_eq!(out.metadata["was_active"], false);
        assert_eq!(out.metadata["session_id"], "ghost");
        assert!(out.title.contains("no active session"));
    }

    #[tokio::test]
    async fn t131tool_terminate_missing_session_id_returns_invalid_args() {
        let t = DebugTerminateTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t.execute(json!({}), &ctx, &mut perms).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn t131tool_terminate_empty_session_id_returns_invalid_args() {
        let t = DebugTerminateTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(json!({"session_id": ""}), &ctx, &mut perms)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    // ── shared helpers ────────────────────────────────────────────

    #[test]
    fn t131tool_check_response_passes_on_success() {
        let resp = DapResponse {
            seq: 1,
            message_type: "response".into(),
            request_seq: 1,
            command: "anything".into(),
            success: true,
            message: None,
            body: None,
        };
        check_response(&resp, "anything").unwrap();
    }

    #[test]
    fn t131tool_check_response_returns_execution_error_on_failure() {
        let resp = DapResponse {
            seq: 1,
            message_type: "response".into(),
            request_seq: 1,
            command: "evaluate".into(),
            success: false,
            message: Some("expression not in scope".into()),
            body: None,
        };
        let err = check_response(&resp, "evaluate").unwrap_err();
        match err {
            ToolError::Execution(msg) => {
                assert!(msg.contains("evaluate"));
                assert!(msg.contains("expression not in scope"));
            }
            other => panic!("expected Execution error, got {other:?}"),
        }
    }

    // ── debug_step ────────────────────────────────────────────────

    #[test]
    fn t131tool_step_id_and_category() {
        let t = DebugStepTool::new(empty_manager());
        assert_eq!(t.id(), "debug_step");
        assert_eq!(t.category(), ToolCategory::Search);
    }

    #[test]
    fn t131tool_step_schema_validates_and_requires_kind_thread() {
        let t = DebugStepTool::new(empty_manager());
        let schema = t.schema();
        schema.validate().unwrap();
        let required: Vec<&str> = schema
            .params
            .iter()
            .filter(|p| p.required)
            .map(|p| p.name.as_str())
            .collect();
        for r in ["session_id", "kind", "thread_id"] {
            assert!(required.contains(&r), "{r} must be required");
        }
    }

    #[tokio::test]
    async fn t131tool_step_missing_kind_returns_invalid_args() {
        let t = DebugStepTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"session_id": "a", "thread_id": 1}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn t131tool_step_invalid_kind_returns_invalid_args_with_options() {
        let t = DebugStepTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"session_id": "a", "kind": "sideways", "thread_id": 1}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::InvalidArgs(msg) => {
                assert!(msg.contains("over"));
                assert!(msg.contains("in"));
                assert!(msg.contains("out"));
                assert!(msg.contains("sideways"));
            }
            other => panic!("expected InvalidArgs, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t131tool_step_missing_thread_id_returns_invalid_args_with_hint() {
        // Common bug: copying continue() args (where thread_id is
        // optional) into step(). Error message points at the
        // `stopped` event source.
        let t = DebugStepTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"session_id": "a", "kind": "over"}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::InvalidArgs(msg) => {
                assert!(msg.contains("thread_id"));
                assert!(msg.contains("stopped event"));
            }
            other => panic!("expected InvalidArgs, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t131tool_step_unknown_session_returns_actionable_error() {
        let t = DebugStepTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"session_id": "ghost", "kind": "over", "thread_id": 1}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::Execution(msg) => assert!(msg.contains("no active debug session")),
            other => panic!("expected Execution error, got {other:?}"),
        }
    }

    // ── debug_eval ────────────────────────────────────────────────

    #[test]
    fn t131tool_eval_id_and_category() {
        let t = DebugEvalTool::new(empty_manager());
        assert_eq!(t.id(), "debug_eval");
        assert_eq!(t.category(), ToolCategory::Search);
    }

    #[test]
    fn t131tool_eval_schema_validates() {
        let t = DebugEvalTool::new(empty_manager());
        t.schema().validate().unwrap();
    }

    #[test]
    fn t131tool_eval_schema_marks_frame_id_and_context_optional() {
        let t = DebugEvalTool::new(empty_manager());
        let schema = t.schema();
        let optional: Vec<&str> = schema
            .params
            .iter()
            .filter(|p| !p.required)
            .map(|p| p.name.as_str())
            .collect();
        assert!(optional.contains(&"frame_id"));
        assert!(optional.contains(&"context"));
    }

    #[tokio::test]
    async fn t131tool_eval_missing_expression_returns_invalid_args() {
        let t = DebugEvalTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(json!({"session_id": "a"}), &ctx, &mut perms)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn t131tool_eval_empty_expression_returns_invalid_args() {
        let t = DebugEvalTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"session_id": "a", "expression": "   "}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::InvalidArgs(msg) => assert!(msg.contains("`expression` is empty")),
            other => panic!("expected InvalidArgs, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t131tool_eval_invalid_context_returns_invalid_args() {
        let t = DebugEvalTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({
                    "session_id": "a",
                    "expression": "x",
                    "context": "side_effects_pls"
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::InvalidArgs(msg) => {
                assert!(msg.contains("`context`"));
                assert!(msg.contains("watch"));
                assert!(msg.contains("repl"));
                assert!(msg.contains("hover"));
            }
            other => panic!("expected InvalidArgs, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t131tool_eval_unknown_session_returns_actionable_error() {
        let t = DebugEvalTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"session_id": "ghost", "expression": "x"}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::Execution(msg) => assert!(msg.contains("no active debug session")),
            other => panic!("expected Execution error, got {other:?}"),
        }
    }

    #[test]
    fn t131tool_format_eval_includes_result_type_and_variables_reference() {
        let resp = DapResponse {
            seq: 1,
            message_type: "response".into(),
            request_seq: 1,
            command: "evaluate".into(),
            success: true,
            message: None,
            body: Some(json!({
                "result": "Some(42)",
                "type": "Option<i32>",
                "variablesReference": 7,
            })),
        };
        let out = format_eval_output(&resp, "a", "my_var", "watch", Some(3));
        assert_eq!(out.metadata["result"], "Some(42)");
        assert_eq!(out.metadata["value_type"], "Option<i32>");
        assert_eq!(out.metadata["variables_reference"], 7);
        assert_eq!(out.metadata["frame_id"], 3);
        assert_eq!(out.metadata["context"], "watch");
        assert!(out.output.contains("expression: my_var"));
        assert!(out.output.contains("result: Some(42)"));
        assert!(out.output.contains("type: Option<i32>"));
        assert!(out.output.contains("variablesReference: 7"));
    }

    #[test]
    fn t131tool_format_eval_handles_zero_variables_reference_silently() {
        // Primitive values have variablesReference == 0; no drill-down hint.
        let resp = DapResponse {
            seq: 1,
            message_type: "response".into(),
            request_seq: 1,
            command: "evaluate".into(),
            success: true,
            message: None,
            body: Some(json!({
                "result": "42",
                "type": "i32",
                "variablesReference": 0,
            })),
        };
        let out = format_eval_output(&resp, "a", "x", "watch", None);
        assert_eq!(out.metadata["variables_reference"], 0);
        assert!(!out.output.contains("variablesReference:"));
    }

    #[test]
    fn t131tool_format_eval_handles_missing_body_gracefully() {
        let resp = DapResponse {
            seq: 1,
            message_type: "response".into(),
            request_seq: 1,
            command: "evaluate".into(),
            success: true,
            message: None,
            body: None,
        };
        let out = format_eval_output(&resp, "a", "x", "watch", None);
        assert_eq!(out.metadata["result"], "(no result)");
        assert!(out.metadata["value_type"].is_null());
        assert_eq!(out.metadata["variables_reference"], 0);
    }

    // ── debug_stack_trace ─────────────────────────────────────────

    #[test]
    fn t131tool_stack_trace_id_and_category() {
        let t = DebugStackTraceTool::new(empty_manager());
        assert_eq!(t.id(), "debug_stack_trace");
        assert_eq!(t.category(), ToolCategory::Search);
    }

    #[test]
    fn t131tool_stack_trace_schema_validates_with_required_thread_id() {
        let t = DebugStackTraceTool::new(empty_manager());
        let schema = t.schema();
        schema.validate().unwrap();
        let required: Vec<&str> = schema
            .params
            .iter()
            .filter(|p| p.required)
            .map(|p| p.name.as_str())
            .collect();
        assert!(required.contains(&"thread_id"));
        assert!(required.contains(&"session_id"));
    }

    #[tokio::test]
    async fn t131tool_stack_trace_missing_thread_id_returns_invalid_args() {
        let t = DebugStackTraceTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(json!({"session_id": "a"}), &ctx, &mut perms)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn t131tool_stack_trace_unknown_session_returns_actionable_error() {
        let t = DebugStackTraceTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"session_id": "ghost", "thread_id": 1}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::Execution(msg) => assert!(msg.contains("no active debug session")),
            other => panic!("expected Execution error, got {other:?}"),
        }
    }

    #[test]
    fn t131tool_format_stack_trace_includes_frame_ids_and_source_locations() {
        let resp = DapResponse {
            seq: 1,
            message_type: "response".into(),
            request_seq: 1,
            command: "stackTrace".into(),
            success: true,
            message: None,
            body: Some(json!({
                "stackFrames": [
                    {
                        "id": 1000,
                        "name": "main",
                        "source": {"path": "/abs/main.rs"},
                        "line": 42,
                        "column": 12,
                    },
                    {
                        "id": 1001,
                        "name": "<rust_main>",
                        "source": {"path": "/abs/lib.rs"},
                        "line": 5,
                        "column": 1,
                    },
                ],
                "totalFrames": 2,
            })),
        };
        let out = format_stack_trace_output(&resp, "a", 1, 0, 20);
        assert_eq!(out.metadata["frame_count"], 2);
        assert_eq!(out.metadata["total_frames"], 2);
        assert_eq!(out.metadata["thread_id"], 1);
        // Frame id MUST be preserved — it's the input to debug_eval(frame_id).
        let frames = out.metadata["frames"].as_array().unwrap();
        assert_eq!(frames[0]["id"], 1000);
        assert_eq!(frames[0]["source_path"], "/abs/main.rs");
        assert_eq!(frames[0]["line"], 42);
        assert!(out.output.contains("main"));
        assert!(out.output.contains("/abs/main.rs:42:12"));
    }

    #[test]
    fn t131tool_format_stack_trace_handles_empty_frames_with_explanation() {
        let resp = DapResponse {
            seq: 1,
            message_type: "response".into(),
            request_seq: 1,
            command: "stackTrace".into(),
            success: true,
            message: None,
            body: Some(json!({"stackFrames": [], "totalFrames": 0})),
        };
        let out = format_stack_trace_output(&resp, "a", 1, 0, 20);
        assert_eq!(out.metadata["frame_count"], 0);
        // Helps the agent diagnose: a stopped thread should HAVE frames.
        assert!(out.output.contains("no frames returned"));
        assert!(out.output.contains("stopped state"));
    }

    #[test]
    fn t131tool_format_stack_trace_offsets_frame_numbers_by_start_frame() {
        let resp = DapResponse {
            seq: 1,
            message_type: "response".into(),
            request_seq: 1,
            command: "stackTrace".into(),
            success: true,
            message: None,
            body: Some(json!({
                "stackFrames": [
                    {"id": 5000, "name": "fn_a", "source": {"path": "/x"}, "line": 1, "column": 1},
                    {"id": 5001, "name": "fn_b", "source": {"path": "/x"}, "line": 2, "column": 1},
                ],
                "totalFrames": 100,
            })),
        };
        let out = format_stack_trace_output(&resp, "a", 1, 50, 2);
        // start_frame=50 means the rendered frames are #50, #51 — not #0, #1.
        assert!(out.output.contains("#50"));
        assert!(out.output.contains("#51"));
        assert!(!out.output.contains("#0  id=5000"));
    }

    // ── debug_variables ───────────────────────────────────────────

    #[test]
    fn t131tool_variables_id_and_category() {
        let t = DebugVariablesTool::new(empty_manager());
        assert_eq!(t.id(), "debug_variables");
        assert_eq!(t.category(), ToolCategory::Search);
    }

    #[test]
    fn t131tool_variables_schema_validates() {
        let t = DebugVariablesTool::new(empty_manager());
        t.schema().validate().unwrap();
    }

    #[tokio::test]
    async fn t131tool_variables_missing_reference_returns_invalid_args() {
        let t = DebugVariablesTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(json!({"session_id": "a"}), &ctx, &mut perms)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn t131tool_variables_zero_reference_returns_invalid_args_with_explanation() {
        // variablesReference == 0 means "not drillable". Calling
        // debug_variables on it is a logic error — explain WHY.
        let t = DebugVariablesTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"session_id": "a", "variables_reference": 0}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::InvalidArgs(msg) => {
                assert!(msg.contains("must be > 0"));
                assert!(msg.contains("scalar value"));
            }
            other => panic!("expected InvalidArgs, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t131tool_variables_invalid_filter_returns_invalid_args() {
        let t = DebugVariablesTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({
                    "session_id": "a",
                    "variables_reference": 7,
                    "filter": "weird"
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::InvalidArgs(msg) => {
                assert!(msg.contains("`filter`"));
                assert!(msg.contains("indexed"));
                assert!(msg.contains("named"));
            }
            other => panic!("expected InvalidArgs, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t131tool_variables_unknown_session_returns_actionable_error() {
        let t = DebugVariablesTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"session_id": "ghost", "variables_reference": 7}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::Execution(msg) => assert!(msg.contains("no active debug session")),
            other => panic!("expected Execution error, got {other:?}"),
        }
    }

    #[test]
    fn t131tool_format_variables_counts_drillable_children() {
        let resp = DapResponse {
            seq: 1,
            message_type: "response".into(),
            request_seq: 1,
            command: "variables".into(),
            success: true,
            message: None,
            body: Some(json!({
                "variables": [
                    {"name": "x", "value": "42", "type": "i32", "variablesReference": 0},
                    {"name": "v", "value": "[1, 2, 3]", "type": "Vec<i32>", "variablesReference": 11},
                    {"name": "name", "value": "\"hi\"", "type": "&str", "variablesReference": 0},
                    {"name": "map", "value": "HashMap{2}", "type": "HashMap<i32, String>", "variablesReference": 12},
                ]
            })),
        };
        let out = format_variables_output(&resp, "a", 7, 0, 100, None);
        assert_eq!(out.metadata["child_count"], 4);
        assert_eq!(out.metadata["drillable_count"], 2);
        assert_eq!(out.metadata["session_id"], "a");
        // Output text shows drill hints for the two structured ones.
        assert!(out.output.contains("[drill: 11]"));
        assert!(out.output.contains("[drill: 12]"));
        assert!(out.output.contains("x = 42"));
        // Scalar children must NOT have a drill hint.
        let x_line = out
            .output
            .lines()
            .find(|l| l.contains("x = 42"))
            .unwrap();
        assert!(!x_line.contains("[drill:"));
    }

    #[test]
    fn t131tool_format_variables_handles_empty_response_gracefully() {
        let resp = DapResponse {
            seq: 1,
            message_type: "response".into(),
            request_seq: 1,
            command: "variables".into(),
            success: true,
            message: None,
            body: Some(json!({"variables": []})),
        };
        let out = format_variables_output(&resp, "a", 7, 0, 100, None);
        assert_eq!(out.metadata["child_count"], 0);
        assert_eq!(out.metadata["drillable_count"], 0);
    }

    #[test]
    fn t131tool_format_variables_passes_filter_through_to_metadata() {
        let resp = DapResponse {
            seq: 1,
            message_type: "response".into(),
            request_seq: 1,
            command: "variables".into(),
            success: true,
            message: None,
            body: Some(json!({"variables": []})),
        };
        let out = format_variables_output(&resp, "a", 7, 0, 50, Some("indexed"));
        assert_eq!(out.metadata["filter"], "indexed");
        assert_eq!(out.metadata["count"], 50);
    }

    // ── debug_scopes ──────────────────────────────────────────────

    #[test]
    fn t131tool_scopes_id_and_category() {
        let t = DebugScopesTool::new(empty_manager());
        assert_eq!(t.id(), "debug_scopes");
        assert_eq!(t.category(), ToolCategory::Search);
    }

    #[test]
    fn t131tool_scopes_schema_validates() {
        let t = DebugScopesTool::new(empty_manager());
        let schema = t.schema();
        schema.validate().unwrap();
        for p in &schema.params {
            assert!(p.required, "{} should be required", p.name);
        }
    }

    #[tokio::test]
    async fn t131tool_scopes_missing_frame_id_returns_invalid_args() {
        let t = DebugScopesTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(json!({"session_id": "a"}), &ctx, &mut perms)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn t131tool_scopes_unknown_session_returns_actionable_error() {
        let t = DebugScopesTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"session_id": "ghost", "frame_id": 1}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::Execution(msg) => assert!(msg.contains("no active debug session")),
            other => panic!("expected Execution error, got {other:?}"),
        }
    }

    #[test]
    fn t131tool_format_scopes_marks_drillable_children() {
        let resp = DapResponse {
            seq: 1,
            message_type: "response".into(),
            request_seq: 1,
            command: "scopes".into(),
            success: true,
            message: None,
            body: Some(json!({
                "scopes": [
                    {"name": "Locals", "variablesReference": 11, "expensive": false},
                    {"name": "Globals", "variablesReference": 12, "expensive": true},
                    {"name": "Empty", "variablesReference": 0, "expensive": false},
                ]
            })),
        };
        let out = format_scopes_output(&resp, "a", 1000);
        assert_eq!(out.metadata["scope_count"], 3);
        assert!(out.output.contains("Locals"));
        assert!(out.output.contains("[drill: 11]"));
        assert!(out.output.contains("[drill: 12]"));
        // Expensive scopes get a clear marker so the agent knows
        // fetching them is non-trivial.
        assert!(out.output.contains("EXPENSIVE"));
        // The empty scope (variablesReference == 0) gets no drill hint.
        assert!(out.output.contains("Empty"));
        let empty_line = out.output.lines().find(|l| l.contains("Empty")).unwrap();
        assert!(!empty_line.contains("[drill:"));
    }

    #[test]
    fn t131tool_format_scopes_empty_response_includes_diagnostic_hint() {
        let resp = DapResponse {
            seq: 1,
            message_type: "response".into(),
            request_seq: 1,
            command: "scopes".into(),
            success: true,
            message: None,
            body: Some(json!({"scopes": []})),
        };
        let out = format_scopes_output(&resp, "a", 1000);
        assert_eq!(out.metadata["scope_count"], 0);
        assert!(out.output.contains("no scopes returned"));
        assert!(out.output.contains("frame may be invalid"));
    }

    // ── debug_threads ─────────────────────────────────────────────

    #[test]
    fn t131tool_threads_id_and_category() {
        let t = DebugThreadsTool::new(empty_manager());
        assert_eq!(t.id(), "debug_threads");
        assert_eq!(t.category(), ToolCategory::Search);
    }

    #[test]
    fn t131tool_threads_schema_validates_with_only_session_id() {
        let t = DebugThreadsTool::new(empty_manager());
        let schema = t.schema();
        schema.validate().unwrap();
        assert_eq!(schema.params.len(), 1);
        assert_eq!(schema.params[0].name, "session_id");
        assert!(schema.params[0].required);
    }

    #[tokio::test]
    async fn t131tool_threads_unknown_session_returns_actionable_error() {
        let t = DebugThreadsTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(json!({"session_id": "ghost"}), &ctx, &mut perms)
            .await
            .unwrap_err();
        match err {
            ToolError::Execution(msg) => assert!(msg.contains("no active debug session")),
            other => panic!("expected Execution error, got {other:?}"),
        }
    }

    #[test]
    fn t131tool_format_threads_lists_each_thread() {
        let resp = DapResponse {
            seq: 1,
            message_type: "response".into(),
            request_seq: 1,
            command: "threads".into(),
            success: true,
            message: None,
            body: Some(json!({
                "threads": [
                    {"id": 1, "name": "main"},
                    {"id": 2, "name": "worker-0"},
                    {"id": 3, "name": "worker-1"},
                ]
            })),
        };
        let out = format_threads_output(&resp, "a");
        assert_eq!(out.metadata["thread_count"], 3);
        assert!(out.output.contains("main"));
        assert!(out.output.contains("worker-0"));
        assert!(out.output.contains("worker-1"));
    }

    #[test]
    fn t131tool_format_threads_empty_response_includes_relaunch_hint() {
        let resp = DapResponse {
            seq: 1,
            message_type: "response".into(),
            request_seq: 1,
            command: "threads".into(),
            success: true,
            message: None,
            body: Some(json!({"threads": []})),
        };
        let out = format_threads_output(&resp, "a");
        assert_eq!(out.metadata["thread_count"], 0);
        // Empty → adapter probably terminated. Tell the agent to
        // call debug_terminate + re-launch instead of looping.
        assert!(out.output.contains("debug_terminate"));
        assert!(out.output.contains("re-launch"));
    }

    // Suppress unused-helper warning when no test needs make_ctx +
    // PathBuf together (e.g. cfg-gated builds).
    #[allow(dead_code)]
    fn _force_paths_ref(_: &Path) {}
}
