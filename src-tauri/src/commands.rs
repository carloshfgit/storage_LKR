//! Commands expostos ao frontend. Só repassam para o `AppState`; a lista que o
//! frontend pode chamar está em `build.rs` e `capabilities/default.json`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use scanner::{
    Category, DEFAULT_DOCKER_TIMEOUT, DiskInfo, DockerDetails, DockerMode, DockerTarget,
    ScanOptions, ScanResult, docker_status, docker_usage,
};
use tauri::State;
use tauri::ipc::Channel;

use crate::state::{
    AppState, Cleanup, FileItem, PurgeReport, ScanEvent, TrashItem, TrashListing, TrashReport,
};

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
            Box::new(|| docker_status(DEFAULT_DOCKER_TIMEOUT)),
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

/// Downloads antigos do último scan, cada um dizendo se pode ir para a Lixeira.
#[tauri::command(async)]
pub fn old_downloads(state: State<'_, Arc<AppState>>) -> Result<Vec<FileItem>, String> {
    state.old_downloads().map_err(|e| e.to_string())
}

/// Os `n` maiores arquivos que podem ir para a Lixeira (no máximo 50).
#[tauri::command(async)]
pub fn largest_files(state: State<'_, Arc<AppState>>, n: usize) -> Result<Vec<FileItem>, String> {
    state.largest_files(n).map_err(|e| e.to_string())
}

/// Move itens inteiros de `~/.cache` para a Lixeira (os apps recriam o cache).
#[tauri::command(async)]
pub fn trash_caches(
    state: State<'_, Arc<AppState>>,
    paths: Vec<PathBuf>,
) -> Result<TrashReport, String> {
    state
        .trash_caches(&paths, |p| trash::delete(p).map_err(|e| e.to_string()))
        .map_err(|e| e.to_string())
}

/// Abre o gerenciador de arquivos com `path` selecionado. Só aceita arquivos (e caches) do
/// último scan e itens que estão na Lixeira agora. `async` porque a chamada D-Bus ao
/// Nautilus bloqueia, e commands síncronos rodam na thread principal.
#[tauri::command(async)]
pub fn reveal(state: State<'_, Arc<AppState>>, path: PathBuf) -> Result<(), String> {
    let in_trash = || list_trash().is_ok_and(|l| l.iter().any(|i| i.stored_path() == path));
    if !state.contains_file(&path) && !in_trash() {
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

/// Apaga permanentemente tudo o que está na Lixeira, item por item: um item que o usuário
/// não consegue apagar volta recusado, com os comandos, e não impede o resto. Exige
/// `confirm == true`.
#[tauri::command(async)]
pub fn empty_trash(state: State<'_, Arc<AppState>>, confirm: bool) -> Result<PurgeReport, String> {
    state
        .empty_trash(
            confirm,
            list_trash,
            purge_trash_item,
            root_disk,
            scanner::locked_dirs,
        )
        .map_err(|e| e.to_string())
}

/// Roda um dos prunes fixos no daemon de `mode` (socket fixo, montado no Rust). Exige
/// `confirm == true`.
#[tauri::command(async)]
pub fn docker_prune(
    state: State<'_, Arc<AppState>>,
    mode: DockerMode,
    target: DockerTarget,
    confirm: bool,
) -> Result<Cleanup, String> {
    state
        .docker_prune(
            mode,
            target,
            confirm,
            scanner::docker_prune,
            root_disk,
            |m| docker_usage(m, DEFAULT_DOCKER_TIMEOUT),
        )
        .map_err(|e| e.to_string())
}

/// Imagens, containers, volumes e build cache do daemon de `mode`, item a item. Só leitura;
/// o socket é o fixo do modo. Leva uns 5 s, por isso `async`.
#[tauri::command(async)]
pub fn docker_details(mode: DockerMode) -> Result<DockerDetails, String> {
    scanner::docker_details(mode, DEFAULT_DOCKER_TIMEOUT)
        .ok_or_else(|| "o Docker não respondeu (parado, sem permissão ou demorou demais)".into())
}

/// Itens da Lixeira de agora, com o tamanho do último scan ou medido na hora.
#[tauri::command(async)]
pub fn trash_items(state: State<'_, Arc<AppState>>) -> Result<Vec<TrashItem>, String> {
    state
        .trash_items(list_trash, scanner::tree_size, scanner::locked_dirs)
        .map_err(|e| e.to_string())
}

/// Devolve itens da Lixeira ao local original, sem sobrescrever nada. `ids` são os que o
/// `trash_items` devolveu.
#[tauri::command(async)]
pub fn restore_from_trash(
    state: State<'_, Arc<AppState>>,
    ids: Vec<String>,
) -> Result<TrashReport, String> {
    state
        .restore_from_trash(&ids, list_trash, scanner::tree_size, restore_trash_item)
        .map_err(|e| e.to_string())
}

/// Apaga permanentemente só os itens escolhidos da Lixeira. Exige `confirm == true`.
#[tauri::command(async)]
pub fn purge_from_trash(
    state: State<'_, Arc<AppState>>,
    ids: Vec<String>,
    confirm: bool,
) -> Result<PurgeReport, String> {
    state
        .purge_from_trash(
            &ids,
            confirm,
            list_trash,
            purge_trash_item,
            root_disk,
            scanner::locked_dirs,
        )
        .map_err(|e| e.to_string())
}

fn list_trash() -> Result<Vec<TrashListing>, String> {
    let items = trash::os_limited::list().map_err(|e| e.to_string())?;
    Ok(items
        .into_iter()
        .map(|i| TrashListing {
            id: i.id.into(),
            name: i.name,
            original_parent: i.original_parent,
            deleted_at: i.time_deleted,
        })
        .collect())
}

fn os_item(item: &TrashListing) -> trash::TrashItem {
    trash::TrashItem {
        id: item.id.clone().into_os_string(),
        name: item.name.clone(),
        original_parent: item.original_parent.clone(),
        time_deleted: item.deleted_at,
    }
}

/// O crate não sobrescreve: um nome já ocupado no local original vira `RestoreCollision`.
fn restore_trash_item(item: &TrashListing) -> Result<(), String> {
    match trash::os_limited::restore_all([os_item(item)]) {
        Ok(()) => Ok(()),
        Err(trash::Error::RestoreCollision { .. }) => {
            Err("já existe um item com esse nome no local original".into())
        }
        Err(e) => Err(e.to_string()),
    }
}

fn root_disk() -> std::io::Result<DiskInfo> {
    scanner::disk_info(Path::new(SCAN_ROOT))
}

fn purge_trash_item(item: &TrashListing) -> Result<(), String> {
    trash::os_limited::purge_all([os_item(item)]).map_err(|e| e.to_string())
}
