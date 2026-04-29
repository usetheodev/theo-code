//! Convert ChatRequest to Codex Responses API format.
//!
//! The Codex endpoint (`https://chatgpt.com/backend-api/codex/responses`)
//! expects the OpenAI Responses API format, not Chat Completions.
//!
//! Key differences:
//! - `instructions` (top-level string) instead of system message in messages
//! - `input` array instead of `messages`
//! - Tool calls are `function_call` items, not in assistant messages
//! - Tool results are `function_call_output` items

use crate::types::*;

/// Convert a ChatRequest (Chat Completions format) to Codex Responses API body.
pub fn to_codex_body(request: &ChatRequest) -> serde_json::Value {
    let mut instructions: Option<String> = None;
    let mut input: Vec<serde_json::Value> = Vec::new();

    for msg in &request.messages {
        match msg.role {
            Role::System => {
                // System messages become `instructions` field
                if let Some(ref content) = msg.content {
                    match &instructions {
                        None => instructions = Some(content.clone()),
                        Some(existing) => instructions = Some(format!("{existing}\n\n{content}")),
                    }
                }
            }
            Role::User => {
                if let Some(ref content) = msg.content {
                    input.push(serde_json::json!({
                        "role": "user",
                        "content": [{"type": "input_text", "text": content}]
                    }));
                }
            }
            Role::Assistant => {
                // Text content
                if let Some(ref content) = msg.content
                    && !content.is_empty() {
                        input.push(serde_json::json!({
                            "role": "assistant",
                            "content": [{"type": "output_text", "text": content}]
                        }));
                    }
                // Tool calls become function_call items
                if let Some(ref tool_calls) = msg.tool_calls {
                    for tc in tool_calls {
                        input.push(serde_json::json!({
                            "type": "function_call",
                            "call_id": tc.id,
                            "name": tc.function.name,
                            "arguments": tc.function.arguments,
                        }));
                    }
                }
            }
            Role::Tool => {
                // Tool results become function_call_output items
                let content = msg.content.as_deref().unwrap_or("");
                let call_id = msg.tool_call_id.as_deref().unwrap_or("");
                input.push(serde_json::json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": content,
                }));
            }
        }
    }

    // Convert tools
    let tools: Option<Vec<serde_json::Value>> = request.tools.as_ref().map(|tools| {
        tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "name": t.function.name,
                    "description": t.function.description,
                    "parameters": t.function.parameters,
                })
            })
            .collect()
    });

    let mut body = serde_json::json!({
        "model": request.model,
        "input": input,
        "stream": true,
        "store": false,
    });

    if let Some(inst) = instructions {
        body["instructions"] = serde_json::json!(inst);
    }
    if let Some(tools) = tools {
        body["tools"] = serde_json::json!(tools);
        // Phase 29 follow-up (sota-gaps-followup) — closes gap #7.
        // Honor `request.tool_choice` when set (e.g. THEO_FORCE_TOOL_CHOICE
        // env via run_engine). Strings starting with `{` are parsed as
        // JSON to support the per-tool forcing format
        // `{"type":"function","name":"delegate_task"}`. Other values are
        // emitted as the bare string (auto / required / none).
        // Default stays "auto" for backward-compat.
        let raw = request.tool_choice.as_deref().unwrap_or("auto");
        let choice_value: serde_json::Value = if raw.starts_with('{') {
            serde_json::from_str(raw).unwrap_or_else(|_| serde_json::json!("auto"))
        } else {
            serde_json::json!(raw)
        };
        body["tool_choice"] = choice_value;
    }

    // Reasoning effort (Codex Responses API format)
    if let Some(ref effort) = request.reasoning_effort {
        body["reasoning"] = serde_json::json!({
            "effort": effort,
        });
    }

    body
}

/// Parse a Codex Responses API response into a ChatResponse.
///
/// Handles both non-streaming full responses and the common response envelope.
pub fn from_codex_response(body: &serde_json::Value) -> Option<ChatResponse> {
    // The Codex response has `output` array with items
    let output = body.get("output")?.as_array()?;

    let mut content_parts: Vec<String> = Vec::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();

    for item in output {
        let item_type = item
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or_default();

        match item_type {
            "message" => {
                if let Some(content_arr) = item.get("content").and_then(|c| c.as_array()) {
                    for part in content_arr {
                        if part.get("type").and_then(|t| t.as_str()) == Some("output_text")
                            && let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                content_parts.push(text.to_string());
                            }
                    }
                }
            }
            "function_call" => {
                let id = item
                    .get("id")
                    .or_else(|| item.get("call_id"))
                    .and_then(|i| i.as_str())
                    .unwrap_or_default()
                    .to_string();
                let name = item
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or_default()
                    .to_string();
                let arguments = item
                    .get("arguments")
                    .map(|a| {
                        if let Some(s) = a.as_str() {
                            s.to_string()
                        } else {
                            a.to_string()
                        }
                    })
                    .unwrap_or_else(|| "{}".to_string());

                tool_calls.push(ToolCall::new(id, name, arguments));
            }
            _ => {}
        }
    }

    let content = if content_parts.is_empty() {
        None
    } else {
        Some(content_parts.join(""))
    };
    let tool_calls_opt = if tool_calls.is_empty() {
        None
    } else {
        Some(tool_calls)
    };

    let finish_reason = body.get("stop_reason").and_then(|r| r.as_str()).map(|r| {
        match r {
            "stop" => "stop",
            "tool_call" | "tool_calls" => "tool_calls",
            "max_output_tokens" | "length" => "length",
            other => other,
        }
        .to_string()
    });

    Some(ChatResponse {
        id: body.get("id").and_then(|i| i.as_str()).map(String::from),
        choices: vec![Choice {
            index: 0,
            message: ChoiceMessage {
                role: Role::Assistant,
                content,
                tool_calls: tool_calls_opt,
            },
            finish_reason,
        }],
        usage: body.get("usage").map(|u| Usage {
                prompt_tokens: u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                completion_tokens: u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
                    as u32,
                total_tokens: (u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
                    + u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0))
                    as u32,
            }),
    })
}

/// Parse a Codex SSE stream into a ChatResponse.
///
/// The stream contains events like:
/// ```text
/// event: response.output_text.delta
/// data: {"type":"response.output_text.delta","delta":"Hello"}
///
/// event: response.output_item.added
/// data: {"type":"response.output_item.added","item":{"type":"function_call","name":"read","id":"fc_1"}}
///
/// event: response.function_call_arguments.delta
/// data: {"type":"response.function_call_arguments.delta","delta":"{\"path\":\"a.py\"}"}
///
/// event: response.completed
/// data: {"type":"response.completed","response":{"id":"resp_1","output":[...],...}}
/// ```
pub fn from_codex_stream(stream_body: &str) -> Option<ChatResponse> {
    if let Some(response) = try_completed_event_response(stream_body) {
        return Some(response);
    }
    let acc = accumulate_deltas(stream_body);
    finalise_streamed_response(acc)
}

/// Strategy 1: Look for the `response.completed` SSE event with a non-empty
/// `output` array. Returns the parsed full response when found.
fn try_completed_event_response(stream_body: &str) -> Option<ChatResponse> {
    for chunk in stream_body.split("\n\n") {
        let (event, data) = parse_sse_event_data(chunk)?;
        if event != "response.completed" {
            continue;
        }
        let json: serde_json::Value = theo_domain::safe_json::from_str_bounded(
            data,
            theo_domain::safe_json::DEFAULT_JSON_LIMIT,
        )
        .ok()?;
        let response = json.get("response").unwrap_or(&json);
        // Some Codex responses ship an empty `output` array in the completed
        // event even when the stream emitted message/function_call items via
        // delta events. Fall through to the delta accumulator in that case.
        let output_empty = response
            .get("output")
            .and_then(|o| o.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(true);
        if !output_empty {
            return from_codex_response(response);
        }
        break;
    }
    None
}

/// Returns `(event, data)` for a single SSE chunk; both fields stripped of
/// their `event: ` / `data: ` prefixes.
fn parse_sse_event_data(chunk: &str) -> Option<(&str, &str)> {
    let mut event_line: Option<&str> = None;
    let mut data_line: Option<&str> = None;
    for l in chunk.lines() {
        if event_line.is_none()
            && let Some(rest) = l.strip_prefix("event: ")
        {
            event_line = Some(rest);
        }
        if data_line.is_none()
            && let Some(rest) = l.strip_prefix("data: ")
        {
            data_line = Some(rest);
        }
    }
    Some((event_line?.trim_end(), data_line?))
}

#[derive(Default)]
struct StreamedAccumulator {
    text: String,
    tool_calls: Vec<ToolCall>,
    current_tc_name: Option<String>,
    current_tc_id: Option<String>,
    current_tc_args: String,
    usage: Option<Usage>,
    response_id: Option<String>,
}

/// Strategy 2: Walk every SSE chunk, accumulating `output_text.delta`,
/// `output_item.added` (function calls), and `function_call_arguments.delta`.
fn accumulate_deltas(stream_body: &str) -> StreamedAccumulator {
    let mut acc = StreamedAccumulator::default();
    for chunk in stream_body.split("\n\n") {
        let Some((event, data)) = parse_sse_event_data(chunk) else {
            continue;
        };
        let Ok(json) = theo_domain::safe_json::from_str_bounded::<serde_json::Value>(
            data,
            theo_domain::safe_json::DEFAULT_JSON_LIMIT,
        ) else {
            continue;
        };
        apply_codex_delta(event, &json, &mut acc);
    }
    if let Some(name) = acc.current_tc_name.take() {
        acc.tool_calls.push(ToolCall::new(
            acc.current_tc_id.take().unwrap_or_default(),
            name,
            std::mem::take(&mut acc.current_tc_args),
        ));
    }
    acc
}

fn apply_codex_delta(event: &str, json: &serde_json::Value, acc: &mut StreamedAccumulator) {
    match event {
        "response.output_text.delta" => {
            if let Some(d) = json.get("delta").and_then(|d| d.as_str()) {
                acc.text.push_str(d);
            }
        }
        "response.output_item.added" => {
            if let Some(name) = acc.current_tc_name.take() {
                acc.tool_calls.push(ToolCall::new(
                    acc.current_tc_id.take().unwrap_or_default(),
                    name,
                    std::mem::take(&mut acc.current_tc_args),
                ));
            }
            if let Some(item) = json.get("item")
                && item.get("type").and_then(|t| t.as_str()) == Some("function_call")
            {
                acc.current_tc_name = item.get("name").and_then(|n| n.as_str()).map(String::from);
                acc.current_tc_id = item.get("id").and_then(|i| i.as_str()).map(String::from);
                acc.current_tc_args.clear();
            }
        }
        "response.function_call_arguments.delta" => {
            if let Some(d) = json.get("delta").and_then(|d| d.as_str()) {
                acc.current_tc_args.push_str(d);
            }
        }
        "response.completed" => {
            let resp = json.get("response").unwrap_or(json);
            if let Some(u) = resp.get("usage") {
                let inp = u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let out = u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                acc.usage = Some(Usage {
                    prompt_tokens: inp,
                    completion_tokens: out,
                    total_tokens: inp + out,
                });
            }
            acc.response_id = resp.get("id").and_then(|i| i.as_str()).map(String::from);
        }
        _ => {}
    }
}

fn finalise_streamed_response(acc: StreamedAccumulator) -> Option<ChatResponse> {
    if acc.text.is_empty() && acc.tool_calls.is_empty() {
        return None;
    }
    let has_tool_calls = !acc.tool_calls.is_empty();
    Some(ChatResponse {
        id: acc.response_id,
        choices: vec![Choice {
            index: 0,
            message: ChoiceMessage {
                role: Role::Assistant,
                content: if acc.text.is_empty() {
                    None
                } else {
                    Some(acc.text)
                },
                tool_calls: if acc.tool_calls.is_empty() {
                    None
                } else {
                    Some(acc.tool_calls)
                },
            },
            finish_reason: Some(if has_tool_calls { "tool_calls" } else { "stop" }.to_string()),
        }],
        usage: acc.usage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_codex_body_basic() {
        let req = ChatRequest::new(
            "gpt-5.3-codex",
            vec![Message::system("You are helpful."), Message::user("Hello")],
        )
        .with_max_tokens(1024);

        let body = to_codex_body(&req);

        assert_eq!(body["model"], "gpt-5.3-codex");
        assert_eq!(body["instructions"], "You are helpful.");
        // Codex endpoint does NOT support max_output_tokens
        assert!(body.get("max_output_tokens").is_none() || body["max_output_tokens"].is_null());

        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 1); // only user, system became instructions
        assert_eq!(input[0]["role"], "user");
    }

    #[test]
    fn test_to_codex_body_with_tool_calls() {
        let req = ChatRequest::new(
            "gpt-5.3-codex",
            vec![
                Message::system("Be helpful"),
                Message::user("Read main.py"),
                Message::assistant_with_tool_calls(
                    None,
                    vec![ToolCall::new("call_1", "read", r#"{"filePath":"main.py"}"#)],
                ),
                Message::tool_result("call_1", "read", "print('hello')"),
            ],
        );

        let body = to_codex_body(&req);
        let input = body["input"].as_array().unwrap();

        assert_eq!(input.len(), 3); // user + function_call + function_call_output
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["name"], "read");
        assert_eq!(input[2]["type"], "function_call_output");
        assert_eq!(input[2]["output"], "print('hello')");
    }

    #[test]
    fn test_from_codex_response() {
        let body = serde_json::json!({
            "id": "resp_123",
            "output": [
                {
                    "type": "message",
                    "content": [{"type": "output_text", "text": "Hello!"}]
                }
            ],
            "stop_reason": "stop",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });

        let resp = from_codex_response(&body).unwrap();
        assert_eq!(resp.content(), Some("Hello!"));
        assert_eq!(resp.finish_reason(), Some("stop"));
    }

    #[test]
    fn test_from_codex_response_with_tool_call() {
        let body = serde_json::json!({
            "id": "resp_456",
            "output": [
                {"type": "message", "content": [{"type": "output_text", "text": "Let me read that."}]},
                {"type": "function_call", "id": "fc_1", "name": "read", "arguments": "{\"filePath\":\"a.py\"}"}
            ],
            "stop_reason": "tool_call"
        });

        let resp = from_codex_response(&body).unwrap();
        assert_eq!(resp.content(), Some("Let me read that."));
        let tc = resp.tool_calls();
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0].function.name, "read");
        assert_eq!(resp.finish_reason(), Some("tool_calls"));
    }

    // ── Phase 29 follow-up (sota-gaps-followup) ──

    #[test]
    fn to_codex_body_defaults_tool_choice_to_auto() {
        use crate::types::ToolDefinition;
        let req = ChatRequest::new("gpt-5.3-codex", vec![])
            .with_tools(vec![ToolDefinition::new(
                "x",
                "y",
                serde_json::json!({"type": "object"}),
            )]);
        // with_tools sets tool_choice to "auto" by default.
        let body = to_codex_body(&req);
        assert_eq!(body.get("tool_choice").and_then(|v| v.as_str()), Some("auto"));
    }

    #[test]
    fn to_codex_body_honors_required_tool_choice() {
        use crate::types::ToolDefinition;
        let req = ChatRequest::new("gpt-5.3-codex", vec![])
            .with_tools(vec![ToolDefinition::new(
                "x",
                "y",
                serde_json::json!({"type": "object"}),
            )])
            .with_tool_choice("required");
        let body = to_codex_body(&req);
        assert_eq!(
            body.get("tool_choice").and_then(|v| v.as_str()),
            Some("required")
        );
    }

    #[test]
    fn to_codex_body_omits_tool_choice_when_no_tools() {
        let req = ChatRequest::new("gpt-5.3-codex", vec![])
            .with_tool_choice("required");
        let body = to_codex_body(&req);
        // No tools → tool_choice not emitted (Codex would reject it).
        assert!(body.get("tool_choice").is_none());
    }

    #[test]
    fn to_codex_body_parses_per_tool_choice_json_object() {
        use crate::types::ToolDefinition;
        let req = ChatRequest::new("gpt-5.3-codex", vec![])
            .with_tools(vec![ToolDefinition::new(
                "delegate_task",
                "y",
                serde_json::json!({"type": "object"}),
            )])
            .with_tool_choice(r#"{"type":"function","name":"delegate_task"}"#);
        let body = to_codex_body(&req);
        let tc = body.get("tool_choice").expect("must be set");
        assert_eq!(tc.get("type").and_then(|v| v.as_str()), Some("function"));
        assert_eq!(tc.get("name").and_then(|v| v.as_str()), Some("delegate_task"));
    }

    /// Phase 29 follow-up — gap revealed during real OAuth testing:
    /// gpt-5.4 / gpt-5.2-codex emit function_call via INCREMENTAL events
    /// (output_item.added → function_call_arguments.delta → done) but the
    /// final response.completed has `output: []`. gpt-5.3-codex includes
    /// the function_call in response.completed.output.
    /// This test simulates the gpt-5.4 pattern and verifies the fallback
    /// delta accumulator correctly extracts the tool call.
    #[test]
    fn from_codex_stream_extracts_function_call_when_response_completed_output_is_empty() {
        let sse = r#"event: response.created
data: {"type":"response.created","response":{"id":"r1","status":"in_progress","output":[]}}

event: response.in_progress
data: {"type":"response.in_progress","response":{"id":"r1","output":[]}}

event: response.output_item.added
data: {"type":"response.output_item.added","item":{"id":"fc_1","type":"function_call","status":"in_progress","arguments":"","call_id":"call_xyz","name":"delegate_task_single"},"output_index":0,"sequence_number":2}

event: response.function_call_arguments.delta
data: {"type":"response.function_call_arguments.delta","delta":"{\"agent\":\"audit-bot\",\"objective\":\"audit\"}","item_id":"fc_1","output_index":0,"sequence_number":3}

event: response.function_call_arguments.done
data: {"type":"response.function_call_arguments.done","arguments":"{\"agent\":\"audit-bot\",\"objective\":\"audit\"}","item_id":"fc_1","output_index":0,"sequence_number":4}

event: response.output_item.done
data: {"type":"response.output_item.done","item":{"id":"fc_1","type":"function_call","status":"completed","arguments":"{\"agent\":\"audit-bot\",\"objective\":\"audit\"}","call_id":"call_xyz","name":"delegate_task_single"},"output_index":0,"sequence_number":5}

event: response.completed
data: {"type":"response.completed","response":{"id":"r1","status":"completed","output":[],"usage":{"input_tokens":50,"output_tokens":20}},"sequence_number":6}

"#;
        let resp = from_codex_stream(sse).expect("must parse non-None");
        let choice = resp.choices.first().expect("at least one choice");
        let tool_calls = choice
            .message
            .tool_calls
            .as_ref()
            .expect("tool_calls must be Some");
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].function.name, "delegate_task_single");
        assert!(
            tool_calls[0].function.arguments.contains("audit-bot"),
            "args must include audit-bot, got: {}",
            tool_calls[0].function.arguments
        );
    }

    #[test]
    fn to_codex_body_falls_back_to_auto_for_malformed_json_choice() {
        use crate::types::ToolDefinition;
        let req = ChatRequest::new("gpt-5.3-codex", vec![])
            .with_tools(vec![ToolDefinition::new(
                "x",
                "y",
                serde_json::json!({"type": "object"}),
            )])
            .with_tool_choice("{not valid json");
        let body = to_codex_body(&req);
        assert_eq!(
            body.get("tool_choice").and_then(|v| v.as_str()),
            Some("auto")
        );
    }
}
