use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::permission::{PermissionRequest, PermissionType};
use theo_domain::tool::{
    FileAttachment, PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam,
    ToolSchema, optional_string, require_string,
};

pub struct WebFetchTool;

impl WebFetchTool {
    pub fn new() -> Self {
        Self
    }

    /// SSRF protection: reject dangerous URLs before fetching.
    fn validate_url(url: &str) -> Result<(), ToolError> {
        // Only allow http/https schemes
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(ToolError::Execution(format!(
                "URL scheme not allowed: {url}. Only http:// and https:// are permitted."
            )));
        }

        // Extract host from URL
        let host = url
            .strip_prefix("http://")
            .or_else(|| url.strip_prefix("https://"))
            .and_then(|rest| rest.split('/').next())
            .and_then(|host_port| host_port.split(':').next())
            .unwrap_or("");

        // Block private IP ranges and metadata endpoints
        let blocked_hosts = [
            "127.0.0.1",
            "localhost",
            "0.0.0.0",
            "169.254.169.254",          // AWS IMDS
            "metadata.google.internal", // GCP metadata
            "metadata.internal",
        ];
        if blocked_hosts.contains(&host) {
            return Err(ToolError::Execution(format!(
                "URL host blocked (SSRF protection): {host}"
            )));
        }

        // Block private IP ranges by prefix
        let blocked_prefixes = [
            "10.", "172.16.", "172.17.", "172.18.", "172.19.", "172.20.", "172.21.", "172.22.",
            "172.23.", "172.24.", "172.25.", "172.26.", "172.27.", "172.28.", "172.29.", "172.30.",
            "172.31.", "192.168.", "169.254.",
        ];
        for prefix in &blocked_prefixes {
            if host.starts_with(prefix) {
                return Err(ToolError::Execution(format!(
                    "URL host blocked (private IP): {host}"
                )));
            }
        }

        Ok(())
    }

    fn is_image_content_type(content_type: &str) -> bool {
        let ct = content_type.to_lowercase();
        ct.starts_with("image/") && !ct.contains("svg")
    }

    fn extract_mime(content_type: &str) -> String {
        content_type
            .split(';')
            .next()
            .unwrap_or(content_type)
            .trim()
            .to_lowercase()
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn id(&self) -> &str {
        "webfetch"
    }

    fn description(&self) -> &str {
        "Fetch a URL and return its content"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![ToolParam {
                name: "url".to_string(),
                param_type: "string".to_string(),
                description: "URL to fetch".to_string(),
                required: true,
            }],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Web
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
        permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let url = require_string(&args, "url")?;
        let _format = optional_string(&args, "format").unwrap_or_else(|| "markdown".to_string());

        // SSRF protection: validate URL before fetching
        Self::validate_url(&url)?;

        permissions.record(PermissionRequest {
            permission: PermissionType::WebFetch,
            patterns: vec![url.clone()],
            always: vec![],
            metadata: serde_json::json!({}),
        });

        let response = reqwest::get(&url)
            .await
            .map_err(|e| ToolError::Execution(format!("Fetch failed: {e}")))?;

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/plain")
            .to_string();

        let mime = Self::extract_mime(&content_type);

        // Handle image responses
        if Self::is_image_content_type(&content_type) {
            let bytes = response
                .bytes()
                .await
                .map_err(|e| ToolError::Execution(format!("Failed to read response: {e}")))?;

            let b64 = base64_encode(&bytes);

            return Ok(ToolOutput {
                title: url,
                output: "Image fetched successfully".to_string(),
                metadata: serde_json::json!({}),
                attachments: Some(vec![FileAttachment {
                    file_type: "file".to_string(),
                    mime: Some(mime),
                    url: format!("data:{};base64,{b64}", Self::extract_mime(&content_type)),
                }]),
                llm_suffix: None,
            });
        }

        // Handle SVG as text
        let text = response
            .text()
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to read response: {e}")))?;

        Ok(ToolOutput {
            title: url,
            output: text,
            metadata: serde_json::json!({}),
            attachments: None,
            llm_suffix: None,
        })
    }
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[(n >> 18 & 63) as usize] as char);
        result.push(CHARS[(n >> 12 & 63) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[(n >> 6 & 63) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(n & 63) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // Unit tests for content type detection (no network needed)

    #[test]
    fn detects_image_content_type() {
        assert!(WebFetchTool::is_image_content_type(
            "IMAGE/PNG; charset=binary"
        ));
        assert!(WebFetchTool::is_image_content_type("image/jpeg"));
        assert!(WebFetchTool::is_image_content_type("image/webp"));
    }

    #[test]
    fn svg_is_not_image_content_type() {
        assert!(!WebFetchTool::is_image_content_type(
            "image/svg+xml; charset=UTF-8"
        ));
    }

    #[test]
    fn extracts_mime_from_content_type() {
        assert_eq!(
            WebFetchTool::extract_mime("IMAGE/PNG; charset=binary"),
            "image/png"
        );
        assert_eq!(WebFetchTool::extract_mime("text/plain"), "text/plain");
    }

    #[test]
    fn base64_encode_roundtrip() {
        let input = &[137, 80, 78, 71, 13, 10, 26, 10]; // PNG magic bytes
        let encoded = base64_encode(input);
        assert!(encoded.starts_with("iVBOR"));
    }
}
