//! HTTP Client builtin plugin — typed HTTP requests with SSRF protection.

use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

fn validate_url(url: &str) -> Result<(), ToolError> {
    // Reuse SSRF protection logic from WebFetchTool
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(ToolError::Execution(format!(
            "Only http/https URLs allowed: {url}"
        )));
    }
    let host = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .and_then(|rest| rest.split('/').next())
        .and_then(|h| h.split(':').next())
        .unwrap_or("");
    let blocked = [
        "127.0.0.1",
        "localhost",
        "0.0.0.0",
        "169.254.169.254",
        "metadata.google.internal",
    ];
    if blocked.contains(&host) {
        return Err(ToolError::Execution(format!("SSRF blocked: {host}")));
    }
    let blocked_prefixes = [
        "10.", "172.16.", "172.17.", "172.18.", "172.19.", "172.20.", "172.21.", "172.22.",
        "172.23.", "172.24.", "172.25.", "172.26.", "172.27.", "172.28.", "172.29.", "172.30.",
        "172.31.", "192.168.", "169.254.",
    ];
    for prefix in &blocked_prefixes {
        if host.starts_with(prefix) {
            return Err(ToolError::Execution(format!(
                "SSRF blocked (private IP): {host}"
            )));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// HttpGetTool
// ---------------------------------------------------------------------------

pub struct HttpGetTool;

#[async_trait]
impl Tool for HttpGetTool {
    fn id(&self) -> &str {
        "http_get"
    }
    fn description(&self) -> &str {
        "Make an HTTP GET request and return the response body. Has SSRF protection."
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::Utility
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "url".into(),
                    param_type: "string".into(),
                    description: "URL to GET".into(),
                    required: true,
                },
                ToolParam {
                    name: "headers".into(),
                    param_type: "object".into(),
                    description: "Optional headers as key-value pairs".into(),
                    required: false,
                },
            ],
        input_examples: Vec::new(),
    }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
        _p: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::Execution("url required".into()))?;
        validate_url(url)?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| ToolError::Execution(format!("client error: {e}")))?;

        let mut req = client.get(url);
        if let Some(headers) = args.get("headers").and_then(|v| v.as_object()) {
            for (k, v) in headers {
                if let Some(val) = v.as_str() {
                    req = req.header(k.as_str(), val);
                }
            }
        }

        let resp = req
            .send()
            .await
            .map_err(|e| ToolError::Execution(format!("request failed: {e}")))?;
        let status = resp.status().as_u16();
        let body = resp
            .text()
            .await
            .map_err(|e| ToolError::Execution(format!("read body: {e}")))?;

        // Truncate large responses
        let body_display = if body.len() > 8000 {
            format!(
                "{}...\n[truncated, {} bytes total]",
                &body[..8000],
                body.len()
            )
        } else {
            body
        };

        Ok(ToolOutput {
            title: format!("HTTP GET {url} → {status}"),
            output: format!("Status: {status}\n\n{body_display}"),
            metadata: serde_json::json!({"status": status, "url": url}),
            attachments: None,
            llm_suffix: None,
        })
    }
}

// ---------------------------------------------------------------------------
// HttpPostTool
// ---------------------------------------------------------------------------

pub struct HttpPostTool;

#[async_trait]
impl Tool for HttpPostTool {
    fn id(&self) -> &str {
        "http_post"
    }
    fn description(&self) -> &str {
        "Make an HTTP POST request with JSON body. Has SSRF protection."
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::Utility
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "url".into(),
                    param_type: "string".into(),
                    description: "URL to POST".into(),
                    required: true,
                },
                ToolParam {
                    name: "body".into(),
                    param_type: "object".into(),
                    description: "JSON body to send".into(),
                    required: false,
                },
                ToolParam {
                    name: "headers".into(),
                    param_type: "object".into(),
                    description: "Optional headers".into(),
                    required: false,
                },
            ],
        input_examples: Vec::new(),
    }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
        _p: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::Execution("url required".into()))?;
        validate_url(url)?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| ToolError::Execution(format!("client error: {e}")))?;

        let body = args.get("body").cloned().unwrap_or(serde_json::json!({}));
        let mut req = client.post(url).json(&body);

        if let Some(headers) = args.get("headers").and_then(|v| v.as_object()) {
            for (k, v) in headers {
                if let Some(val) = v.as_str() {
                    req = req.header(k.as_str(), val);
                }
            }
        }

        let resp = req
            .send()
            .await
            .map_err(|e| ToolError::Execution(format!("request failed: {e}")))?;
        let status = resp.status().as_u16();
        let resp_body = resp
            .text()
            .await
            .map_err(|e| ToolError::Execution(format!("read body: {e}")))?;

        Ok(ToolOutput {
            title: format!("HTTP POST {url} → {status}"),
            output: format!("Status: {status}\n\n{resp_body}"),
            metadata: serde_json::json!({"status": status, "url": url}),
            attachments: None,
            llm_suffix: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_tools_ids() {
        assert_eq!(HttpGetTool.id(), "http_get");
        assert_eq!(HttpPostTool.id(), "http_post");
    }

    #[test]
    fn ssrf_blocks_private_ips() {
        assert!(validate_url("http://169.254.169.254/latest/meta-data/").is_err());
        assert!(validate_url("http://localhost/admin").is_err());
        assert!(validate_url("http://10.0.0.1/internal").is_err());
        assert!(validate_url("file:///etc/passwd").is_err());
    }

    #[test]
    fn ssrf_allows_public_urls() {
        assert!(validate_url("https://api.github.com/repos").is_ok());
        assert!(validate_url("http://example.com").is_ok());
    }

    #[test]
    fn http_get_requires_url() {
        let schema = HttpGetTool.schema();
        let url_param = schema.params.iter().find(|p| p.name == "url").unwrap();
        assert!(url_param.required);
    }
}
