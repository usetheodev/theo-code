use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::codex;
use crate::error::LlmError;
use crate::hermes;
use crate::stream::{SseStream, StreamCollector, StreamDelta};
use crate::types::*;

/// Trait for resolving API keys dynamically per-request.
///
/// This supports short-lived OAuth tokens (e.g., GitHub Copilot tokens that
/// expire during long tool execution phases). Called before each LLM request.
///
/// **Pi-mono ref:** `packages/agent/src/types.ts:152-157`
pub trait ApiKeyResolver: Send + Sync {
    /// Resolve the API key for the given provider.
    /// Returns `Some(key)` to override the static key, or `None` to use the default.
    fn resolve(
        &self,
        provider: &str,
    ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + '_>>;
}

/// Client for OpenAI-compatible chat completions API.
///
/// Supports standard `/v1/chat/completions` and custom endpoint overrides
/// (e.g., Codex endpoint at `https://chatgpt.com/backend-api/codex/responses`).
pub struct LlmClient {
    base_url: String,
    api_key: Option<String>,
    model: String,
    /// Override the full endpoint URL (instead of `{base_url}/v1/chat/completions`).
    endpoint_override: Option<String>,
    /// Extra headers sent with every request.
    extra_headers: HashMap<String, String>,
    http: reqwest::Client,
    /// Optional dynamic API key resolver called per-request.
    api_key_resolver: Option<Arc<dyn ApiKeyResolver>>,
}

impl LlmClient {
    pub fn new(
        base_url: impl Into<String>,
        api_key: Option<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key,
            model: model.into(),
            endpoint_override: None,
            extra_headers: HashMap::new(),
            http: reqwest::Client::new(),
            api_key_resolver: None,
        }
    }

    /// Set a dynamic API key resolver called per-request.
    /// The resolver can return `Some(key)` to override the static key,
    /// or `None` to use the default key.
    pub fn with_api_key_resolver(mut self, resolver: Arc<dyn ApiKeyResolver>) -> Self {
        self.api_key_resolver = Some(resolver);
        self
    }

    /// Set a full endpoint URL override.
    /// When set, requests go to this URL instead of `{base_url}/v1/chat/completions`.
    pub fn with_endpoint(mut self, url: impl Into<String>) -> Self {
        self.endpoint_override = Some(url.into());
        self
    }

    /// Add an extra header to send with every request.
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra_headers.insert(key.into(), value.into());
        self
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn url(&self) -> String {
        self.endpoint_override
            .clone()
            .unwrap_or_else(|| format!("{}/v1/chat/completions", self.base_url))
    }

    /// Check if this client is configured to use the Codex endpoint.
    fn is_codex(&self) -> bool {
        self.endpoint_override
            .as_ref()
            .is_some_and(|u| u.contains("codex"))
    }

    /// Resolve the effective API key: dynamic resolver takes precedence over static.
    async fn resolve_api_key(&self) -> Option<String> {
        if let Some(ref resolver) = self.api_key_resolver
            && let Some(key) = resolver.resolve(&self.model).await {
                return Some(key);
            }
        self.api_key.clone()
    }

    fn apply_auth_with_key(
        &self,
        builder: reqwest::RequestBuilder,
        resolved_key: &Option<String>,
    ) -> reqwest::RequestBuilder {
        let mut b = builder;
        if let Some(key) = resolved_key {
            b = b.header("Authorization", format!("Bearer {key}"));
        }
        for (k, v) in &self.extra_headers {
            b = b.header(k.as_str(), v.as_str());
        }
        b
    }

    /// Send a chat completion request (non-streaming).
    ///
    /// Automatically converts to Codex Responses API format when the endpoint
    /// is the Codex endpoint (`chatgpt.com/backend-api/codex/responses`).
    pub async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        let url = self.url();

        if self.is_codex() {
            return self.chat_codex(request, &url).await;
        }

        let resolved_key = self.resolve_api_key().await;
        let builder = self.apply_auth_with_key(self.http.post(&url).json(request), &resolved_key);

        let response = builder.send().await?;
        let status = response.status().as_u16();

        if status >= 400 {
            let body = response.text().await.unwrap_or_default();
            // Use from_status to correctly classify 429/503/504 as retryable
            return Err(LlmError::from_status(status, body));
        }

        let mut chat_response: ChatResponse = response
            .json()
            .await
            .map_err(|e| LlmError::Parse(e.to_string()))?;

        // Hermes fallback: if no tool_calls but content has <function=...>, parse them
        if let Some(choice) = chat_response.choices.first_mut() {
            let has_tool_calls = choice
                .message
                .tool_calls
                .as_ref()
                .is_some_and(|tc| !tc.is_empty());

            if !has_tool_calls
                && let Some(ref content) = choice.message.content {
                    let hermes_calls = hermes::parse_hermes_tool_calls(content);
                    if !hermes_calls.is_empty() {
                        choice.message.tool_calls = Some(hermes_calls);
                    }
                }
        }

        Ok(chat_response)
    }

    /// Send a request to the Codex Responses API endpoint.
    ///
    /// Codex requires `stream: true`, so we read SSE events and collect
    /// the full response from the `response.completed` event.
    async fn chat_codex(&self, request: &ChatRequest, url: &str) -> Result<ChatResponse, LlmError> {
        let body = codex::to_codex_body(request);
        let resolved_key = self.resolve_api_key().await;
        let builder = self.apply_auth_with_key(self.http.post(url).json(&body), &resolved_key);

        let response = builder.send().await?;
        let status = response.status().as_u16();

        if status >= 400 {
            let body = response.text().await.unwrap_or_default();
            return Err(LlmError::from_status(status, body));
        }

        // Read the full SSE stream and collect events
        let full_body = response
            .text()
            .await
            .map_err(|e| LlmError::Parse(format!("read stream: {e}")))?;

        codex::from_codex_stream(&full_body).ok_or_else(|| {
            LlmError::Parse(format!(
                "failed to parse Codex stream response. Body start: {}",
                &full_body[..full_body.len().min(500)]
            ))
        })
    }

    /// Send a streaming chat completion request.
    /// Returns a stream of deltas that can be collected into a full response.
    pub async fn chat_stream(&self, request: &ChatRequest) -> Result<SseStream, LlmError> {
        let url = self.url();
        let mut req = request.clone();
        req.stream = Some(true);
        let resolved_key = self.resolve_api_key().await;
        let builder = self.apply_auth_with_key(self.http.post(&url).json(&req), &resolved_key);

        let response = builder.send().await?;
        let status = response.status().as_u16();

        if status >= 400 {
            let body = response.text().await.unwrap_or_default();
            return Err(LlmError::from_status(status, body));
        }

        Ok(SseStream::new(response.bytes_stream()))
    }

    /// Streaming chat with delta callback.
    ///
    /// Returns the complete ChatResponse after streaming finishes.
    /// Calls `on_delta` for each StreamDelta received — useful for
    /// displaying reasoning/content in real-time.
    ///
    /// Works for both OA-compatible and Codex endpoints.
    pub async fn chat_streaming<F>(
        &self,
        request: &ChatRequest,
        mut on_delta: F,
    ) -> Result<ChatResponse, LlmError>
    where
        F: FnMut(&StreamDelta),
    {
        let url = self.url();

        if self.is_codex() {
            // Codex: real incremental streaming via bytes_stream()
            use futures::StreamExt;

            let body = codex::to_codex_body(request);
            let resolved_key = self.resolve_api_key().await;
            let builder = self.apply_auth_with_key(self.http.post(&url).json(&body), &resolved_key);
            let response = builder.send().await?;
            let status = response.status().as_u16();

            if status >= 400 {
                let body = response.text().await.unwrap_or_default();
                return Err(LlmError::from_status(status, body));
            }

            // Stream bytes incrementally and parse SSE lines as they arrive
            let mut byte_stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut full_body = String::new();

            while let Some(chunk) = byte_stream.next().await {
                let chunk = chunk.map_err(|e| LlmError::Parse(format!("stream chunk: {e}")))?;
                let text = String::from_utf8_lossy(&chunk);
                buffer.push_str(&text);
                full_body.push_str(&text);

                // Process complete lines from buffer
                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].to_string();
                    buffer = buffer[newline_pos + 1..].to_string();

                    // T2.7: bound each SSE chunk to the default 10 MiB limit
                    // before serde allocates.
                    if let Some(data) = line.strip_prefix("data: ")
                        && let Ok(json) = theo_domain::safe_json::from_str_bounded::<serde_json::Value>(
                            data,
                            theo_domain::safe_json::DEFAULT_JSON_LIMIT,
                        ) {
                            let event_type =
                                json.get("type").and_then(|v| v.as_str()).unwrap_or("");

                            match event_type {
                                // Reasoning/thinking deltas (real-time)
                                "response.reasoning.delta" => {
                                    if let Some(text) =
                                        json.pointer("/delta/text").and_then(|v| v.as_str())
                                    {
                                        on_delta(&StreamDelta::Reasoning(text.to_string()));
                                    }
                                }
                                // Content text deltas (real-time)
                                "response.output_text.delta" => {
                                    if let Some(text) =
                                        json.pointer("/delta").and_then(|v| v.as_str())
                                    {
                                        on_delta(&StreamDelta::Content(text.to_string()));
                                    }
                                }
                                // Completion signal
                                "response.completed" => {
                                    on_delta(&StreamDelta::Done);
                                }
                                _ => {}
                            }
                        }
                }
            }

            // Build ChatResponse from the accumulated SSE body
            codex::from_codex_stream(&full_body)
                .ok_or_else(|| LlmError::Parse("failed to parse Codex stream".to_string()))
        } else {
            // OA-compatible: use SseStream
            use futures::StreamExt;

            let mut req = request.clone();
            req.stream = Some(true);
            let resolved_key = self.resolve_api_key().await;
            let builder = self.apply_auth_with_key(self.http.post(&url).json(&req), &resolved_key);
            let response = builder.send().await?;
            let status = response.status().as_u16();

            if status >= 400 {
                let body = response.text().await.unwrap_or_default();
                return Err(LlmError::from_status(status, body));
            }

            let mut stream = SseStream::new(response.bytes_stream());
            let mut collector = StreamCollector::new();

            while let Some(result) = stream.next().await {
                match result {
                    Ok(delta) => {
                        on_delta(&delta);
                        collector.push(&delta);
                        if matches!(delta, StreamDelta::Done) {
                            break;
                        }
                    }
                    Err(e) => return Err(e),
                }
            }

            Ok(collector.finish())
        }
    }

    /// Build a ChatRequest with this client's model pre-filled.
    pub fn request(&self, messages: Vec<Message>) -> ChatRequest {
        ChatRequest::new(&self.model, messages)
    }
}

/// Backward-compatible LlmProvider implementation for LlmClient.
///
/// This allows existing code that uses LlmClient to work with the new
/// trait-based provider system without changes.
#[async_trait::async_trait]
impl crate::provider::LlmProvider for LlmClient {
    async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        LlmClient::chat(self, request).await
    }

    async fn chat_stream(&self, request: &ChatRequest) -> Result<SseStream, LlmError> {
        LlmClient::chat_stream(self, request).await
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn provider_id(&self) -> &str {
        "legacy"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_builder() {
        let client = LlmClient::new("http://localhost:8000", None, "test-model");
        let req = client
            .request(vec![Message::user("hello")])
            .with_max_tokens(1024)
            .with_temperature(0.1);

        assert_eq!(req.model, "test-model");
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.max_tokens, Some(1024));
        assert_eq!(req.temperature, Some(0.1));
    }

    #[test]
    fn test_request_with_tools() {
        let client = LlmClient::new("http://localhost:8000", None, "test-model");
        let tools = vec![ToolDefinition::new(
            "read_file",
            "Read a file",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
        )];

        let req = client
            .request(vec![Message::user("read main.py")])
            .with_tools(tools);

        assert!(req.tools.is_some());
        assert_eq!(req.tools.as_ref().unwrap().len(), 1);
        assert_eq!(req.tool_choice, Some("auto".to_string()));
    }

    #[test]
    fn test_base_url_trailing_slash() {
        let client = LlmClient::new("http://localhost:8000/", None, "m");
        assert_eq!(client.base_url(), "http://localhost:8000");
    }

    #[test]
    fn test_default_url() {
        let client = LlmClient::new("https://api.openai.com", None, "gpt-4o");
        assert_eq!(client.url(), "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn test_endpoint_override() {
        let client = LlmClient::new("https://api.openai.com", None, "gpt-4o")
            .with_endpoint("https://chatgpt.com/backend-api/codex/responses");
        assert_eq!(
            client.url(),
            "https://chatgpt.com/backend-api/codex/responses"
        );
    }

    #[test]
    fn test_extra_headers() {
        let client = LlmClient::new("http://localhost", None, "m")
            .with_header("ChatGPT-Account-Id", "acc_123")
            .with_header("X-Custom", "value");
        assert_eq!(client.extra_headers.len(), 2);
        assert_eq!(client.extra_headers["ChatGPT-Account-Id"], "acc_123");
    }

    #[test]
    fn test_chat_request_serialization() {
        let req = ChatRequest::new("gpt-4", vec![Message::user("hi")]);
        let json = serde_json::to_value(&req).unwrap();

        assert_eq!(json["model"], "gpt-4");
        assert_eq!(json["messages"][0]["role"], "user");
        assert_eq!(json["messages"][0]["content"], "hi");
        assert!(json.get("tools").is_none());
        assert!(json.get("stream").is_none());
    }

    #[test]
    fn test_chat_response_deserialization() {
        let json = serde_json::json!({
            "id": "chatcmpl-123",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello!",
                    "tool_calls": null
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        });

        let resp: ChatResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.content(), Some("Hello!"));
        assert!(resp.tool_calls().is_empty());
        assert_eq!(resp.finish_reason(), Some("stop"));
        assert_eq!(resp.usage.unwrap().total_tokens, 15);
    }

    #[test]
    fn test_chat_response_with_tool_calls() {
        let json = serde_json::json!({
            "id": "chatcmpl-456",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": {
                            "name": "read_file",
                            "arguments": "{\"path\":\"main.py\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": null
        });

        let resp: ChatResponse = serde_json::from_value(json).unwrap();
        assert!(resp.content().is_none());

        let calls = resp.tool_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "read_file");

        let args = calls[0].parse_arguments().unwrap();
        assert_eq!(args["path"], "main.py");
    }

    #[test]
    fn test_tool_result_message() {
        let msg = Message::tool_result("call_abc", "read_file", "file contents here");
        let json = serde_json::to_value(&msg).unwrap();

        assert_eq!(json["role"], "tool");
        assert_eq!(json["tool_call_id"], "call_abc");
        assert_eq!(json["name"], "read_file");
        assert_eq!(json["content"], "file contents here");
    }

    // ── ApiKeyResolver tests ────────────────────────────────────

    struct StaticResolver {
        key: Option<String>,
    }

    impl ApiKeyResolver for StaticResolver {
        fn resolve(
            &self,
            _provider: &str,
        ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + '_>> {
            let key = self.key.clone();
            Box::pin(async move { key })
        }
    }

    #[test]
    fn resolve_api_key_defaults_to_static() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = LlmClient::new("http://localhost", Some("static-key".into()), "m");

        let resolved = rt.block_on(client.resolve_api_key());
        assert_eq!(resolved, Some("static-key".to_string()));
    }

    #[test]
    fn resolve_api_key_resolver_overrides_static() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let resolver = Arc::new(StaticResolver {
            key: Some("dynamic-key".into()),
        });
        let client = LlmClient::new("http://localhost", Some("static-key".into()), "m")
            .with_api_key_resolver(resolver);

        let resolved = rt.block_on(client.resolve_api_key());
        assert_eq!(resolved, Some("dynamic-key".to_string()));
    }

    #[test]
    fn resolve_api_key_resolver_none_falls_back_to_static() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let resolver = Arc::new(StaticResolver { key: None });
        let client = LlmClient::new("http://localhost", Some("static-key".into()), "m")
            .with_api_key_resolver(resolver);
        let resolved = rt.block_on(client.resolve_api_key());
        assert_eq!(resolved, Some("static-key".to_string()));
    }

    #[test]
    fn resolve_api_key_no_resolver_no_static_returns_none() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = LlmClient::new("http://localhost", None, "m");
        let resolved = rt.block_on(client.resolve_api_key());
        assert_eq!(resolved, None);
    }
}