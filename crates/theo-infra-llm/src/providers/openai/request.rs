//! Single-purpose slice extracted from `providers/openai.rs` (D5 split).

#![allow(unused_imports)]

use super::super::common::*;
use serde_json::Value;

pub fn from_request(body: &Value) -> CommonRequest {
    let mut messages = Vec::new();
    let input = extract_input_array(body);
    for m in &input {
        if try_parse_roleless_item(m, &mut messages) {
            continue;
        }
        let Some(role) = m.get("role").and_then(|r| r.as_str()) else {
            continue;
        };
        match role {
            "system" | "developer" => parse_system_role(m, &mut messages),
            "user" => parse_user_role(m, &mut messages),
            "assistant" => parse_assistant_role(m, &mut messages),
            "tool" => parse_tool_role(m, &mut messages),
            _ => {}
        }
    }
    let tool_choice = parse_request_tool_choice(body);
    let stop = parse_request_stop_sequence(body);
    let tools = parse_request_tools(body);
    assemble_common_request(body, messages, tools, tool_choice, stop)
}

fn extract_input_array(body: &Value) -> Vec<Value> {
    body.get("input")
        .and_then(|i| i.as_array())
        .or_else(|| body.get("messages").and_then(|m| m.as_array()))
        .cloned()
        .unwrap_or_default()
}

/// Responses-API roleless items (function_call, function_call_output).
/// Returns `true` if the item was consumed (caller should skip role
/// dispatch).
fn try_parse_roleless_item(m: &Value, messages: &mut Vec<CommonMessage>) -> bool {
    if m.get("role").is_some() {
        return false;
    }
    let Some(item_type) = m.get("type").and_then(|t| t.as_str()) else {
        return false;
    };
    match item_type {
        "function_call" => {
            parse_function_call_item(m, messages);
            true
        }
        "function_call_output" => {
            parse_function_call_output_item(m, messages);
            true
        }
        _ => true,
    }
}

fn parse_function_call_item(m: &Value, messages: &mut Vec<CommonMessage>) {
    let name = m.get("name").and_then(|n| n.as_str()).unwrap_or_default();
    let args = m
        .get("arguments")
        .map(|a| {
            if let Some(s) = a.as_str() {
                s.to_string()
            } else {
                a.to_string()
            }
        })
        .unwrap_or_else(|| "{}".to_string());
    let id = m.get("id").and_then(|i| i.as_str()).unwrap_or_default();
    messages.push(CommonMessage {
        role: Role::Assistant,
        content: None,
        tool_call_id: None,
        tool_calls: Some(vec![CommonToolCall {
            id: id.to_string(),
            call_type: "function".to_string(),
            function: CommonFunctionCall {
                name: name.to_string(),
                arguments: args,
            },
        }]),
        name: None,
    });
}

fn parse_function_call_output_item(m: &Value, messages: &mut Vec<CommonMessage>) {
    let call_id = m
        .get("call_id")
        .and_then(|i| i.as_str())
        .unwrap_or_default();
    let output = m
        .get("output")
        .map(|o| {
            if let Some(s) = o.as_str() {
                s.to_string()
            } else {
                o.to_string()
            }
        })
        .unwrap_or_default();
    messages.push(CommonMessage {
        role: Role::Tool,
        content: Some(Content::Text(output)),
        tool_call_id: Some(call_id.to_string()),
        tool_calls: None,
        name: None,
    });
}

fn parse_system_role(m: &Value, messages: &mut Vec<CommonMessage>) {
    let content = m.get("content");
    let text = if let Some(s) = content.and_then(|c| c.as_str()) {
        Some(s.to_string())
    } else if let Some(arr) = content.and_then(|c| c.as_array()) {
        arr.iter()
            .find_map(|p| p.get("text").and_then(|t| t.as_str()).map(String::from))
    } else {
        None
    };
    if let Some(text) = text
        && !text.is_empty()
    {
        messages.push(CommonMessage {
            role: Role::System,
            content: Some(Content::Text(text)),
            tool_call_id: None,
            tool_calls: None,
            name: None,
        });
    }
}

fn parse_user_role(m: &Value, messages: &mut Vec<CommonMessage>) {
    let content = m.get("content");
    if let Some(s) = content.and_then(|c| c.as_str()) {
        messages.push(CommonMessage {
            role: Role::User,
            content: Some(Content::Text(s.to_string())),
            tool_call_id: None,
            tool_calls: None,
            name: None,
        });
        return;
    }
    let Some(arr) = content.and_then(|c| c.as_array()) else {
        return;
    };
    let parts = parse_user_parts(arr);
    if parts.len() == 1
        && let ContentPart::Text { text } = &parts[0]
    {
        messages.push(CommonMessage {
            role: Role::User,
            content: Some(Content::Text(text.clone())),
            tool_call_id: None,
            tool_calls: None,
            name: None,
        });
        return;
    }
    if !parts.is_empty() {
        messages.push(CommonMessage {
            role: Role::User,
            content: Some(Content::Parts(parts)),
            tool_call_id: None,
            tool_calls: None,
            name: None,
        });
    }
}

fn parse_user_parts(arr: &[Value]) -> Vec<ContentPart> {
    arr.iter()
        .filter_map(|p| {
            let ptype = p.get("type").and_then(|t| t.as_str())?;
            match ptype {
                "text" | "input_text" => {
                    let text = p.get("text").and_then(|t| t.as_str())?;
                    Some(ContentPart::Text {
                        text: text.to_string(),
                    })
                }
                "image_url" | "input_image" => {
                    let url = p
                        .get("image_url")
                        .and_then(|i| i.get("url"))
                        .and_then(|u| u.as_str())?;
                    Some(ContentPart::ImageUrl {
                        image_url: ImageUrl {
                            url: url.to_string(),
                        },
                    })
                }
                _ => None,
            }
        })
        .collect()
}

fn parse_assistant_role(m: &Value, messages: &mut Vec<CommonMessage>) {
    let content = m.get("content").and_then(|c| c.as_str()).map(String::from);
    let tool_calls = m.get("tool_calls").and_then(|tc| tc.as_array()).map(|arr| {
        arr.iter()
            .filter_map(|tc| {
                let func = tc.get("function")?;
                Some(CommonToolCall {
                    id: tc.get("id").and_then(|i| i.as_str())?.to_string(),
                    call_type: "function".to_string(),
                    function: CommonFunctionCall {
                        name: func.get("name").and_then(|n| n.as_str())?.to_string(),
                        arguments: func
                            .get("arguments")
                            .and_then(|a| a.as_str())?
                            .to_string(),
                    },
                })
            })
            .collect()
    });
    messages.push(CommonMessage {
        role: Role::Assistant,
        content: content.filter(|s| !s.is_empty()).map(Content::Text),
        tool_call_id: None,
        tool_calls,
        name: None,
    });
}

fn parse_tool_role(m: &Value, messages: &mut Vec<CommonMessage>) {
    let content = m.get("content").map(|c| {
        if let Some(s) = c.as_str() {
            s.to_string()
        } else {
            c.to_string()
        }
    });
    messages.push(CommonMessage {
        role: Role::Tool,
        content: content.map(Content::Text),
        tool_call_id: m
            .get("tool_call_id")
            .and_then(|i| i.as_str())
            .map(String::from),
        tool_calls: None,
        name: None,
    });
}

fn parse_request_tool_choice(body: &Value) -> Option<ToolChoice> {
    body.get("tool_choice").and_then(|tc| {
        if let Some(s) = tc.as_str() {
            return Some(ToolChoice::Mode(s.to_string()));
        }
        let tc_type = tc.get("type").and_then(|t| t.as_str())?;
        if tc_type == "function" {
            let name = tc
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())?;
            return Some(ToolChoice::Function {
                choice_type: "function".to_string(),
                function: ToolChoiceFunction {
                    name: name.to_string(),
                },
            });
        }
        None
    })
}

fn parse_request_stop_sequence(body: &Value) -> Option<StopSequence> {
    body.get("stop_sequences")
        .or(body.get("stop"))
        .and_then(|v| {
            if let Some(arr) = v.as_array() {
                let strs: Vec<String> = arr
                    .iter()
                    .filter_map(|s| s.as_str().map(String::from))
                    .collect();
                let mut iter = strs.into_iter();
                match (iter.next(), iter.next()) {
                    (Some(only), None) => Some(StopSequence::Single(only)),
                    (Some(first), Some(second)) => {
                        let mut rest: Vec<String> = vec![first, second];
                        rest.extend(iter);
                        Some(StopSequence::Multiple(rest))
                    }
                    (None, _) => None,
                }
            } else {
                v.as_str().map(|s| StopSequence::Single(s.to_string()))
            }
        })
}

fn parse_request_tools(body: &Value) -> Option<Vec<CommonTool>> {
    body.get("tools").and_then(|t| t.as_array()).map(|arr| {
        arr.iter()
            .filter_map(|t| {
                if t.get("type").and_then(|tt| tt.as_str()) != Some("function") {
                    return None;
                }
                Some(CommonTool {
                    tool_type: "function".to_string(),
                    function: CommonFunctionDef {
                        name: t
                            .get("name")
                            .or_else(|| t.get("function").and_then(|f| f.get("name")))
                            .and_then(|n| n.as_str())?
                            .to_string(),
                        description: t
                            .get("description")
                            .or_else(|| t.get("function").and_then(|f| f.get("description")))
                            .and_then(|d| d.as_str())
                            .map(String::from),
                        parameters: t
                            .get("parameters")
                            .or_else(|| t.get("function").and_then(|f| f.get("parameters")))
                            .cloned(),
                    },
                })
            })
            .collect()
    })
}

fn assemble_common_request(
    body: &Value,
    messages: Vec<CommonMessage>,
    tools: Option<Vec<CommonTool>>,
    tool_choice: Option<ToolChoice>,
    stop: Option<StopSequence>,
) -> CommonRequest {
    CommonRequest {
        model: body
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or_default()
            .to_string(),
        max_tokens: body
            .get("max_output_tokens")
            .or(body.get("max_tokens"))
            .and_then(|m| m.as_u64())
            .map(|v| v as u32),
        temperature: body
            .get("temperature")
            .and_then(|t| t.as_f64())
            .map(|v| v as f32),
        top_p: body.get("top_p").and_then(|t| t.as_f64()).map(|v| v as f32),
        stop,
        messages,
        stream: body.get("stream").and_then(|s| s.as_bool()),
        tools,
        tool_choice,
    }
}

/// Convert a CommonRequest to OpenAI Responses API format.
pub fn to_request(body: &CommonRequest) -> Value {
    let mut input: Vec<Value> = Vec::new();

    for m in &body.messages {
        match m.role {
            Role::System => {
                if let Some(text) = m.content.as_ref().map(|c| c.to_text()) {
                    input.push(serde_json::json!({"role": "system", "content": text}));
                }
            }
            Role::User => {
                if let Some(content) = &m.content {
                    match content {
                        Content::Text(s) => {
                            input.push(serde_json::json!({"role": "user", "content": [{"type": "input_text", "text": s}]}));
                        }
                        Content::Parts(parts) => {
                            let items: Vec<Value> = parts.iter().map(|p| match p {
                                ContentPart::Text { text } => serde_json::json!({"type": "input_text", "text": text}),
                                ContentPart::ImageUrl { image_url } => serde_json::json!({"type": "input_image", "image_url": {"url": image_url.url}}),
                            }).collect();
                            input.push(serde_json::json!({"role": "user", "content": items}));
                        }
                    }
                }
            }
            Role::Assistant => {
                if let Some(text) = m.content.as_ref().map(|c| c.to_text())
                    && !text.is_empty() {
                        input.push(serde_json::json!({"role": "assistant", "content": [{"type": "output_text", "text": text}]}));
                    }
                if let Some(tool_calls) = &m.tool_calls {
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
                let output = m.content.as_ref().map(|c| c.to_text()).unwrap_or_default();
                input.push(serde_json::json!({
                    "type": "function_call_output",
                    "call_id": m.tool_call_id,
                    "output": output,
                }));
            }
        }
    }

    let tools: Option<Vec<Value>> = body.tools.as_ref().map(|tools| {
        tools
            .iter()
            .filter(|t| t.tool_type == "function")
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

    let mut result = serde_json::json!({
        "model": body.model,
        "input": input,
        "stream": body.stream.unwrap_or(false),
    });

    if let Some(max_tokens) = body.max_tokens {
        result["max_output_tokens"] = serde_json::json!(max_tokens);
    }
    if let Some(top_p) = body.top_p {
        result["top_p"] = serde_json::json!(top_p);
    }
    if let Some(stop) = &body.stop {
        result["stop_sequences"] = match stop {
            StopSequence::Single(s) => serde_json::json!([s]),
            StopSequence::Multiple(v) => serde_json::json!(v),
        };
    }
    if let Some(tools) = tools {
        result["tools"] = Value::Array(tools);
    }
    if let Some(tc) = &body.tool_choice {
        result["tool_choice"] = match tc {
            ToolChoice::Mode(m) => Value::String(m.clone()),
            ToolChoice::Function { function, .. } => {
                serde_json::json!({"type": "function", "function": {"name": function.name}})
            }
        };
    }

    result
}
