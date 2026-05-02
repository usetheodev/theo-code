//! Event-based extension system for lifecycle hooks.
//!
//! Extensions can intercept tool calls, modify results, inject context,
//! and transform input. Backward compatible with existing `.theo/hooks/`.
//!
//! Pi-mono ref: `packages/coding-agent/src/core/extensions/index.ts`
//!
//! Reserved-for-future-use extension framework; not yet wired into the runtime.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// Decision from an extension about whether to allow a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub enum ToolCallDecision {
    #[default]
    Allow,
    Block { reason: String },
}


/// Context provided to extension hooks.
#[derive(Debug, Clone)]
pub struct ExtensionContext {
    pub tool_name: String,
    pub tool_args: serde_json::Value,
}

/// Trait for agent lifecycle extensions.
///
/// All methods have default no-op implementations, so extensions
/// only need to override what they care about.
#[async_trait::async_trait]
pub trait Extension: Send + Sync {
    /// Called before the agent starts processing.
    async fn before_agent_start(&self) {}

    /// Called before a tool is executed. Return `Block` to prevent execution.
    async fn on_tool_call(&self, _ctx: &ExtensionContext) -> ToolCallDecision {
        ToolCallDecision::Allow
    }

    /// Called after a tool returns. Can modify the result content.
    async fn on_tool_result(&self, _tool_name: &str, result: String) -> String {
        result
    }

    /// Called before messages are sent to the LLM. Can inject or modify messages.
    async fn on_context_transform(
        &self,
        messages: Vec<theo_infra_llm::types::Message>,
    ) -> Vec<theo_infra_llm::types::Message> {
        messages
    }

    /// Called when the user submits input. Can transform it.
    async fn on_input(&self, input: String) -> String {
        input
    }

    /// Extension name for logging/debugging.
    fn name(&self) -> &str {
        "unnamed"
    }
}

/// Registry of loaded extensions.
pub struct ExtensionRegistry {
    extensions: Vec<Box<dyn Extension>>,
}

impl ExtensionRegistry {
    pub fn new() -> Self {
        Self {
            extensions: Vec::new(),
        }
    }

    pub fn register(&mut self, ext: Box<dyn Extension>) {
        self.extensions.push(ext);
    }

    pub fn len(&self) -> usize {
        self.extensions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.extensions.is_empty()
    }

    /// Run all on_tool_call hooks. First Block wins.
    pub async fn check_tool_call(&self, ctx: &ExtensionContext) -> ToolCallDecision {
        for ext in &self.extensions {
            let decision = ext.on_tool_call(ctx).await;
            if let ToolCallDecision::Block { .. } = &decision {
                return decision;
            }
        }
        ToolCallDecision::Allow
    }

    /// Run all on_tool_result hooks in order (pipeline).
    pub async fn transform_tool_result(&self, tool_name: &str, mut result: String) -> String {
        for ext in &self.extensions {
            result = ext.on_tool_result(tool_name, result).await;
        }
        result
    }

    /// Run all on_context_transform hooks in order (pipeline).
    pub async fn transform_context(
        &self,
        mut messages: Vec<theo_infra_llm::types::Message>,
    ) -> Vec<theo_infra_llm::types::Message> {
        for ext in &self.extensions {
            messages = ext.on_context_transform(messages).await;
        }
        messages
    }
}

impl Default for ExtensionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_call_decision_default_is_allow() {
        let decision = ToolCallDecision::default();
        assert!(matches!(decision, ToolCallDecision::Allow));
    }

    #[test]
    fn test_registry_empty_on_creation() {
        let registry = ExtensionRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_registry_default_is_empty() {
        let registry = ExtensionRegistry::default();
        assert!(registry.is_empty());
    }

    struct NoopExtension;

    #[async_trait::async_trait]
    impl Extension for NoopExtension {
        fn name(&self) -> &str {
            "noop"
        }
    }

    #[test]
    fn test_register_increases_len() {
        let mut registry = ExtensionRegistry::new();
        registry.register(Box::new(NoopExtension));
        assert_eq!(registry.len(), 1);
        assert!(!registry.is_empty());
    }

    #[tokio::test]
    async fn test_check_tool_call_allows_by_default() {
        let registry = ExtensionRegistry::new();
        let ctx = ExtensionContext {
            tool_name: "read".to_string(),
            tool_args: serde_json::Value::Null,
        };
        let decision = registry.check_tool_call(&ctx).await;
        assert!(matches!(decision, ToolCallDecision::Allow));
    }

    struct BlockingExtension;

    #[async_trait::async_trait]
    impl Extension for BlockingExtension {
        fn name(&self) -> &str {
            "blocker"
        }

        async fn on_tool_call(&self, _ctx: &ExtensionContext) -> ToolCallDecision {
            ToolCallDecision::Block {
                reason: "blocked by policy".to_string(),
            }
        }
    }

    #[tokio::test]
    async fn test_check_tool_call_first_block_wins() {
        let mut registry = ExtensionRegistry::new();
        registry.register(Box::new(BlockingExtension));
        registry.register(Box::new(NoopExtension));

        let ctx = ExtensionContext {
            tool_name: "bash".to_string(),
            tool_args: serde_json::Value::Null,
        };
        let decision = registry.check_tool_call(&ctx).await;
        assert!(matches!(decision, ToolCallDecision::Block { .. }));
    }

    struct UpperCaseExtension;

    #[async_trait::async_trait]
    impl Extension for UpperCaseExtension {
        fn name(&self) -> &str {
            "upper"
        }

        async fn on_tool_result(&self, _tool_name: &str, result: String) -> String {
            result.to_uppercase()
        }
    }

    #[tokio::test]
    async fn test_transform_tool_result_pipelines_extensions() {
        let mut registry = ExtensionRegistry::new();
        registry.register(Box::new(UpperCaseExtension));

        let result = registry
            .transform_tool_result("read", "hello world".to_string())
            .await;
        assert_eq!(result, "HELLO WORLD");
    }

    #[tokio::test]
    async fn test_transform_context_with_no_extensions_is_identity() {
        let registry = ExtensionRegistry::new();
        let messages: Vec<theo_infra_llm::types::Message> = vec![];
        let result = registry.transform_context(messages).await;
        assert!(result.is_empty());
    }
}
