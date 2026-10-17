//! Commands expostos ao frontend. Só repassam para o `AppState`; a lista que o
//! frontend pode chamar está em `build.rs` e `capabilities/default.json`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use scanner::{
    Category, DEFAULT_DOCKER_TIMEOUT, DiskInfo, DockerTarget, ScanOptions, ScanResult, docker_usage,
};
use tauri::State;
use tauri::ipc::Channel;

use crate::state::{AppState, Cleanup, FileItem, ScanEvent, TrashReport};

/// O app sempre mostra o disco do sistema.
const SCAN_ROOT: &str = "/";

#[tauri::command]
pub fn disk_info() -> Result<DiskInfo, String> {
    scanner::disk_info(Path::new(SCAN_ROOT)).map_err(|e| e.to_string())
}

/// Último resultado conhecido; ao abrir o app, é o que estava no cache.
#[tauri::command]
pub fn last_scan(state: State<'_, Arc<AppState>>) -> Option<ScanResult> {
    state.last_result()
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

/// Maiores arquivos da categoria no último scan, cada um dizendo se pode ir para a Lixeira.
/// `async`: a checagem faz um `canonicalize` por arquivo.
#[tauri::command(async)]
pub fn category_files(
    state: State<'_, Arc<AppState>>,
    category: Category,
) -> Result<Vec<FileItem>, String> {
    state.category_files(category).map_err(|e| e.to_string())
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

/// Move para a Lixeira do usuário (padrão freedesktop, restaurável pelo Nautilus).
#[tauri::command(async)]
pub fn move_to_trash(
    state: State<'_, Arc<AppState>>,
    paths: Vec<PathBuf>,
) -> Result<TrashReport, String> {
    state
        .move_to_trash(&paths, |p| trash::delete(p).map_err(|e| e.to_string()))
        .map_err(|e| e.to_string())
}

/// Apaga permanentemente tudo o que está na Lixeira. Exige `confirm == true`.
#[tauri::command(async)]
pub fn empty_trash(state: State<'_, Arc<AppState>>, confirm: bool) -> Result<Cleanup, String> {
    state
        .empty_trash(confirm, purge_trash, root_disk)
        .map_err(|e| e.to_string())
}

/// Roda um dos prunes fixos do Docker. Exige `confirm == true`.
#[tauri::command(async)]
pub fn docker_prune(
    state: State<'_, Arc<AppState>>,
    target: DockerTarget,
    confirm: bool,
) -> Result<Cleanup, String> {
    state
        .docker_prune(target, confirm, scanner::docker_prune, root_disk, || {
            docker_usage(DEFAULT_DOCKER_TIMEOUT)
        })
        .map_err(|e| e.to_string())
}

fn root_disk() -> std::io::Result<DiskInfo> {
    scanner::disk_info(Path::new(SCAN_ROOT))
}

fn purge_trash() -> Result<(), String> {
    let items = trash::os_limited::list().map_err(|e| e.to_string())?;
    trash::os_limited::purge_all(items).map_err(|e| e.to_string())
}
