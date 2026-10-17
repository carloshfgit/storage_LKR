//! Commands expostos ao frontend. Só repassam para o `AppState`; a lista que o
//! frontend pode chamar está em `build.rs` e `capabilities/default.json`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use scanner::{Category, DEFAULT_DOCKER_TIMEOUT, DiskInfo, FileEntry, ScanOptions, docker_usage};
use tauri::State;
use tauri::ipc::Channel;

use crate::state::{AppState, ScanEvent};

/// O app sempre mostra o disco do sistema.
const SCAN_ROOT: &str = "/";

#[tauri::command]
pub fn disk_info() -> Result<DiskInfo, String> {
    scanner::disk_info(Path::new(SCAN_ROOT)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn start_scan(
    state: State<'_, Arc<AppState>>,
    on_event: Channel<ScanEvent>,
) -> Result<(), String> {
    state
        .start_scan(
            ScanOptions::new(SCAN_ROOT),
            Box::new(|| docker_usage(DEFAULT_DOCKER_TIMEOUT)),
            move |event| {
                // Falha só se a janela fechou; não há a quem avisar.
                let _ = on_event.send(event);
            },
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cancel_scan(state: State<'_, Arc<AppState>>) {
    state.cancel_scan();
}

/// Maiores arquivos da categoria no último scan.
#[tauri::command]
pub fn category_files(
    state: State<'_, Arc<AppState>>,
    category: Category,
) -> Result<Vec<FileEntry>, String> {
    let result = state
        .last_result()
        .ok_or("nenhuma varredura concluída ainda")?;
    Ok(result
        .categories
        .get(&category)
        .map(|c| c.top.clone())
        .unwrap_or_default())
}

/// Abre o gerenciador de arquivos com `path` selecionado. Só aceita arquivos do último
/// scan. `async` porque a chamada D-Bus ao Nautilus bloqueia, e commands síncronos
/// rodam na thread principal.
#[tauri::command(async)]
pub fn reveal(state: State<'_, Arc<AppState>>, path: PathBuf) -> Result<(), String> {
    if !state.contains_file(&path) {
        return Err("arquivo fora do último scan".into());
    }
    tauri_plugin_opener::reveal_item_in_dir(&path).map_err(|e| e.to_string())
}
