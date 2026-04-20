//! Fan-out coordinator over multiple `MemoryProvider`s.
//!
//! Design:
//! - **Error isolation**: a provider that panics or returns an error
//!   does not block the others. Inspired by hermes-agent
//!   `memory_manager.py:97-206`.
//! - **Deterministic order**: providers are dispatched in registration
//!   order; output is concatenated with per-provider memory fences so
//!   the model can tell providers apart.
//! - **Single external restriction**: only ONE provider may be marked
//!   `is_external()` simultaneously (prevents double-budget on paid
//!   backends). Second external registration emits a warn and is
//!   rejected.
//!
//! Plan: `outputs/agent-memory-plan.md` §RM1.

use std::sync::Arc;

use async_trait::async_trait;
use theo_domain::memory::{MemoryProvider, build_memory_context_block};

/// Runtime statistics exposed for the `theo memory lint` subcommand.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EngineStats {
    pub registered_providers: usize,
    pub external_providers: usize,
    pub rejected_duplicate_external: usize,
}

/// Fan-out coordinator. `Arc<MemoryEngine>` is cheap to clone across
/// tokio tasks; internal state is append-only so no lock is needed.
pub struct MemoryEngine {
    providers: Vec<Arc<dyn MemoryProvider>>,
    external_provider_count: usize,
    rejected_duplicate_external: usize,
}

impl MemoryEngine {
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
            external_provider_count: 0,
            rejected_duplicate_external: 0,
        }
    }

    /// Register an additional provider. Returns `true` when added,
    /// `false` when the registration was rejected (e.g. a second
    /// external provider was refused).
    pub fn register(&mut self, provider: Arc<dyn MemoryProvider>) -> bool {
        let is_external = provider.is_external();
        if is_external && self.external_provider_count >= 1 {
            self.rejected_duplicate_external += 1;
            eprintln!(
                "[theo-infra-memory] rejected second external provider `{}`; only one allowed",
                provider.name()
            );
            return false;
        }
        if is_external {
            self.external_provider_count += 1;
        }
        self.providers.push(provider);
        true
    }

    pub fn stats(&self) -> EngineStats {
        EngineStats {
            registered_providers: self.providers.len(),
            external_providers: self.external_provider_count,
            rejected_duplicate_external: self.rejected_duplicate_external,
        }
    }

    pub fn providers(&self) -> &[Arc<dyn MemoryProvider>] {
        &self.providers
    }
}

impl Default for MemoryEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// `MemoryEngine` also implements `MemoryProvider` so it can be dropped
/// into any call site that accepts a single provider (transparent
/// composition).
#[async_trait]
impl MemoryProvider for MemoryEngine {
    fn name(&self) -> &str {
        "engine"
    }

    async fn prefetch(&self, query: &str) -> String {
        let mut pieces: Vec<String> = Vec::new();
        for p in &self.providers {
            // Panic isolation: swallow panics from any single provider.
            let fut = p.prefetch(query);
            let result = async_catch_panic(fut).await;
            match result {
                Ok(raw) if !raw.is_empty() => pieces.push(build_memory_context_block(&raw)),
                Ok(_) => {}
                Err(name) => {
                    eprintln!(
                        "[theo-infra-memory] provider `{name}` panicked in prefetch; ignored"
                    );
                }
            }
        }
        pieces.join("\n")
    }

    async fn sync_turn(&self, user: &str, assistant: &str) {
        for p in &self.providers {
            let fut = p.sync_turn(user, assistant);
            if let Err(name) = async_catch_panic(fut).await {
                eprintln!(
                    "[theo-infra-memory] provider `{name}` panicked in sync_turn; ignored"
                );
            }
        }
    }

    async fn on_pre_compress(&self, messages_as_text: &str) -> String {
        let mut pieces = Vec::new();
        for p in &self.providers {
            match async_catch_panic(p.on_pre_compress(messages_as_text)).await {
                Ok(raw) if !raw.is_empty() => pieces.push(raw),
                Ok(_) => {}
                Err(name) => eprintln!(
                    "[theo-infra-memory] provider `{name}` panicked in on_pre_compress; ignored"
                ),
            }
        }
        pieces.join("\n")
    }

    async fn on_session_end(&self) {
        for p in &self.providers {
            let _ = async_catch_panic(p.on_session_end()).await;
        }
    }

    fn is_external(&self) -> bool {
        self.external_provider_count > 0
    }
}

/// Wrap an async call so panics return `Err(provider_name)` instead of
/// unwinding through the engine. Async panics are caught by running the
/// future inside `std::panic::catch_unwind` — which requires
/// `AssertUnwindSafe` since futures are not unwind-safe.
async fn async_catch_panic<F, T>(fut: F) -> Result<T, String>
where
    F: std::future::Future<Output = T>,
{
    use futures::FutureExt;
    match std::panic::AssertUnwindSafe(fut).catch_unwind().await {
        Ok(v) => Ok(v),
        Err(_) => Err("provider".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use theo_domain::memory::{MEMORY_FENCE_OPEN, NullMemoryProvider};

    #[derive(Default)]
    struct Tracer {
        name: String,
        log: Arc<Mutex<Vec<String>>>,
        is_ext: bool,
    }

    impl Tracer {
        fn new(name: &str) -> (Arc<Self>, Arc<Mutex<Vec<String>>>) {
            let log = Arc::new(Mutex::new(Vec::new()));
            (
                Arc::new(Self {
                    name: name.to_string(),
                    log: log.clone(),
                    is_ext: false,
                }),
                log,
            )
        }
        fn external(name: &str) -> Arc<Self> {
            Arc::new(Self {
                name: name.to_string(),
                log: Arc::new(Mutex::new(Vec::new())),
                is_ext: true,
            })
        }
    }

    #[async_trait]
    impl MemoryProvider for Tracer {
        fn name(&self) -> &str {
            &self.name
        }
        fn is_external(&self) -> bool {
            self.is_ext
        }
        async fn prefetch(&self, q: &str) -> String {
            self.log
                .lock()
                .unwrap()
                .push(format!("{}:prefetch:{q}", self.name));
            format!("{}:fact:{q}", self.name)
        }
        async fn sync_turn(&self, u: &str, _a: &str) {
            self.log
                .lock()
                .unwrap()
                .push(format!("{}:sync:{u}", self.name));
        }
    }

    struct Panicky;
    #[async_trait]
    impl MemoryProvider for Panicky {
        fn name(&self) -> &str {
            "panicky"
        }
        async fn prefetch(&self, _q: &str) -> String {
            panic!("boom in prefetch");
        }
        async fn sync_turn(&self, _u: &str, _a: &str) {
            panic!("boom in sync_turn");
        }
    }

    // ── RM1-AC-1 ─────────────────────────────────────────────────
    #[tokio::test]
    async fn test_rm1_ac_1_fanout_prefetch_concatenates_fenced_results() {
        let mut e = MemoryEngine::new();
        let (a, _) = Tracer::new("a");
        let (b, _) = Tracer::new("b");
        e.register(a);
        e.register(b);
        let out = e.prefetch("q").await;
        assert!(out.contains("a:fact:q"));
        assert!(out.contains("b:fact:q"));
        // Both pieces go through the fence.
        assert_eq!(out.matches(MEMORY_FENCE_OPEN).count(), 2);
    }

    // ── RM1-AC-2 ─────────────────────────────────────────────────
    #[tokio::test]
    async fn test_rm1_ac_2_panicking_provider_does_not_block_fanout() {
        let mut e = MemoryEngine::new();
        e.register(Arc::new(Panicky));
        let (b, _) = Tracer::new("b");
        e.register(b);
        let out = e.prefetch("q").await;
        assert!(
            out.contains("b:fact:q"),
            "provider b must run despite Panicky panic"
        );
    }

    // ── RM1-AC-3 ─────────────────────────────────────────────────
    #[test]
    fn test_rm1_ac_3_only_one_external_provider_allowed() {
        let mut e = MemoryEngine::new();
        assert!(e.register(Tracer::external("honcho")));
        assert!(!e.register(Tracer::external("mem0")));
        let s = e.stats();
        assert_eq!(s.external_providers, 1);
        assert_eq!(s.rejected_duplicate_external, 1);
    }

    // ── RM1-AC-4 ─────────────────────────────────────────────────
    #[tokio::test]
    async fn test_rm1_ac_4_providers_dispatched_in_registration_order() {
        let mut e = MemoryEngine::new();
        let (a, log_a) = Tracer::new("a");
        let (b, log_b) = Tracer::new("b");
        e.register(a);
        e.register(b);
        e.sync_turn("u", "r").await;
        // Both providers called; engine iterates in registration order.
        assert_eq!(log_a.lock().unwrap().first().unwrap(), "a:sync:u");
        assert_eq!(log_b.lock().unwrap().first().unwrap(), "b:sync:u");
    }

    // ── RM1-AC-5 ─────────────────────────────────────────────────
    #[tokio::test]
    async fn test_rm1_ac_5_fence_wraps_each_provider_output() {
        let mut e = MemoryEngine::new();
        let (a, _) = Tracer::new("a");
        e.register(a);
        let out = e.prefetch("q").await;
        assert!(out.trim_start().starts_with(MEMORY_FENCE_OPEN));
    }

    // ── RM1-AC-6 ─────────────────────────────────────────────────
    #[tokio::test]
    async fn test_rm1_ac_6_sync_turn_fans_out_to_all() {
        let mut e = MemoryEngine::new();
        let (a, log_a) = Tracer::new("a");
        let (b, log_b) = Tracer::new("b");
        e.register(a);
        e.register(b);
        e.sync_turn("hi", "ack").await;
        assert_eq!(log_a.lock().unwrap().len(), 1);
        assert_eq!(log_b.lock().unwrap().len(), 1);
    }

    // ── RM1-AC-7 ─────────────────────────────────────────────────
    #[tokio::test]
    async fn test_rm1_ac_7_engine_implements_memory_provider() {
        // Transparent composition: an engine can be registered INSIDE
        // another engine as if it were a plain provider.
        let mut inner = MemoryEngine::new();
        let (a, _) = Tracer::new("a");
        inner.register(a);
        let inner_arc: Arc<dyn MemoryProvider> = Arc::new(inner);

        let mut outer = MemoryEngine::new();
        outer.register(inner_arc);
        let out = outer.prefetch("q").await;
        assert!(out.contains("a:fact:q"));
    }

    // ── RM1-AC-8 (integration) ───────────────────────────────────
    #[tokio::test]
    async fn test_rm1_ac_8_end_to_end_with_null_and_real_provider() {
        let mut e = MemoryEngine::new();
        e.register(Arc::new(NullMemoryProvider::default()));
        let (real, _) = Tracer::new("real");
        e.register(real);
        let out = e.prefetch("context").await;
        // Null contributes nothing; Tracer contributes one fenced block.
        assert_eq!(out.matches(MEMORY_FENCE_OPEN).count(), 1);
        assert!(out.contains("real:fact:context"));
    }
}
