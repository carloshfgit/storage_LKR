mod commands;
mod state;

use std::sync::Arc;

use state::{AppState, MonitorState, default_cache_path};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Arc::new(AppState::with_cache(default_cache_path())))
        .manage(Arc::new(MonitorState::default()))
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
            commands::docker_details,
            commands::old_downloads,
            commands::largest_files,
            commands::trash_caches,
            commands::trash_items,
            commands::restore_from_trash,
            commands::purge_from_trash,
            commands::start_monitor,
            commands::stop_monitor,
            commands::system_info,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
