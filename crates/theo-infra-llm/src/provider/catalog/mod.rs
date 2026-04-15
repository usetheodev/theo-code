//! Built-in provider catalog — const ProviderSpecs for all supported providers.

pub mod anthropic;
pub mod cloud;
pub mod local;
pub mod openai;

use super::spec::ProviderSpec;

/// Return all built-in provider specs.
pub fn built_in_providers() -> Vec<ProviderSpec> {
    vec![
        // Tier 1: OA-Compatible
        openai::OPENAI,
        openai::OPENROUTER,
        openai::XAI,
        openai::MISTRAL,
        openai::GROQ,
        openai::DEEPINFRA,
        openai::CEREBRAS,
        openai::COHERE,
        openai::TOGETHERAI,
        openai::PERPLEXITY,
        openai::VERCEL,
        openai::CHATGPT_CODEX,
        // Tier 2: Non-OA with existing converter
        anthropic::ANTHROPIC,
        // Tier 3: OA-Compatible with special auth/headers
        cloud::AZURE,
        cloud::AZURE_COGNITIVE,
        cloud::GITHUB_COPILOT,
        cloud::GITLAB,
        cloud::CLOUDFLARE_WORKERS,
        cloud::CLOUDFLARE_GATEWAY,
        cloud::SAP_AI_CORE,
        // Tier 4: Cloud with complex auth (stubs — feature-gated in Phase 4)
        cloud::AMAZON_BEDROCK,
        cloud::GOOGLE_VERTEX,
        cloud::GOOGLE_VERTEX_ANTHROPIC,
        // Tier 5: Local models
        local::OLLAMA,
        local::VLLM,
        local::LM_STUDIO,
    ]
}

/// Count of built-in providers.
pub fn provider_count() -> usize {
    built_in_providers().len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_has_expected_count() {
        let providers = built_in_providers();
        assert!(
            providers.len() >= 25,
            "Expected 25+ providers, got {}",
            providers.len()
        );
    }

    #[test]
    fn all_providers_have_unique_ids() {
        let providers = built_in_providers();
        let mut ids: Vec<&str> = providers.iter().map(|p| p.id).collect();
        let total = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), total, "Duplicate provider IDs found");
    }

    #[test]
    fn all_providers_have_nonempty_fields() {
        for p in built_in_providers() {
            assert!(!p.id.is_empty(), "Provider has empty id");
            assert!(
                !p.display_name.is_empty(),
                "Provider {} has empty display_name",
                p.id
            );
            assert!(
                !p.base_url.is_empty(),
                "Provider {} has empty base_url",
                p.id
            );
            assert!(
                !p.chat_path.is_empty(),
                "Provider {} has empty chat_path",
                p.id
            );
        }
    }

    #[test]
    fn endpoint_urls_are_valid() {
        for p in built_in_providers() {
            let url = p.endpoint_url();
            assert!(
                url.starts_with("http://") || url.starts_with("https://"),
                "Provider {} has invalid URL: {url}",
                p.id
            );
        }
    }
}
