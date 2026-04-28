//! Single-purpose slice extracted from `providers/openai.rs` (D5 split).

#![allow(unused_imports)]

use super::super::common::*;
use serde_json::Value;

pub fn from_response(resp: &Value) -> CommonResponse {
    // If already in common format
    if resp.get("choices").and_then(|c| c.as_array()).is_some() {
        return serde_json::from_value(resp.clone()).unwrap_or_else(|_| empty_response());
    }

    let r = resp.get("response").unwrap_or(resp);
    let id = r
        .get("id")
        .and_then(|i| i.as_str())
        .map(|i| i.replace("resp_", "chatcmpl_"))
        .unwrap_or_default();
    let model = r
        .get("model")
        .or_else(|| resp.get("model"))
        .and_then(|m| m.as_str())
        .unwrap_or_default()
        .to_string();

    let output = r
        .get("output")
        .and_then(|o| o.as_array())
        .cloned()
        .unwrap_or_default();

    let text: String = output
        .iter()
        .filter(|o| o.get("type").and_then(|t| t.as_str()) == Some("message"))
        .filter_map(|o| o.get("content").and_then(|c| c.as_array()))
        .flatten()
        .filter(|p| p.get("type").and_then(|t| t.as_str()) == Some("output_text"))
        .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
        .collect::<Vec<_>>()
        .join("");

    let tool_calls: Vec<CommonToolCall> = output
        .iter()
        .filter(|o| o.get("type").and_then(|t| t.as_str()) == Some("function_call"))
        .map(|o| {
            let name = o
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or_default()
                .to_string();
            let args = o
                .get("arguments")
                .map(|a| {
                    if let Some(s) = a.as_str() {
                        s.to_string()
                    } else {
                        a.to_string()
                    }
                })
                .unwrap_or_else(|| "{}".to_string());
            let id = o
                .get("id")
                .and_then(|i| i.as_str())
                .unwrap_or_default()
                .to_string();
            CommonToolCall {
                id,
                call_type: "function".to_string(),
                function: CommonFunctionCall {
                    name,
                    arguments: args,
                },
            }
        })
        .collect();

    let finish_reason = r.get("stop_reason").and_then(|r| r.as_str()).map(|r| {
        match r {
            "stop" => "stop",
            "tool_call" | "tool_calls" => "tool_calls",
            "length" | "max_output_tokens" => "length",
            other => other,
        }
        .to_string()
    });

    let usage_val = r.get("usage").or_else(|| resp.get("usage"));
    let usage = usage_val.map(|u| {
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

/// Convert a CommonResponse to OpenAI Responses API format.
pub fn to_response(resp: &CommonResponse) -> Value {
    let choice = resp.choices.first();
    let mut output_items: Vec<Value> = Vec::new();

    if let Some(choice) = choice {
        if let Some(text) = &choice.message.content
            && !text.is_empty() {
                output_items.push(serde_json::json!({
                    "type": "message",
                    "status": "completed",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": text}],
                }));
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

    let stop_reason = choice
        .and_then(|c| c.finish_reason.as_deref())
        .map(|r| match r {
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
        if let Some(details) = &u.prompt_tokens_details
            && let Some(cached) = details.cached_tokens {
                usage["input_tokens_details"] = serde_json::json!({"cached_tokens": cached});
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

    if let Some(u) = usage {
        result["usage"] = u;
    }

    result
}

pub(super) fn empty_response() -> CommonResponse {
    CommonResponse {
        id: String::new(),
        object: "chat.completion".to_string(),
        created: now_unix(),
        model: String::new(),
        choices: Vec::new(),
        usage: None,
    }
}

