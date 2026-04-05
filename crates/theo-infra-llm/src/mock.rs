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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_returns_configured_text_response() {
        let mock = MockLlmProvider::new("test-model")
            .with_text_response("Hello from mock!");

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
        let mock = MockLlmProvider::new("test-model")
            .with_tool_call("done", "call_1", r#"{"summary":"task complete"}"#);

        let request = ChatRequest::new("test-model", vec![Message::user("do it")]);
        let response = mock.chat(&request).await.unwrap();

        let tool_calls = response.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].function.name, "done");
    }

    #[tokio::test]
    async fn mock_returns_error() {
        let mock = MockLlmProvider::new("test-model")
            .with_error(LlmError::Api {
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
        assert!(r3.choices[0].message.content.as_deref().unwrap().contains("no more queued"));
    }

    #[tokio::test]
    async fn mock_chat_stream_returns_empty_stream() {
        use futures::StreamExt;

        let mock = MockLlmProvider::new("test");
        let request = ChatRequest::new("test", vec![Message::user("hi")]);
        let mut stream = mock.chat_stream(&request).await.unwrap();

        // Stream should be empty (no items)
        let next = stream.next().await;
        assert!(next.is_none(), "Empty mock stream should return None immediately");
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
}
