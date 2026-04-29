use super::common::*;
use serde_json::Value;

/// Convert an OA-Compatible (standard chat completions) request to CommonRequest.
pub fn from_request(body: &Value) -> CommonRequest {
    let mut messages = Vec::new();
    parse_messages_array(body, &mut messages);
    let tools = parse_tools_compat(body);
    let tool_choice = parse_tool_choice_compat(body);
    let stop = parse_stop_compat(body);
    assemble_common_request_compat(body, messages, tools, tool_choice, stop)
}

fn parse_messages_array(body: &Value, messages: &mut Vec<CommonMessage>) {
    let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) else {
        return;
    };
    for m in msgs {
        let Some(role) = m.get("role").and_then(|r| r.as_str()) else {
            continue;
        };
        match role {
            "system" => parse_system_role_compat(m, messages),
            "user" => parse_user_role_compat(m, messages),
            "assistant" => parse_assistant_role_compat(m, messages),
            "tool" => parse_tool_role_compat(m, messages),
            _ => {}
        }
    }
}

fn parse_system_role_compat(m: &Value, messages: &mut Vec<CommonMessage>) {
    if let Some(text) = m.get("content").and_then(|c| c.as_str())
        && !text.is_empty()
    {
        messages.push(CommonMessage {
            role: Role::System,
            content: Some(Content::Text(text.to_string())),
            tool_call_id: None,
            tool_calls: None,
            name: None,
        });
    }
}

fn parse_user_role_compat(m: &Value, messages: &mut Vec<CommonMessage>) {
    let content = m.get("content");
    if let Some(text) = content.and_then(|c| c.as_str()) {
        messages.push(CommonMessage {
            role: Role::User,
            content: Some(Content::Text(text.to_string())),
            tool_call_id: None,
            tool_calls: None,
            name: None,
        });
        return;
    }
    let Some(parts) = content.and_then(|c| c.as_array()) else {
        return;
    };
    let content_parts = parse_user_parts_compat(parts);
    if content_parts.len() == 1
        && let ContentPart::Text { text } = &content_parts[0]
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
    if !content_parts.is_empty() {
        messages.push(CommonMessage {
            role: Role::User,
            content: Some(Content::Parts(content_parts)),
            tool_call_id: None,
            tool_calls: None,
            name: None,
        });
    }
}

fn parse_user_parts_compat(parts: &[Value]) -> Vec<ContentPart> {
    parts
        .iter()
        .filter_map(|p| {
            let ptype = p.get("type").and_then(|t| t.as_str())?;
            match ptype {
                "text" => {
                    let text = p.get("text").and_then(|t| t.as_str())?;
                    Some(ContentPart::Text {
                        text: text.to_string(),
                    })
                }
                "image_url" => {
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

fn parse_assistant_role_compat(m: &Value, messages: &mut Vec<CommonMessage>) {
    let content = m.get("content").and_then(|c| c.as_str()).map(String::from);
    let tool_calls = m.get("tool_calls").and_then(|tc| tc.as_array()).map(|arr| {
        arr.iter()
            .filter_map(|tc| {
                Some(CommonToolCall {
                    id: tc.get("id").and_then(|i| i.as_str())?.to_string(),
                    call_type: "function".to_string(),
                    function: CommonFunctionCall {
                        name: tc
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())?
                            .to_string(),
                        arguments: tc
                            .get("function")
                            .and_then(|f| f.get("arguments"))
                            .and_then(|a| a.as_str())?
                            .to_string(),
                    },
                })
            })
            .collect()
    });
    messages.push(CommonMessage {
        role: Role::Assistant,
        content: content.map(Content::Text),
        tool_call_id: None,
        tool_calls,
        name: None,
    });
}

fn parse_tool_role_compat(m: &Value, messages: &mut Vec<CommonMessage>) {
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

fn parse_tools_compat(body: &Value) -> Option<Vec<CommonTool>> {
    body.get("tools").and_then(|t| t.as_array()).map(|arr| {
        arr.iter()
            .filter_map(|t| {
                let func = t.get("function")?;
                Some(CommonTool {
                    tool_type: "function".to_string(),
                    function: CommonFunctionDef {
                        name: func.get("name").and_then(|n| n.as_str())?.to_string(),
                        description: func
                            .get("description")
                            .and_then(|d| d.as_str())
                            .map(String::from),
                        parameters: func.get("parameters").cloned(),
                    },
                })
            })
            .collect()
    })
}

fn parse_tool_choice_compat(body: &Value) -> Option<ToolChoice> {
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

fn parse_stop_compat(body: &Value) -> Option<StopSequence> {
    body.get("stop").and_then(|v| {
        if let Some(s) = v.as_str() {
            return Some(StopSequence::Single(s.to_string()));
        }
        let arr = v.as_array()?;
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
    })
}

fn assemble_common_request_compat(
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
            .get("max_tokens")
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

/// Convert a CommonRequest to OA-Compatible chat completions format.
pub fn to_request(body: &CommonRequest) -> Value {
    let messages: Vec<Value> = body.messages.iter().map(|m| {
        let mut msg = serde_json::json!({ "role": match m.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }});

        if let Some(content) = &m.content {
            match content {
                Content::Text(s) => { msg["content"] = Value::String(s.clone()); }
                Content::Parts(parts) => {
                    let arr: Vec<Value> = parts.iter().map(|p| match p {
                        ContentPart::Text { text } => serde_json::json!({"type":"text","text":text}),
                        ContentPart::ImageUrl { image_url } => serde_json::json!({"type":"image_url","image_url":{"url":image_url.url}}),
                    }).collect();
                    msg["content"] = Value::Array(arr);
                }
            }
        }

        if let Some(tool_call_id) = &m.tool_call_id {
            msg["tool_call_id"] = Value::String(tool_call_id.clone());
        }

        if let Some(tool_calls) = &m.tool_calls {
            let tcs: Vec<Value> = tool_calls.iter().map(|tc| serde_json::json!({
                "id": tc.id,
                "type": "function",
                "function": { "name": tc.function.name, "arguments": tc.function.arguments }
            })).collect();
            msg["tool_calls"] = Value::Array(tcs);
        }

        msg
    }).collect();

    let tools: Option<Vec<Value>> = body.tools.as_ref().map(|tools| {
        tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.function.name,
                        "description": t.function.description,
                        "parameters": t.function.parameters,
                    }
                })
            })
            .collect()
    });

    let mut result = serde_json::json!({
        "model": body.model,
        "messages": messages,
    });

    if let Some(max_tokens) = body.max_tokens {
        result["max_tokens"] = serde_json::json!(max_tokens);
    }
    if let Some(temp) = body.temperature {
        result["temperature"] = serde_json::json!(temp);
    }
    if let Some(top_p) = body.top_p {
        result["top_p"] = serde_json::json!(top_p);
    }
    if let Some(stream) = body.stream {
        result["stream"] = serde_json::json!(stream);
    }
    if let Some(stop) = &body.stop {
        result["stop"] = match stop {
            StopSequence::Single(s) => Value::String(s.clone()),
            StopSequence::Multiple(v) => serde_json::json!(v),
        };
    }
    if let Some(tools) = tools {
        result["tools"] = Value::Array(tools);
    }
    if let Some(tc) = &body.tool_choice {
        result["tool_choice"] = match tc {
            ToolChoice::Mode(m) => Value::String(m.clone()),
            ToolChoice::Function {
                choice_type,
                function,
            } => serde_json::json!({
                "type": choice_type, "function": { "name": function.name }
            }),
        };
    }

    result
}

/// Convert an OA-Compatible response to CommonResponse.
pub fn from_response(resp: &Value) -> CommonResponse {
    let choices = resp
        .get("choices")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();
    let choice = choices.first();

    let message = choice.and_then(|c| c.get("message"));

    let content = message
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .map(String::from);

    let tool_calls: Option<Vec<CommonToolCall>> = message
        .and_then(|m| m.get("tool_calls"))
        .and_then(|tc| tc.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|tc| {
                    Some(CommonToolCall {
                        id: tc.get("id").and_then(|i| i.as_str())?.to_string(),
                        call_type: "function".to_string(),
                        function: CommonFunctionCall {
                            name: tc
                                .get("function")
                                .and_then(|f| f.get("name"))
                                .and_then(|n| n.as_str())?
                                .to_string(),
                            arguments: tc
                                .get("function")
                                .and_then(|f| f.get("arguments"))
                                .and_then(|a| a.as_str())?
                                .to_string(),
                        },
                    })
                })
                .collect()
        });

    let finish_reason = choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(|r| r.as_str())
        .map(String::from);

    let usage = resp.get("usage").map(|u| CommonUsage {
        prompt_tokens: u
            .get("prompt_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
        completion_tokens: u
            .get("completion_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
        total_tokens: u
            .get("total_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
        prompt_tokens_details: u
            .get("prompt_tokens_details")
            .and_then(|d| d.get("cached_tokens"))
            .and_then(|c| c.as_u64())
            .map(|c| PromptTokensDetails {
                cached_tokens: Some(c as u32),
            }),
    });

    CommonResponse {
        id: resp
            .get("id")
            .and_then(|i| i.as_str())
            .unwrap_or_default()
            .to_string(),
        object: "chat.completion".to_string(),
        created: now_unix(),
        model: resp
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or_default()
            .to_string(),
        choices: vec![CommonChoice {
            index: 0,
            message: CommonChoiceMessage {
                role: "assistant".to_string(),
                content,
                tool_calls: tool_calls.filter(|tc| !tc.is_empty()),
            },
            finish_reason,
        }],
        usage,
    }
}

/// Convert a CommonResponse to OA-Compatible format (passthrough — already in the right format).
pub fn to_response(resp: &CommonResponse) -> Value {
    serde_json::to_value(resp).unwrap_or_default()
}

/// Parse an OA-Compatible SSE chunk string into a CommonChunk.
pub fn from_chunk(chunk: &str) -> Result<CommonChunk, String> {
    let data = chunk
        .strip_prefix("data: ")
        .ok_or_else(|| chunk.to_string())?;

    // T2.7: bound the SSE chunk to 10 MiB.
    let json: Value =
        theo_domain::safe_json::from_str_bounded(data, theo_domain::safe_json::DEFAULT_JSON_LIMIT)
            .map_err(|_| chunk.to_string())?;

    let choices = json.get("choices").and_then(|c| c.as_array());
    let choice = match choices.and_then(|c| c.first()) {
        Some(c) => c,
        None => return Err(chunk.to_string()),
    };
    let delta = choice.get("delta");

    let mut out = CommonChunk {
        id: json
            .get("id")
            .and_then(|i| i.as_str())
            .unwrap_or_default()
            .to_string(),
        object: "chat.completion.chunk".to_string(),
        created: json
            .get("created")
            .and_then(|c| c.as_u64())
            .unwrap_or_else(now_unix),
        model: json
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or_default()
            .to_string(),
        choices: Vec::new(),
        usage: None,
    };

    if let Some(delta) = delta {
        if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
            out.choices.push(CommonChunkChoice {
                index: choice.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32,
                delta: ChunkDelta {
                    content: Some(content.to_string()),
                    ..Default::default()
                },
                finish_reason: None,
            });
        }

        if let Some(tool_calls) = delta.get("tool_calls").and_then(|tc| tc.as_array()) {
            for tc in tool_calls {
                out.choices.push(CommonChunkChoice {
                    index: choice.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32,
                    delta: ChunkDelta {
                        tool_calls: Some(vec![ChunkToolCall {
                            index: tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32,
                            id: tc.get("id").and_then(|i| i.as_str()).map(String::from),
                            call_type: tc.get("type").and_then(|t| t.as_str()).map(String::from),
                            function: tc.get("function").map(|f| ChunkFunction {
                                name: f.get("name").and_then(|n| n.as_str()).map(String::from),
                                arguments: f
                                    .get("arguments")
                                    .and_then(|a| a.as_str())
                                    .map(String::from),
                            }),
                        }]),
                        ..Default::default()
                    },
                    finish_reason: None,
                });
            }
        }
    }

    if let Some(fr) = choice.get("finish_reason").and_then(|r| r.as_str()) {
        out.choices.push(CommonChunkChoice {
            index: choice.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32,
            delta: ChunkDelta::default(),
            finish_reason: Some(fr.to_string()),
        });
    }

    if let Some(usage) = json.get("usage") {
        out.usage = Some(CommonUsage {
            prompt_tokens: usage
                .get("prompt_tokens")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32),
            completion_tokens: usage
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32),
            total_tokens: usage
                .get("total_tokens")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32),
            prompt_tokens_details: usage
                .get("prompt_tokens_details")
                .and_then(|d| d.get("cached_tokens"))
                .and_then(|c| c.as_u64())
                .map(|c| PromptTokensDetails {
                    cached_tokens: Some(c as u32),
                }),
        });
    }

    Ok(out)
}

/// Convert a CommonChunk to OA-Compatible SSE format string.
pub fn to_chunk(chunk: &CommonChunk) -> String {
    let json = serde_json::to_string(chunk).unwrap_or_default();
    format!("data: {json}")
}

/// Normalize OA-Compatible usage into UsageInfo.
pub fn normalize_usage(usage: &Value, adjust_cache: bool) -> UsageInfo {
    let input = usage
        .get("prompt_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let output = usage
        .get("completion_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let reasoning = usage
        .get("completion_tokens_details")
        .and_then(|d| d.get("reasoning_tokens"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    let mut cache_read = usage
        .get("cached_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| {
            usage
                .get("prompt_tokens_details")
                .and_then(|d| d.get("cached_tokens"))
                .and_then(|v| v.as_u64())
        })
        .map(|v| v as u32);

    if adjust_cache && cache_read.is_none() {
        cache_read = Some((input as f64 * 0.9) as u32);
    }

    UsageInfo {
        input_tokens: input.saturating_sub(cache_read.unwrap_or(0)),
        output_tokens: output,
        reasoning_tokens: reasoning,
        cache_read_tokens: cache_read,
        cache_write_5m_tokens: None,
        cache_write_1h_tokens: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_oa_compat_request() {
        let body = serde_json::json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Hi!", "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "read", "arguments": "{\"path\":\"a.py\"}"}}
                ]},
                {"role": "tool", "tool_call_id": "call_1", "content": "file contents"}
            ],
            "tools": [{"type": "function", "function": {"name": "read", "description": "Read file", "parameters": {}}}],
            "tool_choice": "auto",
            "max_tokens": 1024
        });

        let req = from_request(&body);
        assert_eq!(req.model, "gpt-4");
        assert_eq!(req.messages.len(), 4);
        assert_eq!(req.messages[0].role, Role::System);
        assert_eq!(req.messages[2].tool_calls.as_ref().unwrap().len(), 1);
        assert!(req.tools.is_some());
    }

    #[test]
    fn test_roundtrip_oa_compat() {
        let body = serde_json::json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}],
            "max_tokens": 100,
            "temperature": 0.5
        });

        let common = from_request(&body);
        let back = to_request(&common);

        assert_eq!(back["model"], "gpt-4");
        assert_eq!(back["messages"][0]["content"], "Hello");
        assert_eq!(back["max_tokens"], 100);
    }

    #[test]
    fn test_from_oa_compat_response() {
        let resp = serde_json::json!({
            "id": "chatcmpl-123",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello!"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        });

        let common = from_response(&resp);
        assert_eq!(common.choices[0].message.content.as_deref(), Some("Hello!"));
        assert_eq!(common.choices[0].finish_reason.as_deref(), Some("stop"));
        assert_eq!(common.usage.as_ref().unwrap().total_tokens, Some(15));
    }

    #[test]
    fn test_from_oa_compat_chunk() {
        let chunk =
            "data: {\"id\":\"cmpl-1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hi\"}}]}";
        let result = from_chunk(chunk).unwrap();
        assert_eq!(result.choices[0].delta.content.as_deref(), Some("Hi"));
    }

    #[test]
    fn test_normalize_usage_with_cache_adjust() {
        let usage = serde_json::json!({"prompt_tokens": 1000, "completion_tokens": 50});
        let info = normalize_usage(&usage, true);
        assert_eq!(info.cache_read_tokens, Some(900)); // 90% of 1000
        assert_eq!(info.input_tokens, 100); // 1000 - 900
    }
}
