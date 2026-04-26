//! T1.2 ↔ T0.1 — Vision propagation glue.
//!
//! When a tool emits an image (e.g. `read_image` writes
//! `metadata.image_block` with the wire shape from
//! `theo_infra_llm::types::ContentBlock::ImageBase64`), the next LLM
//! turn needs that block as a **user content block** so the
//! vision-capable model actually sees the image.
//!
//! This module bridges the two sides:
//! - INPUT: a `ToolOutput.metadata` JSON value possibly carrying
//!   `image_block` (single object) or `image_blocks` (array).
//! - OUTPUT: zero or more `ContentBlock`s ready to be wrapped in a
//!   `Message::user_with_blocks` and pushed into the conversation
//!   AFTER the regular `Message::tool_result`.
//!
//! Why a separate helper instead of mutating `tool_bridge`:
//! - Keeps `execute_tool_call`'s `(Message, bool)` signature stable
//!   (177 call sites depend on it across the runtime + tests).
//! - Caller decides when to inject the follow-up user message — pilot
//!   loop and main loop have slightly different semantics.
//! - Pure function: trivially unit-testable without any LLM round-trip.

use serde_json::Value;

use theo_infra_llm::types::{ContentBlock, ImageSource, ImageUrlBlock, Message};

/// Extract any image blocks attached to a tool's `metadata` JSON.
/// Recognised shapes:
///
/// 1. Single block: `metadata.image_block = { type: "image_base64", source: {...} }`
/// 2. Single block: `metadata.image_block = { type: "image_url", image_url: {...} }`
/// 3. Array of blocks: `metadata.image_blocks = [ <block>, <block>, ... ]`
///
/// Unknown shapes / malformed entries are silently skipped — the agent
/// can always fall back to the textual tool result.
pub fn extract_image_blocks(metadata: &Value) -> Vec<ContentBlock> {
    let mut out = Vec::new();
    if let Some(single) = metadata.get("image_block")
        && let Some(block) = parse_block(single)
    {
        out.push(block);
    }
    if let Some(array) = metadata.get("image_blocks").and_then(Value::as_array) {
        for v in array {
            if let Some(block) = parse_block(v) {
                out.push(block);
            }
        }
    }
    out
}

/// Build a follow-up user message carrying the extracted image blocks.
/// Returns `None` when the tool emitted no image blocks — caller can
/// skip pushing anything in that case.
///
/// The follow-up is attributed as a `user` role because vision-capable
/// providers (Anthropic, OpenAI) only accept image blocks inside a
/// user-role message. The text part is a short pointer back to the
/// tool call so the LLM has linguistic context.
pub fn build_image_followup(metadata: &Value, tool_name: &str) -> Option<Message> {
    let blocks = extract_image_blocks(metadata);
    if blocks.is_empty() {
        return None;
    }
    let mut all = vec![ContentBlock::text(format!(
        "(Image attached by `{tool_name}`. The following blocks accompany the previous tool result.)"
    ))];
    all.extend(blocks);
    Some(Message::user_with_blocks(all))
}

fn parse_block(v: &Value) -> Option<ContentBlock> {
    let kind = v.get("type").and_then(Value::as_str)?;
    match kind {
        "image_base64" => {
            let source = v.get("source")?;
            let media_type = source.get("media_type").and_then(Value::as_str)?.to_string();
            let data = source.get("data").and_then(Value::as_str)?.to_string();
            Some(ContentBlock::ImageBase64 {
                source: ImageSource {
                    source_type: "base64".to_string(),
                    media_type,
                    data,
                },
            })
        }
        "image_url" => {
            let image_url = v.get("image_url")?;
            let url = image_url.get("url").and_then(Value::as_str)?.to_string();
            Some(ContentBlock::ImageUrl {
                image_url: ImageUrlBlock { url, detail: None },
            })
        }
        "text" => {
            // text blocks are not "images" but we accept them inside an
            // `image_blocks` array because some tools may want to mix
            // text+image. Skip otherwise.
            v.get("text")
                .and_then(Value::as_str)
                .map(|s| ContentBlock::text(s))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn t12prop_no_metadata_returns_empty() {
        let m = json!({});
        assert!(extract_image_blocks(&m).is_empty());
    }

    #[test]
    fn t12prop_extract_single_image_base64_block() {
        let m = json!({
            "image_block": {
                "type": "image_base64",
                "source": {
                    "type": "base64",
                    "media_type": "image/png",
                    "data": "ZGF0YQ=="
                }
            }
        });
        let blocks = extract_image_blocks(&m);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            ContentBlock::ImageBase64 { source } => {
                assert_eq!(source.media_type, "image/png");
                assert_eq!(source.data, "ZGF0YQ==");
            }
            _ => panic!("expected ImageBase64"),
        }
    }

    #[test]
    fn t12prop_extract_single_image_url_block() {
        let m = json!({
            "image_block": {
                "type": "image_url",
                "image_url": {"url": "https://e.x/a.png"}
            }
        });
        let blocks = extract_image_blocks(&m);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            ContentBlock::ImageUrl { image_url } => {
                assert_eq!(image_url.url, "https://e.x/a.png");
            }
            _ => panic!("expected ImageUrl"),
        }
    }

    #[test]
    fn t12prop_extract_image_blocks_array() {
        let m = json!({
            "image_blocks": [
                {"type": "image_base64", "source": {"type":"base64","media_type":"image/jpeg","data":"AAAA"}},
                {"type": "image_url", "image_url": {"url": "https://e.x/b.png"}}
            ]
        });
        let blocks = extract_image_blocks(&m);
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn t12prop_combines_single_and_array() {
        let m = json!({
            "image_block": {"type":"image_url","image_url":{"url":"u1"}},
            "image_blocks": [{"type":"image_url","image_url":{"url":"u2"}}]
        });
        let blocks = extract_image_blocks(&m);
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn t12prop_malformed_block_silently_skipped() {
        let m = json!({
            "image_block": {"type": "image_url"} // missing image_url field
        });
        assert!(extract_image_blocks(&m).is_empty());
    }

    #[test]
    fn t12prop_unknown_block_type_skipped() {
        let m = json!({
            "image_blocks": [
                {"type": "video", "url": "..."},
                {"type": "image_url", "image_url": {"url": "kept"}}
            ]
        });
        let blocks = extract_image_blocks(&m);
        assert_eq!(blocks.len(), 1);
    }

    #[test]
    fn t12prop_text_block_inside_image_blocks_is_kept() {
        // Some tools emit a mixed text+image array. We accept text so the
        // surrounding context isn't lost, but the function name still says
        // "image_blocks" — trust caller intent.
        let m = json!({
            "image_blocks": [
                {"type": "text", "text": "Caption"},
                {"type": "image_url", "image_url": {"url": "u"}}
            ]
        });
        let blocks = extract_image_blocks(&m);
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn t12prop_build_followup_returns_none_for_empty_metadata() {
        let m = json!({});
        assert!(build_image_followup(&m, "any_tool").is_none());
    }

    #[test]
    fn t12prop_build_followup_user_message_with_text_pointer() {
        let m = json!({
            "image_block": {
                "type": "image_base64",
                "source": {"type":"base64","media_type":"image/png","data":"AAAA"}
            }
        });
        let msg = build_image_followup(&m, "read_image").expect("some");
        assert_eq!(msg.role, theo_infra_llm::types::Role::User);
        let blocks = msg.content_blocks.as_ref().unwrap();
        // 1 text pointer + 1 image
        assert_eq!(blocks.len(), 2);
        assert!(matches!(blocks[0], ContentBlock::Text { .. }));
        assert!(matches!(blocks[1], ContentBlock::ImageBase64 { .. }));
        // Text pointer references the tool name so the LLM can reason
        // about which tool produced the image.
        match &blocks[0] {
            ContentBlock::Text { text } => assert!(text.contains("read_image")),
            _ => unreachable!(),
        }
    }

    #[test]
    fn t12prop_build_followup_marks_message_has_image() {
        let m = json!({
            "image_block": {"type":"image_url","image_url":{"url":"u"}}
        });
        let msg = build_image_followup(&m, "browser_screenshot").unwrap();
        assert!(msg.has_image());
    }

    #[test]
    fn t12prop_handles_nested_metadata_under_other_keys_gracefully() {
        // Tool metadata may contain unrelated fields — those don't
        // confuse the extractor.
        let m = json!({
            "type": "read_image",
            "bytes": 1234,
            "irrelevant_object": {"image_block": {"this": "is nested too deep"}},
            "image_block": {"type":"image_url","image_url":{"url":"top_level"}}
        });
        let blocks = extract_image_blocks(&m);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            ContentBlock::ImageUrl { image_url } => assert_eq!(image_url.url, "top_level"),
            _ => panic!(),
        }
    }
}
