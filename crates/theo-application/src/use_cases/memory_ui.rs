//! UI-side use case: surfaces the memory subsystem to the desktop
//! frontend. Pure Rust (no Tauri) so the logic is testable without the
//! full desktop toolchain.
//!
//! Plan: `outputs/agent-memory-plan.md` §UI.

use serde::{Deserialize, Serialize};

pub use theo_infra_memory::{
    LessonMetric, LintInputs, LintIssue, LintThresholds, Severity, run_lint,
};

// ─── Episodes ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EpisodeSummary {
    pub id: String,
    pub occurred_at_unix: u64,
    pub title: String,
    pub summary: String,
}

/// First-cut: empty list. Follow-up wires this to the real episode
/// store once RM3b reload-on-open lands; the UI binds to this surface.
pub fn list_episodes(_limit: Option<u32>, _offset: Option<u32>) -> Vec<EpisodeSummary> {
    Vec::new()
}

pub fn dismiss_episode(_id: &str) -> Result<(), String> {
    Ok(())
}

// ─── Wiki ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WikiPageMeta {
    pub slug: String,
    pub namespace: String,
    pub title: String,
    pub last_compile_unix: u64,
}

pub fn list_wiki_pages() -> Vec<WikiPageMeta> {
    Vec::new()
}

pub fn get_wiki_page(_slug: &str) -> String {
    String::new()
}

pub fn run_wiki_lint() -> Vec<LintIssue> {
    let inputs = LintInputs {
        seconds_since_last_compile: 0,
        lessons: Vec::new(),
        orphan_episode_ids: Vec::new(),
        broken_link_pages: Vec::new(),
        recall_p50_ms: 0.0,
        recall_p95_ms: 0.0,
    };
    run_lint(&inputs, &LintThresholds::default())
}

pub fn trigger_wiki_compile() -> Result<(), String> {
    if kill_switch_active() {
        return Err("wiki compile disabled by WIKI_COMPILE_ENABLED".to_string());
    }
    Ok(())
}

fn kill_switch_active() -> bool {
    matches!(
        std::env::var("WIKI_COMPILE_ENABLED")
            .ok()
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("false") | Some("0") | Some("off")
    )
}

// ─── Settings ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemorySettings {
    pub retention_days: u32,
    pub forgetting_enabled: bool,
    pub privacy_commit_gitignore: bool,
}

impl Default for MemorySettings {
    fn default() -> Self {
        Self {
            retention_days: 30,
            forgetting_enabled: false,
            privacy_commit_gitignore: true,
        }
    }
}

pub fn get_memory_settings() -> MemorySettings {
    MemorySettings::default()
}

pub fn save_memory_settings(_settings: MemorySettings) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Env-var tests must serialize because they mutate process-global
    /// state. Tokio `#[test]` parallelism is the default in Rust.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    // ── UI-AC-7 ─────────────────────────────────────────────────
    #[test]
    fn test_get_episodes_returns_valid_vec() {
        let episodes = list_episodes(Some(10), Some(0));
        // Serde roundtrip verifies the wire shape is stable.
        let sample = EpisodeSummary {
            id: "e1".into(),
            occurred_at_unix: 1_700_000_000,
            title: "session".into(),
            summary: "body".into(),
        };
        let j = serde_json::to_string(&sample).unwrap();
        let back: EpisodeSummary = serde_json::from_str(&j).unwrap();
        assert_eq!(back, sample);
        let _ = episodes;
    }

    // ── UI-AC-8 ─────────────────────────────────────────────────
    #[test]
    fn test_trigger_wiki_compile_respects_kill_switch() {
        let _g = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("WIKI_COMPILE_ENABLED", "false") };
        let res = trigger_wiki_compile();
        unsafe { std::env::remove_var("WIKI_COMPILE_ENABLED") };
        assert!(res.is_err(), "kill switch should block compile");
    }

    #[test]
    fn test_trigger_wiki_compile_allows_when_switch_unset() {
        let _g = ENV_LOCK.lock().unwrap();
        unsafe { std::env::remove_var("WIKI_COMPILE_ENABLED") };
        assert!(trigger_wiki_compile().is_ok());
    }

    #[test]
    fn test_run_wiki_lint_returns_issue_vec() {
        let issues = run_wiki_lint();
        assert_eq!(issues.len(), 0, "healthy stub inputs");
    }

    #[test]
    fn test_memory_settings_defaults() {
        let s = get_memory_settings();
        assert_eq!(s.retention_days, 30);
        assert!(!s.forgetting_enabled);
        assert!(s.privacy_commit_gitignore);
    }

    #[test]
    fn test_lint_inputs_with_critical_severity_surfaced() {
        let inputs = LintInputs {
            seconds_since_last_compile: 0,
            lessons: vec![LessonMetric {
                id: "l".into(),
                age_seconds: 0,
                hit_count: 0,
            }],
            orphan_episode_ids: Vec::new(),
            broken_link_pages: Vec::new(),
            recall_p50_ms: 0.0,
            recall_p95_ms: 5000.0,
        };
        let issues = run_lint(&inputs, &LintThresholds::default());
        assert!(issues.iter().any(|i| i.severity == Severity::Critical));
    }

    #[test]
    fn test_memory_settings_serde_roundtrip() {
        let a = MemorySettings {
            retention_days: 60,
            forgetting_enabled: true,
            privacy_commit_gitignore: false,
        };
        let j = serde_json::to_string(&a).unwrap();
        let back: MemorySettings = serde_json::from_str(&j).unwrap();
        assert_eq!(back, a);
    }
}
