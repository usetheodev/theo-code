//! Single-purpose slice extracted from `providers/anthropic.rs` (D5 split).

#![allow(unused_imports)]

use super::super::common::*;
use serde_json::Value;
use super::image::{convert_anthropic_image_source, convert_url_to_anthropic_source};

/// Anthropic request conversion.
pub fn from_request(body: &Value) -> CommonRequest {
    let mut messages = Vec::new();
    extract_system_messages(body, &mut messages);
    extract_chat_messages(body, &mut messages);
    let tools = convert_tools(body);
    let tool_choice = convert_tool_choice(body);
    let stop = parse_stop_sequences(body);
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

fn extract_system_messages(body: &Value, messages: &mut Vec<CommonMessage>) {
    let Some(sys) = body.get("system").and_then(|s| s.as_array()) else {
        return;
    };
    for s in sys {
        if s.get("type").and_then(|t| t.as_str()) != Some("text") {
            continue;
        }
        if let Some(text) = s.get("text").and_then(|t| t.as_str())
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
}

fn extract_chat_messages(body: &Value, messages: &mut Vec<CommonMessage>) {
    let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) else {
        return;
    };
    for m in msgs {
        let Some(role) = m.get("role").and_then(|r| r.as_str()) else {
            continue;
        };
        match role {
            "user" => extract_user_message(m, messages),
            "assistant" => extract_assistant_message(m, messages),
            _ => {}
        }
    }
}

fn extract_user_message(m: &Value, messages: &mut Vec<CommonMessage>) {
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
    if text_parts.is_empty() {
        return;
    }
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

fn extract_assistant_message(m: &Value, messages: &mut Vec<CommonMessage>) {
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
                let arguments = match p.get("input") {
                    Some(v) => v.as_str().map_or_else(|| v.to_string(), str::to_owned),
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

fn convert_tools(body: &Value) -> Option<Vec<CommonTool>> {
    body.get("tools").and_then(|t| t.as_array()).map(|arr| {
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
    })
}

fn convert_tool_choice(body: &Value) -> Option<ToolChoice> {
    body.get("tool_choice").and_then(|tc| {
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
    })
}

fn parse_stop_sequences(body: &Value) -> Option<StopSequence> {
    body.get("stop_sequences").and_then(|v| {
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

/// Convert a CommonRequest to Anthropic Messages API format.
pub fn to_request(body: &CommonRequest) -> Value {
    let mut cc_count = 0u32;
    let mut system_blocks: Vec<Value> = Vec::new();
    let mut messages_out: Vec<Value> = Vec::new();
    for msg in &body.messages {
        match msg.role {
            Role::System => append_system_block(msg, &mut system_blocks, &mut cc_count),
            Role::User => append_user_message(msg, &mut messages_out, &mut cc_count),
            Role::Assistant => append_assistant_message(msg, &mut messages_out, &mut cc_count),
            Role::Tool => append_tool_result_message(msg, &mut messages_out, &mut cc_count),
        }
    }
    let tools = convert_request_tools(body.tools.as_ref(), &mut cc_count);
    let tool_choice = convert_request_tool_choice(body.tool_choice.as_ref());
    let stop_sequences = convert_stop_sequences(body.stop.as_ref());
    assemble_anthropic_request(
        body,
        system_blocks,
        messages_out,
        tools,
        tool_choice,
        stop_sequences,
    )
}

/// Anthropic prompt-caching: only the first 4 cacheable blocks
/// receive `cache_control: {type: ephemeral}`. Subsequent blocks
/// produce an empty object (caller still merges, but no-op).
fn next_cache_control(count: &mut u32) -> Value {
    *count += 1;
    if *count <= 4 {
        serde_json::json!({ "cache_control": { "type": "ephemeral" } })
    } else {
        serde_json::json!({})
    }
}

fn apply_cache_control(block: &mut Value, count: &mut u32) {
    let cache = next_cache_control(count);
    if let Some(obj) = cache.as_object() {
        for (k, v) in obj {
            block[k] = v.clone();
        }
    }
}

fn append_system_block(msg: &CommonMessage, system_blocks: &mut Vec<Value>, cc_count: &mut u32) {
    let Some(content) = &msg.content else {
        return;
    };
    let text = content.to_text();
    if text.is_empty() {
        return;
    }
    let mut block = serde_json::json!({ "type": "text", "text": text });
    apply_cache_control(&mut block, cc_count);
    system_blocks.push(block);
}

fn append_user_message(msg: &CommonMessage, messages_out: &mut Vec<Value>, cc_count: &mut u32) {
    let Some(content) = &msg.content else {
        return;
    };
    let parts = match content {
        Content::Text(text) => {
            let mut block = serde_json::json!({ "type": "text", "text": text });
            apply_cache_control(&mut block, cc_count);
            vec![block]
        }
        Content::Parts(parts) => parts
            .iter()
            .map(|p| build_user_part_block(p, cc_count))
            .collect(),
    };
    messages_out.push(serde_json::json!({ "role": "user", "content": parts }));
}

fn build_user_part_block(part: &ContentPart, cc_count: &mut u32) -> Value {
    let mut block = match part {
        ContentPart::Text { text } => serde_json::json!({ "type": "text", "text": text }),
        ContentPart::ImageUrl { image_url } => {
            let source = convert_url_to_anthropic_source(&image_url.url);
            serde_json::json!({ "type": "image", "source": source })
        }
    };
    apply_cache_control(&mut block, cc_count);
    block
}

fn append_assistant_message(
    msg: &CommonMessage,
    messages_out: &mut Vec<Value>,
    cc_count: &mut u32,
) {
    let mut content_blocks: Vec<Value> = Vec::new();
    if let Some(content) = &msg.content {
        let text = content.to_text();
        if !text.is_empty() {
            let mut block = serde_json::json!({ "type": "text", "text": text });
            apply_cache_control(&mut block, cc_count);
            content_blocks.push(block);
        }
    }
    if let Some(tool_calls) = &msg.tool_calls {
        for tc in tool_calls {
            content_blocks.push(build_tool_use_block(tc, cc_count));
        }
    }
    if !content_blocks.is_empty() {
        messages_out.push(serde_json::json!({
            "role": "assistant",
            "content": content_blocks,
        }));
    }
}

fn build_tool_use_block(tc: &CommonToolCall, cc_count: &mut u32) -> Value {
    // T2.7: bound tool-call arguments so a malicious provider cannot
    // force unbounded allocation.
    let input: Value = theo_domain::safe_json::from_str_bounded(
        &tc.function.arguments,
        theo_domain::safe_json::DEFAULT_JSON_LIMIT,
    )
    .unwrap_or_else(|_| Value::String(tc.function.arguments.clone()));
    let mut block = serde_json::json!({
        "type": "tool_use",
        "id": tc.id,
        "name": tc.function.name,
        "input": input,
    });
    apply_cache_control(&mut block, cc_count);
    block
}

fn append_tool_result_message(
    msg: &CommonMessage,
    messages_out: &mut Vec<Value>,
    cc_count: &mut u32,
) {
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
    apply_cache_control(&mut block, cc_count);
    messages_out.push(serde_json::json!({ "role": "user", "content": [block] }));
}

fn convert_request_tools(
    tools: Option<&Vec<CommonTool>>,
    cc_count: &mut u32,
) -> Option<Vec<Value>> {
    tools.map(|tools| {
        tools
            .iter()
            .filter(|t| t.tool_type == "function")
            .map(|t| {
                let mut tool = serde_json::json!({
                    "name": t.function.name,
                    "description": t.function.description,
                    "input_schema": t.function.parameters,
                });
                apply_cache_control(&mut tool, cc_count);
                tool
            })
            .collect()
    })
}

fn convert_request_tool_choice(tc: Option<&ToolChoice>) -> Option<Value> {
    tc.map(|tc| match tc {
        ToolChoice::Mode(mode) => match mode.as_str() {
            "auto" => serde_json::json!({ "type": "auto" }),
            "required" => serde_json::json!({ "type": "any" }),
            _ => serde_json::json!({ "type": "auto" }),
        },
        ToolChoice::Function { function, .. } => {
            serde_json::json!({ "type": "tool", "name": function.name })
        }
    })
}

fn convert_stop_sequences(stop: Option<&StopSequence>) -> Option<Value> {
    stop.map(|s| match s {
        StopSequence::Single(s) => serde_json::json!([s]),
        StopSequence::Multiple(v) => serde_json::json!(v),
    })
}

fn assemble_anthropic_request(
    body: &CommonRequest,
    system_blocks: Vec<Value>,
    messages_out: Vec<Value>,
    tools: Option<Vec<Value>>,
    tool_choice: Option<Value>,
    stop_sequences: Option<Value>,
) -> Value {
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
