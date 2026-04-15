//! LLM provider trait — abstraction over different LLM backends.

use async_trait::async_trait;

use crate::error::LlmError;
use crate::stream::SseStream;
use crate::types::ChatRequest;
use crate::types::ChatResponse;

/// Core trait for LLM providers (DIP — runtime depends on abstraction).
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Send a chat request and get a complete response.
    async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError>;

    /// Send a chat request and get a streaming response.
    async fn chat_stream(&self, request: &ChatRequest) -> Result<SseStream, LlmError>;

    /// The model name this provider uses.
    fn model(&self) -> &str;

    /// Provider identifier (e.g., "openai", "anthropic").
    fn provider_id(&self) -> &str;
}

/// Provider registry — stub for provider catalog.
pub mod registry {
    use std::collections::HashMap;

    /// Specification of an LLM provider endpoint.
    #[derive(Debug, Clone)]
    pub struct ProviderSpec {
        pub id: &'static str,
        pub name: &'static str,
        pub display_name: &'static str,
        pub base_url: &'static str,
        pub endpoint_path: &'static str,
        pub api_key_env_var: Option<&'static str>,
    }

    impl ProviderSpec {
        pub fn endpoint_url(&self) -> String {
            format!("{}{}", self.base_url, self.endpoint_path)
        }

        pub fn api_key_env_var(&self) -> Option<&str> {
            self.api_key_env_var
        }
    }

    /// Simple provider registry backed by a HashMap.
    pub struct ProviderRegistry {
        providers: HashMap<&'static str, ProviderSpec>,
    }

    impl ProviderRegistry {
        pub fn get(&self, id: &str) -> Option<&ProviderSpec> {
            self.providers.get(id)
        }

        pub fn ids(&self) -> Vec<&'static str> {
            self.providers.keys().copied().collect()
        }

        pub fn list(&self) -> Vec<&'static str> {
            self.ids()
        }
    }

    /// Create the default provider registry with common providers.
    pub fn create_default_registry() -> ProviderRegistry {
        let mut providers = HashMap::new();

        providers.insert("openai", ProviderSpec {
            id: "openai",
            name: "OpenAI",
            display_name: "OpenAI",
            base_url: "https://api.openai.com",
            endpoint_path: "/v1/chat/completions",
            api_key_env_var: Some("OPENAI_API_KEY"),
        });

        providers.insert("chatgpt-codex", ProviderSpec {
            id: "chatgpt-codex",
            name: "ChatGPT Codex",
            display_name: "ChatGPT Codex",
            base_url: "https://chatgpt.com",
            endpoint_path: "/backend-api/codex/responses",
            api_key_env_var: Some("OPENAI_API_KEY"),
        });

        providers.insert("anthropic", ProviderSpec {
            id: "anthropic",
            name: "Anthropic",
            display_name: "Anthropic",
            base_url: "https://api.anthropic.com",
            endpoint_path: "/v1/messages",
            api_key_env_var: Some("ANTHROPIC_API_KEY"),
        });

        ProviderRegistry { providers }
    }
}
