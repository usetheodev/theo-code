use crate::error::LlmError;
use crate::hermes;
use crate::types::*;
use futures::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};

/// A delta chunk from the SSE stream.
#[derive(Debug, Clone)]
pub enum StreamDelta {
    /// A text content chunk.
    Content(String),
    /// A reasoning/thinking chunk from the LLM's internal reasoning.
    Reasoning(String),
    /// A partial tool call update.
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments: Option<String>,
    },
    /// Stream finished.
    Done,
}

/// Collects streaming deltas into a complete ChatResponse.
#[derive(Debug, Default)]
pub struct StreamCollector {
    content: String,
    tool_calls: Vec<PartialToolCall>,
    finish_reason: Option<String>,
}

#[derive(Debug, Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

impl StreamCollector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a delta into the collector.
    pub fn push(&mut self, delta: &StreamDelta) {
        match delta {
            StreamDelta::Content(text) => self.content.push_str(text),
            StreamDelta::Reasoning(_) => {
                // Reasoning is observed/displayed but not accumulated into the response content.
                // The LLM's reasoning is internal — the final content/tool_calls are what matter.
            }
            StreamDelta::ToolCallDelta {
                index,
                id,
                name,
                arguments,
            } => {
                while self.tool_calls.len() <= *index {
                    self.tool_calls.push(PartialToolCall::default());
                }
                let tc = &mut self.tool_calls[*index];
                if let Some(id) = id {
                    tc.id.push_str(id);
                }
                if let Some(name) = name {
                    tc.name.push_str(name);
                }
                if let Some(args) = arguments {
                    tc.arguments.push_str(args);
                }
            }
            StreamDelta::Done => {}
        }
    }

    /// Build the final ChatResponse from accumulated deltas.
    pub fn finish(mut self) -> ChatResponse {
        let mut tool_calls: Vec<ToolCall> = self
            .tool_calls
            .into_iter()
            .filter(|tc| !tc.name.is_empty())
            .map(|tc| ToolCall::new(tc.id, tc.name, tc.arguments))
            .collect();

        // Hermes fallback: check content for XML tool calls
        if tool_calls.is_empty() && !self.content.is_empty() {
            let hermes = hermes::parse_hermes_tool_calls(&self.content);
            if !hermes.is_empty() {
                tool_calls = hermes;
            }
        }

        let content = if self.content.is_empty() {
            None
        } else {
            Some(self.content)
        };

        let tool_calls_opt = if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        };

        if self.finish_reason.is_none() {
            self.finish_reason = if tool_calls_opt.is_some() {
                Some("tool_calls".to_string())
            } else {
                Some("stop".to_string())
            };
        }

        ChatResponse {
            id: None,
            choices: vec![Choice {
                index: 0,
                message: ChoiceMessage {
                    role: Role::Assistant,
                    content,
                    tool_calls: tool_calls_opt,
                },
                finish_reason: self.finish_reason,
            }],
            usage: None,
        }
    }
}

/// Parse a single SSE data line into a StreamDelta.
///
/// OpenAI SSE format:
/// ```text
/// data: {"choices":[{"delta":{"content":"Hello"}}]}
/// data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read","arguments":""}}]}}]}
/// data: [DONE]
/// ```
pub fn parse_sse_line(line: &str) -> Option<StreamDelta> {
    let data = line.strip_prefix("data: ")?;

    if data == "[DONE]" {
        return Some(StreamDelta::Done);
    }

    let json: serde_json::Value = serde_json::from_str(data).ok()?;
    let delta = json
        .get("choices")?
        .get(0)?
        .get("delta")?;

    // Check for reasoning/thinking (OpenAI extended thinking)
    if let Some(reasoning) = delta.get("reasoning").and_then(|r| r.as_str()) {
        if !reasoning.is_empty() {
            return Some(StreamDelta::Reasoning(reasoning.to_string()));
        }
    }

    // Check for content
    if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
        if !content.is_empty() {
            return Some(StreamDelta::Content(content.to_string()));
        }
    }

    // Check for tool calls
    if let Some(tool_calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
        if let Some(tc) = tool_calls.first() {
            let index = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
            let id = tc.get("id").and_then(|i| i.as_str()).map(String::from);
            let name = tc
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())
                .map(String::from);
            let arguments = tc
                .get("function")
                .and_then(|f| f.get("arguments"))
                .and_then(|a| a.as_str())
                .map(String::from);

            return Some(StreamDelta::ToolCallDelta {
                index,
                id,
                name,
                arguments,
            });
        }
    }

    None
}

/// Stream of deltas from an SSE response body.
pub struct SseStream {
    inner: Pin<Box<dyn Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>,
    buffer: String,
}

impl SseStream {
    pub fn new(byte_stream: impl Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static) -> Self {
        Self {
            inner: Box::pin(byte_stream),
            buffer: String::new(),
        }
    }
}

impl Stream for SseStream {
    type Item = Result<StreamDelta, LlmError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            // Check if buffer has a complete line
            if let Some(newline_pos) = self.buffer.find('\n') {
                let line = self.buffer[..newline_pos].trim().to_string();
                self.buffer = self.buffer[newline_pos + 1..].to_string();

                if line.is_empty() {
                    continue;
                }

                if let Some(delta) = parse_sse_line(&line) {
                    return Poll::Ready(Some(Ok(delta)));
                }
                continue;
            }

            // Need more data
            match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    let text = String::from_utf8_lossy(&bytes);
                    self.buffer.push_str(&text);
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(LlmError::Network(e))));
                }
                Poll::Ready(None) => {
                    // Stream ended
                    if self.buffer.trim().is_empty() {
                        return Poll::Ready(None);
                    }
                    // Process remaining buffer
                    let remaining = std::mem::take(&mut self.buffer);
                    for line in remaining.lines() {
                        let line = line.trim();
                        if !line.is_empty() {
                            if let Some(delta) = parse_sse_line(line) {
                                return Poll::Ready(Some(Ok(delta)));
                            }
                        }
                    }
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_content_delta() {
        let line = r#"data: {"choices":[{"delta":{"content":"Hello"}}]}"#;
        match parse_sse_line(line) {
            Some(StreamDelta::Content(text)) => assert_eq!(text, "Hello"),
            other => panic!("Expected Content, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_tool_call_delta() {
        let line = r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read_file","arguments":""}}]}}]}"#;
        match parse_sse_line(line) {
            Some(StreamDelta::ToolCallDelta {
                index,
                id,
                name,
                arguments,
            }) => {
                assert_eq!(index, 0);
                assert_eq!(id, Some("call_1".to_string()));
                assert_eq!(name, Some("read_file".to_string()));
                assert_eq!(arguments, Some(String::new()));
            }
            other => panic!("Expected ToolCallDelta, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_done() {
        let line = "data: [DONE]";
        assert!(matches!(parse_sse_line(line), Some(StreamDelta::Done)));
    }

    #[test]
    fn test_parse_non_data_line() {
        assert!(parse_sse_line("event: message").is_none());
        assert!(parse_sse_line("").is_none());
        assert!(parse_sse_line(": comment").is_none());
    }

    #[test]
    fn test_collector_text_only() {
        let mut collector = StreamCollector::new();
        collector.push(&StreamDelta::Content("Hello ".to_string()));
        collector.push(&StreamDelta::Content("world!".to_string()));
        collector.push(&StreamDelta::Done);

        let response = collector.finish();
        assert_eq!(response.content(), Some("Hello world!"));
        assert!(response.tool_calls().is_empty());
    }

    #[test]
    fn test_collector_tool_calls() {
        let mut collector = StreamCollector::new();
        collector.push(&StreamDelta::ToolCallDelta {
            index: 0,
            id: Some("call_1".to_string()),
            name: Some("read".to_string()),
            arguments: None,
        });
        collector.push(&StreamDelta::ToolCallDelta {
            index: 0,
            id: None,
            name: Some("_file".to_string()),
            arguments: Some("{\"pa".to_string()),
        });
        collector.push(&StreamDelta::ToolCallDelta {
            index: 0,
            id: None,
            name: None,
            arguments: Some("th\":\"a.py\"}".to_string()),
        });
        collector.push(&StreamDelta::Done);

        let response = collector.finish();
        let calls = response.tool_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "read_file");
        assert_eq!(calls[0].function.arguments, "{\"path\":\"a.py\"}");
    }

    #[test]
    fn test_collector_hermes_fallback() {
        let mut collector = StreamCollector::new();
        collector.push(&StreamDelta::Content(
            "<function=read_file>\n<parameter=path>main.py</parameter>\n</function>".to_string(),
        ));
        collector.push(&StreamDelta::Done);

        let response = collector.finish();
        let calls = response.tool_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "read_file");
    }
}
