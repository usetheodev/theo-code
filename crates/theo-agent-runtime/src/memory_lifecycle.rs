//! Memory lifecycle helper (plan phase RM0).
//!
//! Central dispatch for the four MemoryProvider hooks called from the
//! agent loop. Every entry point short-circuits to a no-op when the
//! feature flag `AgentConfig.memory_enabled` is false or when no provider
//! is configured — runtime behaviour is identical to pre-RM0 in that
//! case. Keeps the hot path in `run_engine.rs` free of explicit
//! `if memory_enabled && provider.is_some() { ... }` noise.
//!
//! Reference: `referencias/hermes-agent/agent/memory_manager.py:97-206`
//! (fan-out + error isolation) and plan `outputs/agent-memory-plan.md` §RM0.

use theo_domain::memory::build_memory_context_block;

use crate::config::AgentConfig;

/// Entry point for the four hooks. Methods borrow from `AgentConfig`
/// rather than owning state so the helper stays zero-size.
pub struct MemoryLifecycle;

impl MemoryLifecycle {
    /// Pre-LLM hook. Returns a fenced memory block for injection into the
    /// next LLM prompt, or an empty string when memory is disabled or
    /// the provider has nothing relevant.
    pub async fn prefetch(cfg: &AgentConfig, query: &str) -> String {
        let Some(handle) = Self::active_handle(cfg) else {
            return String::new();
        };
        let raw = handle.as_provider().prefetch(query).await;
        if raw.is_empty() {
            String::new()
        } else {
            build_memory_context_block(&raw)
        }
    }

    /// Post-LLM hook. Persists the just-completed exchange. Silent on
    /// disabled/no-provider (pre-RM0 behaviour).
    pub async fn sync_turn(cfg: &AgentConfig, user: &str, assistant: &str) {
        if let Some(handle) = Self::active_handle(cfg) {
            handle.as_provider().sync_turn(user, assistant).await;
        }
    }

    /// Invoked just before compaction destroys message detail. Returns
    /// any fact-extraction payload the provider generated (empty string
    /// when disabled).
    pub async fn on_pre_compress(cfg: &AgentConfig, messages_as_text: &str) -> String {
        let Some(handle) = Self::active_handle(cfg) else {
            return String::new();
        };
        handle.as_provider().on_pre_compress(messages_as_text).await
    }

    /// Session lifecycle hook — called at convergence/abort.
    pub async fn on_session_end(cfg: &AgentConfig) {
        if let Some(handle) = Self::active_handle(cfg) {
            handle.as_provider().on_session_end().await;
        }
    }

    fn active_handle(cfg: &AgentConfig) -> Option<&crate::config::MemoryHandle> {
        if cfg.memory_enabled {
            cfg.memory_provider.as_ref()
        } else {
            None
        }
    }
}

/// Run-engine helpers (Phase 0 T0.1). Extracted here so `run_engine.rs`
/// stays under the 2500-line structural-hygiene cap while still hooking
/// every lifecycle point the plan requires.
pub mod run_engine_hooks {
    use super::MemoryLifecycle;
    use crate::config::AgentConfig;
    use theo_infra_llm::types::{Message, Role};

    /// Inject memory prefetch result as a fenced system message, if any.
    /// Returns true when a message was actually pushed.
    pub async fn inject_prefetch(
        cfg: &AgentConfig,
        messages: &mut Vec<Message>,
        query: &str,
    ) -> bool {
        if !cfg.memory_enabled {
            return false;
        }
        let block = MemoryLifecycle::prefetch(cfg, query).await;
        if block.is_empty() {
            return false;
        }
        messages.push(Message::system(&block));
        true
    }

    /// Invoke `on_pre_compress` and push any extracted content into
    /// `messages` so it survives the subsequent compaction step.
    pub async fn pre_compress_push(cfg: &AgentConfig, messages: &mut Vec<Message>) {
        if !cfg.memory_enabled {
            return;
        }
        let text: String = messages
            .iter()
            .filter_map(|m| m.content.clone())
            .collect::<Vec<_>>()
            .join("\n");
        let extracted = MemoryLifecycle::on_pre_compress(cfg, &text).await;
        if !extracted.is_empty() {
            messages.push(Message::system(format!(
                "## Memory (pre-compress extract)\n{extracted}"
            ).as_str()));
        }
    }

    /// Pair-end sync: find the most recent user message and persist it
    /// against `assistant_content`. No-op when memory is disabled.
    pub async fn sync_final_turn(cfg: &AgentConfig, messages: &[Message], assistant_content: &str) {
        if !cfg.memory_enabled {
            return;
        }
        let user_msg = messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, Role::User))
            .and_then(|m| m.content.clone())
            .unwrap_or_default();
        MemoryLifecycle::sync_turn(cfg, &user_msg, assistant_content).await;
    }

    /// Legacy memory fallback (pre-RM0 behaviour): loads kv entries from
    /// `$HOME/.config/theo/memory` and pushes them as a system message.
    /// Invoked ONLY when `memory_enabled=false` — preserves existing users'
    /// behaviour while the formal provider path is rolled out.
    pub async fn inject_legacy_file_memory(
        project_dir: &std::path::Path,
        messages: &mut Vec<Message>,
    ) {
        let memory_root = std::env::var("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"))
            .join(".config")
            .join("theo")
            .join("memory");
        let memory_store =
            theo_tooling::memory::FileMemoryStore::for_project(&memory_root, project_dir);
        if let Ok(memories) = memory_store.list().await
            && !memories.is_empty()
        {
            let block = memories
                .iter()
                .map(|m| format!("- **{}**: {}", m.key, m.value))
                .collect::<Vec<_>>()
                .join("\n");
            messages.push(Message::system(format!(
                "## Memory from previous runs\n{block}"
            ).as_str()));
        }
    }

    /// Phase 0 T0.3: feed eligible episode summaries back into the
    /// session context.
    ///
    /// Filtering (AC-0.3.1..0.3.6):
    /// - Lifecycle == Archived → skip (AC-0.3.2).
    /// - TTL expired → skip (AC-0.3.3).
    /// - Top-5 most recent (AC-0.3.1).
    /// - Emits `learned_constraints` as warnings (AC-0.3.4) and
    ///   `failed_attempts` visible to the LLM (AC-0.3.5).
    /// - Caps the injected block at 5% of the context window using a
    ///   rough chars/4 token estimate (AC-0.3.6).
    /// - No episodes → no message pushed (AC-0.3.7).
    pub fn inject_episode_history(
        project_dir: &std::path::Path,
        context_window_tokens: usize,
        messages: &mut Vec<Message>,
    ) {
        use theo_domain::episode::{MemoryLifecycle as Lc, TtlPolicy};

        let all = crate::state_manager::StateManager::load_episode_summaries(project_dir);
        if all.is_empty() {
            return;
        }

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let eligible: Vec<_> = all
            .iter()
            .rev()
            .filter(|ep| ep.lifecycle != Lc::Archived)
            .filter(|ep| match ep.ttl_policy {
                TtlPolicy::Permanent => true,
                TtlPolicy::RunScoped => true,
                TtlPolicy::TimeScoped { seconds } => {
                    now_ms.saturating_sub(ep.created_at) < seconds.saturating_mul(1000)
                }
            })
            .take(5)
            .collect();

        if eligible.is_empty() {
            return;
        }

        let mut parts: Vec<String> = Vec::new();
        for ep in &eligible {
            let mut piece = format!(
                "### {} — {}\nfiles: {}",
                ep.run_id,
                ep.machine_summary.objective,
                ep.affected_files.join(", ")
            );
            if !ep.machine_summary.learned_constraints.is_empty() {
                piece.push_str("\n\n**Learned constraints (treat as warnings):**");
                for c in &ep.machine_summary.learned_constraints {
                    piece.push_str(&format!("\n- {c}"));
                }
            }
            if !ep.machine_summary.failed_attempts.is_empty() {
                piece.push_str("\n\n**Past failures:**");
                for f in &ep.machine_summary.failed_attempts {
                    piece.push_str(&format!("\n- {f}"));
                }
            }
            parts.push(piece);
        }

        let mut body = format!("## Recent Episode History\n\n{}", parts.join("\n\n"));
        // Token budget: 5% of context window (chars/4 ≈ tokens).
        let budget_chars = context_window_tokens.saturating_mul(4) / 20;
        if body.len() > budget_chars && budget_chars > 0 {
            body.truncate(budget_chars);
            body.push_str("\n… [truncated to 5% context budget]");
        }
        messages.push(Message::system(&body));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MemoryHandle;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};
    use theo_domain::memory::{MEMORY_FENCE_OPEN, MemoryProvider, NullMemoryProvider};

    /// Records every hook invocation in order so ACs can assert on the
    /// full sequence.
    #[derive(Default)]
    struct RecordingProvider {
        log: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingProvider {
        fn new() -> (Arc<Self>, Arc<Mutex<Vec<String>>>) {
            let log = Arc::new(Mutex::new(Vec::new()));
            (
                Arc::new(Self { log: log.clone() }),
                log,
            )
        }
    }

    #[async_trait]
    impl MemoryProvider for RecordingProvider {
        fn name(&self) -> &str {
            "recording"
        }
        async fn prefetch(&self, query: &str) -> String {
            self.log
                .lock()
                .expect("t")
                .push(format!("prefetch:{query}"));
            format!("past fact about {query}")
        }
        async fn sync_turn(&self, user: &str, assistant: &str) {
            self.log
                .lock()
                .expect("t")
                .push(format!("sync:{user}>>{assistant}"));
        }
        async fn on_pre_compress(&self, txt: &str) -> String {
            self.log
                .lock()
                .expect("t")
                .push(format!("pre_compress:{}", txt.len()));
            "extracted".to_string()
        }
        async fn on_session_end(&self) {
            self.log.lock().expect("t").push("end".into());
        }
    }

    fn cfg_with(provider: Arc<dyn MemoryProvider>, enabled: bool) -> AgentConfig {
        AgentConfig {
            memory_enabled: enabled,
            memory_provider: Some(MemoryHandle::new(provider)),
            ..AgentConfig::default()
        }
    }

    // ── RM0-AC-1 ─────────────────────────────────────────────────
    #[tokio::test]
    async fn test_rm0_ac_1_prefetch_invokes_provider_when_enabled() {
        let (provider, log) = RecordingProvider::new();
        let cfg = cfg_with(provider, true);

        let block = MemoryLifecycle::prefetch(&cfg, "routing").await;

        assert!(
            block.contains(MEMORY_FENCE_OPEN),
            "block must be fenced: {block}"
        );
        assert!(block.contains("past fact about routing"));
        assert_eq!(log.lock().expect("t").first().expect("t"), "prefetch:routing");
    }

    // ── RM0-AC-2 ─────────────────────────────────────────────────
    #[tokio::test]
    async fn test_rm0_ac_2_sync_turn_persists_user_and_assistant() {
        let (provider, log) = RecordingProvider::new();
        let cfg = cfg_with(provider, true);
        MemoryLifecycle::sync_turn(&cfg, "hello", "world").await;
        assert_eq!(log.lock().expect("t").last().expect("t"), "sync:hello>>world");
    }

    // ── RM0-AC-3 ─────────────────────────────────────────────────
    #[tokio::test]
    async fn test_rm0_ac_3_on_pre_compress_receives_messages_text() {
        let (provider, log) = RecordingProvider::new();
        let cfg = cfg_with(provider, true);
        let out = MemoryLifecycle::on_pre_compress(&cfg, "abc").await;
        assert_eq!(out, "extracted");
        assert_eq!(log.lock().expect("t").last().expect("t"), "pre_compress:3");
    }

    // ── RM0-AC-4 ─────────────────────────────────────────────────
    #[tokio::test]
    async fn test_rm0_ac_4_on_session_end_triggers_provider_close() {
        let (provider, log) = RecordingProvider::new();
        let cfg = cfg_with(provider, true);
        MemoryLifecycle::on_session_end(&cfg).await;
        assert_eq!(log.lock().expect("t").last().expect("t"), "end");
    }

    // ── RM0-AC-5 ─────────────────────────────────────────────────
    #[tokio::test]
    async fn test_rm0_ac_5_memory_disabled_short_circuits_all_hooks() {
        let (provider, log) = RecordingProvider::new();
        let cfg = cfg_with(provider, false);

        let block = MemoryLifecycle::prefetch(&cfg, "q").await;
        MemoryLifecycle::sync_turn(&cfg, "u", "a").await;
        let fx = MemoryLifecycle::on_pre_compress(&cfg, "any").await;
        MemoryLifecycle::on_session_end(&cfg).await;

        assert_eq!(block, "");
        assert_eq!(fx, "");
        assert!(
            log.lock().expect("t").is_empty(),
            "disabled memory must not call provider; got {:?}",
            log.lock().expect("t")
        );
    }

    // ── RM0-AC-6 ─────────────────────────────────────────────────
    #[tokio::test]
    async fn test_rm0_ac_6_null_provider_preserves_behavior() {
        // With NullMemoryProvider + enabled, hooks complete without side effects.
        let null: Arc<dyn MemoryProvider> = Arc::new(NullMemoryProvider);
        let cfg = cfg_with(null, true);

        let block = MemoryLifecycle::prefetch(&cfg, "anything").await;
        MemoryLifecycle::sync_turn(&cfg, "u", "a").await;
        let fx = MemoryLifecycle::on_pre_compress(&cfg, "m").await;
        MemoryLifecycle::on_session_end(&cfg).await;

        assert_eq!(block, "", "null provider returns empty (no fence)");
        assert_eq!(fx, "");
    }

    // ── RM0-AC-7 (integration) ───────────────────────────────────
    #[tokio::test]
    async fn test_rm0_ac_7_hooks_invoked_in_canonical_order() {
        let (provider, log) = RecordingProvider::new();
        let cfg = cfg_with(provider, true);

        // Canonical sequence for a single-turn session:
        // prefetch → [LLM call happens here] → sync_turn → on_pre_compress
        // (maybe) → on_session_end.
        MemoryLifecycle::prefetch(&cfg, "q").await;
        MemoryLifecycle::sync_turn(&cfg, "u", "a").await;
        MemoryLifecycle::on_pre_compress(&cfg, "mid-session text").await;
        MemoryLifecycle::on_session_end(&cfg).await;

        let entries = log.lock().expect("t").clone();
        assert_eq!(entries.len(), 4);
        assert!(entries[0].starts_with("prefetch:"));
        assert!(entries[1].starts_with("sync:"));
        assert!(entries[2].starts_with("pre_compress:"));
        assert_eq!(entries[3], "end");
    }

    // ── Bonus: no provider + enabled also short-circuits ─────────
    #[tokio::test]
    async fn test_rm0_bonus_enabled_without_provider_is_noop() {
        let cfg = AgentConfig {
            memory_enabled: true,
            memory_provider: None,
            ..AgentConfig::default()
        };

        assert_eq!(MemoryLifecycle::prefetch(&cfg, "q").await, "");
        MemoryLifecycle::sync_turn(&cfg, "u", "a").await; // no panic
        assert_eq!(MemoryLifecycle::on_pre_compress(&cfg, "x").await, "");
        MemoryLifecycle::on_session_end(&cfg).await;
    }

    // ── Phase 0 T0.3 tests: inject_episode_history ──────────────
    mod t0_3 {
        use super::super::run_engine_hooks::inject_episode_history;
        use theo_infra_llm::types::Message;

        fn write_episode(
            dir: &std::path::Path,
            id: &str,
            lifecycle: &str,
            ttl: serde_json::Value,
            constraints: &[&str],
            failed: &[&str],
            created_at: u64,
        ) {
            let episodes_dir = dir.join(".theo/memory/episodes");
            std::fs::create_dir_all(&episodes_dir).expect("t");
            let payload = serde_json::json!({
                "summary_id": id,
                "run_id": id,
                "task_id": null,
                "window_start_event_id": "",
                "window_end_event_id": "",
                "machine_summary": {
                    "objective": format!("goal-{id}"),
                    "key_actions": [],
                    "outcome": "Success",
                    "successful_steps": [],
                    "failed_attempts": failed,
                    "learned_constraints": constraints,
                    "files_touched": []
                },
                "human_summary": null,
                "evidence_event_ids": [],
                "affected_files": ["src/main.rs"],
                "open_questions": [],
                "unresolved_hypotheses": [],
                "referenced_community_ids": [],
                "supersedes_summary_id": null,
                "schema_version": 1,
                "created_at": created_at,
                "ttl_policy": ttl,
                "lifecycle": lifecycle
            });
            std::fs::write(
                episodes_dir.join(format!("{id}.json")),
                serde_json::to_string(&payload).expect("t"),
            )
            .expect("t");
        }

        #[test]
        fn test_t0_3_ac_1_loads_recent_episodes() {
            let dir = tempfile::tempdir().expect("t");
            write_episode(
                dir.path(),
                "ep-a",
                "Active",
                serde_json::json!("RunScoped"),
                &["no unwrap"],
                &[],
                1,
            );
            let mut messages: Vec<Message> = Vec::new();
            inject_episode_history(dir.path(), 100_000, &mut messages);
            assert_eq!(messages.len(), 1);
            assert!(messages[0].content.as_ref().expect("t").contains("goal-ep-a"));
            assert!(messages[0].content.as_ref().expect("t").contains("no unwrap"));
        }

        #[test]
        fn test_t0_3_ac_2_archived_excluded() {
            let dir = tempfile::tempdir().expect("t");
            write_episode(
                dir.path(),
                "ep-old",
                "Archived",
                serde_json::json!("Permanent"),
                &[],
                &[],
                1,
            );
            let mut messages: Vec<Message> = Vec::new();
            inject_episode_history(dir.path(), 100_000, &mut messages);
            assert!(
                messages.is_empty(),
                "archived episodes must not be injected"
            );
        }

        #[test]
        fn test_t0_3_ac_3_expired_ttl_excluded() {
            let dir = tempfile::tempdir().expect("t");
            // created_at = 1 ms ago, seconds = 0 → expired
            write_episode(
                dir.path(),
                "ep-expired",
                "Active",
                serde_json::json!({"TimeScoped": {"seconds": 0}}),
                &[],
                &[],
                1,
            );
            let mut messages: Vec<Message> = Vec::new();
            inject_episode_history(dir.path(), 100_000, &mut messages);
            assert!(messages.is_empty());
        }

        #[test]
        fn test_t0_3_ac_5_failed_attempts_visible() {
            let dir = tempfile::tempdir().expect("t");
            write_episode(
                dir.path(),
                "ep-fail",
                "Active",
                serde_json::json!("RunScoped"),
                &[],
                &["bash: permission denied"],
                1,
            );
            let mut messages: Vec<Message> = Vec::new();
            inject_episode_history(dir.path(), 100_000, &mut messages);
            assert_eq!(messages.len(), 1);
            assert!(
                messages[0]
                    .content
                    .as_ref()
                    .expect("t")
                    .contains("permission denied")
            );
        }

        #[test]
        fn test_t0_3_ac_6_respects_5pct_token_budget() {
            let dir = tempfile::tempdir().expect("t");
            // Write a huge constraint string to force truncation.
            let huge: String = "x".repeat(100_000);
            write_episode(
                dir.path(),
                "ep-big",
                "Active",
                serde_json::json!("RunScoped"),
                &[huge.as_str()],
                &[],
                1,
            );
            // 1000 tokens * 4 chars / 20 = 200 chars budget.
            let mut messages: Vec<Message> = Vec::new();
            inject_episode_history(dir.path(), 1000, &mut messages);
            assert_eq!(messages.len(), 1);
            let body = messages[0].content.as_ref().expect("t");
            assert!(
                body.len() <= 260,
                "must respect 5% budget, got {} chars",
                body.len()
            );
            assert!(body.contains("truncated"), "must mark truncation");
        }

        #[test]
        fn test_t0_3_ac_7_no_episodes_is_noop() {
            let dir = tempfile::tempdir().expect("t");
            let mut messages: Vec<Message> = Vec::new();
            inject_episode_history(dir.path(), 100_000, &mut messages);
            assert!(messages.is_empty(), "no episodes → no system message");
        }
    }
}
