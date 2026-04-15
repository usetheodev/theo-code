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
                if let Some(ref content) = msg.content {
                    if !content.is_empty() {
                        input.push(serde_json::json!({
                            "role": "assistant",
                            "content": [{"type": "output_text", "text": content}]
                        }));
                    }
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
        body["tool_choice"] = serde_json::json!("auto");
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
                        if part.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                content_parts.push(text.to_string());
                            }
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
        usage: body.get("usage").and_then(|u| {
            Some(Usage {
                prompt_tokens: u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                completion_tokens: u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
                    as u32,
                total_tokens: (u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
                    + u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0))
                    as u32,
            })
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
    // Strategy: look for response.completed event which has the full response.
    // If not found, accumulate deltas.
    for chunk in stream_body.split("\n\n") {
        let lines: Vec<&str> = chunk.lines().collect();
        let event_line = lines.iter().find(|l| l.starts_with("event: "));
        let data_line = lines.iter().find(|l| l.starts_with("data: "));

        let Some(event) = event_line.and_then(|l| l.strip_prefix("event: ")) else {
            continue;
        };
        let Some(data) = data_line.and_then(|l| l.strip_prefix("data: ")) else {
            continue;
        };

        if event.trim() == "response.completed" {
            let json: serde_json::Value = serde_json::from_str(data).ok()?;
            // The completed event has a "response" field with the full response
            let response = json.get("response").unwrap_or(&json);
            // Some Codex responses ship an empty `output` array in the completed
            // event even when the stream emitted message/function_call items via
            // delta events. In that case we must fall through to the delta
            // accumulator instead of returning an empty response.
            let output_empty = response
                .get("output")
                .and_then(|o| o.as_array())
                .map(|a| a.is_empty())
                .unwrap_or(true);
            if !output_empty {
                return from_codex_response(response);
            }
            // else: fall through to delta accumulation below
            break;
        }
    }

    // Fallback: accumulate text and tool call deltas.
    // Also extract usage from response.completed even though output was empty.
    let mut text = String::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    let mut current_tc_name: Option<String> = None;
    let mut current_tc_id: Option<String> = None;
    let mut current_tc_args = String::new();
    let mut usage: Option<Usage> = None;
    let mut response_id: Option<String> = None;

    for chunk in stream_body.split("\n\n") {
        let lines: Vec<&str> = chunk.lines().collect();
        let event_line = lines.iter().find(|l| l.starts_with("event: "));
        let data_line = lines.iter().find(|l| l.starts_with("data: "));

        let Some(event) = event_line.and_then(|l| l.strip_prefix("event: ")) else {
            continue;
        };
        let Some(data) = data_line.and_then(|l| l.strip_prefix("data: ")) else {
            continue;
        };
        let event = event.trim();

        let json: serde_json::Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => continue,
        };

        match event {
            "response.output_text.delta" => {
                if let Some(d) = json.get("delta").and_then(|d| d.as_str()) {
                    text.push_str(d);
                }
            }
            "response.output_item.added" => {
                if let Some(name) = current_tc_name.take() {
                    tool_calls.push(ToolCall::new(
                        current_tc_id.take().unwrap_or_default(),
                        name,
                        std::mem::take(&mut current_tc_args),
                    ));
                }

                if let Some(item) = json.get("item") {
                    if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                        current_tc_name =
                            item.get("name").and_then(|n| n.as_str()).map(String::from);
                        current_tc_id = item.get("id").and_then(|i| i.as_str()).map(String::from);
                        current_tc_args.clear();
                    }
                }
            }
            "response.function_call_arguments.delta" => {
                if let Some(d) = json.get("delta").and_then(|d| d.as_str()) {
                    current_tc_args.push_str(d);
                }
            }
            "response.completed" => {
                let resp = json.get("response").unwrap_or(&json);
                if let Some(u) = resp.get("usage") {
                    let inp = u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    let out = u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    usage = Some(Usage {
                        prompt_tokens: inp,
                        completion_tokens: out,
                        total_tokens: inp + out,
                    });
                }
                response_id = resp.get("id").and_then(|i| i.as_str()).map(String::from);
            }
            _ => {}
        }
    }

    // Flush last tool call
    if let Some(name) = current_tc_name.take() {
        tool_calls.push(ToolCall::new(
            current_tc_id.take().unwrap_or_default(),
            name,
            current_tc_args,
        ));
    }

    if text.is_empty() && tool_calls.is_empty() {
        return None;
    }

    let has_tool_calls = !tool_calls.is_empty();
    Some(ChatResponse {
        id: response_id,
        choices: vec![Choice {
            index: 0,
            message: ChoiceMessage {
                role: Role::Assistant,
                content: if text.is_empty() { None } else { Some(text) },
                tool_calls: if tool_calls.is_empty() {
                    None
                } else {
                    Some(tool_calls)
                },
            },
            finish_reason: Some(if has_tool_calls { "tool_calls" } else { "stop" }.to_string()),
        }],
        usage,
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
}
