//! T0.1 / D1 — Serialize `ChatRequest` to OA-compatible JSON, bridging
//! `Message.content_blocks` (multimodal) to the OA `content: [<parts>]`
//! array shape that `providers/openai_compatible::from_request` understands.
//!
//! This is the boundary where the additive `content_blocks` field becomes
//! the wire format the provider expects. Without this, vision content is
//! invisible to all converters.

use serde_json::{Value, json};

use crate::types::{ChatRequest, ContentBlock, Message, Role};

/// Serialize a `ChatRequest` into OA-compatible JSON.
///
/// When a `Message` carries `content_blocks` with at least one image block,
/// `content` is emitted as an array of OA `{type:text, text:...}` /
/// `{type:image_url, image_url:{url:...}}` parts. Otherwise the standard
/// serde serialization is used (string `content` field).
///
/// Anthropic-specific shape (image as `{type:image, source:{base64,...}}`)
/// is handled later by `providers::converter::convert_request(OaCompat →
/// Anthropic, ...)`.
pub fn serialize_oa_compat(request: &ChatRequest) -> Value {
    let messages: Vec<Value> = request.messages.iter().map(message_to_oa).collect();

    let mut req = json!({
        "model": request.model,
        "messages": messages,
    });

    if let Some(tools) = &request.tools {
        req["tools"] = serde_json::to_value(tools).unwrap_or(Value::Null);
    }
    if let Some(tc) = &request.tool_choice {
        req["tool_choice"] = json!(tc);
    }
    if let Some(mt) = request.max_tokens {
        req["max_tokens"] = json!(mt);
    }
    if let Some(t) = request.temperature {
        req["temperature"] = json!(t);
    }
    if let Some(s) = request.stream {
        req["stream"] = json!(s);
    }
    if let Some(re) = &request.reasoning_effort {
        req["reasoning_effort"] = json!(re);
    }

    req
}

fn role_to_str(role: &Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

fn message_to_oa(m: &Message) -> Value {
    // Multimodal path: only when content_blocks is present AND has at least
    // one image. Pure-text blocks keep going through the legacy string path
    // for maximum compatibility with providers that don't accept arrays.
    if let Some(blocks) = &m.content_blocks
        && blocks.iter().any(ContentBlock::is_image)
    {
        let parts: Vec<Value> = blocks.iter().map(content_block_to_oa_part).collect();
        let mut obj = json!({
            "role": role_to_str(&m.role),
            "content": parts,
        });
        if let Some(tc) = &m.tool_calls {
            obj["tool_calls"] = serde_json::to_value(tc).unwrap_or(Value::Null);
        }
        if let Some(id) = &m.tool_call_id {
            obj["tool_call_id"] = json!(id);
        }
        if let Some(n) = &m.name {
            obj["name"] = json!(n);
        }
        return obj;
    }

    // Default serde path — preserves all existing semantics.
    serde_json::to_value(m).unwrap_or_default()
}

fn content_block_to_oa_part(b: &ContentBlock) -> Value {
    match b {
        ContentBlock::Text { text } => json!({ "type": "text", "text": text }),
        ContentBlock::ImageUrl { image_url } => {
            let mut iu = json!({ "url": image_url.url });
            if let Some(d) = &image_url.detail {
                iu["detail"] = serde_json::to_value(d).unwrap_or(Value::Null);
            }
            json!({ "type": "image_url", "image_url": iu })
        }
        ContentBlock::ImageBase64 { source } => {
            // OA-compat exposes base64 as `data:` URL inside image_url.
            // Anthropic converter later remaps to its native shape.
            let data_url = format!("data:{};base64,{}", source.media_type, source.data);
            json!({ "type": "image_url", "image_url": { "url": data_url } })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Message;

    #[test]
    fn t01b_text_only_message_uses_string_content() {
        let req = ChatRequest::new("gpt-4", vec![Message::user("hello")]);
        let json = serialize_oa_compat(&req);
        assert_eq!(json["model"], "gpt-4");
        let msg = &json["messages"][0];
        assert_eq!(msg["role"], "user");
        assert_eq!(msg["content"], "hello");
    }

    #[test]
    fn t01b_image_url_message_emits_array_content() {
        let req = ChatRequest::new(
            "gpt-4o",
            vec![Message::user_with_image_url("describe", "https://e.x/a.png")],
        );
        let json = serialize_oa_compat(&req);
        let msg = &json["messages"][0];
        let content = msg["content"].as_array().expect("array content");
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "describe");
        assert_eq!(content[1]["type"], "image_url");
        assert_eq!(content[1]["image_url"]["url"], "https://e.x/a.png");
    }

    #[test]
    fn t01b_image_base64_serializes_as_data_url() {
        let req = ChatRequest::new(
            "claude-3",
            vec![Message::user_with_image_base64(
                "look",
                "image/png",
                "AAAA",
            )],
        );
        let json = serialize_oa_compat(&req);
        let msg = &json["messages"][0];
        let parts = msg["content"].as_array().unwrap();
        assert_eq!(parts[1]["type"], "image_url");
        let url = parts[1]["image_url"]["url"].as_str().unwrap();
        assert!(url.starts_with("data:image/png;base64,"));
        assert!(url.ends_with("AAAA"));
    }

    #[test]
    fn t01b_passthrough_preserves_tools_and_options() {
        use crate::types::ToolDefinition;
        let mut req = ChatRequest::new("m", vec![Message::user("hi")]).with_tools(vec![
            ToolDefinition::new("foo", "desc", json!({"type": "object"})),
        ]);
        req.max_tokens = Some(100);
        req.temperature = Some(0.5);

        let json = serialize_oa_compat(&req);
        assert!(json["tools"].is_array());
        assert_eq!(json["max_tokens"], 100);
        assert_eq!(json["temperature"], 0.5);
        assert_eq!(json["tool_choice"], "auto"); // set by with_tools
    }

    #[test]
    fn t01b_blocks_without_image_take_legacy_path() {
        // Only text blocks → legacy string path (avoid breaking
        // text-only providers that don't accept array content).
        let req = ChatRequest::new(
            "m",
            vec![Message::user_with_blocks(vec![ContentBlock::text("hi")])],
        );
        let json = serialize_oa_compat(&req);
        let msg = &json["messages"][0];
        // content_blocks is preserved in the JSON AND content (string)
        // is the fallback. The string path is taken because no image.
        assert_eq!(msg["content"], "hi");
    }

    #[test]
    fn t01b_image_detail_propagates() {
        use crate::types::ImageDetail;
        let m = Message::user_with_blocks(vec![
            ContentBlock::text("look"),
            ContentBlock::image_url_with_detail("https://e.x/a.png", ImageDetail::High),
        ]);
        let req = ChatRequest::new("m", vec![m]);
        let json = serialize_oa_compat(&req);
        let parts = json["messages"][0]["content"].as_array().unwrap();
        assert_eq!(parts[1]["image_url"]["detail"], "high");
    }

    #[test]
    fn t01b_assistant_with_tool_calls_array_content() {
        use crate::types::ToolCall;
        let mut m = Message::assistant_with_tool_calls(
            None,
            vec![ToolCall::new("c1", "f", "{}")],
        );
        // Force multimodal path with mixed content
        m.content_blocks = Some(vec![
            ContentBlock::text("note"),
            ContentBlock::image_url("https://e.x/a.png"),
        ]);
        let req = ChatRequest::new("m", vec![m]);
        let json = serialize_oa_compat(&req);
        let msg = &json["messages"][0];
        assert_eq!(msg["role"], "assistant");
        assert!(msg["content"].is_array());
        assert!(msg["tool_calls"].is_array());
    }

    #[test]
    fn t01b_role_mapping_is_correct() {
        for (role, expected) in [
            (Role::System, "system"),
            (Role::User, "user"),
            (Role::Assistant, "assistant"),
            (Role::Tool, "tool"),
        ] {
            assert_eq!(role_to_str(&role), expected);
        }
    }
}
