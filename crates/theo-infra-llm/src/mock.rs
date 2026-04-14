//! Mock LLM provider for testing.
//!
//! Implements the `LlmProvider` trait with configurable responses.
//! For use in unit tests that need to exercise the agent loop
//! without making real HTTP calls.

use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use futures::stream;

use crate::error::LlmError;
use crate::provider::LlmProvider;
use crate::stream::SseStream;
use crate::types::*;

/// A mock LLM provider that returns pre-configured responses.
///
/// # Example
/// ```ignore
/// let mock = MockLlmProvider::new("test-model")
///     .with_response(ChatResponse { ... });
/// let response = mock.chat(&request).await.unwrap();
/// ```
pub struct MockLlmProvider {
    model: String,
    provider_id: String,
    responses: Mutex<VecDeque<Result<ChatResponse, LlmError>>>,
}

impl MockLlmProvider {
    /// Create a mock provider with a model name.
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            provider_id: "mock".to_string(),
            responses: Mutex::new(VecDeque::new()),
        }
    }

    /// Queue a successful response that will be returned by the next chat() call.
    pub fn with_response(self, response: ChatResponse) -> Self {
        self.responses.lock().unwrap().push_back(Ok(response));
        self
    }

    /// Queue an error that will be returned by the next chat() call.
    pub fn with_error(self, error: LlmError) -> Self {
        self.responses.lock().unwrap().push_back(Err(error));
        self
    }

    /// Queue a simple text response (convenience).
    pub fn with_text_response(self, text: impl Into<String>) -> Self {
        self.with_response(ChatResponse {
            id: Some("mock-resp".to_string()),
            choices: vec![Choice {
                index: 0,
                message: ChoiceMessage {
                    role: Role::Assistant,
                    content: Some(text.into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
        })
    }

    /// Queue a response with a tool call (convenience for testing done()).
    pub fn with_tool_call(self, tool_name: &str, tool_id: &str, arguments: &str) -> Self {
        self.with_response(ChatResponse {
            id: Some("mock-resp".to_string()),
            choices: vec![Choice {
                index: 0,
                message: ChoiceMessage {
                    role: Role::Assistant,
                    content: None,
                    tool_calls: Some(vec![ToolCall::new(
                        tool_id.to_string(),
                        tool_name,
                        arguments,
                    )]),
                },
                finish_reason: Some("tool_calls".to_string()),
            }],
            usage: Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
        })
    }

    /// Get the number of remaining queued responses.
    pub fn remaining_responses(&self) -> usize {
        self.responses.lock().unwrap().len()
    }
}

#[async_trait]
impl LlmProvider for MockLlmProvider {
    async fn chat(&self, _request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        let mut queue = self.responses.lock().unwrap();
        queue.pop_front().unwrap_or_else(|| {
            // Default: return a simple "I'm done" text response
            Ok(ChatResponse {
                id: Some("mock-default".to_string()),
                choices: vec![Choice {
                    index: 0,
                    message: ChoiceMessage {
                        role: Role::Assistant,
                        content: Some("Mock response (no more queued responses)".to_string()),
                        tool_calls: None,
                    },
                    finish_reason: Some("stop".to_string()),
                }],
                usage: Some(Usage {
                    prompt_tokens: 10,
                    completion_tokens: 5,
                    total_tokens: 15,
                }),
            })
        })
    }

    async fn chat_stream(&self, _request: &ChatRequest) -> Result<SseStream, LlmError> {
        // Return an empty stream that immediately terminates.
        // For real streaming tests, a more sophisticated mock would be needed.
        let empty = stream::empty::<Result<bytes::Bytes, reqwest::Error>>();
        Ok(SseStream::new(empty))
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn provider_id(&self) -> &str {
        &self.provider_id
    }
}

// ---------------------------------------------------------------------------
// FauxProvider — Enhanced faux provider with realistic behavior
// ---------------------------------------------------------------------------

/// Enhanced faux provider with realistic streaming simulation.
///
/// Unlike `MockLlmProvider` which is a simple queue, `FauxProvider` adds:
/// - Token estimation from content length (~1 token per 4 chars)
/// - Configurable streaming speed (tokens/second)
/// - Convenience methods for common response types
///
/// Pi-mono ref: `packages/ai/src/providers/faux.ts`
pub struct FauxProvider {
    responses: Mutex<Vec<ChatResponse>>,
    /// Simulated tokens per second for streaming.
    tokens_per_second: u32,
    model: String,
}

impl FauxProvider {
    /// Create a new faux provider with the given model name.
    /// Default streaming speed: 100 tokens/second.
    pub fn new(model: &str) -> Self {
        Self {
            responses: Mutex::new(Vec::new()),
            tokens_per_second: 100,
            model: model.to_string(),
        }
    }

    /// Configure streaming speed (tokens/second). Default: 100.
    pub fn with_speed(mut self, tps: u32) -> Self {
        self.tokens_per_second = tps;
        self
    }

    /// Queue a response (FIFO — first pushed is first returned).
    pub fn push_response(&self, response: ChatResponse) {
        self.responses.lock().unwrap().push(response);
    }

    /// Queue a simple text response with estimated token usage.
    pub fn push_text(&self, text: &str) {
        let completion_tokens = Self::estimate_tokens(text);
        let prompt_tokens = 10; // Fixed estimate for faux prompt overhead
        self.push_response(ChatResponse {
            id: Some(format!("faux-{}", self.remaining())),
            choices: vec![Choice {
                index: 0,
                message: ChoiceMessage {
                    role: Role::Assistant,
                    content: Some(text.to_string()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(Usage {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
            }),
        });
    }

    /// Queue a tool call response with estimated token usage.
    pub fn push_tool_call(&self, tool_name: &str, args: &str) {
        let completion_tokens = Self::estimate_tokens(tool_name) + Self::estimate_tokens(args);
        let prompt_tokens = 10;
        self.push_response(ChatResponse {
            id: Some(format!("faux-tc-{}", self.remaining())),
            choices: vec![Choice {
                index: 0,
                message: ChoiceMessage {
                    role: Role::Assistant,
                    content: None,
                    tool_calls: Some(vec![ToolCall::new(
                        format!("faux_call_{}", self.remaining()),
                        tool_name,
                        args,
                    )]),
                },
                finish_reason: Some("tool_calls".to_string()),
            }],
            usage: Some(Usage {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
            }),
        });
    }

    /// Estimate token count from text (~1 token per 4 chars).
    fn estimate_tokens(text: &str) -> u32 {
        let len = text.len() as u32;
        // Minimum 1 token for non-empty text
        if len == 0 { 0 } else { (len / 4).max(1) }
    }

    /// Number of queued responses remaining.
    pub fn remaining(&self) -> usize {
        self.responses.lock().unwrap().len()
    }

    /// Configured tokens per second for streaming simulation.
    pub fn tokens_per_second(&self) -> u32 {
        self.tokens_per_second
    }
}

#[async_trait]
impl LlmProvider for FauxProvider {
    async fn chat(&self, _request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        let mut queue = self.responses.lock().unwrap();
        if queue.is_empty() {
            // Default response when queue is exhausted
            return Ok(ChatResponse {
                id: Some("faux-default".to_string()),
                choices: vec![Choice {
                    index: 0,
                    message: ChoiceMessage {
                        role: Role::Assistant,
                        content: Some("Faux response (queue empty)".to_string()),
                        tool_calls: None,
                    },
                    finish_reason: Some("stop".to_string()),
                }],
                usage: Some(Usage {
                    prompt_tokens: 10,
                    completion_tokens: 7,
                    total_tokens: 17,
                }),
            });
        }
        // FIFO: remove first element
        Ok(queue.remove(0))
    }

    /// Streaming is not fully supported — `SseStream` requires a `reqwest` byte stream
    /// which cannot be faked without a real HTTP connection.
    /// Falls back to `chat()` wrapped in an empty stream.
    /// TODO: implement a proper faux stream when SseStream supports generic byte sources.
    async fn chat_stream(&self, _request: &ChatRequest) -> Result<SseStream, LlmError> {
        // Limitation: SseStream requires reqwest::Error in the Item type,
        // so we cannot easily inject synthetic SSE bytes without a real HTTP layer.
        // Return an empty stream for now.
        let empty = stream::empty::<Result<bytes::Bytes, reqwest::Error>>();
        Ok(SseStream::new(empty))
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn provider_id(&self) -> &str {
        "faux"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_returns_configured_text_response() {
        let mock = MockLlmProvider::new("test-model").with_text_response("Hello from mock!");

        let request = ChatRequest::new("test-model", vec![Message::user("hi")]);
        let response = mock.chat(&request).await.unwrap();

        assert_eq!(response.choices.len(), 1);
        assert_eq!(
            response.choices[0].message.content.as_deref(),
            Some("Hello from mock!")
        );
    }

    #[tokio::test]
    async fn mock_returns_tool_call_response() {
        let mock = MockLlmProvider::new("test-model").with_tool_call(
            "done",
            "call_1",
            r#"{"summary":"task complete"}"#,
        );

        let request = ChatRequest::new("test-model", vec![Message::user("do it")]);
        let response = mock.chat(&request).await.unwrap();

        let tool_calls = response.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].function.name, "done");
    }

    #[tokio::test]
    async fn mock_returns_error() {
        let mock = MockLlmProvider::new("test-model").with_error(LlmError::Api {
            status: 429,
            message: "rate limited".to_string(),
        });

        let request = ChatRequest::new("test-model", vec![Message::user("hi")]);
        let result = mock.chat(&request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn mock_model_and_provider_id() {
        let mock = MockLlmProvider::new("gpt-4o");
        assert_eq!(mock.model(), "gpt-4o");
        assert_eq!(mock.provider_id(), "mock");
    }

    #[tokio::test]
    async fn mock_responses_consumed_in_order() {
        let mock = MockLlmProvider::new("test")
            .with_text_response("first")
            .with_text_response("second");

        let request = ChatRequest::new("test", vec![Message::user("q")]);

        let r1 = mock.chat(&request).await.unwrap();
        assert_eq!(r1.choices[0].message.content.as_deref(), Some("first"));

        let r2 = mock.chat(&request).await.unwrap();
        assert_eq!(r2.choices[0].message.content.as_deref(), Some("second"));

        // After queue exhausted, returns default
        let r3 = mock.chat(&request).await.unwrap();
        assert!(
            r3.choices[0]
                .message
                .content
                .as_deref()
                .unwrap()
                .contains("no more queued")
        );
    }

    #[tokio::test]
    async fn mock_chat_stream_returns_empty_stream() {
        use futures::StreamExt;

        let mock = MockLlmProvider::new("test");
        let request = ChatRequest::new("test", vec![Message::user("hi")]);
        let mut stream = mock.chat_stream(&request).await.unwrap();

        // Stream should be empty (no items)
        let next = stream.next().await;
        assert!(
            next.is_none(),
            "Empty mock stream should return None immediately"
        );
    }

    #[tokio::test]
    async fn mock_remaining_responses_tracks_queue() {
        let mock = MockLlmProvider::new("test")
            .with_text_response("a")
            .with_text_response("b");

        assert_eq!(mock.remaining_responses(), 2);

        let request = ChatRequest::new("test", vec![Message::user("q")]);
        let _ = mock.chat(&request).await;
        assert_eq!(mock.remaining_responses(), 1);
    }

    // -----------------------------------------------------------------------
    // FauxProvider tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn faux_push_text_and_pop_via_chat() {
        // Arrange
        let faux = FauxProvider::new("faux-model");
        faux.push_text("Hello from faux!");
        let request = ChatRequest::new("faux-model", vec![Message::user("hi")]);

        // Act
        let response = faux.chat(&request).await.unwrap();

        // Assert
        assert_eq!(
            response.choices[0].message.content.as_deref(),
            Some("Hello from faux!")
        );
        assert_eq!(response.choices[0].message.tool_calls, None);
    }

    #[tokio::test]
    async fn faux_push_tool_call_returns_tool_call_response() {
        // Arrange
        let faux = FauxProvider::new("faux-model");
        faux.push_tool_call("read_file", r#"{"path":"main.rs"}"#);
        let request = ChatRequest::new("faux-model", vec![Message::user("read it")]);

        // Act
        let response = faux.chat(&request).await.unwrap();

        // Assert
        let tool_calls = response.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].function.name, "read_file");
        assert_eq!(tool_calls[0].function.arguments, r#"{"path":"main.rs"}"#);
        assert_eq!(
            response.choices[0].finish_reason.as_deref(),
            Some("tool_calls")
        );
    }

    #[tokio::test]
    async fn faux_empty_queue_returns_default_response() {
        // Arrange
        let faux = FauxProvider::new("faux-model");
        let request = ChatRequest::new("faux-model", vec![Message::user("hi")]);

        // Act
        let response = faux.chat(&request).await.unwrap();

        // Assert
        let content = response.choices[0].message.content.as_deref().unwrap();
        assert!(
            content.contains("queue empty"),
            "Expected default message, got: {content}"
        );
    }

    #[tokio::test]
    async fn faux_usage_estimated_from_content_length() {
        // Arrange
        let faux = FauxProvider::new("faux-model");
        // "Hello world!!" = 13 chars → 13/4 = 3 tokens
        faux.push_text("Hello world!!");
        let request = ChatRequest::new("faux-model", vec![Message::user("hi")]);

        // Act
        let response = faux.chat(&request).await.unwrap();

        // Assert
        let usage = response.usage.as_ref().unwrap();
        assert_eq!(usage.completion_tokens, 3); // 13 / 4 = 3
        assert_eq!(usage.prompt_tokens, 10); // fixed faux overhead
        assert_eq!(usage.total_tokens, 13); // 10 + 3
    }

    #[tokio::test]
    async fn faux_remaining_count_decrements() {
        // Arrange
        let faux = FauxProvider::new("faux-model");
        faux.push_text("first");
        faux.push_text("second");
        faux.push_text("third");
        assert_eq!(faux.remaining(), 3);

        let request = ChatRequest::new("faux-model", vec![Message::user("go")]);

        // Act
        let _ = faux.chat(&request).await.unwrap();

        // Assert
        assert_eq!(faux.remaining(), 2);

        let _ = faux.chat(&request).await.unwrap();
        assert_eq!(faux.remaining(), 1);

        let _ = faux.chat(&request).await.unwrap();
        assert_eq!(faux.remaining(), 0);
    }

    #[tokio::test]
    async fn faux_model_and_provider_id() {
        let faux = FauxProvider::new("claude-faux");
        assert_eq!(faux.model(), "claude-faux");
        assert_eq!(faux.provider_id(), "faux");
    }

    #[tokio::test]
    async fn faux_with_speed_configures_tps() {
        let faux = FauxProvider::new("test").with_speed(50);
        assert_eq!(faux.tokens_per_second(), 50);
    }

    #[tokio::test]
    async fn faux_responses_consumed_fifo_order() {
        // Arrange
        let faux = FauxProvider::new("test");
        faux.push_text("first");
        faux.push_text("second");
        let request = ChatRequest::new("test", vec![Message::user("go")]);

        // Act & Assert
        let r1 = faux.chat(&request).await.unwrap();
        assert_eq!(r1.choices[0].message.content.as_deref(), Some("first"));

        let r2 = faux.chat(&request).await.unwrap();
        assert_eq!(r2.choices[0].message.content.as_deref(), Some("second"));
    }

    #[test]
    fn faux_estimate_tokens_empty_string() {
        assert_eq!(FauxProvider::estimate_tokens(""), 0);
    }

    #[test]
    fn faux_estimate_tokens_short_string() {
        // "hi" = 2 chars → 2/4 = 0, but min is 1
        assert_eq!(FauxProvider::estimate_tokens("hi"), 1);
    }

    #[test]
    fn faux_estimate_tokens_longer_string() {
        // "Hello World!" = 12 chars → 12/4 = 3
        assert_eq!(FauxProvider::estimate_tokens("Hello World!"), 3);
    }
}
