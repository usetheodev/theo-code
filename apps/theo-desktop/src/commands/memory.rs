//! Tauri commands for the memory subsystem UI.
//!
//! Thin shim over `theo_application::use_cases::memory_ui`. Every
//! function is a one-line delegate so the shape lives in application,
//! not in the desktop binary — keeps logic testable on machines that
//! can't build the full Tauri dep graph (no glib/gtk).
//!
//! Plan: `outputs/agent-memory-plan.md` §UI.

use theo_application::use_cases::memory_ui::{
    self, EpisodeSummary, LintIssue, MemorySettings, WikiPageMeta,
};

#[tauri::command]
pub async fn get_episodes(limit: Option<u32>, offset: Option<u32>) -> Vec<EpisodeSummary> {
    memory_ui::list_episodes(limit, offset)
}

#[tauri::command]
pub async fn dismiss_episode(id: String) -> Result<(), String> {
    memory_ui::dismiss_episode(&id)
}

#[tauri::command]
pub async fn list_wiki_pages() -> Vec<WikiPageMeta> {
    memory_ui::list_wiki_pages()
}

#[tauri::command]
pub async fn get_wiki_page(slug: String) -> String {
    memory_ui::get_wiki_page(&slug)
}

#[tauri::command]
pub async fn run_wiki_lint() -> Vec<LintIssue> {
    memory_ui::run_wiki_lint()
}

#[tauri::command]
pub async fn trigger_wiki_compile() -> Result<(), String> {
    memory_ui::trigger_wiki_compile()
}

#[tauri::command]
pub async fn get_memory_settings() -> MemorySettings {
    memory_ui::get_memory_settings()
}

#[tauri::command]
pub async fn save_memory_settings(settings: MemorySettings) -> Result<(), String> {
    memory_ui::save_memory_settings(settings)
}
