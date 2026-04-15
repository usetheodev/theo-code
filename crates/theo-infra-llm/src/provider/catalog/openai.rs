//! OpenAI and OpenAI-compatible provider specs.
//! Each is a const — zero code, just config.

use crate::provider::spec::*;

pub const OPENAI: ProviderSpec = ProviderSpec {
    id: "openai",
    display_name: "OpenAI",
    base_url: "https://api.openai.com",
    chat_path: "/v1/chat/completions",
    format: FormatKind::OaCompatible,
    auth: AuthKind::BearerFromEnv("OPENAI_API_KEY"),
    default_headers: &[],
    supports_streaming: true,
    hermes_fallback: false,
};

pub const OPENROUTER: ProviderSpec = ProviderSpec {
    id: "openrouter",
    display_name: "OpenRouter",
    base_url: "https://openrouter.ai/api",
    chat_path: "/v1/chat/completions",
    format: FormatKind::OaCompatible,
    auth: AuthKind::BearerFromEnv("OPENROUTER_API_KEY"),
    default_headers: &[],
    supports_streaming: true,
    hermes_fallback: false,
};

pub const XAI: ProviderSpec = ProviderSpec {
    id: "xai",
    display_name: "xAI (Grok)",
    base_url: "https://api.x.ai",
    chat_path: "/v1/chat/completions",
    format: FormatKind::OaCompatible,
    auth: AuthKind::BearerFromEnv("XAI_API_KEY"),
    default_headers: &[],
    supports_streaming: true,
    hermes_fallback: false,
};

pub const MISTRAL: ProviderSpec = ProviderSpec {
    id: "mistral",
    display_name: "Mistral AI",
    base_url: "https://api.mistral.ai",
    chat_path: "/v1/chat/completions",
    format: FormatKind::OaCompatible,
    auth: AuthKind::BearerFromEnv("MISTRAL_API_KEY"),
    default_headers: &[],
    supports_streaming: true,
    hermes_fallback: false,
};

pub const GROQ: ProviderSpec = ProviderSpec {
    id: "groq",
    display_name: "Groq",
    base_url: "https://api.groq.com/openai",
    chat_path: "/v1/chat/completions",
    format: FormatKind::OaCompatible,
    auth: AuthKind::BearerFromEnv("GROQ_API_KEY"),
    default_headers: &[],
    supports_streaming: true,
    hermes_fallback: false,
};

pub const DEEPINFRA: ProviderSpec = ProviderSpec {
    id: "deepinfra",
    display_name: "DeepInfra",
    base_url: "https://api.deepinfra.com",
    chat_path: "/v1/openai/chat/completions",
    format: FormatKind::OaCompatible,
    auth: AuthKind::BearerFromEnv("DEEPINFRA_API_KEY"),
    default_headers: &[],
    supports_streaming: true,
    hermes_fallback: false,
};

pub const CEREBRAS: ProviderSpec = ProviderSpec {
    id: "cerebras",
    display_name: "Cerebras",
    base_url: "https://api.cerebras.ai",
    chat_path: "/v1/chat/completions",
    format: FormatKind::OaCompatible,
    auth: AuthKind::BearerFromEnv("CEREBRAS_API_KEY"),
    default_headers: &[],
    supports_streaming: true,
    hermes_fallback: false,
};

pub const COHERE: ProviderSpec = ProviderSpec {
    id: "cohere",
    display_name: "Cohere",
    base_url: "https://api.cohere.com/compatibility",
    chat_path: "/v1/chat/completions",
    format: FormatKind::OaCompatible,
    auth: AuthKind::BearerFromEnv("COHERE_API_KEY"),
    default_headers: &[],
    supports_streaming: true,
    hermes_fallback: false,
};

pub const TOGETHERAI: ProviderSpec = ProviderSpec {
    id: "togetherai",
    display_name: "Together AI",
    base_url: "https://api.together.xyz",
    chat_path: "/v1/chat/completions",
    format: FormatKind::OaCompatible,
    auth: AuthKind::BearerFromEnv("TOGETHER_API_KEY"),
    default_headers: &[],
    supports_streaming: true,
    hermes_fallback: false,
};

pub const PERPLEXITY: ProviderSpec = ProviderSpec {
    id: "perplexity",
    display_name: "Perplexity",
    base_url: "https://api.perplexity.ai",
    chat_path: "/chat/completions",
    format: FormatKind::OaCompatible,
    auth: AuthKind::BearerFromEnv("PERPLEXITY_API_KEY"),
    default_headers: &[],
    supports_streaming: true,
    hermes_fallback: false,
};

pub const VERCEL: ProviderSpec = ProviderSpec {
    id: "vercel",
    display_name: "Vercel AI",
    base_url: "https://api.vercel.ai",
    chat_path: "/v1/chat/completions",
    format: FormatKind::OaCompatible,
    auth: AuthKind::BearerFromEnv("VERCEL_API_KEY"),
    default_headers: &[],
    supports_streaming: true,
    hermes_fallback: false,
};

/// ChatGPT Codex — used with OAuth tokens from auth.openai.com.
///
/// AuthKind::None because the OAuth token is not from an env var —
/// it's passed as `api_key_override` to `SpecBasedProvider::new()` at runtime.
/// The `ChatGPT-Account-Id` header is also dynamic (from OAuth store)
/// and must be added via `config.extra_headers` by the caller.
pub const CHATGPT_CODEX: ProviderSpec = ProviderSpec {
    id: "chatgpt-codex",
    display_name: "ChatGPT Codex (OAuth)",
    base_url: "https://chatgpt.com",
    chat_path: "/backend-api/codex/responses",
    format: FormatKind::OpenAiResponses,
    auth: AuthKind::None,
    default_headers: &[],
    supports_streaming: true,
    hermes_fallback: false,
};
