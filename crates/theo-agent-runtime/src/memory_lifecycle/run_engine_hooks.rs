//! Run-engine helpers . Extracted here so `run_engine.rs`
//! stays under the 2500-line structural-hygiene cap while still hooking
//! every lifecycle point the plan requires.
//!
//! Fase 4 (REMEDIATION_PLAN T4.6). Moved from `memory_lifecycle.rs` to a
//! sibling file to shrink the parent module. Behavior is byte-identical.

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
    if !cfg.memory().enabled {
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
    if !cfg.memory().enabled {
        return;
    }
    let text: String = messages
        .iter()
        .filter_map(|m| m.content.clone())
        .collect::<Vec<_>>()
        .join("\n");
    let extracted = MemoryLifecycle::on_pre_compress(cfg, &text).await;
    if !extracted.is_empty() {
        messages.push(Message::system(
            format!("## Memory (pre-compress extract)\n{extracted}").as_str(),
        ));
    }
}

/// Pair-end sync: find the most recent user message and persist it
/// against `assistant_content`. No-op when memory is disabled.
pub async fn sync_final_turn(
    cfg: &AgentConfig,
    messages: &[Message],
    assistant_content: &str,
) {
    if !cfg.memory().enabled {
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
    // Skip legacy file-memory when HOME is missing (container / CI
    // without a proper user env) — do not fall back to /tmp.
    let Some(memory_root) = theo_domain::user_paths::theo_config_subdir("memory") else {
        return;
    };
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
        messages.push(Message::system(
            format!("## Memory from previous runs\n{block}").as_str(),
        ));
    }
}

/// T0.3: feed eligible episode summaries back into the session
/// context.
///
/// Filtering (AC-0.3.1..0.3.6):
/// - Lifecycle == Archived → skip (AC-0.3.2).
/// - TTL expired → skip (AC-0.3.3).
/// - Top-5 most recent (AC-0.3.1).
/// - Emits `learned_constraints` as warnings (AC-0.3.4) and
///   `failed_attempts` visible to the LLM (AC-0.3.5).
/// - Caps the injected block at 5% of the context window using a rough
///   chars/4 token estimate (AC-0.3.6).
/// - No episodes → no message pushed (AC-0.3.7).
///
/// Returns the number of episodes actually injected (0 if none were
/// eligible or the run had no prior episode history).
pub fn inject_episode_history(
    project_dir: &std::path::Path,
    context_window_tokens: usize,
    messages: &mut Vec<Message>,
) -> usize {
    use theo_domain::episode::{MemoryLifecycle as Lc, TtlPolicy};

    let all = crate::state_manager::StateManager::load_episode_summaries(project_dir);
    if all.is_empty() {
        return 0;
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
        return 0;
    }

    let injected_count = eligible.len();
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
    injected_count
}
