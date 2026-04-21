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
            messages.push(Message::system(&format!(
                "## Memory (pre-compress extract)\n{extracted}"
            )));
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
        if let Ok(memories) = memory_store.list().await {
            if !memories.is_empty() {
                let block = memories
                    .iter()
                    .map(|m| format!("- **{}**: {}", m.key, m.value))
                    .collect::<Vec<_>>()
                    .join("\n");
                messages.push(Message::system(&format!(
                    "## Memory from previous runs\n{block}"
                )));
            }
        }
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
                .unwrap()
                .push(format!("prefetch:{query}"));
            format!("past fact about {query}")
        }
        async fn sync_turn(&self, user: &str, assistant: &str) {
            self.log
                .lock()
                .unwrap()
                .push(format!("sync:{user}>>{assistant}"));
        }
        async fn on_pre_compress(&self, txt: &str) -> String {
            self.log
                .lock()
                .unwrap()
                .push(format!("pre_compress:{}", txt.len()));
            "extracted".to_string()
        }
        async fn on_session_end(&self) {
            self.log.lock().unwrap().push("end".into());
        }
    }

    fn cfg_with(provider: Arc<dyn MemoryProvider>, enabled: bool) -> AgentConfig {
        let mut cfg = AgentConfig::default();
        cfg.memory_enabled = enabled;
        cfg.memory_provider = Some(MemoryHandle::new(provider));
        cfg
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
        assert_eq!(log.lock().unwrap().first().unwrap(), "prefetch:routing");
    }

    // ── RM0-AC-2 ─────────────────────────────────────────────────
    #[tokio::test]
    async fn test_rm0_ac_2_sync_turn_persists_user_and_assistant() {
        let (provider, log) = RecordingProvider::new();
        let cfg = cfg_with(provider, true);
        MemoryLifecycle::sync_turn(&cfg, "hello", "world").await;
        assert_eq!(log.lock().unwrap().last().unwrap(), "sync:hello>>world");
    }

    // ── RM0-AC-3 ─────────────────────────────────────────────────
    #[tokio::test]
    async fn test_rm0_ac_3_on_pre_compress_receives_messages_text() {
        let (provider, log) = RecordingProvider::new();
        let cfg = cfg_with(provider, true);
        let out = MemoryLifecycle::on_pre_compress(&cfg, "abc").await;
        assert_eq!(out, "extracted");
        assert_eq!(log.lock().unwrap().last().unwrap(), "pre_compress:3");
    }

    // ── RM0-AC-4 ─────────────────────────────────────────────────
    #[tokio::test]
    async fn test_rm0_ac_4_on_session_end_triggers_provider_close() {
        let (provider, log) = RecordingProvider::new();
        let cfg = cfg_with(provider, true);
        MemoryLifecycle::on_session_end(&cfg).await;
        assert_eq!(log.lock().unwrap().last().unwrap(), "end");
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
            log.lock().unwrap().is_empty(),
            "disabled memory must not call provider; got {:?}",
            log.lock().unwrap()
        );
    }

    // ── RM0-AC-6 ─────────────────────────────────────────────────
    #[tokio::test]
    async fn test_rm0_ac_6_null_provider_preserves_behavior() {
        // With NullMemoryProvider + enabled, hooks complete without side effects.
        let null: Arc<dyn MemoryProvider> = Arc::new(NullMemoryProvider::default());
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

        let entries = log.lock().unwrap().clone();
        assert_eq!(entries.len(), 4);
        assert!(entries[0].starts_with("prefetch:"));
        assert!(entries[1].starts_with("sync:"));
        assert!(entries[2].starts_with("pre_compress:"));
        assert_eq!(entries[3], "end");
    }

    // ── Bonus: no provider + enabled also short-circuits ─────────
    #[tokio::test]
    async fn test_rm0_bonus_enabled_without_provider_is_noop() {
        let mut cfg = AgentConfig::default();
        cfg.memory_enabled = true;
        cfg.memory_provider = None;

        assert_eq!(MemoryLifecycle::prefetch(&cfg, "q").await, "");
        MemoryLifecycle::sync_turn(&cfg, "u", "a").await; // no panic
        assert_eq!(MemoryLifecycle::on_pre_compress(&cfg, "x").await, "");
        MemoryLifecycle::on_session_end(&cfg).await;
    }
}
