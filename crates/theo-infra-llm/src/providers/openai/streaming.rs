//! Single-purpose slice extracted from `providers/openai.rs` (D5 split).

#![allow(unused_imports)]

use super::super::common::*;
use serde_json::Value;

pub fn from_chunk(chunk: &str) -> Result<CommonChunk, String> {
    let lines: Vec<&str> = chunk.lines().collect();
    let event_line = lines.first().ok_or_else(|| chunk.to_string())?;
    let data_line = lines
        .iter()
        .find(|l| l.starts_with("data: "))
        .ok_or_else(|| chunk.to_string())?;
    let json: Value = theo_domain::safe_json::from_str_bounded(
        &data_line[6..],
        theo_domain::safe_json::DEFAULT_JSON_LIMIT,
    )
    .map_err(|_| chunk.to_string())?;
    let resp_obj = json
        .get("response")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let event = event_line.strip_prefix("event: ").unwrap_or_default().trim();
    let mut out = build_initial_chunk(&json, &resp_obj);
    match event {
        "response.output_text.delta" => append_text_delta(&json, &mut out),
        "response.output_item.added"
            if json
                .get("item")
                .and_then(|i| i.get("type"))
                .and_then(|t| t.as_str())
                == Some("function_call") =>
        {
            append_function_call_added(&json, &mut out);
        }
        "response.function_call_arguments.delta" => {
            append_function_call_arguments_delta(&json, &mut out);
        }
        "response.completed" => append_response_completed(&json, &resp_obj, &mut out),
        _ => {}
    }
    Ok(out)
}

fn build_initial_chunk(json: &Value, resp_obj: &Value) -> CommonChunk {
    CommonChunk {
        id: resp_obj
            .get("id")
            .or_else(|| json.get("id"))
            .and_then(|i| i.as_str())
            .unwrap_or_default()
            .to_string(),
        object: "chat.completion.chunk".to_string(),
        created: now_unix(),
        model: resp_obj
            .get("model")
            .or_else(|| json.get("model"))
            .and_then(|m| m.as_str())
            .unwrap_or_default()
            .to_string(),
        choices: Vec::new(),
        usage: None,
    }
}

fn append_text_delta(json: &Value, out: &mut CommonChunk) {
    let delta = json
        .get("delta")
        .or_else(|| json.get("text"))
        .and_then(|d| d.as_str());
    let Some(d) = delta else {
        return;
    };
    if d.is_empty() {
        return;
    }
    out.choices.push(CommonChunkChoice {
        index: 0,
        delta: ChunkDelta {
            content: Some(d.to_string()),
            ..Default::default()
        },
        finish_reason: None,
    });
}

fn append_function_call_added(json: &Value, out: &mut CommonChunk) {
    let name = json
        .get("item")
        .and_then(|i| i.get("name"))
        .and_then(|n| n.as_str());
    let id = json
        .get("item")
        .and_then(|i| i.get("id"))
        .and_then(|i| i.as_str());
    let Some(name) = name else {
        return;
    };
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

fn append_function_call_arguments_delta(json: &Value, out: &mut CommonChunk) {
    let args = json
        .get("delta")
        .or_else(|| json.get("arguments_delta"))
        .and_then(|a| a.as_str());
    let Some(a) = args else {
        return;
    };
    if a.is_empty() {
        return;
    }
    out.choices.push(CommonChunkChoice {
        index: 0,
        delta: ChunkDelta {
            tool_calls: Some(vec![ChunkToolCall {
                index: 0,
                id: None,
                call_type: None,
                function: Some(ChunkFunction {
                    name: None,
                    arguments: Some(a.to_string()),
                }),
            }]),
            ..Default::default()
        },
        finish_reason: None,
    });
}

fn append_response_completed(json: &Value, resp_obj: &Value, out: &mut CommonChunk) {
    let sr = resp_obj
        .get("stop_reason")
        .or_else(|| json.get("stop_reason"))
        .and_then(|r| r.as_str());
    let finish = sr.map(|r| {
        match r {
            "stop" => "stop",
            "tool_call" | "tool_calls" => "tool_calls",
            "length" | "max_output_tokens" => "length",
            other => other,
        }
        .to_string()
    });
    out.choices.push(CommonChunkChoice {
        index: 0,
        delta: ChunkDelta::default(),
        finish_reason: finish,
    });
    let u = resp_obj
        .get("usage")
        .or_else(|| json.get("response").and_then(|r| r.get("usage")));
    if let Some(u) = u {
        out.usage = Some(parse_streaming_usage(u));
    }
}

fn parse_streaming_usage(u: &Value) -> CommonUsage {
    let pt = u
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    let ct = u
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    let cached = u
        .get("input_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .and_then(|c| c.as_u64())
        .map(|v| v as u32);
    CommonUsage {
        prompt_tokens: pt,
        completion_tokens: ct,
        total_tokens: match (pt, ct) {
            (Some(p), Some(c)) => Some(p + c),
            _ => None,
        },
        prompt_tokens_details: cached.map(|c| PromptTokensDetails {
            cached_tokens: Some(c),
        }),
    }
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
    let input = usage
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let output = usage
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let reasoning = usage
        .get("output_tokens_details")
        .and_then(|d| d.get("reasoning_tokens"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    let cache_read = usage
        .get("input_tokens_details")
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

