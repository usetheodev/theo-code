//! T13.1 — DAP command builders.
//!
//! Constructs `DapRequest` shapes for the standard Debug Adapter
//! Protocol commands. Pure JSON construction; testable without a
//! real adapter.
//!
//! Commands covered:
//! - Lifecycle: `initialize`, `launch`, `attach`, `configurationDone`,
//!   `disconnect`, `terminate`.
//! - Breakpoints: `setBreakpoints`.
//! - Stepping: `next`, `stepIn`, `stepOut`, `continue`, `pause`.
//! - Inspection: `stackTrace`, `scopes`, `variables`, `evaluate`.
//!
//! Spec: <https://microsoft.github.io/debug-adapter-protocol/specification>.

use serde_json::{Value, json};

use crate::dap::protocol::DapRequest;

/// One source breakpoint definition for `setBreakpoints`.
/// `condition` is an optional expression that must be true for the
/// breakpoint to trigger; `hit_condition` is an optional hit-count
/// expression (e.g. `"5"` or `">=10"`).
#[derive(Debug, Clone)]
pub struct SourceBreakpoint {
    pub line: u32,
    pub column: Option<u32>,
    pub condition: Option<String>,
    pub hit_condition: Option<String>,
    pub log_message: Option<String>,
}

impl SourceBreakpoint {
    /// Bare line breakpoint with no condition / log.
    pub fn at_line(line: u32) -> Self {
        Self {
            line,
            column: None,
            condition: None,
            hit_condition: None,
            log_message: None,
        }
    }

    /// JSON shape consumed by the `breakpoints` array.
    pub fn to_json(&self) -> Value {
        let mut o = json!({"line": self.line});
        if let Some(c) = self.column {
            o["column"] = json!(c);
        }
        if let Some(c) = &self.condition {
            o["condition"] = json!(c);
        }
        if let Some(c) = &self.hit_condition {
            o["hitCondition"] = json!(c);
        }
        if let Some(m) = &self.log_message {
            o["logMessage"] = json!(m);
        }
        o
    }
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

/// `initialize` — first request after spawning the adapter. The
/// `client_id` is a short label the adapter shows in its logs;
/// `adapter_id` identifies the target adapter (e.g. `"lldb-vscode"`).
pub fn initialize(seq: u64, client_id: &str, adapter_id: &str) -> DapRequest {
    DapRequest::new(
        seq,
        "initialize",
        Some(json!({
            "clientID": client_id,
            "clientName": "theo",
            "adapterID": adapter_id,
            "pathFormat": "path",
            "linesStartAt1": true,
            "columnsStartAt1": true,
            "supportsVariableType": true,
            "supportsRunInTerminalRequest": false,
        })),
    )
}

/// `launch` — start the debuggee. `arguments` is adapter-specific
/// (e.g. `{program: "/path", args: [...], cwd: ...}` for lldb-vscode).
pub fn launch(seq: u64, arguments: Value) -> DapRequest {
    DapRequest::new(seq, "launch", Some(arguments))
}

/// `attach` — connect to an already-running debuggee. Same
/// adapter-specific args as `launch`.
pub fn attach(seq: u64, arguments: Value) -> DapRequest {
    DapRequest::new(seq, "attach", Some(arguments))
}

/// `configurationDone` — sent after all initial breakpoints are
/// set. Tells the adapter the debuggee may now run.
pub fn configuration_done(seq: u64) -> DapRequest {
    DapRequest::new(seq, "configurationDone", None)
}

/// `disconnect` — request the adapter to detach (if attached) or
/// terminate (if launched). `terminate_debuggee` may be ignored by
/// some adapters.
pub fn disconnect(seq: u64, terminate_debuggee: bool) -> DapRequest {
    DapRequest::new(
        seq,
        "disconnect",
        Some(json!({"terminateDebuggee": terminate_debuggee})),
    )
}

/// `terminate` — politely ask the debuggee to shut down.
pub fn terminate(seq: u64) -> DapRequest {
    DapRequest::new(seq, "terminate", Some(json!({"restart": false})))
}

// ---------------------------------------------------------------------------
// Breakpoints
// ---------------------------------------------------------------------------

/// `setBreakpoints` — replaces ALL breakpoints for the source. Pass
/// an empty `breakpoints` array to clear breakpoints in `path`.
pub fn set_breakpoints(seq: u64, source_path: &str, breakpoints: &[SourceBreakpoint]) -> DapRequest {
    let bps: Vec<Value> = breakpoints.iter().map(SourceBreakpoint::to_json).collect();
    DapRequest::new(
        seq,
        "setBreakpoints",
        Some(json!({
            "source": {"path": source_path},
            "breakpoints": bps,
        })),
    )
}

// ---------------------------------------------------------------------------
// Stepping
// ---------------------------------------------------------------------------

/// `next` — step over.
pub fn next(seq: u64, thread_id: i64) -> DapRequest {
    DapRequest::new(seq, "next", Some(json!({"threadId": thread_id})))
}

/// `stepIn` — step into.
pub fn step_in(seq: u64, thread_id: i64) -> DapRequest {
    DapRequest::new(seq, "stepIn", Some(json!({"threadId": thread_id})))
}

/// `stepOut` — step out of the current frame.
pub fn step_out(seq: u64, thread_id: i64) -> DapRequest {
    DapRequest::new(seq, "stepOut", Some(json!({"threadId": thread_id})))
}

/// `continue` — resume execution.
pub fn continue_(seq: u64, thread_id: i64) -> DapRequest {
    DapRequest::new(seq, "continue", Some(json!({"threadId": thread_id})))
}

/// `pause` — pause a running thread.
pub fn pause(seq: u64, thread_id: i64) -> DapRequest {
    DapRequest::new(seq, "pause", Some(json!({"threadId": thread_id})))
}

// ---------------------------------------------------------------------------
// Inspection
// ---------------------------------------------------------------------------

/// `stackTrace` — get the call stack for `thread_id`. `start_frame`
/// + `levels` paginate; pass `0`/`0` for "give me everything".
pub fn stack_trace(seq: u64, thread_id: i64, start_frame: u32, levels: u32) -> DapRequest {
    DapRequest::new(
        seq,
        "stackTrace",
        Some(json!({
            "threadId": thread_id,
            "startFrame": start_frame,
            "levels": levels,
        })),
    )
}

/// `scopes` — list scopes (locals, registers, globals) for a frame.
pub fn scopes(seq: u64, frame_id: i64) -> DapRequest {
    DapRequest::new(seq, "scopes", Some(json!({"frameId": frame_id})))
}

/// `variables` — list variables in a `variables_reference` (typically
/// a scope or a complex variable like a struct).
pub fn variables(seq: u64, variables_reference: i64) -> DapRequest {
    DapRequest::new(
        seq,
        "variables",
        Some(json!({"variablesReference": variables_reference})),
    )
}

/// `evaluate` — evaluate `expression` in the context of `frame_id`
/// (or globally when None). `context` is one of `"watch"`, `"repl"`,
/// `"hover"`, or `"clipboard"` (defaults to `"repl"`).
pub fn evaluate(
    seq: u64,
    expression: &str,
    frame_id: Option<i64>,
    context: Option<&str>,
) -> DapRequest {
    let mut args = json!({
        "expression": expression,
        "context": context.unwrap_or("repl"),
    });
    if let Some(fid) = frame_id {
        args["frameId"] = json!(fid);
    }
    DapRequest::new(seq, "evaluate", Some(args))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- SourceBreakpoint ----

    #[test]
    fn t131ops_source_breakpoint_at_line_minimal_json() {
        let bp = SourceBreakpoint::at_line(42);
        let json = bp.to_json();
        assert_eq!(json["line"], 42);
        assert!(json.get("condition").is_none());
        assert!(json.get("column").is_none());
    }

    #[test]
    fn t131ops_source_breakpoint_with_optional_fields() {
        let bp = SourceBreakpoint {
            line: 10,
            column: Some(5),
            condition: Some("x > 0".into()),
            hit_condition: Some(">=3".into()),
            log_message: Some("hit at {x}".into()),
        };
        let json = bp.to_json();
        assert_eq!(json["line"], 10);
        assert_eq!(json["column"], 5);
        assert_eq!(json["condition"], "x > 0");
        assert_eq!(json["hitCondition"], ">=3");
        assert_eq!(json["logMessage"], "hit at {x}");
    }

    // ---- Lifecycle ----

    #[test]
    fn t131ops_initialize_includes_client_and_adapter_id() {
        let req = initialize(1, "theo-test", "lldb-vscode");
        assert_eq!(req.command, "initialize");
        assert_eq!(req.seq, 1);
        let p = req.arguments.unwrap();
        assert_eq!(p["clientID"], "theo-test");
        assert_eq!(p["clientName"], "theo");
        assert_eq!(p["adapterID"], "lldb-vscode");
        assert_eq!(p["pathFormat"], "path");
        // Lines/columns are 1-based by spec convention; Theo follows it
        // so editors that consume the output don't need to translate.
        assert_eq!(p["linesStartAt1"], true);
        assert_eq!(p["columnsStartAt1"], true);
    }

    #[test]
    fn t131ops_launch_passes_arguments_through() {
        let args = json!({
            "program": "/usr/bin/cat",
            "args": ["file.txt"],
            "cwd": "/tmp",
        });
        let req = launch(2, args.clone());
        assert_eq!(req.command, "launch");
        assert_eq!(req.arguments.unwrap(), args);
    }

    #[test]
    fn t131ops_attach_passes_arguments_through() {
        let args = json!({"pid": 12345});
        let req = attach(2, args.clone());
        assert_eq!(req.command, "attach");
        assert_eq!(req.arguments.unwrap(), args);
    }

    #[test]
    fn t131ops_configuration_done_has_no_arguments() {
        let req = configuration_done(3);
        assert_eq!(req.command, "configurationDone");
        assert!(req.arguments.is_none());
    }

    #[test]
    fn t131ops_disconnect_carries_terminate_debuggee_flag() {
        let req = disconnect(99, true);
        assert_eq!(req.command, "disconnect");
        let p = req.arguments.unwrap();
        assert_eq!(p["terminateDebuggee"], true);
    }

    #[test]
    fn t131ops_terminate_request_includes_no_restart() {
        let req = terminate(1);
        let p = req.arguments.unwrap();
        assert_eq!(p["restart"], false);
    }

    // ---- Breakpoints ----

    #[test]
    fn t131ops_set_breakpoints_includes_source_and_array() {
        let bps = vec![
            SourceBreakpoint::at_line(10),
            SourceBreakpoint::at_line(20),
        ];
        let req = set_breakpoints(1, "/path/to/file.rs", &bps);
        assert_eq!(req.command, "setBreakpoints");
        let p = req.arguments.unwrap();
        assert_eq!(p["source"]["path"], "/path/to/file.rs");
        let arr = p["breakpoints"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["line"], 10);
        assert_eq!(arr[1]["line"], 20);
    }

    #[test]
    fn t131ops_set_breakpoints_with_empty_array_clears_breakpoints() {
        // Empty array is the canonical "clear breakpoints in path" shape.
        let req = set_breakpoints(1, "/x", &[]);
        let p = req.arguments.unwrap();
        let arr = p["breakpoints"].as_array().unwrap();
        assert!(arr.is_empty());
    }

    // ---- Stepping ----

    #[test]
    fn t131ops_next_step_in_step_out_carry_thread_id() {
        for (name, builder) in [
            ("next", next as fn(u64, i64) -> DapRequest),
            ("stepIn", step_in),
            ("stepOut", step_out),
            ("continue", continue_),
            ("pause", pause),
        ] {
            let req = builder(1, 7);
            assert_eq!(req.command, name, "command name");
            assert_eq!(req.arguments.as_ref().unwrap()["threadId"], 7);
        }
    }

    // ---- Inspection ----

    #[test]
    fn t131ops_stack_trace_includes_pagination() {
        let req = stack_trace(1, 7, 0, 50);
        assert_eq!(req.command, "stackTrace");
        let p = req.arguments.unwrap();
        assert_eq!(p["threadId"], 7);
        assert_eq!(p["startFrame"], 0);
        assert_eq!(p["levels"], 50);
    }

    #[test]
    fn t131ops_scopes_includes_frame_id() {
        let req = scopes(1, 100);
        assert_eq!(req.command, "scopes");
        assert_eq!(req.arguments.unwrap()["frameId"], 100);
    }

    #[test]
    fn t131ops_variables_includes_reference() {
        let req = variables(1, 1000);
        assert_eq!(req.command, "variables");
        assert_eq!(req.arguments.unwrap()["variablesReference"], 1000);
    }

    #[test]
    fn t131ops_evaluate_with_frame_id_includes_it() {
        let req = evaluate(1, "x + 1", Some(42), Some("watch"));
        assert_eq!(req.command, "evaluate");
        let p = req.arguments.unwrap();
        assert_eq!(p["expression"], "x + 1");
        assert_eq!(p["frameId"], 42);
        assert_eq!(p["context"], "watch");
    }

    #[test]
    fn t131ops_evaluate_without_frame_id_uses_repl_default() {
        let req = evaluate(1, "1 + 1", None, None);
        let p = req.arguments.unwrap();
        assert!(p.get("frameId").is_none());
        assert_eq!(p["context"], "repl");
    }

    #[test]
    fn t131ops_request_serializes_with_type_request_field() {
        let req = next(1, 7);
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"request\""));
        assert!(json.contains("\"command\":\"next\""));
        assert!(json.contains("\"seq\":1"));
    }
}
