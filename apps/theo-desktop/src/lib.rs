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
            // Auth commands
            commands::auth::auth_login_browser,
            commands::auth::auth_start_device_flow,
            commands::auth::auth_poll_device_flow,
            commands::auth::auth_status,
            commands::auth::auth_logout,
            commands::auth::auth_apply_to_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Theo Code");
}
