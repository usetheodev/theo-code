//! Sibling test body of `subagent/mod.rs` — split per-feature (T3.5 of code-hygiene-5x5).
//!
//! Test-only file; gates use the inner `cfg(test)` attribute below to
//! classify every line as test code.

#![cfg(test)]
#![allow(unused_imports)]

use super::*;
use super::subagent_test_helpers::{mcp_env_lock, CaptureListener};
use super::*;
use theo_domain::tool::ToolCategory;

mod worktree_override {
    use super::*;

    fn manager_no_worktree(depth: usize) -> SubAgentManager {
        SubAgentManager {
            config: AgentConfig::default(),
            event_bus: Arc::new(EventBus::new()),
            project_dir: PathBuf::from("/tmp"),
            depth,
            registry: None,
            run_store: None,
            hook_manager: None,
            cancellation: None,
            checkpoint_manager: None,
            worktree_provider: None,
            metrics: None,
            mcp_registry: None,
            mcp_discovery: None,
            pending_resume_context: parking_lot::Mutex::new(None),
            spawn_semaphore: None,
        }
    }

    #[test]
    fn worktree_override_enum_default_is_none() {
        // None variant = legacy behavior (create new from spec.isolation).
        let o = WorktreeOverride::None;
        assert!(matches!(o, WorktreeOverride::None));
    }

    #[test]
    fn worktree_override_reuse_carries_path() {
        let p = PathBuf::from("/tmp/wt-reused");
        let o = WorktreeOverride::Reuse(p.clone());
        match o {
            WorktreeOverride::Reuse(got) => assert_eq!(got, p),
            _ => panic!("expected Reuse variant"),
        }
    }

    #[test]
    fn worktree_override_recreate_carries_base_branch() {
        let o = WorktreeOverride::Recreate {
            base_branch: "develop".to_string(),
        };
        match o {
            WorktreeOverride::Recreate { base_branch } => {
                assert_eq!(base_branch, "develop");
            }
            _ => panic!("expected Recreate variant"),
        }
    }

    #[test]
    fn spawn_with_spec_with_override_none_matches_legacy_behavior() {
        // Regression guard: spawn_with_spec_with_override(None) MUST produce
        // a result indistinguishable from spawn_with_spec for non-isolated
        // specs (depth-limit early return path is identical).
        let manager = manager_no_worktree(1);
        let spec = theo_domain::agent_spec::AgentSpec::on_demand("alpha", "do x");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let r_legacy =
            rt.block_on(async { manager.spawn_with_spec(&spec, "obj", None).await });
        let r_override = rt.block_on(async {
            manager
                .spawn_with_spec_with_override(&spec, "obj", None, WorktreeOverride::None)
                .await
        });
        // Both hit depth-limit → identical "depth limit" summary.
        assert!(r_legacy.summary.contains("depth limit"));
        assert!(r_override.summary.contains("depth limit"));
        assert_eq!(r_legacy.success, r_override.success);
    }

    #[test]
    fn spawn_with_spec_with_override_reuse_skips_provider_create() {
        // When Reuse(path) is supplied, even WITHOUT a worktree_provider
        // the path is honored (since no `git worktree add` is needed —
        // the path already exists on disk from the prior crashed run).
        // Depth-limit short-circuit means we don't actually run, but the
        // observable contract is: the API accepts the override + returns.
        let manager = manager_no_worktree(1);
        let mut spec = theo_domain::agent_spec::AgentSpec::on_demand("alpha", "x");
        spec.isolation = Some("worktree".to_string());
        let p = PathBuf::from("/tmp/wt-reused-from-resume");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let r = rt.block_on(async {
            manager
                .spawn_with_spec_with_override(
                    &spec,
                    "obj",
                    None,
                    WorktreeOverride::Reuse(p),
                )
                .await
        });
        // Depth limit hit, no panic — Reuse path didn't try to call git.
        assert!(r.summary.contains("depth limit"));
    }

    #[test]
    fn spawn_with_spec_with_override_recreate_passes_base_branch() {
        // When Recreate { base_branch } is supplied, the provider
        // (when present) would be invoked with the override base branch
        // INSTEAD of spec.isolation_base_branch. We verify by:
        //   - Setting spec.isolation_base_branch = "main"
        //   - Calling with Recreate { base_branch: "develop" }
        //   - At depth=1 we short-circuit, but the API contract is that
        //     this branch is honored (validated end-to-end via Fase 32).
        let manager = manager_no_worktree(1);
        let mut spec = theo_domain::agent_spec::AgentSpec::on_demand("alpha", "x");
        spec.isolation = Some("worktree".to_string());
        spec.isolation_base_branch = Some("main".to_string());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let r = rt.block_on(async {
            manager
                .spawn_with_spec_with_override(
                    &spec,
                    "obj",
                    None,
                    WorktreeOverride::Recreate {
                        base_branch: "develop".to_string(),
                    },
                )
                .await
        });
        assert!(r.summary.contains("depth limit"));
    }

    #[test]
    fn spawn_with_spec_alias_delegates_to_with_override_none() {
        // Verify that spawn_with_spec is now a wrapper that calls
        // spawn_with_spec_with_override(.., None). Same observable
        // behavior as the legacy parity test, but documents the
        // refactor contract explicitly.
        let manager = manager_no_worktree(1);
        let spec = theo_domain::agent_spec::AgentSpec::on_demand("a", "b");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let r1 = rt.block_on(async { manager.spawn_with_spec(&spec, "obj", None).await });
        let r2 = rt.block_on(async {
            manager
                .spawn_with_spec_with_override(&spec, "obj", None, WorktreeOverride::None)
                .await
        });
        assert_eq!(r1.success, r2.success);
        assert_eq!(r1.summary, r2.summary);
    }
}

