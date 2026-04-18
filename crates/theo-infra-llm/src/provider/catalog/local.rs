//! Local model provider specs — Ollama, vLLM, LM Studio.

use crate::provider::spec::*;

pub const OLLAMA: ProviderSpec = ProviderSpec {
    id: "ollama",
    display_name: "Ollama",
    base_url: "http://localhost:11434",
    chat_path: "/v1/chat/completions",
    format: FormatKind::OaCompatible,
    auth: AuthKind::None,
    default_headers: &[],
    supports_streaming: true,
    hermes_fallback: true,
};

pub const VLLM: ProviderSpec = ProviderSpec {
    id: "vllm",
    display_name: "vLLM",
    base_url: "http://localhost:8000",
    chat_path: "/v1/chat/completions",
    format: FormatKind::OaCompatible,
    auth: AuthKind::None,
    default_headers: &[],
    supports_streaming: true,
    hermes_fallback: true,
};

pub const LM_STUDIO: ProviderSpec = ProviderSpec {
    id: "lm-studio",
    display_name: "LM Studio",
    base_url: "http://localhost:1234",
    chat_path: "/v1/chat/completions",
    format: FormatKind::OaCompatible,
    auth: AuthKind::None,
    default_headers: &[],
    supports_streaming: true,
    hermes_fallback: true,
};
