//! Provider registry — register and lookup providers by ID.
//!
//! Supports lazy provider initialization: `ProviderSpec` (lightweight metadata)
//! is registered eagerly, but the actual `SpecBasedProvider` (HTTP client,
//! headers, auth) is only created on first `get_or_init_provider()` call.
//!
//! Inspired by pi-mono's lazy module loading pattern where provider modules
//! are imported only when the provider is first used.

use super::client::SpecBasedProvider;
use super::spec::ProviderSpec;
use std::collections::HashMap;
use std::sync::OnceLock;

/// A lazily-initialized provider entry.
///
/// Stores the lightweight `ProviderSpec` eagerly and defers creation of the
/// `SpecBasedProvider` (which allocates an HTTP client) until first use.
pub struct LazyProviderEntry {
    spec: ProviderSpec,
    provider: OnceLock<SpecBasedProvider>,
}

impl LazyProviderEntry {
    /// Create a new lazy entry from a spec.
    pub fn new(spec: ProviderSpec) -> Self {
        Self {
            spec,
            provider: OnceLock::new(),
        }
    }

    /// Get the spec (always available, no initialization cost).
    pub fn spec(&self) -> &ProviderSpec {
        &self.spec
    }

    /// Get or initialize the provider for the given model.
    ///
    /// The provider is created on first call and cached for subsequent calls.
    /// `model` and `api_key_override` are only used during the first initialization.
    pub fn get_or_init_provider(
        &self,
        model: &str,
        api_key_override: Option<String>,
    ) -> &SpecBasedProvider {
        self.provider.get_or_init(|| {
            SpecBasedProvider::new(self.spec.clone(), model, api_key_override)
        })
    }

    /// Check whether the provider has been initialized.
    pub fn is_initialized(&self) -> bool {
        self.provider.get().is_some()
    }
}

impl std::fmt::Debug for LazyProviderEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LazyProviderEntry")
            .field("spec", &self.spec)
            .field("initialized", &self.is_initialized())
            .finish()
    }
}

/// Registry of available LLM providers.
///
/// Specs are registered eagerly (they are const-constructible and cheap).
/// Actual provider instances are created lazily on first access via
/// `get_or_init_provider()`.
pub struct ProviderRegistry {
    entries: HashMap<&'static str, LazyProviderEntry>,
}

impl ProviderRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Register a provider spec (lazy — no HTTP client created yet).
    pub fn register(&mut self, spec: ProviderSpec) {
        self.entries
            .insert(spec.id, LazyProviderEntry::new(spec));
    }

    /// Look up a provider spec by ID (cheap, no initialization).
    pub fn get(&self, id: &str) -> Option<&ProviderSpec> {
        self.entries.get(id).map(|entry| entry.spec())
    }

    /// Get the lazy entry for a provider (allows deferred initialization).
    pub fn get_entry(&self, id: &str) -> Option<&LazyProviderEntry> {
        self.entries.get(id)
    }

    /// Get or initialize the provider for the given ID and model.
    ///
    /// Returns `None` if the provider ID is not registered.
    /// On first call for a given ID, creates the `SpecBasedProvider`.
    /// Subsequent calls return the cached instance.
    pub fn get_or_init_provider(
        &self,
        id: &str,
        model: &str,
        api_key_override: Option<String>,
    ) -> Option<&SpecBasedProvider> {
        self.entries
            .get(id)
            .map(|entry| entry.get_or_init_provider(model, api_key_override))
    }

    /// List all registered provider IDs (sorted).
    pub fn list(&self) -> Vec<&'static str> {
        let mut ids: Vec<&'static str> = self.entries.keys().copied().collect();
        ids.sort();
        ids
    }

    /// Number of registered providers.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get all provider specs (sorted by ID).
    pub fn all_specs(&self) -> Vec<&ProviderSpec> {
        let mut specs: Vec<&ProviderSpec> =
            self.entries.values().map(|e| e.spec()).collect();
        specs.sort_by_key(|s| s.id);
        specs
    }

    /// Count of providers that have been initialized (for diagnostics).
    pub fn initialized_count(&self) -> usize {
        self.entries.values().filter(|e| e.is_initialized()).count()
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a registry pre-loaded with all built-in provider specs.
///
/// No providers are initialized at this point — only specs are registered.
/// Actual `SpecBasedProvider` instances are created lazily on first use.
pub fn create_default_registry() -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    for spec in super::catalog::built_in_providers() {
        registry.register(spec);
    }
    registry
}

#[cfg(test)]
mod tests {
    use super::super::LlmProvider;
    use super::super::spec::*;
    use super::*;

    const PROVIDER_A: ProviderSpec = ProviderSpec {
        id: "provider_a",
        display_name: "Provider A",
        base_url: "https://a.example.com",
        chat_path: "/v1/chat/completions",
        format: FormatKind::OaCompatible,
        auth: AuthKind::BearerFromEnv("A_KEY"),
        default_headers: &[],
        supports_streaming: true,
        hermes_fallback: false,
    };

    const PROVIDER_B: ProviderSpec = ProviderSpec {
        id: "provider_b",
        display_name: "Provider B",
        base_url: "https://b.example.com",
        chat_path: "/v1/chat/completions",
        format: FormatKind::Anthropic,
        auth: AuthKind::CustomHeaderFromEnv {
            header: "x-api-key",
            env_var: "B_KEY",
        },
        default_headers: &[("anthropic-version", "2023-06-01")],
        supports_streaming: true,
        hermes_fallback: false,
    };

    #[test]
    fn empty_registry() {
        let reg = ProviderRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(reg.list().is_empty());
    }

    #[test]
    fn register_and_get() {
        let mut reg = ProviderRegistry::new();
        reg.register(PROVIDER_A);
        assert_eq!(reg.len(), 1);
        let spec = reg.get("provider_a").unwrap();
        assert_eq!(spec.id, "provider_a");
        assert_eq!(spec.display_name, "Provider A");
    }

    #[test]
    fn get_returns_none_for_unknown() {
        let reg = ProviderRegistry::new();
        assert!(reg.get("nonexistent").is_none());
    }

    #[test]
    fn list_returns_sorted_ids() {
        let mut reg = ProviderRegistry::new();
        reg.register(PROVIDER_B);
        reg.register(PROVIDER_A);
        assert_eq!(reg.list(), vec!["provider_a", "provider_b"]);
    }

    #[test]
    fn all_specs_returns_sorted() {
        let mut reg = ProviderRegistry::new();
        reg.register(PROVIDER_B);
        reg.register(PROVIDER_A);
        let specs = reg.all_specs();
        assert_eq!(specs[0].id, "provider_a");
        assert_eq!(specs[1].id, "provider_b");
    }

    #[test]
    fn default_registry_has_all_builtin_providers() {
        let registry = super::create_default_registry();
        assert!(
            registry.len() >= 25,
            "Expected 25+ providers, got {}",
            registry.len()
        );
        assert!(registry.get("openai").is_some());
        assert!(registry.get("anthropic").is_some());
        assert!(registry.get("groq").is_some());
        assert!(registry.get("ollama").is_some());
        assert!(registry.get("azure").is_some());
        assert!(registry.get("amazon-bedrock").is_some());
    }

    #[test]
    fn register_overwrites_duplicate() {
        let mut reg = ProviderRegistry::new();
        reg.register(PROVIDER_A);
        let updated = ProviderSpec {
            display_name: "Updated A",
            ..PROVIDER_A
        };
        reg.register(updated);
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.get("provider_a").unwrap().display_name, "Updated A");
    }

    // --- Lazy initialization tests ---

    #[test]
    fn provider_not_initialized_until_first_access() {
        let mut reg = ProviderRegistry::new();
        reg.register(PROVIDER_A);
        reg.register(PROVIDER_B);

        // Specs are registered but no providers initialized yet
        assert_eq!(reg.initialized_count(), 0);
        assert!(!reg.get_entry("provider_a").unwrap().is_initialized());
        assert!(!reg.get_entry("provider_b").unwrap().is_initialized());
    }

    #[test]
    fn get_or_init_creates_provider_on_first_call() {
        let mut reg = ProviderRegistry::new();
        reg.register(PROVIDER_A);

        assert_eq!(reg.initialized_count(), 0);

        // First access initializes the provider
        let provider = reg
            .get_or_init_provider("provider_a", "gpt-4", None)
            .unwrap();
        assert_eq!(provider.provider_id(), "provider_a");
        assert_eq!(provider.model(), "gpt-4");

        assert_eq!(reg.initialized_count(), 1);
        assert!(reg.get_entry("provider_a").unwrap().is_initialized());
    }

    #[test]
    fn get_or_init_returns_same_instance_on_subsequent_calls() {
        let mut reg = ProviderRegistry::new();
        reg.register(PROVIDER_A);

        let first = reg
            .get_or_init_provider("provider_a", "gpt-4", None)
            .unwrap();
        let first_ptr = first as *const SpecBasedProvider;

        let second = reg
            .get_or_init_provider("provider_a", "gpt-4o", Some("different-key".into()))
            .unwrap();
        let second_ptr = second as *const SpecBasedProvider;

        // Same instance — second call's model/key are ignored (already initialized)
        assert_eq!(first_ptr, second_ptr);
        assert_eq!(second.model(), "gpt-4"); // original model preserved
    }

    #[test]
    fn get_or_init_returns_none_for_unknown() {
        let reg = ProviderRegistry::new();
        assert!(
            reg.get_or_init_provider("nonexistent", "model", None)
                .is_none()
        );
    }

    #[test]
    fn only_accessed_providers_are_initialized() {
        let mut reg = ProviderRegistry::new();
        reg.register(PROVIDER_A);
        reg.register(PROVIDER_B);

        assert_eq!(reg.initialized_count(), 0);

        // Only initialize provider_a
        reg.get_or_init_provider("provider_a", "gpt-4", None);

        assert_eq!(reg.initialized_count(), 1);
        assert!(reg.get_entry("provider_a").unwrap().is_initialized());
        assert!(!reg.get_entry("provider_b").unwrap().is_initialized());
    }

    #[test]
    fn lazy_entry_debug_shows_initialization_state() {
        let entry = LazyProviderEntry::new(PROVIDER_A);
        let debug_before = format!("{:?}", entry);
        assert!(debug_before.contains("initialized: false"));

        entry.get_or_init_provider("gpt-4", None);
        let debug_after = format!("{:?}", entry);
        assert!(debug_after.contains("initialized: true"));
    }

    #[test]
    fn default_registry_starts_with_zero_initialized() {
        let registry = create_default_registry();
        assert!(registry.len() >= 25);
        assert_eq!(
            registry.initialized_count(),
            0,
            "Default registry should have zero initialized providers at startup"
        );
    }
}
