//! Anthropic provider spec.

use crate::provider::spec::*;

pub const ANTHROPIC: ProviderSpec = ProviderSpec {
    id: "anthropic",
    display_name: "Anthropic",
    base_url: "https://api.anthropic.com",
    chat_path: "/v1/messages",
    format: FormatKind::Anthropic,
    auth: AuthKind::CustomHeaderFromEnv {
        header: "x-api-key",
        env_var: "ANTHROPIC_API_KEY",
    },
    default_headers: &[("anthropic-version", "2023-06-01")],
    supports_streaming: true,
    hermes_fallback: false,
};
