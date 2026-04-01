use super::common::*;
use serde_json::Value;

/// Convert an OpenAI Responses API request to CommonRequest.
pub fn from_request(body: &Value) -> CommonRequest {
    let mut messages = Vec::new();

    let input = body.get("input").and_then(|i| i.as_array())
        .or_else(|| body.get("messages").and_then(|m| m.as_array()))
        .cloned()
        .unwrap_or_default();

    for m in &input {
        // Responses API items without role (function_call, function_call_output)
        if m.get("role").is_none() {
            if let Some(item_type) = m.get("type").and_then(|t| t.as_str()) {
                match item_type {
                    "function_call" => {
                        let name = m.get("name").and_then(|n| n.as_str()).unwrap_or_default();
                        let args = m.get("arguments").map(|a| {
                            if let Some(s) = a.as_str() { s.to_string() } else { a.to_string() }
                        }).unwrap_or_else(|| "{}".to_string());
                        let id = m.get("id").and_then(|i| i.as_str()).unwrap_or_default();
                        messages.push(CommonMessage {
                            role: Role::Assistant,
                            content: None,
                            tool_call_id: None,
                            tool_calls: Some(vec![CommonToolCall {
                                id: id.to_string(),
                                call_type: "function".to_string(),
                                function: CommonFunctionCall { name: name.to_string(), arguments: args },
                            }]),
                            name: None,
                        });
                    }
                    "function_call_output" => {
                        let call_id = m.get("call_id").and_then(|i| i.as_str()).unwrap_or_default();
                        let output = m.get("output").map(|o| {
                            if let Some(s) = o.as_str() { s.to_string() } else { o.to_string() }
                        }).unwrap_or_default();
                        messages.push(CommonMessage {
                            role: Role::Tool,
                            content: Some(Content::Text(output)),
                            tool_call_id: Some(call_id.to_string()),
                            tool_calls: None,
                            name: None,
                        });
                    }
                    _ => {}
                }
                continue;
            }
        }

        let Some(role) = m.get("role").and_then(|r| r.as_str()) else { continue };

        match role {
            "system" | "developer" => {
                let content = m.get("content");
                let text = if let Some(s) = content.and_then(|c| c.as_str()) {
                    Some(s.to_string())
                } else if let Some(arr) = content.and_then(|c| c.as_array()) {
                    arr.iter().find_map(|p| p.get("text").and_then(|t| t.as_str()).map(String::from))
                } else {
                    None
                };
                if let Some(text) = text {
                    if !text.is_empty() {
                        messages.push(CommonMessage {
                            role: Role::System, content: Some(Content::Text(text)),
                            tool_call_id: None, tool_calls: None, name: None,
                        });
                    }
                }
            }
            "user" => {
                let content = m.get("content");
                if let Some(s) = content.and_then(|c| c.as_str()) {
                    messages.push(CommonMessage {
                        role: Role::User, content: Some(Content::Text(s.to_string())),
                        tool_call_id: None, tool_calls: None, name: None,
                    });
                } else if let Some(arr) = content.and_then(|c| c.as_array()) {
                    let parts: Vec<ContentPart> = arr.iter().filter_map(|p| {
                        let ptype = p.get("type").and_then(|t| t.as_str())?;
                        match ptype {
                            "text" | "input_text" => {
                                let text = p.get("text").and_then(|t| t.as_str())?;
                                Some(ContentPart::Text { text: text.to_string() })
                            }
                            "image_url" | "input_image" => {
                                let url = p.get("image_url").and_then(|i| i.get("url")).and_then(|u| u.as_str())?;
                                Some(ContentPart::ImageUrl { image_url: ImageUrl { url: url.to_string() } })
                            }
                            _ => None,
                        }
                    }).collect();

                    if parts.len() == 1 {
                        if let ContentPart::Text { text } = &parts[0] {
                            messages.push(CommonMessage {
                                role: Role::User, content: Some(Content::Text(text.clone())),
                                tool_call_id: None, tool_calls: None, name: None,
                            });
                            continue;
                        }
                    }
                    if !parts.is_empty() {
                        messages.push(CommonMessage {
                            role: Role::User, content: Some(Content::Parts(parts)),
                            tool_call_id: None, tool_calls: None, name: None,
                        });
                    }
                }
            }
            "assistant" => {
                let content = m.get("content").and_then(|c| c.as_str()).map(String::from);
                let tool_calls = m.get("tool_calls").and_then(|tc| tc.as_array()).map(|arr| {
                    arr.iter().filter_map(|tc| {
                        let func = tc.get("function")?;
                        Some(CommonToolCall {
                            id: tc.get("id").and_then(|i| i.as_str())?.to_string(),
                            call_type: "function".to_string(),
                            function: CommonFunctionCall {
                                name: func.get("name").and_then(|n| n.as_str())?.to_string(),
                                arguments: func.get("arguments").and_then(|a| a.as_str())?.to_string(),
                            },
                        })
                    }).collect()
                });
                messages.push(CommonMessage {
                    role: Role::Assistant,
                    content: content.filter(|s| !s.is_empty()).map(Content::Text),
                    tool_call_id: None, tool_calls, name: None,
                });
            }
            "tool" => {
                let content = m.get("content").map(|c| {
                    if let Some(s) = c.as_str() { s.to_string() } else { c.to_string() }
                });
                messages.push(CommonMessage {
                    role: Role::Tool,
                    content: content.map(Content::Text),
                    tool_call_id: m.get("tool_call_id").and_then(|i| i.as_str()).map(String::from),
                    tool_calls: None, name: None,
                });
            }
            _ => {}
        }
    }

    let tool_choice = body.get("tool_choice").and_then(|tc| {
        if let Some(s) = tc.as_str() { return Some(ToolChoice::Mode(s.to_string())); }
        let tc_type = tc.get("type").and_then(|t| t.as_str())?;
        if tc_type == "function" {
            let name = tc.get("function").and_then(|f| f.get("name")).and_then(|n| n.as_str())?;
            return Some(ToolChoice::Function {
                choice_type: "function".to_string(),
                function: ToolChoiceFunction { name: name.to_string() },
            });
        }
        None
    });

    let stop = body.get("stop_sequences").or(body.get("stop")).and_then(|v| {
        if let Some(arr) = v.as_array() {
            let strs: Vec<String> = arr.iter().filter_map(|s| s.as_str().map(String::from)).collect();
            if strs.len() == 1 { Some(StopSequence::Single(strs.into_iter().next().unwrap())) }
            else if !strs.is_empty() { Some(StopSequence::Multiple(strs)) }
            else { None }
        } else { v.as_str().map(|s| StopSequence::Single(s.to_string())) }
    });

    CommonRequest {
        model: body.get("model").and_then(|m| m.as_str()).unwrap_or_default().to_string(),
        max_tokens: body.get("max_output_tokens").or(body.get("max_tokens")).and_then(|m| m.as_u64()).map(|v| v as u32),
        temperature: body.get("temperature").and_then(|t| t.as_f64()).map(|v| v as f32),
        top_p: body.get("top_p").and_then(|t| t.as_f64()).map(|v| v as f32),
        stop,
        messages,
        stream: body.get("stream").and_then(|s| s.as_bool()),
        tools: body.get("tools").and_then(|t| t.as_array()).map(|arr| {
            arr.iter().filter_map(|t| {
                if t.get("type").and_then(|tt| tt.as_str()) == Some("function") {
                    Some(CommonTool {
                        tool_type: "function".to_string(),
                        function: CommonFunctionDef {
                            name: t.get("name").or_else(|| t.get("function").and_then(|f| f.get("name")))
                                .and_then(|n| n.as_str())?.to_string(),
                            description: t.get("description").or_else(|| t.get("function").and_then(|f| f.get("description")))
                                .and_then(|d| d.as_str()).map(String::from),
                            parameters: t.get("parameters").or_else(|| t.get("function").and_then(|f| f.get("parameters"))).cloned(),
                        },
                    })
                } else { None }
            }).collect()
        }),
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
                if let Some(text) = m.content.as_ref().map(|c| c.to_text()) {
                    if !text.is_empty() {
                        input.push(serde_json::json!({"role": "assistant", "content": [{"type": "output_text", "text": text}]}));
                    }
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
        tools.iter().filter(|t| t.tool_type == "function").map(|t| {
            serde_json::json!({
                "type": "function",
                "name": t.function.name,
                "description": t.function.description,
                "parameters": t.function.parameters,
            })
        }).collect()
    });

    let mut result = serde_json::json!({
        "model": body.model,
        "input": input,
        "stream": body.stream.unwrap_or(false),
    });

    if let Some(max_tokens) = body.max_tokens { result["max_output_tokens"] = serde_json::json!(max_tokens); }
    if let Some(top_p) = body.top_p { result["top_p"] = serde_json::json!(top_p); }
    if let Some(stop) = &body.stop {
        result["stop_sequences"] = match stop {
            StopSequence::Single(s) => serde_json::json!([s]),
            StopSequence::Multiple(v) => serde_json::json!(v),
        };
    }
    if let Some(tools) = tools { result["tools"] = Value::Array(tools); }
    if let Some(tc) = &body.tool_choice {
        result["tool_choice"] = match tc {
            ToolChoice::Mode(m) => Value::String(m.clone()),
            ToolChoice::Function { function, .. } => serde_json::json!({"type": "function", "function": {"name": function.name}}),
        };
    }

    result
}

/// Convert an OpenAI Responses API response to CommonResponse.
pub fn from_response(resp: &Value) -> CommonResponse {
    // If already in common format
    if resp.get("choices").and_then(|c| c.as_array()).is_some() {
        return serde_json::from_value(resp.clone()).unwrap_or_else(|_| empty_response());
    }

    let r = resp.get("response").unwrap_or(resp);
    let id = r.get("id").and_then(|i| i.as_str())
        .map(|i| i.replace("resp_", "chatcmpl_"))
        .unwrap_or_default();
    let model = r.get("model").or_else(|| resp.get("model"))
        .and_then(|m| m.as_str()).unwrap_or_default().to_string();

    let output = r.get("output").and_then(|o| o.as_array()).cloned().unwrap_or_default();

    let text: String = output.iter()
        .filter(|o| o.get("type").and_then(|t| t.as_str()) == Some("message"))
        .filter_map(|o| o.get("content").and_then(|c| c.as_array()))
        .flatten()
        .filter(|p| p.get("type").and_then(|t| t.as_str()) == Some("output_text"))
        .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
        .collect::<Vec<_>>()
        .join("");

    let tool_calls: Vec<CommonToolCall> = output.iter()
        .filter(|o| o.get("type").and_then(|t| t.as_str()) == Some("function_call"))
        .map(|o| {
            let name = o.get("name").and_then(|n| n.as_str()).unwrap_or_default().to_string();
            let args = o.get("arguments").map(|a| {
                if let Some(s) = a.as_str() { s.to_string() } else { a.to_string() }
            }).unwrap_or_else(|| "{}".to_string());
            let id = o.get("id").and_then(|i| i.as_str()).unwrap_or_default().to_string();
            CommonToolCall {
                id,
                call_type: "function".to_string(),
                function: CommonFunctionCall { name, arguments: args },
            }
        })
        .collect();

    let finish_reason = r.get("stop_reason").and_then(|r| r.as_str()).map(|r| match r {
        "stop" => "stop",
        "tool_call" | "tool_calls" => "tool_calls",
        "length" | "max_output_tokens" => "length",
        other => other,
    }.to_string());

    let usage_val = r.get("usage").or_else(|| resp.get("usage"));
    let usage = usage_val.map(|u| {
        let pt = u.get("input_tokens").and_then(|v| v.as_u64()).map(|v| v as u32);
        let ct = u.get("output_tokens").and_then(|v| v.as_u64()).map(|v| v as u32);
        let cached = u.get("input_tokens_details").and_then(|d| d.get("cached_tokens")).and_then(|c| c.as_u64()).map(|v| v as u32);
        CommonUsage {
            prompt_tokens: pt,
            completion_tokens: ct,
            total_tokens: match (pt, ct) { (Some(p), Some(c)) => Some(p + c), _ => None },
            prompt_tokens_details: cached.map(|c| PromptTokensDetails { cached_tokens: Some(c) }),
        }
    });

    CommonResponse {
        id,
        object: "chat.completion".to_string(),
        created: now_unix(),
        model,
        choices: vec![CommonChoice {
            index: 0,
            message: CommonChoiceMessage {
                role: "assistant".to_string(),
                content: if text.is_empty() { None } else { Some(text) },
                tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
            },
            finish_reason,
        }],
        usage,
    }
}

/// Convert a CommonResponse to OpenAI Responses API format.
pub fn to_response(resp: &CommonResponse) -> Value {
    let choice = resp.choices.first();
    let mut output_items: Vec<Value> = Vec::new();

    if let Some(choice) = choice {
        if let Some(text) = &choice.message.content {
            if !text.is_empty() {
                output_items.push(serde_json::json!({
                    "type": "message",
                    "status": "completed",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": text}],
                }));
            }
        }
        if let Some(tool_calls) = &choice.message.tool_calls {
            for tc in tool_calls {
                output_items.push(serde_json::json!({
                    "id": tc.id,
                    "type": "function_call",
                    "name": tc.function.name,
                    "call_id": tc.id,
                    "arguments": tc.function.arguments,
                }));
            }
        }
    }

    let stop_reason = choice.and_then(|c| c.finish_reason.as_deref()).map(|r| match r {
        "stop" => "stop",
        "tool_calls" => "tool_call",
        "length" => "max_output_tokens",
        other => other,
    });

    let usage = resp.usage.as_ref().map(|u| {
        let mut usage = serde_json::json!({
            "input_tokens": u.prompt_tokens,
            "output_tokens": u.completion_tokens,
            "total_tokens": u.total_tokens,
        });
        if let Some(details) = &u.prompt_tokens_details {
            if let Some(cached) = details.cached_tokens {
                usage["input_tokens_details"] = serde_json::json!({"cached_tokens": cached});
            }
        }
        usage
    });

    let mut result = serde_json::json!({
        "id": resp.id.replace("chatcmpl_", "resp_"),
        "object": "response",
        "model": resp.model,
        "output": output_items,
        "stop_reason": stop_reason,
    });

    if let Some(u) = usage { result["usage"] = u; }

    result
}

/// Parse an OpenAI Responses API SSE chunk.
pub fn from_chunk(chunk: &str) -> Result<CommonChunk, String> {
    let lines: Vec<&str> = chunk.lines().collect();
    let event_line = lines.first().ok_or_else(|| chunk.to_string())?;
    let data_line = lines.iter().find(|l| l.starts_with("data: ")).ok_or_else(|| chunk.to_string())?;

    let json: Value = serde_json::from_str(&data_line[6..]).map_err(|_| chunk.to_string())?;
    let resp_obj = json.get("response").cloned().unwrap_or_else(|| serde_json::json!({}));

    let event = event_line.strip_prefix("event: ").unwrap_or_default().trim();

    let mut out = CommonChunk {
        id: resp_obj.get("id").or_else(|| json.get("id")).and_then(|i| i.as_str()).unwrap_or_default().to_string(),
        object: "chat.completion.chunk".to_string(),
        created: now_unix(),
        model: resp_obj.get("model").or_else(|| json.get("model")).and_then(|m| m.as_str()).unwrap_or_default().to_string(),
        choices: Vec::new(),
        usage: None,
    };

    match event {
        "response.output_text.delta" => {
            let delta = json.get("delta").or_else(|| json.get("text")).and_then(|d| d.as_str());
            if let Some(d) = delta {
                if !d.is_empty() {
                    out.choices.push(CommonChunkChoice {
                        index: 0,
                        delta: ChunkDelta { content: Some(d.to_string()), ..Default::default() },
                        finish_reason: None,
                    });
                }
            }
        }
        "response.output_item.added" => {
            if json.get("item").and_then(|i| i.get("type")).and_then(|t| t.as_str()) == Some("function_call") {
                let name = json.get("item").and_then(|i| i.get("name")).and_then(|n| n.as_str());
                let id = json.get("item").and_then(|i| i.get("id")).and_then(|i| i.as_str());
                if let Some(name) = name {
                    out.choices.push(CommonChunkChoice {
                        index: 0,
                        delta: ChunkDelta {
                            tool_calls: Some(vec![ChunkToolCall {
                                index: 0,
                                id: id.map(String::from),
                                call_type: Some("function".to_string()),
                                function: Some(ChunkFunction {
                                    name: Some(name.to_string()),
                                    arguments: Some(String::new()),
                                }),
                            }]),
                            ..Default::default()
                        },
                        finish_reason: None,
                    });
                }
            }
        }
        "response.function_call_arguments.delta" => {
            let args = json.get("delta").or_else(|| json.get("arguments_delta")).and_then(|a| a.as_str());
            if let Some(a) = args {
                if !a.is_empty() {
                    out.choices.push(CommonChunkChoice {
                        index: 0,
                        delta: ChunkDelta {
                            tool_calls: Some(vec![ChunkToolCall {
                                index: 0, id: None, call_type: None,
                                function: Some(ChunkFunction { name: None, arguments: Some(a.to_string()) }),
                            }]),
                            ..Default::default()
                        },
                        finish_reason: None,
                    });
                }
            }
        }
        "response.completed" => {
            let sr = resp_obj.get("stop_reason").or_else(|| json.get("stop_reason")).and_then(|r| r.as_str());
            let finish = sr.map(|r| match r {
                "stop" => "stop",
                "tool_call" | "tool_calls" => "tool_calls",
                "length" | "max_output_tokens" => "length",
                other => other,
            }.to_string());
            out.choices.push(CommonChunkChoice {
                index: 0, delta: ChunkDelta::default(), finish_reason: finish,
            });

            let u = resp_obj.get("usage").or_else(|| json.get("response").and_then(|r| r.get("usage")));
            if let Some(u) = u {
                let pt = u.get("input_tokens").and_then(|v| v.as_u64()).map(|v| v as u32);
                let ct = u.get("output_tokens").and_then(|v| v.as_u64()).map(|v| v as u32);
                let cached = u.get("input_tokens_details").and_then(|d| d.get("cached_tokens")).and_then(|c| c.as_u64()).map(|v| v as u32);
                out.usage = Some(CommonUsage {
                    prompt_tokens: pt, completion_tokens: ct,
                    total_tokens: match (pt, ct) { (Some(p), Some(c)) => Some(p + c), _ => None },
                    prompt_tokens_details: cached.map(|c| PromptTokensDetails { cached_tokens: Some(c) }),
                });
            }
        }
        _ => {}
    }

    Ok(out)
}

/// Convert a CommonChunk to OpenAI Responses API SSE format.
pub fn to_chunk(chunk: &CommonChunk) -> String {
    let choice = match chunk.choices.first() {
        Some(c) => c,
        None => return String::new(),
    };

    let delta = &choice.delta;

    if let Some(content) = &delta.content {
        let data = serde_json::json!({
            "id": chunk.id,
            "type": "response.output_text.delta",
            "delta": content,
            "response": {"id": chunk.id, "model": chunk.model},
        });
        return format!("event: response.output_text.delta\ndata: {data}");
    }

    if let Some(tool_calls) = &delta.tool_calls {
        for tc in tool_calls {
            if let Some(func) = &tc.function {
                if func.name.is_some() {
                    let data = serde_json::json!({
                        "type": "response.output_item.added",
                        "item": {"id": tc.id, "type": "function_call", "name": func.name, "arguments": ""},
                    });
                    return format!("event: response.output_item.added\ndata: {data}");
                }
                if let Some(args) = &func.arguments {
                    let data = serde_json::json!({
                        "type": "response.function_call_arguments.delta",
                        "delta": args,
                    });
                    return format!("event: response.function_call_arguments.delta\ndata: {data}");
                }
            }
        }
    }

    if let Some(finish) = &choice.finish_reason {
        let stop_reason = match finish.as_str() {
            "stop" => "stop",
            "tool_calls" => "tool_call",
            "length" => "max_output_tokens",
            other => other,
        };

        let mut resp_json = serde_json::json!({"id": chunk.id, "model": chunk.model});
        if let Some(u) = &chunk.usage {
            resp_json["usage"] = serde_json::json!({
                "input_tokens": u.prompt_tokens,
                "output_tokens": u.completion_tokens,
                "total_tokens": u.total_tokens,
            });
        }
        resp_json["stop_reason"] = Value::String(stop_reason.to_string());

        let data = serde_json::json!({"id": chunk.id, "type": "response.completed", "response": resp_json});
        return format!("event: response.completed\ndata: {data}");
    }

    String::new()
}

/// Normalize OpenAI Responses API usage into UsageInfo.
pub fn normalize_usage(usage: &Value) -> UsageInfo {
    let input = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let output = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let reasoning = usage.get("output_tokens_details")
        .and_then(|d| d.get("reasoning_tokens"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    let cache_read = usage.get("input_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    UsageInfo {
        input_tokens: input.saturating_sub(cache_read.unwrap_or(0)),
        output_tokens: output.saturating_sub(reasoning.unwrap_or(0)),
        reasoning_tokens: reasoning,
        cache_read_tokens: cache_read,
        cache_write_5m_tokens: None,
        cache_write_1h_tokens: None,
    }
}

fn empty_response() -> CommonResponse {
    CommonResponse {
        id: String::new(), object: "chat.completion".to_string(),
        created: now_unix(), model: String::new(),
        choices: Vec::new(), usage: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_openai_responses_request() {
        let body = serde_json::json!({
            "model": "gpt-4o",
            "input": [
                {"role": "system", "content": "Be helpful"},
                {"role": "user", "content": [{"type": "input_text", "text": "Hello"}]},
                {"type": "function_call", "id": "fc_1", "name": "read", "arguments": "{\"path\":\"a.py\"}"},
                {"type": "function_call_output", "call_id": "fc_1", "output": "file contents"},
            ],
            "max_output_tokens": 1024,
        });

        let req = from_request(&body);
        assert_eq!(req.model, "gpt-4o");
        assert_eq!(req.messages.len(), 4);
        assert_eq!(req.messages[0].role, Role::System);
        assert_eq!(req.messages[1].role, Role::User);
        assert_eq!(req.messages[2].role, Role::Assistant);
        assert_eq!(req.messages[3].role, Role::Tool);
        assert_eq!(req.max_tokens, Some(1024));
    }

    #[test]
    fn test_from_openai_responses_response() {
        let resp = serde_json::json!({
            "id": "resp_123",
            "object": "response",
            "model": "gpt-4o",
            "output": [
                {"type": "message", "content": [{"type": "output_text", "text": "Hello!"}]},
                {"type": "function_call", "id": "fc_1", "name": "read", "arguments": "{\"path\":\"a.py\"}"},
            ],
            "stop_reason": "tool_call",
            "usage": {"input_tokens": 100, "output_tokens": 50}
        });

        let common = from_response(&resp);
        assert_eq!(common.id, "chatcmpl_123");
        assert_eq!(common.choices[0].message.content.as_deref(), Some("Hello!"));
        assert_eq!(common.choices[0].finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(common.choices[0].message.tool_calls.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_from_openai_chunk_text() {
        let chunk = "event: response.output_text.delta\ndata: {\"delta\":\"Hello\",\"response\":{\"id\":\"r1\",\"model\":\"gpt-4o\"}}";
        let result = from_chunk(chunk).unwrap();
        assert_eq!(result.choices[0].delta.content.as_deref(), Some("Hello"));
    }

    #[test]
    fn test_from_openai_chunk_tool_start() {
        let chunk = "event: response.output_item.added\ndata: {\"item\":{\"id\":\"fc_1\",\"type\":\"function_call\",\"name\":\"read\"}}";
        let result = from_chunk(chunk).unwrap();
        let tc = result.choices[0].delta.tool_calls.as_ref().unwrap();
        assert_eq!(tc[0].function.as_ref().unwrap().name.as_deref(), Some("read"));
    }
}
