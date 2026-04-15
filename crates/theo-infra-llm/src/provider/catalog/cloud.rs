//! Cloud provider specs — Azure, Bedrock, Vertex, GitHub, GitLab, Cloudflare, SAP.

use crate::provider::spec::*;

pub const AZURE: ProviderSpec = ProviderSpec {
    id: "azure",
    display_name: "Azure OpenAI",
    base_url: "https://YOUR_RESOURCE.openai.azure.com",
    chat_path: "/openai/deployments/YOUR_DEPLOYMENT/chat/completions",
    format: FormatKind::OaCompatible,
    auth: AuthKind::BearerFromEnv("AZURE_OPENAI_API_KEY"),
    default_headers: &[("api-version", "2024-02-01")],
    supports_streaming: true,
    hermes_fallback: false,
};

pub const AZURE_COGNITIVE: ProviderSpec = ProviderSpec {
    id: "azure-cognitive-services",
    display_name: "Azure Cognitive Services",
    base_url: "https://YOUR_ENDPOINT.cognitiveservices.azure.com",
    chat_path: "/openai/deployments/YOUR_DEPLOYMENT/chat/completions",
    format: FormatKind::OaCompatible,
    auth: AuthKind::BearerFromEnv("AZURE_COGNITIVE_API_KEY"),
    default_headers: &[("api-version", "2024-02-01")],
    supports_streaming: true,
    hermes_fallback: false,
};

pub const GITHUB_COPILOT: ProviderSpec = ProviderSpec {
    id: "github-copilot",
    display_name: "GitHub Copilot",
    base_url: "https://api.githubcopilot.com",
    chat_path: "/chat/completions",
    format: FormatKind::OaCompatible,
    auth: AuthKind::BearerFromEnv("GITHUB_TOKEN"),
    default_headers: &[
        ("editor-version", "Theo/1.0"),
        ("Copilot-Integration-Id", "theo-code"),
    ],
    supports_streaming: true,
    hermes_fallback: false,
};

pub const GITLAB: ProviderSpec = ProviderSpec {
    id: "gitlab",
    display_name: "GitLab AI",
    base_url: "https://gitlab.com/api/v4",
    chat_path: "/ai/chat/completions",
    format: FormatKind::OaCompatible,
    auth: AuthKind::CustomHeaderFromEnv {
        header: "PRIVATE-TOKEN",
        env_var: "GITLAB_TOKEN",
    },
    default_headers: &[],
    supports_streaming: true,
    hermes_fallback: false,
};

pub const CLOUDFLARE_WORKERS: ProviderSpec = ProviderSpec {
    id: "cloudflare-workers-ai",
    display_name: "Cloudflare Workers AI",
    base_url: "https://api.cloudflare.com/client/v4/accounts/YOUR_ACCOUNT/ai",
    chat_path: "/v1/chat/completions",
    format: FormatKind::OaCompatible,
    auth: AuthKind::BearerFromEnv("CLOUDFLARE_API_TOKEN"),
    default_headers: &[],
    supports_streaming: true,
    hermes_fallback: false,
};

pub const CLOUDFLARE_GATEWAY: ProviderSpec = ProviderSpec {
    id: "cloudflare-ai-gateway",
    display_name: "Cloudflare AI Gateway",
    base_url: "https://gateway.ai.cloudflare.com/v1/YOUR_ACCOUNT/YOUR_GATEWAY",
    chat_path: "/openai/chat/completions",
    format: FormatKind::OaCompatible,
    auth: AuthKind::BearerFromEnv("CLOUDFLARE_API_TOKEN"),
    default_headers: &[],
    supports_streaming: true,
    hermes_fallback: false,
};

pub const SAP_AI_CORE: ProviderSpec = ProviderSpec {
    id: "sap-ai-core",
    display_name: "SAP AI Core",
    base_url: "https://api.ai.YOUR_REGION.sap.hana.ondemand.com",
    chat_path: "/v2/inference/deployments/YOUR_DEPLOYMENT/chat/completions",
    format: FormatKind::OaCompatible,
    auth: AuthKind::BearerFromEnv("SAP_AI_CORE_TOKEN"),
    default_headers: &[("AI-Resource-Group", "default")],
    supports_streaming: true,
    hermes_fallback: false,
};

pub const AMAZON_BEDROCK: ProviderSpec = ProviderSpec {
    id: "amazon-bedrock",
    display_name: "Amazon Bedrock",
    base_url: "https://bedrock-runtime.us-east-1.amazonaws.com",
    chat_path: "/model/MODEL_ID/converse",
    format: FormatKind::OaCompatible,
    auth: AuthKind::AwsSigV4 {
        region_env: "AWS_REGION",
        service: "bedrock",
    },
    default_headers: &[],
    supports_streaming: true,
    hermes_fallback: false,
};

pub const GOOGLE_VERTEX: ProviderSpec = ProviderSpec {
    id: "google-vertex",
    display_name: "Google Vertex AI",
    base_url: "https://us-central1-aiplatform.googleapis.com",
    chat_path: "/v1/projects/YOUR_PROJECT/locations/us-central1/publishers/google/models/MODEL:generateContent",
    format: FormatKind::OaCompatible,
    auth: AuthKind::GcpAdc {
        project_env: "GOOGLE_CLOUD_PROJECT",
        location_env: "GOOGLE_CLOUD_LOCATION",
    },
    default_headers: &[],
    supports_streaming: true,
    hermes_fallback: false,
};

pub const GOOGLE_VERTEX_ANTHROPIC: ProviderSpec = ProviderSpec {
    id: "google-vertex-anthropic",
    display_name: "Google Vertex AI (Anthropic)",
    base_url: "https://us-central1-aiplatform.googleapis.com",
    chat_path: "/v1/projects/YOUR_PROJECT/locations/us-central1/publishers/anthropic/models/MODEL:rawPredict",
    format: FormatKind::Anthropic,
    auth: AuthKind::GcpAdc {
        project_env: "GOOGLE_CLOUD_PROJECT",
        location_env: "GOOGLE_CLOUD_LOCATION",
    },
    default_headers: &[],
    supports_streaming: true,
    hermes_fallback: false,
};
