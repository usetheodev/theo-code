mod commands;
mod events;
mod state;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            // Chat commands
            commands::chat::send_message,
            commands::chat::cancel_agent,
            commands::chat::set_project_dir,
            commands::chat::get_project_dir,
            commands::chat::update_config,
            commands::chat::get_config,
            // Auth commands (OpenAI)
            commands::auth::auth_login_browser,
            commands::auth::auth_start_device_flow,
            commands::auth::auth_poll_device_flow,
            commands::auth::auth_status,
            commands::auth::auth_logout,
            commands::auth::auth_apply_to_config,
            // Auth commands (GitHub Copilot)
            commands::copilot::copilot_start_device_flow,
            commands::copilot::copilot_poll_device_flow,
            commands::copilot::copilot_status,
            commands::copilot::copilot_logout,
            commands::copilot::copilot_apply_to_config,
            commands::copilot::provider_models,
            // Auth commands (Anthropic Console)
            commands::anthropic_auth::anthropic_start_device_flow,
            commands::anthropic_auth::anthropic_poll_device_flow,
            commands::anthropic_auth::anthropic_status,
            commands::anthropic_auth::anthropic_logout,
            commands::anthropic_auth::anthropic_apply_to_config,
            commands::anthropic_auth::anthropic_models,
            // Memory commands
            commands::memory::get_episodes,
            commands::memory::dismiss_episode,
            commands::memory::list_wiki_pages,
            commands::memory::get_wiki_page,
            commands::memory::run_wiki_lint,
            commands::memory::trigger_wiki_compile,
            commands::memory::get_memory_settings,
            commands::memory::save_memory_settings,
            // Observability dashboard
            commands::observability::list_runs,
            commands::observability::get_run_trajectory,
            commands::observability::get_run_metrics,
            commands::observability::compare_runs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Theo Code");
}
