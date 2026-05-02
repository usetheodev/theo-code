//! Single-purpose slice extracted from `providers/anthropic.rs` (D5 split).

#![allow(unused_imports)]

use super::super::common::*;
use serde_json::Value;

/// Anthropic image source helpers.
pub(super) fn convert_anthropic_image_source(source: Option<&Value>) -> Option<ContentPart> {
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

pub(super) fn convert_url_to_anthropic_source(url: &str) -> Value {
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

