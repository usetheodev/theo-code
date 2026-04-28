//! Single-purpose slice extracted from `providers/anthropic.rs` (D5 split).

#![allow(unused_imports)]

use super::super::common::*;
use serde_json::Value;

/// Anthropic streaming chunk conversion.
pub fn from_chunk(chunk: &str) -> Result<CommonChunk, String> {
    let data_line = chunk
        .lines()
        .find(|l| l.starts_with("data: "))
        .ok_or_else(|| chunk.to_string())?;

    // T2.7: bound the SSE chunk to 10 MiB so a misbehaving Anthropic endpoint
    // cannot force unbounded allocation in the parser.
    let json: Value = theo_domain::safe_json::from_str_bounded(
        &data_line[6..],
        theo_domain::safe_json::DEFAULT_JSON_LIMIT,
    )
    .map_err(|_| chunk.to_string())?;

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

