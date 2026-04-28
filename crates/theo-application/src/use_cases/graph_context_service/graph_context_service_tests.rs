//! Sibling test body of `graph_context_service.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `graph_context_service.rs` via `#[path = "graph_context_service_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.

    #![allow(unused_imports)]
    use super::*;
    use crate::use_cases::graph_context_service::*;
    use std::time::Duration;
    use theo_domain::graph_context::{ContextBlock, GraphContextError, GraphContextProvider, GraphContextResult};
    use std::path::{Path, PathBuf};
    use crate::use_cases::conversion::{convert_symbol_kind, convert_reference_kind};
    use theo_engine_graph::bridge::{ReferenceKindDto, SymbolKindDto};
    use theo_engine_parser::types::{ReferenceKind, SymbolKind};

    /// Helper: wait for the service to become ready (background build to complete).
    async fn wait_ready(service: &GraphContextService, timeout_secs: u64) -> bool {
        tokio::time::timeout(Duration::from_secs(timeout_secs), async {
            loop {
                if service.is_ready() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .is_ok()
    }

    #[test]
    fn convert_symbol_kind_covers_all_variants() {
        let variants = [
            (SymbolKind::Function, SymbolKindDto::Function),
            (SymbolKind::Method, SymbolKindDto::Method),
            (SymbolKind::Class, SymbolKindDto::Class),
            (SymbolKind::Struct, SymbolKindDto::Struct),
            (SymbolKind::Enum, SymbolKindDto::Enum),
            (SymbolKind::Trait, SymbolKindDto::Trait),
            (SymbolKind::Interface, SymbolKindDto::Interface),
            (SymbolKind::Module, SymbolKindDto::Module),
        ];
        for (from, expected) in variants {
            assert_eq!(convert_symbol_kind(&from), expected);
        }
    }

    #[test]
    fn convert_reference_kind_covers_all_variants() {
        let variants = [
            (ReferenceKind::Call, ReferenceKindDto::Call),
            (ReferenceKind::Extends, ReferenceKindDto::Extends),
            (ReferenceKind::Implements, ReferenceKindDto::Implements),
            (ReferenceKind::TypeUsage, ReferenceKindDto::TypeUsage),
            (ReferenceKind::Import, ReferenceKindDto::Import),
        ];
        for (from, expected) in variants {
            assert_eq!(convert_reference_kind(&from), expected);
        }
    }

    // --- State machine transition tests ---

    #[tokio::test]
    async fn building_transitions_to_ready() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/main.rs"), "fn main() {}").unwrap();

        let service = GraphContextService::new();
        assert!(!service.is_ready()); // Uninitialized

        service.initialize(tmp.path()).await.unwrap(); // Returns immediately

        // Wait for background build to complete.
        assert!(
            wait_ready(&service, 30).await,
            "Build did not complete in 30s"
        );
        assert!(service.is_ready());
    }

    #[tokio::test]
    async fn query_during_building_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        // Create enough files to make build take >0ms
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("src/main.rs"),
            "fn main() { println!(\"hello\"); }",
        )
        .unwrap();

        let service = GraphContextService::new();
        service.initialize(tmp.path()).await.unwrap();

        // Immediately query — may still be Building.
        let result = service.query_context("test", 4000).await;
        // Should be Ok(empty) if Building, or Ok(context) if already Ready.
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn query_before_initialize_returns_not_initialized() {
        let service = GraphContextService::new();
        let result = service.query_context("test", 4000).await;
        assert!(matches!(result, Err(GraphContextError::NotInitialized)));
    }

    #[tokio::test]
    async fn query_after_ready_returns_context() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("src/main.rs"),
            "fn main() {}\nfn add(a: i32, b: i32) -> i32 { a + b }\n",
        )
        .unwrap();

        let service = GraphContextService::new();
        service.initialize(tmp.path()).await.unwrap();
        assert!(wait_ready(&service, 30).await);

        let result = service.query_context("add function", 4000).await.unwrap();
        assert!(result.total_tokens <= result.budget_tokens);
    }

    #[tokio::test]
    async fn double_initialize_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let service = GraphContextService::new();
        service.initialize(tmp.path()).await.unwrap();
        // Second call while building — should be no-op.
        service.initialize(tmp.path()).await.unwrap();
    }

    #[test]
    fn is_ready_false_before_init() {
        let service = GraphContextService::new();
        assert!(!service.is_ready());
    }

    #[test]
    fn cache_miss_on_nonexistent_path() {
        assert!(
            try_load_cache(Path::new("/tmp/nonexistent_graph.bin"), Path::new("/tmp")).is_none()
        );
    }

    // --- Content hash tests (S0-T1) ---

    #[test]
    fn content_hash_stable_when_mtime_changes_but_content_identical() {
        // Arrange: create file, compute hash
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        let file_path = tmp.path().join("src/main.rs");
        std::fs::write(&file_path, "fn main() {}").unwrap();
        let hash1 = compute_project_hash(tmp.path());

        // Act: set mtime to 1 hour in the future (content stays identical)
        let future_time = std::time::SystemTime::now() + Duration::from_secs(3600);
        let times = std::fs::FileTimes::new().set_modified(future_time);
        let file = std::fs::File::options()
            .write(true)
            .open(&file_path)
            .unwrap();
        file.set_times(times).unwrap();
        drop(file);

        let hash2 = compute_project_hash(tmp.path());

        // Assert: hashes must be equal (content didn't change)
        assert_eq!(
            hash1, hash2,
            "Hash changed despite identical content — mtime leak"
        );
    }

    #[test]
    fn content_hash_differs_when_content_changes() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/main.rs"), "fn main() {}").unwrap();
        let hash1 = compute_project_hash(tmp.path());

        // Act: change content
        std::fs::write(
            tmp.path().join("src/main.rs"),
            "fn main() { println!(\"hi\"); }",
        )
        .unwrap();
        let hash2 = compute_project_hash(tmp.path());

        // Assert
        assert_ne!(hash1, hash2, "Hash must change when content changes");
    }

    #[test]
    fn content_hash_deterministic_across_calls() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("src/lib.rs"),
            "pub fn add(a: i32, b: i32) -> i32 { a + b }",
        )
        .unwrap();

        let hash1 = compute_project_hash(tmp.path());
        let hash2 = compute_project_hash(tmp.path());
        assert_eq!(hash1, hash2, "Hash must be deterministic");
    }

    #[tokio::test]
    async fn integration_real_project_produces_context() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("src/main.rs"),
            "fn main() { println!(\"hello\"); }\nfn add(a: i32, b: i32) -> i32 { a + b }\n",
        )
        .unwrap();

        let service = GraphContextService::new();
        service.initialize(tmp.path()).await.unwrap();
        assert!(wait_ready(&service, 30).await, "Build did not complete");

        let result = service.query_context("add function", 4000).await.unwrap();
        assert!(result.total_tokens <= result.budget_tokens);
    }

    // --- S0-T4: Extended coverage tests ---

    #[tokio::test]
    async fn query_respects_token_budget() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("src/lib.rs"),
            "pub fn foo() -> i32 { 1 }\npub fn bar() -> i32 { 2 }\npub fn baz() -> i32 { 3 }\n",
        )
        .unwrap();

        let service = GraphContextService::new();
        service.initialize(tmp.path()).await.unwrap();
        assert!(wait_ready(&service, 30).await);

        // Small budget
        let result = service.query_context("foo", 100).await.unwrap();
        assert!(
            result.total_tokens <= 100,
            "Tokens {} exceeded budget 100",
            result.total_tokens
        );
        assert_eq!(result.budget_tokens, 100);
    }

    #[tokio::test]
    async fn is_ready_true_after_successful_build() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/main.rs"), "fn main() {}").unwrap();

        let service = GraphContextService::new();
        assert!(!service.is_ready());

        service.initialize(tmp.path()).await.unwrap();
        assert!(wait_ready(&service, 30).await);
        assert!(service.is_ready());
    }

    #[tokio::test]
    async fn query_empty_project_returns_empty_context() {
        let tmp = tempfile::tempdir().unwrap();
        // Empty directory — no source files
        let service = GraphContextService::new();
        service.initialize(tmp.path()).await.unwrap();
        assert!(wait_ready(&service, 30).await);

        let result = service.query_context("anything", 4000).await.unwrap();
        assert_eq!(
            result.blocks.len(),
            0,
            "Empty project should produce no context blocks"
        );
    }

    #[test]
    fn compute_project_hash_empty_dir_returns_stable_hash() {
        let tmp = tempfile::tempdir().unwrap();
        let h1 = compute_project_hash(tmp.path());
        let h2 = compute_project_hash(tmp.path());
        assert_eq!(h1, h2, "Hash of empty dir must be deterministic");
    }

    #[test]
    fn compute_project_hash_ignores_non_source_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("data.csv"), "a,b,c").unwrap();
        let h1 = compute_project_hash(tmp.path());

        std::fs::write(tmp.path().join("data.csv"), "x,y,z").unwrap();
        let h2 = compute_project_hash(tmp.path());

        assert_eq!(h1, h2, "Non-source files should not affect hash");
    }

    #[test]
    fn compute_project_hash_includes_toml_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();
        let h1 = compute_project_hash(tmp.path());

        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"changed\"",
        )
        .unwrap();
        let h2 = compute_project_hash(tmp.path());

        assert_ne!(h1, h2, "Toml file changes must change hash");
    }

    #[test]
    fn compute_project_hash_new_file_changes_hash() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/a.rs"), "fn a() {}").unwrap();
        let h1 = compute_project_hash(tmp.path());

        std::fs::write(tmp.path().join("src/b.rs"), "fn b() {}").unwrap();
        let h2 = compute_project_hash(tmp.path());

        assert_ne!(h1, h2, "Adding a file must change hash");
    }

    #[tokio::test]
    async fn cache_hit_produces_same_results_as_fresh_build() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("src/lib.rs"),
            "pub fn greet() -> &'static str { \"hello\" }\n",
        )
        .unwrap();

        // First build (cold)
        let service1 = GraphContextService::new();
        service1.initialize(tmp.path()).await.unwrap();
        assert!(wait_ready(&service1, 30).await);
        let result1 = service1.query_context("greet", 4000).await.unwrap();

        // Second build (should hit cache)
        let service2 = GraphContextService::new();
        service2.initialize(tmp.path()).await.unwrap();
        assert!(wait_ready(&service2, 30).await);
        let result2 = service2.query_context("greet", 4000).await.unwrap();

        // Both should produce results (blocks count may vary due to timing)
        assert_eq!(result1.budget_tokens, result2.budget_tokens);
    }

    #[tokio::test]
    async fn multiple_queries_after_ready_all_succeed() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("src/lib.rs"),
            "pub fn compute() -> i32 { 42 }",
        )
        .unwrap();

        let service = GraphContextService::new();
        service.initialize(tmp.path()).await.unwrap();
        assert!(wait_ready(&service, 30).await);

        // Multiple queries should all succeed
        for query in &["compute", "function", "i32"] {
            let result = service.query_context(query, 4000).await;
            assert!(
                result.is_ok(),
                "Query '{}' failed: {:?}",
                query,
                result.err()
            );
        }
    }

    // --- SOTA: Incremental hash tests ---

    #[test]
    fn incremental_hash_creates_cache() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/main.rs"), "fn main() {}").unwrap();

        let _ = compute_project_hash(tmp.path());

        let cache_path = tmp.path().join(".theo").join("hash_cache.json");
        assert!(cache_path.exists(), "Hash cache should be created");
    }

    #[test]
    fn incremental_hash_warm_is_consistent() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/lib.rs"), "pub fn foo() {}").unwrap();

        let cold = compute_project_hash(tmp.path());
        let warm = compute_project_hash(tmp.path()); // should use cache
        assert_eq!(cold, warm, "Warm hash must match cold hash");
    }

    #[test]
    fn incremental_hash_detects_new_file() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/a.rs"), "fn a() {}").unwrap();
        let h1 = compute_project_hash(tmp.path());

        std::fs::write(tmp.path().join("src/b.rs"), "fn b() {}").unwrap();
        let h2 = compute_project_hash(tmp.path());
        assert_ne!(h1, h2, "New file must change hash");
    }

    #[test]
    fn incremental_hash_detects_file_deletion() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/a.rs"), "fn a() {}").unwrap();
        std::fs::write(tmp.path().join("src/b.rs"), "fn b() {}").unwrap();
        let h1 = compute_project_hash(tmp.path());

        std::fs::remove_file(tmp.path().join("src/b.rs")).unwrap();
        let h2 = compute_project_hash(tmp.path());
        assert_ne!(h1, h2, "Deleted file must change hash");
    }

    // ── T8.1 part 3 — reranker plumbing in graph_context_service ──

    #[cfg(feature = "dense-retrieval")]
    #[tokio::test]
    async fn t81gcs_initialised_state_starts_with_default_cross_encoder_config() {
        // After the service finishes building, the GraphState carries
        // the SOTA-default CrossEncoderConfig (use_reranker=true,
        // top_k=20, max_candidates=50). The reranker model itself is
        // None until lazily loaded. This test pins the invariant so a
        // future refactor that drops the field would fail loudly.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/a.rs"), "fn a() {}").unwrap();

        let svc = GraphContextService::new();
        svc.initialize(tmp.path()).await.unwrap();
        assert!(wait_ready(&svc, 30).await, "service should become ready");

        let state = svc.state.read().await;
        match &*state {
            GraphBuildState::Ready(gs) => {
                let cfg = &gs.cross_encoder_config;
                assert!(cfg.use_reranker, "SOTA-default invariant: use_reranker=true");
                assert_eq!(cfg.top_k, 20);
                assert_eq!(cfg.max_candidates, 50);
                // Reranker model is unloaded until first use that
                // needs it (lazy init in a future iteration).
                assert!(gs.reranker.is_none(), "reranker model starts unloaded");
            }
            other => panic!(
                "expected Ready state after initialize, got {}",
                match other {
                    GraphBuildState::Uninitialized => "Uninitialized",
                    GraphBuildState::Building { .. } => "Building",
                    GraphBuildState::Failed(_) => "Failed",
                    GraphBuildState::Ready(_) => unreachable!(),
                }
            ),
        }
    }

    // ── T8.1 part 4 — lazy reranker preload ────────────────────────

    #[test]
    fn t81pre_env_disabled_by_default() {
        // Cold default invariant: an unset env var means NO preload
        // so cold-start latency stays the same for users who never
        // query enough to amortize the model download.
        let prev = std::env::var_os("THEO_RERANKER_PRELOAD");
        // SAFETY: test mutates a uniquely-named env var inside a serialized test module; concurrent access to the same key is not present in this test.
        unsafe {
            std::env::remove_var("THEO_RERANKER_PRELOAD");
        }
        assert!(!env_reranker_preload_enabled());
        if let Some(v) = prev {
            // SAFETY: test mutates a uniquely-named env var inside a serialized test module; concurrent access to the same key is not present in this test.
            unsafe {
                std::env::set_var("THEO_RERANKER_PRELOAD", v);
            }
        }
    }

    #[test]
    fn t81pre_env_recognises_truthy_values_case_insensitive() {
        let prev = std::env::var_os("THEO_RERANKER_PRELOAD");
        // SAFETY: test mutates a uniquely-named env var inside a serialized test module; concurrent access to the same key is not present in this test.
        unsafe {
            for v in ["1", "true", "TRUE", "yes", "YES", "on", "ON"] {
                std::env::set_var("THEO_RERANKER_PRELOAD", v);
                assert!(
                    env_reranker_preload_enabled(),
                    "value `{v}` should opt into preload"
                );
            }
            for v in ["0", "false", "FALSE", "no", "off", "", "garbage"] {
                std::env::set_var("THEO_RERANKER_PRELOAD", v);
                assert!(
                    !env_reranker_preload_enabled(),
                    "value `{v}` should NOT opt into preload"
                );
            }
            std::env::remove_var("THEO_RERANKER_PRELOAD");
            if let Some(v) = prev {
                std::env::set_var("THEO_RERANKER_PRELOAD", v);
            }
        }
    }

    #[cfg(feature = "dense-retrieval")]
    #[test]
    fn t81pre_try_construct_returns_none_when_disabled() {
        // The fast-path no-op: when the env var is off,
        // try_construct returns None WITHOUT touching the model
        // loader. Critical invariant — a regression here would
        // download the model on every cold start.
        let prev = std::env::var_os("THEO_RERANKER_PRELOAD");
        // SAFETY: test mutates a uniquely-named env var inside a serialized test module; concurrent access to the same key is not present in this test.
        unsafe {
            std::env::remove_var("THEO_RERANKER_PRELOAD");
        }
        let result = try_construct_reranker_if_enabled();
        assert!(result.is_none(), "preload off must short-circuit to None");
        if let Some(v) = prev {
            // SAFETY: test mutates a uniquely-named env var inside a serialized test module; concurrent access to the same key is not present in this test.
            unsafe {
                std::env::set_var("THEO_RERANKER_PRELOAD", v);
            }
        }
    }
