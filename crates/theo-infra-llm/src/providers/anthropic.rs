use super::common::*;
use serde_json::Value;

/// Convert an Anthropic Messages API request to CommonRequest.
pub fn from_request(body: &Value) -> CommonRequest {
    let mut messages = Vec::new();

    // Extract system messages from "system" array
    if let Some(sys) = body.get("system").and_then(|s| s.as_array()) {
        for s in sys {
            if s.get("type").and_then(|t| t.as_str()) != Some("text") {
                continue;
            }
            if let Some(text) = s.get("text").and_then(|t| t.as_str())
                && !text.is_empty() {
                    messages.push(CommonMessage {
                        role: Role::System,
                        content: Some(Content::Text(text.to_string())),
                        tool_call_id: None,
                        tool_calls: None,
                        name: None,
                    });
                }
        }
    }

    // Process messages
    if let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) {
        for m in msgs {
            let Some(role) = m.get("role").and_then(|r| r.as_str()) else {
                continue;
            };

            if role == "user" {
                let parts_in = m
                    .get("content")
                    .and_then(|c| c.as_array())
                    .cloned()
                    .unwrap_or_default();
                let mut text_parts = Vec::new();

                for p in &parts_in {
                    let Some(ptype) = p.get("type").and_then(|t| t.as_str()) else {
                        continue;
                    };

                    match ptype {
                        "text" => {
                            if let Some(text) = p.get("text").and_then(|t| t.as_str()) {
                                text_parts.push(ContentPart::Text {
                                    text: text.to_string(),
                                });
                            }
                        }
                        "image" => {
                            if let Some(img) = convert_anthropic_image_source(p.get("source")) {
                                text_parts.push(img);
                            }
                        }
                        "tool_result" => {
                            let tool_call_id = p
                                .get("tool_use_id")
                                .and_then(|i| i.as_str())
                                .map(String::from);
                            let content = p.get("content").map(|c| {
                                if let Some(s) = c.as_str() {
                                    s.to_string()
                                } else {
                                    c.to_string()
                                }
                            });
                            messages.push(CommonMessage {
                                role: Role::Tool,
                                content: content.map(Content::Text),
                                tool_call_id,
                                tool_calls: None,
                                name: None,
                            });
                        }
                        _ => {}
                    }
                }

                if !text_parts.is_empty() {
                    let content = if text_parts.len() == 1 {
                        if let ContentPart::Text { text } = &text_parts[0] {
                            Content::Text(text.clone())
                        } else {
                            Content::Parts(text_parts)
                        }
                    } else {
                        Content::Parts(text_parts)
                    };
                    messages.push(CommonMessage {
                        role: Role::User,
                        content: Some(content),
                        tool_call_id: None,
                        tool_calls: None,
                        name: None,
                    });
                }
            } else if role == "assistant" {
                let parts_in = m
                    .get("content")
                    .and_then(|c| c.as_array())
                    .cloned()
                    .unwrap_or_default();
                let mut texts = Vec::new();
                let mut tool_calls = Vec::new();

                for p in &parts_in {
                    let Some(ptype) = p.get("type").and_then(|t| t.as_str()) else {
                        continue;
                    };

                    match ptype {
                        "text" => {
                            if let Some(text) = p.get("text").and_then(|t| t.as_str()) {
                                texts.push(text.to_string());
                            }
                        }
                        "tool_use" => {
                            let name = p
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or_default()
                                .to_string();
                            let id = p
                                .get("id")
                                .and_then(|i| i.as_str())
                                .unwrap_or_default()
                                .to_string();
                            let input = p.get("input");
                            let arguments = match input {
                                Some(v) if v.is_string() => v.as_str().unwrap().to_string(),
                                Some(v) => v.to_string(),
                                None => "{}".to_string(),
                            };
                            tool_calls.push(CommonToolCall {
                                id,
                                call_type: "function".to_string(),
                                function: CommonFunctionCall { name, arguments },
                            });
                        }
                        _ => {}
                    }
                }

                let content_str = texts.join("");
                messages.push(CommonMessage {
                    role: Role::Assistant,
                    content: if content_str.is_empty() {
                        None
                    } else {
                        Some(Content::Text(content_str))
                    },
                    tool_call_id: None,
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                    name: None,
                });
            }
        }
    }

    // Convert tools
    let tools = body.get("tools").and_then(|t| t.as_array()).map(|arr| {
        arr.iter()
            .filter(|t| t.get("input_schema").is_some())
            .map(|t| CommonTool {
                tool_type: "function".to_string(),
                function: CommonFunctionDef {
                    name: t
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    description: t
                        .get("description")
                        .and_then(|d| d.as_str())
                        .map(String::from),
                    parameters: t.get("input_schema").cloned(),
                },
            })
            .collect()
    });

    // Convert tool_choice
    let tool_choice = body.get("tool_choice").and_then(|tc| {
        let tc_type = tc.get("type").and_then(|t| t.as_str())?;
        match tc_type {
            "auto" => Some(ToolChoice::Mode("auto".to_string())),
            "any" => Some(ToolChoice::Mode("required".to_string())),
            "tool" => {
                let name = tc.get("name").and_then(|n| n.as_str())?.to_string();
                Some(ToolChoice::Function {
                    choice_type: "function".to_string(),
                    function: ToolChoiceFunction { name },
                })
            }
            _ => None,
        }
    });

    let stop = body.get("stop_sequences").and_then(|v| {
        if let Some(arr) = v.as_array() {
            let strs: Vec<String> = arr
                .iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect();
            if strs.len() == 1 {
                Some(StopSequence::Single(strs.into_iter().next().unwrap()))
            } else if !strs.is_empty() {
                Some(StopSequence::Multiple(strs))
            } else {
                None
            }
        } else {
            v.as_str().map(|s| StopSequence::Single(s.to_string()))
        }
    });

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

/// Convert a CommonRequest to Anthropic Messages API format.
pub fn to_request(body: &CommonRequest) -> Value {
    let mut system_blocks = Vec::new();
    let mut messages_out: Vec<Value> = Vec::new();
    let mut cc_count = 0u32;

    let cc = |count: &mut u32| -> Value {
        *count += 1;
        if *count <= 4 {
            serde_json::json!({ "cache_control": { "type": "ephemeral" } })
        } else {
            serde_json::json!({})
        }
    };

    for msg in &body.messages {
        match msg.role {
            Role::System => {
                if let Some(content) = &msg.content {
                    let text = content.to_text();
                    if !text.is_empty() {
                        let mut block = serde_json::json!({ "type": "text", "text": text });
                        let cache = cc(&mut cc_count);
                        if let Some(obj) = cache.as_object() {
                            for (k, v) in obj {
                                block[k] = v.clone();
                            }
                        }
                        system_blocks.push(block);
                    }
                }
            }
            Role::User => {
                if let Some(content) = &msg.content {
                    let parts = match content {
                        Content::Text(text) => {
                            let mut block = serde_json::json!({ "type": "text", "text": text });
                            let cache = cc(&mut cc_count);
                            if let Some(obj) = cache.as_object() {
                                for (k, v) in obj {
                                    block[k] = v.clone();
                                }
                            }
                            vec![block]
                        }
                        Content::Parts(parts) => parts
                            .iter()
                            .map(|p| match p {
                                ContentPart::Text { text } => {
                                    let mut block =
                                        serde_json::json!({ "type": "text", "text": text });
                                    let cache = cc(&mut cc_count);
                                    if let Some(obj) = cache.as_object() {
                                        for (k, v) in obj {
                                            block[k] = v.clone();
                                        }
                                    }
                                    block
                                }
                                ContentPart::ImageUrl { image_url } => {
                                    let source = convert_url_to_anthropic_source(&image_url.url);
                                    let mut block =
                                        serde_json::json!({ "type": "image", "source": source });
                                    let cache = cc(&mut cc_count);
                                    if let Some(obj) = cache.as_object() {
                                        for (k, v) in obj {
                                            block[k] = v.clone();
                                        }
                                    }
                                    block
                                }
                            })
                            .collect(),
                    };
                    messages_out.push(serde_json::json!({ "role": "user", "content": parts }));
                }
            }
            Role::Assistant => {
                let mut content_blocks: Vec<Value> = Vec::new();

                if let Some(content) = &msg.content {
                    let text = content.to_text();
                    if !text.is_empty() {
                        let mut block = serde_json::json!({ "type": "text", "text": text });
                        let cache = cc(&mut cc_count);
                        if let Some(obj) = cache.as_object() {
                            for (k, v) in obj {
                                block[k] = v.clone();
                            }
                        }
                        content_blocks.push(block);
                    }
                }

                if let Some(tool_calls) = &msg.tool_calls {
                    for tc in tool_calls {
                        let input: Value = serde_json::from_str(&tc.function.arguments)
                            .unwrap_or_else(|_| Value::String(tc.function.arguments.clone()));
                        let mut block = serde_json::json!({
                            "type": "tool_use",
                            "id": tc.id,
                            "name": tc.function.name,
                            "input": input,
                        });
                        let cache = cc(&mut cc_count);
                        if let Some(obj) = cache.as_object() {
                            for (k, v) in obj {
                                block[k] = v.clone();
                            }
                        }
                        content_blocks.push(block);
                    }
                }

                if !content_blocks.is_empty() {
                    messages_out.push(serde_json::json!({
                        "role": "assistant",
                        "content": content_blocks,
                    }));
                }
            }
            Role::Tool => {
                let content_str = msg
                    .content
                    .as_ref()
                    .map(|c| c.to_text())
                    .unwrap_or_default();
                let mut block = serde_json::json!({
                    "type": "tool_result",
                    "tool_use_id": msg.tool_call_id,
                    "content": content_str,
                });
                let cache = cc(&mut cc_count);
                if let Some(obj) = cache.as_object() {
                    for (k, v) in obj {
                        block[k] = v.clone();
                    }
                }
                messages_out.push(serde_json::json!({ "role": "user", "content": [block] }));
            }
        }
    }

    // Convert tools
    let tools: Option<Vec<Value>> = body.tools.as_ref().map(|tools| {
        tools
            .iter()
            .filter(|t| t.tool_type == "function")
            .map(|t| {
                let mut tool = serde_json::json!({
                    "name": t.function.name,
                    "description": t.function.description,
                    "input_schema": t.function.parameters,
                });
                let cache = cc(&mut cc_count);
                if let Some(obj) = cache.as_object() {
                    for (k, v) in obj {
                        tool[k] = v.clone();
                    }
                }
                tool
            })
            .collect()
    });

    let tool_choice = body.tool_choice.as_ref().map(|tc| match tc {
        ToolChoice::Mode(mode) => match mode.as_str() {
            "auto" => serde_json::json!({ "type": "auto" }),
            "required" => serde_json::json!({ "type": "any" }),
            _ => serde_json::json!({ "type": "auto" }),
        },
        ToolChoice::Function { function, .. } => {
            serde_json::json!({ "type": "tool", "name": function.name })
        }
    });

    let stop_sequences = body.stop.as_ref().map(|s| match s {
        StopSequence::Single(s) => serde_json::json!([s]),
        StopSequence::Multiple(v) => serde_json::json!(v),
    });

    let mut result = serde_json::json!({
        "max_tokens": body.max_tokens.unwrap_or(32_000),
        "messages": messages_out,
        "stream": body.stream.unwrap_or(false),
    });

    if let Some(temp) = body.temperature {
        result["temperature"] = serde_json::json!(temp);
    }
    if let Some(top_p) = body.top_p {
        result["top_p"] = serde_json::json!(top_p);
    }
    if !system_blocks.is_empty() {
        result["system"] = Value::Array(system_blocks);
    }
    if let Some(tools) = tools {
        result["tools"] = Value::Array(tools);
    }
    if let Some(tc) = tool_choice {
        result["tool_choice"] = tc;
    }
    if let Some(stop) = stop_sequences {
        result["stop_sequences"] = stop;
    }

    result
}

/// Convert an Anthropic Messages API response to CommonResponse.
pub fn from_response(resp: &Value) -> CommonResponse {
    let id = resp
        .get("id")
        .and_then(|i| i.as_str())
        .map(|i| i.replace("msg_", "chatcmpl_"))
        .unwrap_or_default();
    let model = resp
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or_default()
        .to_string();

    let blocks = resp
        .get("content")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();

    let text: String = blocks
        .iter()
        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
        .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
        .collect::<Vec<_>>()
        .join("");

    let tool_calls: Vec<CommonToolCall> = blocks
        .iter()
        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
        .map(|b| {
            let name = b
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or_default()
                .to_string();
            let id = b
                .get("id")
                .and_then(|i| i.as_str())
                .unwrap_or_default()
                .to_string();
            let input = b.get("input");
            let arguments = match input {
                Some(v) if v.is_string() => v.as_str().unwrap().to_string(),
                Some(v) => v.to_string(),
                None => "{}".to_string(),
            };
            CommonToolCall {
                id,
                call_type: "function".to_string(),
                function: CommonFunctionCall { name, arguments },
            }
        })
        .collect();

    let finish_reason = resp.get("stop_reason").and_then(|r| r.as_str()).map(|r| {
        match r {
            "end_turn" => "stop",
            "tool_use" => "tool_calls",
            "max_tokens" => "length",
            "content_filter" => "content_filter",
            _ => r,
        }
        .to_string()
    });

    let usage = resp.get("usage").map(|u| {
        let pt = u
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let ct = u
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let total = match (pt, ct) {
            (Some(p), Some(c)) => Some(p + c),
            _ => None,
        };
        let cached = u
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        CommonUsage {
            prompt_tokens: pt,
            completion_tokens: ct,
            total_tokens: total,
            prompt_tokens_details: cached.map(|c| PromptTokensDetails {
                cached_tokens: Some(c),
            }),
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
                tool_calls: if tool_calls.is_empty() {
                    None
                } else {
                    Some(tool_calls)
                },
            },
            finish_reason,
        }],
        usage,
    }
}

/// Convert a CommonResponse to Anthropic Messages API response format.
pub fn to_response(resp: &CommonResponse) -> Value {
    let choice = resp.choices.first();
    let mut content_blocks: Vec<Value> = Vec::new();

    if let Some(choice) = choice {
        if let Some(text) = &choice.message.content
            && !text.is_empty() {
                content_blocks.push(serde_json::json!({ "type": "text", "text": text }));
            }
        if let Some(tool_calls) = &choice.message.tool_calls {
            for tc in tool_calls {
                let input: Value = serde_json::from_str(&tc.function.arguments)
                    .unwrap_or_else(|_| Value::String(tc.function.arguments.clone()));
                content_blocks.push(serde_json::json!({
                    "type": "tool_use",
                    "id": tc.id,
                    "name": tc.function.name,
                    "input": input,
                }));
            }
        }
    }

    let stop_reason = choice
        .and_then(|c| c.finish_reason.as_deref())
        .map(|r| match r {
            "stop" => "end_turn",
            "tool_calls" => "tool_use",
            "length" => "max_tokens",
            other => other,
        });

    let usage = resp.usage.as_ref().map(|u| {
        let mut usage = serde_json::json!({
            "input_tokens": u.prompt_tokens,
            "output_tokens": u.completion_tokens,
        });
        if let Some(details) = &u.prompt_tokens_details
            && let Some(cached) = details.cached_tokens {
                usage["cache_read_input_tokens"] = serde_json::json!(cached);
            }
        usage
    });

    let mut result = serde_json::json!({
        "id": resp.id,
        "type": "message",
        "role": "assistant",
        "content": if content_blocks.is_empty() { vec![serde_json::json!({"type":"text","text":""})] } else { content_blocks },
        "model": resp.model,
        "stop_reason": stop_reason,
    });

    if let Some(u) = usage {
        result["usage"] = u;
    }

    result
}

/// Parse an Anthropic SSE chunk into a CommonChunk.
pub fn from_chunk(chunk: &str) -> Result<CommonChunk, String> {
    let data_line = chunk
        .lines()
        .find(|l| l.starts_with("data: "))
        .ok_or_else(|| chunk.to_string())?;

    let json: Value = serde_json::from_str(&data_line[6..]).map_err(|_| chunk.to_string())?;

    let event_type = json
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or_default();

    let mut out = CommonChunk {
        id: json
            .get("id")
            .or_else(|| json.get("message").and_then(|m| m.get("id")))
            .and_then(|i| i.as_str())
            .unwrap_or_default()
            .to_string(),
        object: "chat.completion.chunk".to_string(),
        created: now_unix(),
        model: json
            .get("model")
            .or_else(|| json.get("message").and_then(|m| m.get("model")))
            .and_then(|m| m.as_str())
            .unwrap_or_default()
            .to_string(),
        choices: Vec::new(),
        usage: None,
    };

    match event_type {
        "content_block_start" => {
            let cb = json.get("content_block");
            let idx = json.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;

            if let Some(cb) = cb {
                let cb_type = cb.get("type").and_then(|t| t.as_str()).unwrap_or_default();
                match cb_type {
                    "text" => {
                        out.choices.push(CommonChunkChoice {
                            index: idx,
                            delta: ChunkDelta {
                                role: Some("assistant".to_string()),
                                content: Some(String::new()),
                                tool_calls: None,
                            },
                            finish_reason: None,
                        });
                    }
                    "tool_use" => {
                        out.choices.push(CommonChunkChoice {
                            index: idx,
                            delta: ChunkDelta {
                                role: None,
                                content: None,
                                tool_calls: Some(vec![ChunkToolCall {
                                    index: idx,
                                    id: cb.get("id").and_then(|i| i.as_str()).map(String::from),
                                    call_type: Some("function".to_string()),
                                    function: Some(ChunkFunction {
                                        name: cb
                                            .get("name")
                                            .and_then(|n| n.as_str())
                                            .map(String::from),
                                        arguments: Some(String::new()),
                                    }),
                                }]),
                            },
                            finish_reason: None,
                        });
                    }
                    _ => {}
                }
            }
        }
        "content_block_delta" => {
            let delta = json.get("delta");
            let idx = json.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;

            if let Some(d) = delta {
                let d_type = d.get("type").and_then(|t| t.as_str()).unwrap_or_default();
                match d_type {
                    "text_delta" => {
                        let text = d.get("text").and_then(|t| t.as_str()).unwrap_or_default();
                        out.choices.push(CommonChunkChoice {
                            index: idx,
                            delta: ChunkDelta {
                                content: Some(text.to_string()),
                                ..Default::default()
                            },
                            finish_reason: None,
                        });
                    }
                    "input_json_delta" => {
                        let partial = d
                            .get("partial_json")
                            .and_then(|p| p.as_str())
                            .unwrap_or_default();
                        out.choices.push(CommonChunkChoice {
                            index: idx,
                            delta: ChunkDelta {
                                tool_calls: Some(vec![ChunkToolCall {
                                    index: idx,
                                    id: None,
                                    call_type: None,
                                    function: Some(ChunkFunction {
                                        name: None,
                                        arguments: Some(partial.to_string()),
                                    }),
                                }]),
                                ..Default::default()
                            },
                            finish_reason: None,
                        });
                    }
                    _ => {}
                }
            }
        }
        "message_delta" => {
            let finish_reason = json
                .get("delta")
                .and_then(|d| d.get("stop_reason"))
                .and_then(|r| r.as_str())
                .map(|r| {
                    match r {
                        "end_turn" => "stop",
                        "tool_use" => "tool_calls",
                        "max_tokens" => "length",
                        other => other,
                    }
                    .to_string()
                });

            out.choices.push(CommonChunkChoice {
                index: 0,
                delta: ChunkDelta::default(),
                finish_reason,
            });
        }
        _ => {}
    }

    // Usage
    let usage_val = json
        .get("usage")
        .or_else(|| json.get("message").and_then(|m| m.get("usage")));
    if let Some(u) = usage_val {
        let pt = u
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let ct = u
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let cached = u
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        out.usage = Some(CommonUsage {
            prompt_tokens: pt,
            completion_tokens: ct,
            total_tokens: match (pt, ct) {
                (Some(p), Some(c)) => Some(p + c),
                _ => None,
            },
            prompt_tokens_details: cached.map(|c| PromptTokensDetails {
                cached_tokens: Some(c),
            }),
        });
    }

    Ok(out)
}

/// Convert a CommonChunk to Anthropic SSE format string.
pub fn to_chunk(chunk: &CommonChunk) -> String {
    let choice = match chunk.choices.first() {
        Some(c) => c,
        None => return String::new(),
    };

    let delta = &choice.delta;

    if let Some(content) = &delta.content {
        let data = serde_json::json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": { "type": "text_delta", "text": content }
        });
        return format!("event: content_block_delta\ndata: {}", data);
    }

    if let Some(tool_calls) = &delta.tool_calls {
        for tc in tool_calls {
            if let Some(func) = &tc.function {
                if func.name.is_some() {
                    let data = serde_json::json!({
                        "type": "content_block_start",
                        "index": tc.index,
                        "content_block": {
                            "type": "tool_use",
                            "id": tc.id,
                            "name": func.name,
                            "input": {}
                        }
                    });
                    return format!("event: content_block_start\ndata: {}", data);
                }
                if let Some(args) = &func.arguments {
                    let data = serde_json::json!({
                        "type": "content_block_delta",
                        "index": tc.index,
                        "delta": { "type": "input_json_delta", "partial_json": args }
                    });
                    return format!("event: content_block_delta\ndata: {}", data);
                }
            }
        }
    }

    if let Some(finish) = &choice.finish_reason {
        let stop_reason = match finish.as_str() {
            "stop" => "end_turn",
            "tool_calls" => "tool_use",
            "length" => "max_tokens",
            other => other,
        };
        let data = serde_json::json!({
            "type": "message_delta",
            "delta": { "stop_reason": stop_reason }
        });
        return format!("event: message_delta\ndata: {}", data);
    }

    String::new()
}

/// Normalize Anthropic usage into UsageInfo.
pub fn normalize_usage(usage: &Value) -> UsageInfo {
    UsageInfo {
        input_tokens: usage
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        output_tokens: usage
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        reasoning_tokens: None,
        cache_read_tokens: usage
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
        cache_write_5m_tokens: usage
            .get("cache_creation")
            .and_then(|c| c.get("ephemeral_5m_input_tokens"))
            .and_then(|v| v.as_u64())
            .or_else(|| {
                usage
                    .get("cache_creation_input_tokens")
                    .and_then(|v| v.as_u64())
            })
            .map(|v| v as u32),
        cache_write_1h_tokens: usage
            .get("cache_creation")
            .and_then(|c| c.get("ephemeral_1h_input_tokens"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
    }
}

fn convert_anthropic_image_source(source: Option<&Value>) -> Option<ContentPart> {
    let src = source?;
    let src_type = src.get("type").and_then(|t| t.as_str())?;
    match src_type {
        "url" => {
            let url = src.get("url").and_then(|u| u.as_str())?;
            Some(ContentPart::ImageUrl {
                image_url: ImageUrl {
                    url: url.to_string(),
                },
            })
        }
        "base64" => {
            let media_type = src.get("media_type").and_then(|m| m.as_str())?;
            let data = src.get("data").and_then(|d| d.as_str())?;
            Some(ContentPart::ImageUrl {
                image_url: ImageUrl {
                    url: format!("data:{media_type};base64,{data}"),
                },
            })
        }
        _ => None,
    }
}

fn convert_url_to_anthropic_source(url: &str) -> Value {
    if let Some(rest) = url.strip_prefix("data:")
        && let Some((media_type, data)) = rest.split_once(";base64,") {
            return serde_json::json!({
                "type": "base64",
                "media_type": media_type,
                "data": data,
            });
        }
    serde_json::json!({ "type": "url", "url": url })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_anthropic_request_basic() {
        let body = serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 1024,
            "system": [{"type": "text", "text": "You are helpful."}],
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "Hello"}]},
            ],
        });

        let req = from_request(&body);
        assert_eq!(req.model, "claude-sonnet-4-20250514");
        assert_eq!(req.messages.len(), 2); // system + user
        assert_eq!(req.messages[0].role, Role::System);
        assert_eq!(req.messages[1].role, Role::User);
    }

    #[test]
    fn test_from_anthropic_response_with_tool_use() {
        let resp = serde_json::json!({
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet-4-20250514",
            "content": [
                {"type": "text", "text": "Let me read that."},
                {"type": "tool_use", "id": "toolu_1", "name": "read", "input": {"path": "main.py"}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 100, "output_tokens": 50}
        });

        let common = from_response(&resp);
        assert_eq!(common.id, "chatcmpl_123");
        let choice = &common.choices[0];
        assert_eq!(choice.message.content.as_deref(), Some("Let me read that."));
        assert_eq!(choice.finish_reason.as_deref(), Some("tool_calls"));
        let tc = choice.message.tool_calls.as_ref().unwrap();
        assert_eq!(tc[0].function.name, "read");
    }

    #[test]
    fn test_roundtrip_request() {
        let common = CommonRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: Some(1024),
            temperature: Some(0.5),
            top_p: None,
            stop: None,
            messages: vec![
                CommonMessage {
                    role: Role::System,
                    content: Some(Content::Text("Be helpful".to_string())),
                    tool_call_id: None,
                    tool_calls: None,
                    name: None,
                },
                CommonMessage {
                    role: Role::User,
                    content: Some(Content::Text("Hello".to_string())),
                    tool_call_id: None,
                    tool_calls: None,
                    name: None,
                },
            ],
            stream: Some(false),
            tools: None,
            tool_choice: None,
        };

        let anthropic = to_request(&common);
        let back = from_request(&anthropic);

        assert_eq!(back.messages.len(), 2);
        assert_eq!(back.messages[0].role, Role::System);
        assert_eq!(back.messages[1].role, Role::User);
    }

    #[test]
    fn test_from_anthropic_chunk_text_delta() {
        let chunk = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}";
        let result = from_chunk(chunk).unwrap();
        assert_eq!(result.choices[0].delta.content.as_deref(), Some("Hello"));
    }

    #[test]
    fn test_from_anthropic_chunk_tool_start() {
        let chunk = "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"read\"}}";
        let result = from_chunk(chunk).unwrap();
        let tc = result.choices[0].delta.tool_calls.as_ref().unwrap();
        assert_eq!(
            tc[0].function.as_ref().unwrap().name.as_deref(),
            Some("read")
        );
    }

    #[test]
    fn test_normalize_usage() {
        let usage = serde_json::json!({
            "input_tokens": 100,
            "output_tokens": 50,
            "cache_read_input_tokens": 80,
            "cache_creation": {
                "ephemeral_5m_input_tokens": 20,
                "ephemeral_1h_input_tokens": 0
            }
        });
        let info = normalize_usage(&usage);
        assert_eq!(info.input_tokens, 100);
        assert_eq!(info.output_tokens, 50);
        assert_eq!(info.cache_read_tokens, Some(80));
        assert_eq!(info.cache_write_5m_tokens, Some(20));
    }
}
