mod commands;
mod state;

use std::sync::Arc;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Arc::new(AppState::default()))
        .invoke_handler(tauri::generate_handler![
            commands::disk_info,
            commands::start_scan,
            commands::cancel_scan,
            commands::category_files,
            commands::reveal,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
