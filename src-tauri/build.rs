/// Commands do app. Declará-los aqui faz o Tauri exigir permissão explícita para cada um
/// (`allow-<command>` em `capabilities/default.json`), em vez de liberar todos por padrão.
const COMMANDS: &[&str] = &[
    "disk_info",
    "last_scan",
    "start_scan",
    "cancel_scan",
    "category_files",
    "reveal",
    "move_to_trash",
    "empty_trash",
    "docker_prune",
    "docker_details",
    "old_downloads",
    "largest_files",
    "trash_caches",
    "trash_items",
    "restore_from_trash",
    "purge_from_trash",
    "start_monitor",
    "stop_monitor",
    "system_info",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("falha ao gerar o contexto do Tauri");
}
