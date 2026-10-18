mod commands;
mod state;

use std::sync::Arc;

use state::{AppState, default_cache_path};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Arc::new(AppState::with_cache(default_cache_path())))
        .invoke_handler(tauri::generate_handler![
            commands::disk_info,
            commands::last_scan,
            commands::start_scan,
            commands::cancel_scan,
            commands::category_files,
            commands::reveal,
            commands::move_to_trash,
            commands::empty_trash,
            commands::docker_prune,
            commands::old_downloads,
            commands::largest_files,
            commands::trash_caches,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
